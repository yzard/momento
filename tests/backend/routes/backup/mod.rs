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
    create_access_token(
        user_id,
        0,
        "backup-user",
        "user",
        &crate::test_utils::test_config(),
        None,
    )
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
            rusqlite::params![5_i64, "+24 hours", upload_id, user_id, 0_i64],
        )
        .expect("complete writing chunk");
    let cancelled = server
        .post("/api/v1/backup/upload/cancel")
        .add_header(AUTHORIZATION, format!("Bearer {access_token}"))
        .json(&json!({"uploadId": upload_id}))
        .await;
    cancelled.assert_status_ok();
    assert_eq!(cancelled.json::<Value>()["status"], "cancelled");
    let (cleanup_state, cleanup_groups, cleanup_claims): (String, i64, i64) = pool
        .get()
        .expect("database connection")
        .query_row(
            "SELECT file_operation_groups.state,
                    (SELECT COUNT(*) FROM file_operation_groups WHERE kind = 'backup_cancel_cleanup'),
                    (SELECT COUNT(*) FROM file_operation_path_claims WHERE group_id = file_operation_groups.id)
               FROM file_operation_groups
              WHERE kind = 'backup_cancel_cleanup'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("cancel cleanup journal");
    assert_eq!(cleanup_state, "cleanup_pending");
    assert_eq!(cleanup_groups, 1);
    assert_eq!(cleanup_claims, 1);

    let repeated = server
        .post("/api/v1/backup/upload/cancel")
        .add_header(AUTHORIZATION, format!("Bearer {access_token}"))
        .json(&json!({"uploadId": upload_id}))
        .await;
    repeated.assert_status_ok();
    assert_eq!(repeated.json::<Value>()["status"], "cancelled");
    let cleanup_groups: i64 = pool
        .get()
        .expect("database connection")
        .query_row(
            "SELECT COUNT(*) FROM file_operation_groups WHERE kind = 'backup_cancel_cleanup'",
            [],
            |row| row.get(0),
        )
        .expect("idempotent cleanup group count");
    assert_eq!(cleanup_groups, 1);
}

