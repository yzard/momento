use axum::http::{header::AUTHORIZATION, StatusCode};
use axum_test::TestServer;
use momento_api::auth::create_access_token;
use momento_api::config::Config;
use serde_json::{json, Value};

use crate::test_utils::{create_test_app, create_test_user, test_executor_handles};

fn token(user_id: i64, username: &str, role: &str) -> String {
    create_access_token(user_id, username, role, &Config::default(), None).expect("token")
}

fn insert_failed_operation(pool: &momento_api::database::DbPool, id: &str, version: i64) {
    pool.get()
        .expect("database")
        .execute(
            "INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, state, entry_count, version) VALUES (?, 'test', 'test', 'test-owner', 'publication_failed', 1, ?)",
            rusqlite::params![id, version],
        )
        .expect("failed file operation");
}

#[tokio::test]
async fn retry_requires_an_administrator() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "journal-user", "journal-user@example.com");
    let server = TestServer::new(app).expect("server");

    server
        .post("/api/v1/admin/file-operations/retry")
        .add_header(
            AUTHORIZATION,
            format!("Bearer {}", token(user_id, "journal-user", "user")),
        )
        .json(&json!({
            "retryRequestId": "retry-forbidden",
            "operationId": "operation-forbidden",
            "expectedVersion": 1
        }))
        .await
        .assert_status_forbidden();
}

#[tokio::test]
async fn retry_is_compare_and_set_and_idempotent() {
    let (app, pool) = create_test_app();
    let admin_id = create_test_user(&pool, "journal-admin", "journal-admin@example.com");
    pool.get()
        .expect("database")
        .execute("UPDATE users SET role = 'admin' WHERE id = ?", [admin_id])
        .expect("administrator");
    insert_failed_operation(&pool, "operation-retry", 3);
    let authorization = format!("Bearer {}", token(admin_id, "journal-admin", "admin"));
    let server = TestServer::new(app).expect("server");
    let request = json!({
        "retryRequestId": "retry-once",
        "operationId": "operation-retry",
        "expectedVersion": 3
    });

    let first = server
        .post("/api/v1/admin/file-operations/retry")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&request)
        .await;
    first.assert_status_ok();
    let first = first.json::<Value>();
    assert_eq!(first["state"], "publishing");
    assert_eq!(first["version"], 4);
    assert_eq!(first["replayed"], false);

    let replay = server
        .post("/api/v1/admin/file-operations/retry")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&request)
        .await;
    replay.assert_status_ok();
    assert_eq!(replay.json::<Value>()["replayed"], true);

    server
        .post("/api/v1/admin/file-operations/retry")
        .add_header(AUTHORIZATION, authorization)
        .json(&json!({
            "retryRequestId": "retry-once",
            "operationId": "different-operation",
            "expectedVersion": 3
        }))
        .await
        .assert_status(StatusCode::CONFLICT);
}

#[tokio::test]
async fn retry_receipt_limit_returns_too_many_requests() {
    let (app, pool) = create_test_app();
    let admin_id = create_test_user(&pool, "journal-limit", "journal-limit@example.com");
    let connection = pool.get().expect("database");
    connection
        .execute("UPDATE users SET role = 'admin' WHERE id = ?", [admin_id])
        .expect("administrator");
    drop(connection);
    insert_failed_operation(&pool, "operation-limit", 7);
    pool.get()
        .expect("database")
        .execute_batch(
            "WITH RECURSIVE receipt(number) AS (SELECT 1 UNION ALL SELECT number + 1 FROM receipt WHERE number < 64)
             INSERT INTO file_operation_retry_requests (retry_request_id, group_id, expected_version, request_hash, response_state, response_version, expires_at)
             SELECT 'seed-' || number, 'operation-limit', 1, zeroblob(32), 'publishing', 2, datetime('now', '+1 day') FROM receipt;",
        )
        .expect("retry receipts");
    let server = TestServer::new(app).expect("server");

    let response = server
        .post("/api/v1/admin/file-operations/retry")
        .add_header(
            AUTHORIZATION,
            format!("Bearer {}", token(admin_id, "journal-limit", "admin")),
        )
        .json(&json!({
            "retryRequestId": "retry-over-limit",
            "operationId": "operation-limit",
            "expectedVersion": 7
        }))
        .await;

    assert_eq!(response.status_code(), StatusCode::TOO_MANY_REQUESTS);
    let state: String = pool
        .get()
        .expect("database")
        .query_row(
            "SELECT state FROM file_operation_groups WHERE id = 'operation-limit'",
            [],
            |row| row.get(0),
        )
        .expect("operation state");
    assert_eq!(state, "publication_failed");
}

