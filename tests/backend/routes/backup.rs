use axum::{
    body::Bytes,
    http::header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_RANGE},
};
use axum_test::TestServer;
use momento_api::{auth::create_access_token, config::Config};
use serde_json::{json, Value};

use crate::test_utils::{create_test_app, create_test_user};

fn token(user_id: i64) -> String {
    create_access_token(user_id, "backup-user", "user", &Config::default(), None)
        .expect("backup access token")
}

async fn register_device(server: &TestServer, access_token: &str, device_id: &str) {
    server
        .post("/api/v1/backup/device/register")
        .add_header(AUTHORIZATION, format!("Bearer {access_token}"))
        .json(&json!({"deviceId": device_id, "deviceName": "Pixel Camera"}))
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn resumable_backup_upload_is_device_scoped_and_idempotent() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "backup-owner", "backup-owner@example.com");
    let other_user_id = create_test_user(&pool, "backup-other", "backup-other@example.com");
    let owner_token = token(user_id);
    let other_token = token(other_user_id);
    let server = TestServer::new(app).expect("test server");
    register_device(&server, &owner_token, "pixel_8").await;

    let created = server
        .post("/api/v1/backup/upload/create")
        .add_header(AUTHORIZATION, format!("Bearer {owner_token}"))
        .json(&json!({
            "deviceId": "pixel_8", "clientAssetId": "asset_001", "operationId": "operation_001",
            "originalFilename": "vacation.jpg", "mimeType": "image/jpeg", "byteSize": 5,
            "sourceModifiedAt": "2024-01-02T03:04:05Z"
        }))
        .await;
    created.assert_status_ok();
    let upload_id = created.json::<Value>()["uploadId"]
        .as_str()
        .expect("upload ID")
        .to_string();

    let repeated = server
        .post("/api/v1/backup/upload/create")
        .add_header(AUTHORIZATION, format!("Bearer {owner_token}"))
        .json(&json!({
            "deviceId": "pixel_8", "clientAssetId": "different_asset", "operationId": "operation_001",
            "originalFilename": "different.jpg", "mimeType": "image/jpeg", "byteSize": 5,
            "sourceModifiedAt": "2024-01-02T03:04:05Z"
        }))
        .await;
    repeated.assert_status_ok();
    assert_eq!(repeated.json::<Value>()["uploadId"], upload_id);

    server
        .put(&format!("/api/v1/backup/upload/chunk/{upload_id}"))
        .add_header(AUTHORIZATION, format!("Bearer {other_token}"))
        .add_header(CONTENT_LENGTH, "5")
        .add_header(CONTENT_RANGE, "bytes 0-4/5")
        .bytes(Bytes::from_static(b"hello"))
        .await
        .assert_status_not_found();

    let chunk = server
        .put(&format!("/api/v1/backup/upload/chunk/{upload_id}"))
        .add_header(AUTHORIZATION, format!("Bearer {owner_token}"))
        .add_header(CONTENT_LENGTH, "5")
        .add_header(CONTENT_RANGE, "bytes 0-4/5")
        .bytes(Bytes::from_static(b"hello"))
        .await;
    chunk.assert_status_ok();
    assert_eq!(chunk.json::<Value>()["uploadedSize"], 5);

    for _ in 0..2 {
        let complete = server
            .post("/api/v1/backup/upload/complete")
            .add_header(AUTHORIZATION, format!("Bearer {owner_token}"))
            .json(&json!({"uploadId": upload_id}))
            .await;
        complete.assert_status_ok();
        assert_eq!(complete.json::<Value>()["status"], "queued");
    }
}

#[tokio::test]
async fn backup_create_validates_registered_device_and_metadata() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "backup-validation", "backup-validation@example.com");
    let access_token = token(user_id);
    let server = TestServer::new(app).expect("test server");

    let response = server
        .post("/api/v1/backup/upload/create")
        .add_header(AUTHORIZATION, format!("Bearer {access_token}"))
        .json(&json!({
            "deviceId": "missing_device", "clientAssetId": "asset_001", "operationId": "operation_001",
            "originalFilename": "photo.jpg", "mimeType": "image/jpeg", "byteSize": 1,
            "sourceModifiedAt": "not-a-timestamp"
        }))
        .await;
    response.assert_status_bad_request();

    register_device(&server, &access_token, "registered_device").await;
    let response = server
        .post("/api/v1/backup/upload/create")
        .add_header(AUTHORIZATION, format!("Bearer {access_token}"))
        .json(&json!({
            "deviceId": "registered_device", "clientAssetId": "asset_002", "operationId": "operation_002",
            "originalFilename": "../photo.jpg", "mimeType": "image/jpeg", "byteSize": 1,
            "sourceModifiedAt": "2024-01-02T03:04:05Z"
        }))
        .await;
    response.assert_status_bad_request();
}

#[tokio::test]
async fn writing_upload_rejects_cancellation_until_chunk_finalization() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "backup-writing", "backup-writing@example.com");
    let access_token = token(user_id);
    let server = TestServer::new(app).expect("test server");
    register_device(&server, &access_token, "writing_device").await;
    let created = server
        .post("/api/v1/backup/upload/create")
        .add_header(AUTHORIZATION, format!("Bearer {access_token}"))
        .json(&json!({
            "deviceId": "writing_device", "clientAssetId": "writing_asset", "operationId": "writing_operation",
            "originalFilename": "writing.jpg", "mimeType": "image/jpeg", "byteSize": 5,
            "sourceModifiedAt": "2024-01-02T03:04:05Z"
        }))
        .await;
    created.assert_status_ok();
    let upload_id = created.json::<Value>()["uploadId"]
        .as_str()
        .expect("upload ID")
        .to_string();

    pool.get()
        .expect("database connection")
        .execute(
            momento_api::database::queries::backup::CLAIM_CHUNK,
            rusqlite::params![upload_id, user_id, 0_i64],
        )
        .expect("claim writing chunk");
    server
        .post("/api/v1/backup/upload/cancel")
        .add_header(AUTHORIZATION, format!("Bearer {access_token}"))
        .json(&json!({"uploadId": upload_id}))
        .await
        .assert_status(axum::http::StatusCode::CONFLICT);

    pool.get()
        .expect("database connection")
        .execute(
            momento_api::database::queries::backup::COMPLETE_CHUNK,
            rusqlite::params![5_i64, upload_id, user_id, 0_i64],
        )
        .expect("complete writing chunk");
    let cancelled = server
        .post("/api/v1/backup/upload/cancel")
        .add_header(AUTHORIZATION, format!("Bearer {access_token}"))
        .json(&json!({"uploadId": upload_id}))
        .await;
    cancelled.assert_status_ok();
    assert_eq!(cancelled.json::<Value>()["status"], "cancelled");
}