#[tokio::test]
async fn backup_rejects_changed_chunks_and_queues_whole_file_verification() {
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
        .assert_status_ok();
    let connection = pool.get().expect("database connection");
    let status: String = connection.query_row(
        "SELECT status FROM backup_assets WHERE id = (SELECT asset_id FROM backup_upload_sessions WHERE upload_id = ?)",
        [&upload_id], |row| row.get(0),
    ).expect("queued status");
    assert_eq!(status, "queued");
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

#[tokio::test]
async fn new_operation_replaces_same_device_asset_and_preserves_old_retry() {
    for previous_status in ["uploading", "expired", "cancelled"] {
        let (app, pool) = create_test_app();
        let user = create_test_user(&pool, "replace", "replace@example.com");
        let auth = token(user);
        let server = TestServer::new(app).expect("server");
        register_device(&server, &auth, "device").await;
        let mut original = upload_create_request(
            "device",
            "asset",
            "old-operation",
            "video.mp4",
            5,
            HELLO_SHA256,
            "2024-01-02T03:04:05Z",
        );
        original["mimeType"] = json!("video/mp4");
        let created = server
            .post("/api/v1/backup/upload/create")
            .add_header(AUTHORIZATION, format!("Bearer {auth}"))
            .json(&original)
            .await;
        created.assert_status_ok();
        let old_id = created.json::<Value>()["uploadId"]
            .as_str()
            .unwrap()
            .to_string();
        if previous_status != "uploading" {
            let connection = pool.get().unwrap();
            connection
                .execute(
                    "UPDATE backup_upload_sessions SET status = ? WHERE upload_id = ?",
                    [previous_status, &old_id],
                )
                .unwrap();
            connection
                .execute(
                    "UPDATE backup_assets SET status = ? WHERE operation_id = 'old-operation'",
                    [previous_status],
                )
                .unwrap();
        }
        let mut replacement = original.clone();
        replacement["operationId"] = json!("new-operation");
        let created = server
            .post("/api/v1/backup/upload/create")
            .add_header(AUTHORIZATION, format!("Bearer {auth}"))
            .json(&replacement)
            .await;
        created.assert_status_ok();
        let new_id = created.json::<Value>()["uploadId"]
            .as_str()
            .unwrap()
            .to_string();
        assert_ne!(new_id, old_id);
        assert_eq!(created.json::<Value>()["uploadedSize"], 0);
        assert_eq!(created.json::<Value>()["status"], "uploading");
        let old = server
            .post("/api/v1/backup/upload/status")
            .add_header(AUTHORIZATION, format!("Bearer {auth}"))
            .json(&json!({"uploadId": old_id}))
            .await;
        assert_eq!(old.json::<Value>()["status"], "cancelled");
        let retry = server
            .post("/api/v1/backup/upload/create")
            .add_header(AUTHORIZATION, format!("Bearer {auth}"))
            .json(&original)
            .await;
        retry.assert_status_ok();
        assert_eq!(retry.json::<Value>()["uploadId"], old_id);
        assert_eq!(retry.json::<Value>()["status"], "cancelled");
        let retry = server
            .post("/api/v1/backup/upload/create")
            .add_header(AUTHORIZATION, format!("Bearer {auth}"))
            .json(&replacement)
            .await;
        retry.assert_status_ok();
        assert_eq!(retry.json::<Value>()["uploadId"], new_id);
    }
}

#[tokio::test]
async fn replacement_waits_for_active_chunk_without_changing_old_job() {
    let (app, pool) = create_test_app();
    let user = create_test_user(&pool, "busy", "busy@example.com");
    let auth = token(user);
    let server = TestServer::new(app).unwrap();
    register_device(&server, &auth, "device").await;
    let mut request = upload_create_request(
        "device",
        "asset",
        "old",
        "video.mp4",
        5,
        HELLO_SHA256,
        "2024-01-02T03:04:05Z",
    );
    request["mimeType"] = json!("video/mp4");
    let created = server
        .post("/api/v1/backup/upload/create")
        .add_header(AUTHORIZATION, format!("Bearer {auth}"))
        .json(&request)
        .await;
    created.assert_status_ok();
    pool.get()
        .unwrap()
        .execute("UPDATE backup_upload_sessions SET status = 'writing'", [])
        .unwrap();
    request["operationId"] = json!("new");
    server
        .post("/api/v1/backup/upload/create")
        .add_header(AUTHORIZATION, format!("Bearer {auth}"))
        .json(&request)
        .await
        .assert_status(axum::http::StatusCode::CONFLICT);
    let connection = pool.get().unwrap();
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM backup_assets", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    let status: String = connection
        .query_row("SELECT status FROM backup_upload_sessions", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(status, "writing");
}

#[tokio::test]
async fn progressing_backup_renews_idle_deadline_and_active_write_survives_expiry() {
    use momento_api::database::queries::backup;
    let (app, pool) = create_test_app();
    let user = create_test_user(&pool, "slow-upload", "slow@example.com");
    let auth = token(user);
    let server = TestServer::new(app).unwrap();
    register_device(&server, &auth, "device").await;
    let request = upload_create_request(
        "device",
        "asset",
        "slow",
        "photo.jpg",
        10,
        HELLO_SHA256,
        "2024-01-02T03:04:05Z",
    );
    let created = server
        .post("/api/v1/backup/upload/create")
        .add_header(AUTHORIZATION, format!("Bearer {auth}"))
        .json(&request)
        .await;
    created.assert_status_ok();
    let upload_id = created.json::<Value>()["uploadId"]
        .as_str()
        .unwrap()
        .to_string();
    let connection = pool.get().unwrap();
    connection.execute("UPDATE backup_upload_sessions SET created_at = datetime('now', '-7 days'), expires_at = datetime('now', '+30 seconds') WHERE upload_id = ?", [&upload_id]).unwrap();
    server
        .put(&format!("/api/v1/backup/upload/chunk/{upload_id}"))
        .add_header(AUTHORIZATION, format!("Bearer {auth}"))
        .add_header(CONTENT_LENGTH, "5")
        .add_header(CONTENT_RANGE, "bytes 0-4/10")
        .add_header("X-Content-SHA256", HELLO_SHA256)
        .bytes(Bytes::from_static(b"hello"))
        .await
        .assert_status_ok();
    let remaining_hours: f64 = connection.query_row("SELECT (julianday(expires_at) - julianday('now')) * 24 FROM backup_upload_sessions WHERE upload_id = ?", [&upload_id], |r| r.get(0)).unwrap();
    assert!(remaining_hours > 23.9 && remaining_hours <= 24.0);
    assert_eq!(
        connection
            .execute(
                backup::CLAIM_CHUNK,
                rusqlite::params![upload_id, user, 5_i64]
            )
            .unwrap(),
        1
    );
    // Simulate a slow in-flight chunk crossing its previous idle deadline.
    connection.execute("UPDATE backup_upload_sessions SET expires_at = datetime('now', '-1 hour') WHERE upload_id = ?", [&upload_id]).unwrap();
    assert_eq!(connection.execute(backup::EXPIRE_SESSIONS, []).unwrap(), 0);
    let next: Option<i64> = connection
        .query_row(backup::SELECT_NEXT_EXPIRATION_SECONDS, [], |r| r.get(0))
        .unwrap();
    assert_eq!(next, None);
    let (executors, _directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let response = executors
        .sqlite
        .finish_backup_chunk_request(momento_api::database::operations::FinishBackupChunk {
            user_id: user,
            upload_id: upload_id.clone(),
            start: 5,
            next_offset: 10,
            session_expiry_hours: 48,
        })
        .await
        .unwrap();
    assert!(matches!(
        response,
        momento_api::database::operations::FinishBackupChunkOutcome::Completed(_)
    ));
    let remaining_hours: f64 = connection.query_row("SELECT (julianday(expires_at) - julianday('now')) * 24 FROM backup_upload_sessions WHERE upload_id = ?", [&upload_id], |r| r.get(0)).unwrap();
    assert!(remaining_hours > 47.9 && remaining_hours <= 48.0);
    // Genuine inactivity still expires; polling or failed chunks do not extend the lease.
    connection.execute("UPDATE backup_upload_sessions SET expires_at = datetime('now', '-1 hour') WHERE upload_id = ?", [&upload_id]).unwrap();
    assert_eq!(connection.execute(backup::EXPIRE_SESSIONS, []).unwrap(), 1);
    connection.execute(backup::EXPIRE_ASSETS, []).unwrap();
    let status = server
        .post("/api/v1/backup/upload/status")
        .add_header(AUTHORIZATION, format!("Bearer {auth}"))
        .json(&json!({"uploadId": upload_id}))
        .await;
    assert_eq!(status.json::<Value>()["status"], "expired");
}
