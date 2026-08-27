use axum::{
    body::Bytes,
    http::header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_RANGE},
};
use axum_test::TestServer;
use momento_api::{auth::create_access_token, config::Config};
use serde_json::{json, Value};

use crate::test_utils::{create_test_app, create_test_user};

const HELLO_SHA256: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

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

fn upload_create_request(
    device_id: &str,
    client_asset_id: &str,
    operation_id: &str,
    original_filename: &str,
    byte_size: u64,
    content_hash: &str,
    source_modified_at: &str,
) -> Value {
    json!({
        "protocolVersion": 2,
        "deviceId": device_id,
        "clientAssetId": client_asset_id,
        "operationId": operation_id,
        "originalFilename": original_filename,
        "mimeType": "image/jpeg",
        "byteSize": byte_size,
        "contentHash": content_hash,
        "sourceModifiedAt": source_modified_at,
        "metadata": {
            "momentoBackup": {
                "schemaVersion": 2,
                "source": "androidMediaStore"
            }
        }
    })
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
        .json(&upload_create_request(
            "pixel_8",
            "asset_001",
            "operation_001",
            "vacation.jpg",
            5,
            HELLO_SHA256,
            "2024-01-02T03:04:05Z",
        ))
        .await;
    created.assert_status_ok();
    let upload_id = created.json::<Value>()["uploadId"]
        .as_str()
        .expect("upload ID")
        .to_string();

    let repeated = server
        .post("/api/v1/backup/upload/create")
        .add_header(AUTHORIZATION, format!("Bearer {owner_token}"))
        .json(&upload_create_request(
            "pixel_8",
            "asset_001",
            "operation_001",
            "vacation.jpg",
            5,
            HELLO_SHA256,
            "2024-01-02T03:04:05Z",
        ))
        .await;
    repeated.assert_status_ok();
    assert_eq!(repeated.json::<Value>()["uploadId"], upload_id);

    server
        .post("/api/v1/backup/upload/create")
        .add_header(AUTHORIZATION, format!("Bearer {owner_token}"))
        .json(&upload_create_request(
            "pixel_8",
            "different_asset",
            "operation_001",
            "different.jpg",
            5,
            HELLO_SHA256,
            "2024-01-02T03:04:05Z",
        ))
        .await
        .assert_status(axum::http::StatusCode::CONFLICT);

    server
        .put(&format!("/api/v1/backup/upload/chunk/{upload_id}"))
        .add_header(AUTHORIZATION, format!("Bearer {other_token}"))
        .add_header(CONTENT_LENGTH, "5")
        .add_header(CONTENT_RANGE, "bytes 0-4/5")
        .add_header("X-Content-SHA256", HELLO_SHA256)
        .bytes(Bytes::from_static(b"hello"))
        .await
        .assert_status_not_found();

    let chunk = server
        .put(&format!("/api/v1/backup/upload/chunk/{upload_id}"))
        .add_header(AUTHORIZATION, format!("Bearer {owner_token}"))
        .add_header(CONTENT_LENGTH, "5")
        .add_header(CONTENT_RANGE, "bytes 0-4/5")
        .add_header("X-Content-SHA256", HELLO_SHA256)
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
        .json(&upload_create_request(
            "missing_device",
            "asset_001",
            "operation_001",
            "photo.jpg",
            1,
            EMPTY_SHA256,
            "not-a-timestamp",
        ))
        .await;
    response.assert_status_bad_request();

    register_device(&server, &access_token, "registered_device").await;
    let mut raw_request = upload_create_request(
        "registered_device",
        "raw_asset",
        "raw_operation",
        "camera.dng",
        1,
        EMPTY_SHA256,
        "2024-01-02T03:04:05Z",
    );
    raw_request["mimeType"] = json!("image/x-adobe-dng");
    server
        .post("/api/v1/backup/upload/create")
        .add_header(AUTHORIZATION, format!("Bearer {access_token}"))
        .json(&raw_request)
        .await
        .assert_status_ok();
    let response = server
        .post("/api/v1/backup/upload/create")
        .add_header(AUTHORIZATION, format!("Bearer {access_token}"))
        .json(&upload_create_request(
            "registered_device",
            "asset_002",
            "operation_002",
            "../photo.jpg",
            1,
            EMPTY_SHA256,
            "2024-01-02T03:04:05Z",
        ))
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
        .json(&upload_create_request(
            "writing_device",
            "writing_asset",
            "writing_operation",
            "writing.jpg",
            5,
            HELLO_SHA256,
            "2024-01-02T03:04:05Z",
        ))
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

#[tokio::test]
async fn backup_rejects_changed_chunks_and_a_changed_whole_file() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "backup-hash", "backup-hash@example.com");
    let access_token = token(user_id);
    let server = TestServer::new(app).expect("test server");
    register_device(&server, &access_token, "hash_device").await;

    let created = server
        .post("/api/v1/backup/upload/create")
        .add_header(AUTHORIZATION, format!("Bearer {access_token}"))
        .json(&upload_create_request(
            "hash_device",
            "hash_asset",
            "hash_operation",
            "hash.jpg",
            5,
            EMPTY_SHA256,
            "2024-01-02T03:04:05Z",
        ))
        .await;
    created.assert_status_ok();
    let upload_id = created.json::<Value>()["uploadId"]
        .as_str()
        .expect("upload ID")
        .to_string();

    server
        .put(&format!("/api/v1/backup/upload/chunk/{upload_id}"))
        .add_header(AUTHORIZATION, format!("Bearer {access_token}"))
        .add_header(CONTENT_LENGTH, "5")
        .add_header(CONTENT_RANGE, "bytes 0-4/5")
        .add_header("X-Content-SHA256", EMPTY_SHA256)
        .bytes(Bytes::from_static(b"hello"))
        .await
        .assert_status_bad_request();

    let status_after_rejected_chunk = server
        .post("/api/v1/backup/upload/status")
        .add_header(AUTHORIZATION, format!("Bearer {access_token}"))
        .json(&json!({"uploadId": upload_id}))
        .await;
    status_after_rejected_chunk.assert_status_ok();
    assert_eq!(
        status_after_rejected_chunk.json::<Value>()["uploadedSize"],
        0
    );

    server
        .put(&format!("/api/v1/backup/upload/chunk/{upload_id}"))
        .add_header(AUTHORIZATION, format!("Bearer {access_token}"))
        .add_header(CONTENT_LENGTH, "5")
        .add_header(CONTENT_RANGE, "bytes 0-4/5")
        .add_header("X-Content-SHA256", HELLO_SHA256)
        .bytes(Bytes::from_static(b"hello"))
        .await
        .assert_status_ok();

    server
        .post("/api/v1/backup/upload/complete")
        .add_header(AUTHORIZATION, format!("Bearer {access_token}"))
        .json(&json!({"uploadId": upload_id}))
        .await
        .assert_status(axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn matching_legacy_upload_is_upgraded_to_the_lossless_manifest() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "backup-upgrade", "backup-upgrade@example.com");
    let access_token = token(user_id);
    let server = TestServer::new(app).expect("test server");
    register_device(&server, &access_token, "upgrade_device").await;
    let request = upload_create_request(
        "upgrade_device",
        "upgrade_asset",
        "upgrade_operation",
        "upgrade.jpg",
        5,
        HELLO_SHA256,
        "2024-01-02T03:04:05Z",
    );

    let created = server
        .post("/api/v1/backup/upload/create")
        .add_header(AUTHORIZATION, format!("Bearer {access_token}"))
        .json(&request)
        .await;
    created.assert_status_ok();
    let upload_id = created.json::<Value>()["uploadId"]
        .as_str()
        .expect("upload ID")
        .to_string();
    pool.get()
        .expect("database connection")
        .execute(
            "DELETE FROM backup_asset_manifests WHERE asset_id = (SELECT asset_id FROM backup_upload_sessions WHERE upload_id = ?)",
            [&upload_id],
        )
        .expect("simulate legacy upload");

    let upgraded = server
        .post("/api/v1/backup/upload/create")
        .add_header(AUTHORIZATION, format!("Bearer {access_token}"))
        .json(&request)
        .await;
    upgraded.assert_status_ok();
    assert_eq!(upgraded.json::<Value>()["contentHash"], HELLO_SHA256);
}