#[tokio::test]
async fn list_filters_states_and_paginates_by_operation_cursor() {
    let (app, pool) = create_test_app();
    let admin_id = create_test_user(&pool, "journal-list", "journal-list@example.com");
    let connection = pool.get().expect("database");
    connection
        .execute("UPDATE users SET role = 'admin' WHERE id = ?", [admin_id])
        .expect("administrator");
    for (id, state) in [
        ("operation-c", "publication_failed"),
        ("operation-b", "publication_failed"),
        ("operation-a", "completed"),
    ] {
        connection
            .execute(
                "INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, state, entry_count) VALUES (?, 'test-kind', 'test-owner', 'owner-1', ?, 1)",
                rusqlite::params![id, state],
            )
            .expect("operation");
    }
    drop(connection);
    let authorization = format!("Bearer {}", token(admin_id, "journal-list", "admin"));
    let server = TestServer::new(app).expect("server");

    let first = server
        .post("/api/v1/admin/file-operations/list")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({"states": ["publication_failed"], "limit": 1}))
        .await;
    first.assert_status_ok();
    let first = first.json::<Value>();
    assert_eq!(first["operations"][0]["operationId"], "operation-c");
    assert_eq!(first["nextCursor"], "operation-c");

    let second = server
        .post("/api/v1/admin/file-operations/list")
        .add_header(AUTHORIZATION, authorization)
        .json(&json!({
            "states": ["publication_failed"],
            "cursor": first["nextCursor"],
            "limit": 1
        }))
        .await;
    second.assert_status_ok();
    let second = second.json::<Value>();
    assert_eq!(second["operations"][0]["operationId"], "operation-b");
    assert!(second["nextCursor"].is_null());
}

#[tokio::test]
async fn detail_returns_only_storage_roots_and_normalized_relative_paths() {
    let (app, pool) = create_test_app();
    let admin_id = create_test_user(&pool, "journal-detail", "journal-detail@example.com");
    let connection = pool.get().expect("database");
    connection
        .execute("UPDATE users SET role = 'admin' WHERE id = ?", [admin_id])
        .expect("administrator");
    connection
        .execute(
            "INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, state, entry_count) VALUES ('operation-detail', 'publish', 'test', 'owner', 'publishing', 1)",
            [],
        )
        .expect("operation");
    connection
        .execute(
            "INSERT INTO file_operation_entries (group_id, sequence, action, storage_root, temporary_path, destination_path, expected_size, expected_sha256) VALUES ('operation-detail', 0, 'publish', 'originals', 'staged/item', 'ready/item', 3, zeroblob(32))",
            [],
        )
        .expect("entry");
    connection
        .execute(
            "INSERT INTO file_operation_path_claims (group_id, sequence, storage_root, relative_path, path_key, mode, scope, role) VALUES ('operation-detail', 0, 'originals', 'ready/item', X'0005726561647900046974656d', 'write', 'exact', 'destination')",
            [],
        )
        .expect("claim");
    drop(connection);
    let server = TestServer::new(app).expect("server");

    let response = server
        .post("/api/v1/admin/file-operations/get")
        .add_header(
            AUTHORIZATION,
            format!("Bearer {}", token(admin_id, "journal-detail", "admin")),
        )
        .json(&json!({"operationId": "operation-detail"}))
        .await;

    response.assert_status_ok();
    let body = response.json::<Value>();
    assert_eq!(body["detailLevel"], "full");
    assert_eq!(body["entries"][0]["storageRoot"], "originals");
    assert_eq!(body["entries"][0]["temporaryPath"], "staged/item");
    assert_eq!(body["entries"][0]["destinationPath"], "ready/item");
    assert_eq!(
        body["entries"][0]["expectedSha256"].as_str().unwrap().len(),
        64
    );
    assert_eq!(body["pathClaims"][0]["relativePath"], "ready/item");
    assert!(!body.to_string().contains("/tmp/"));
    assert!(!body.to_string().contains("/data/"));
}

#[tokio::test]
async fn maintenance_compacts_terminal_detail_prunes_old_groups_and_expires_receipts() {
    let (app, pool) = create_test_app();
    let admin_id = create_test_user(
        &pool,
        "journal-maintenance",
        "journal-maintenance@example.com",
    );
    let media_id = crate::test_utils::create_test_media(&pool, "expired-result.jpg");
    let connection = pool.get().expect("database");
    connection
        .execute("UPDATE users SET role = 'admin' WHERE id = ?", [admin_id])
        .expect("administrator");
    connection
        .execute(
            "INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, state, entry_count, terminal_at) VALUES ('operation-cleaned', 'cleanup', 'test', 'owner', 'cleaned', 2, datetime('now'))",
            [],
        )
        .expect("cleaned operation");
    connection
        .execute_batch(
            "INSERT INTO file_operation_entries (group_id, sequence, action, storage_root, source_path, state, cleanup_state) VALUES
                ('operation-cleaned', 0, 'cleanup', 'journal', 'old/a', 'committed', 'cleaned'),
                ('operation-cleaned', 1, 'cleanup', 'journal', 'old/b', 'committed', 'cleaned');
             INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, state, entry_count, detail_level, entry_action_summary, entry_state_summary, cleanup_summary, terminal_at) VALUES
                ('operation-old', 'cleanup', 'test', 'owner', 'cleaned', 1, 'compacted', '{\"cleanup\":1}', '{\"committed\":1}', '{\"cleaned\":1}', datetime('now', '-8 days'));
             INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, state, entry_count) VALUES
                ('operation-expired-receipt', 'publish', 'test', 'owner', 'publication_failed', 1);
             INSERT INTO file_operation_retry_requests (retry_request_id, group_id, expected_version, request_hash, response_state, response_version, expires_at) VALUES
                ('expired-retry', 'operation-expired-receipt', 1, zeroblob(32), 'publishing', 2, datetime('now', '-1 second'));",
        )
        .expect("maintenance fixtures");
    connection
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES ('ef111111111111111111111111111111', ?, 'ocr', 'completed', 1)",
            [media_id],
        )
        .expect("terminal LLM job");
    connection
        .execute_batch(
            "INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, state, entry_count, detail_level, entry_action_summary, entry_state_summary, cleanup_summary, terminal_at) VALUES ('expired-result-group', 'llm_result_receive', 'llm_result', 'ef111111111111111111111111111111', 'cleaned', 1, 'compacted', '{\"publish\":1}', '{\"committed\":1}', '{\"cleaned\":1}', datetime('now'));",
        )
        .expect("expired result group");
    connection
        .execute_batch(
            "INSERT INTO data_dir_space_reservations (id, class, owner_kind, owner_id, filesystem_id, reserved_peak_additional_bytes, state) VALUES ('expired-result-sqlite', 'sqlite', 'llm_result', 'ef111111111111111111111111111111', 'test', 4096, 'released');",
        )
        .expect("expired result SQLite reservation");
    connection
        .execute(
            "INSERT INTO llm_result_receipts (job_id, attempt, job_version, media_id, task, result_status, model_type, model_version, encoding, record_count, byte_size, content_hash, journal_group_id, sqlite_reservation_id, inbox_path, receive_token, state, result_product_version, received_at, updated_at) VALUES ('ef111111111111111111111111111111', 1, 1, ?, 'ocr', 'completed', 'ocr', 'test', 'momento-result-records-v1', 1, 24, ?, 'expired-result-group', 'expired-result-sqlite', 'expired.records', '00000000-0000-0000-0000-000000000005', 'cleaned', 1, datetime('now', '-8 days'), datetime('now', '-8 days'))",
            rusqlite::params![media_id, "0".repeat(64)],
        )
        .expect("expired result receipt");
    drop(connection);

    let outcome = test_executor_handles(pool.clone())
        .sqlite
        .maintain_file_operation_journal_durable()
        .await
        .expect("journal maintenance");
    assert_eq!(outcome.expired_retry_receipts, 1);
    assert_eq!(outcome.expired_result_receipts, 1);
    assert_eq!(outcome.compacted_groups, 1);
    assert_eq!(outcome.pruned_groups, 1);

    let server = TestServer::new(app).expect("server");
    let response = server
        .post("/api/v1/admin/file-operations/get")
        .add_header(
            AUTHORIZATION,
            format!("Bearer {}", token(admin_id, "journal-maintenance", "admin")),
        )
        .json(&json!({"operationId": "operation-cleaned"}))
        .await;
    response.assert_status_ok();
    let body = response.json::<Value>();
    assert_eq!(body["detailLevel"], "compacted");
    assert!(body.get("entries").is_none());
    assert!(body.get("pathClaims").is_none());
    assert_eq!(body["compacted"]["entryActions"]["cleanup"], 2);
    assert_eq!(body["compacted"]["entryStates"]["committed"], 2);
    assert_eq!(body["compacted"]["cleanupStates"]["cleaned"], 2);

    let connection = pool.get().expect("database");
    let old_groups: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM file_operation_groups WHERE id = 'operation-old'",
            [],
            |row| row.get(0),
        )
        .expect("old groups");
    let expired_receipts: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM file_operation_retry_requests WHERE retry_request_id = 'expired-retry'",
            [],
            |row| row.get(0),
        )
        .expect("expired receipts");
    assert_eq!(old_groups, 0);
    assert_eq!(expired_receipts, 0);
}
