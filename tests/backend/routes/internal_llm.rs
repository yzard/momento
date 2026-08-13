use crate::test_utils::{create_test_db, create_test_media, init_test_paths};
use axum::http::StatusCode;
use axum_test::TestServer;
use base64::Engine;
use momento_api::app::create_app;
use momento_api::config::Config;
use momento_api::database::{create_pool_at, init_database};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

fn callback_body(job_id: &str, media_id: i64, attempt: i64) -> serde_json::Value {
    serde_json::json!({ "jobId": job_id, "mediaId": media_id, "task": "ocr", "attempt": attempt, "status": "completed", "modelType": "ocr", "modelVersion": "test", "result": { "text": "recognized" } })
}

fn create_callback_test_app() -> (axum::Router, momento_api::database::DbPool) {
    init_test_paths();
    let pool = create_test_db();
    let mut config = Config::default();
    config.llm.callback_key = "test-callback-key".to_string();
    (create_app(Arc::new(config), pool.clone()), pool)
}

#[tokio::test]
async fn callback_requires_configured_key_and_rejects_stale_attempts() {
    let (application, pool) = create_callback_test_app();
    let media_id = create_test_media(&pool, "callback.jpg");
    let connection = pool.get().expect("connection");
    connection.execute("INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES ('callback-job', ?, 'ocr', 'submitted', 2)", [media_id]).expect("job");
    drop(connection);
    let server = TestServer::new(application).expect("server");
    server
        .post("/api/v1/internal/llm/callback")
        .json(&callback_body("callback-job", media_id, 2))
        .await
        .assert_status_unauthorized();
    server
        .post("/api/v1/internal/llm/callback")
        .add_header("x-momento-callback-key", "test-callback-key")
        .json(&callback_body("callback-job", media_id, 1))
        .await
        .assert_status(StatusCode::CONFLICT);
}

#[tokio::test]
async fn terminal_callback_is_idempotently_acknowledged() {
    let (application, pool) = create_callback_test_app();
    let media_id = create_test_media(&pool, "callback-idempotent.jpg");
    let connection = pool.get().expect("connection");
    connection.execute("INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES ('callback-idempotent', ?, 'ocr', 'submitted', 1)", [media_id]).expect("job");
    drop(connection);
    let server = TestServer::new(application).expect("server");
    let request = callback_body("callback-idempotent", media_id, 1);
    server
        .post("/api/v1/internal/llm/callback")
        .add_header("x-momento-callback-key", "test-callback-key")
        .json(&request)
        .await
        .assert_status_ok();
    server
        .post("/api/v1/internal/llm/callback")
        .add_header("x-momento-callback-key", "test-callback-key")
        .json(&request)
        .await
        .assert_status_ok();
    let connection = pool.get().expect("connection");
    let text_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_text WHERE media_id = ? AND model_type = 'ocr'",
            [media_id],
            |row| row.get(0),
        )
        .expect("text count");
    assert_eq!(text_count, 1);
}

#[tokio::test]
async fn callback_persists_every_video_frame_result() {
    let (application, pool) = create_callback_test_app();
    let media_id = create_test_media(&pool, "video.mp4");
    let connection = pool.get().expect("connection");
    connection.execute("INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES ('callback-frames', ?, 'ocr', 'submitted', 1)", [media_id]).expect("job");
    drop(connection);
    let server = TestServer::new(application).expect("server");
    let request = serde_json::json!({ "jobId": "callback-frames", "mediaId": media_id, "task": "ocr", "attempt": 1, "status": "completed", "modelType": "ocr", "modelVersion": "test", "result": { "text": "first" }, "inputResults": [{ "sequence": 0, "frameTimestampMs": 0, "result": { "text": "first" } }, { "sequence": 1, "frameTimestampMs": 1000, "result": { "text": "second" } }] });
    server
        .post("/api/v1/internal/llm/callback")
        .add_header("x-momento-callback-key", "test-callback-key")
        .json(&request)
        .await
        .assert_status_ok();
    let connection = pool.get().expect("connection");
    let frame_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_text_inputs WHERE media_id = ? AND model_type = 'ocr'",
            [media_id],
            |row| row.get(0),
        )
        .expect("frame count");
    assert_eq!(frame_count, 2);
    let aggregate_text: String = connection
        .query_row(
            "SELECT string FROM media_text WHERE media_id = ? AND model_type = 'ocr'",
            [media_id],
            |row| row.get(0),
        )
        .expect("aggregate text");
    assert_eq!(aggregate_text, "first\nsecond");
}

#[tokio::test]
async fn clustering_callback_persists_integer_capture_timestamp() {
    let (application, pool) = create_callback_test_app();
    let media_id = create_test_media(&pool, "clustering.jpg");
    let connection = pool.get().expect("connection");
    let run_id = connection
        .query_row(
            "INSERT INTO media_similarity_runs (trigger, status) VALUES ('manual', 'running') RETURNING id",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("similarity run");
    connection.execute("INSERT INTO llm_jobs (id, media_id, deduplicate_run_id, task, status, attempts) VALUES ('callback-clustering', ?, ?, 'image_clustering', 'submitted', 1)", [media_id, run_id]).expect("job");
    drop(connection);
    let embedding = base64::engine::general_purpose::STANDARD.encode(vec![0_u8; 384 * 4]);
    let request = serde_json::json!({
        "jobId": "callback-clustering",
        "mediaId": media_id,
        "task": "image_clustering",
        "attempt": 1,
        "status": "completed",
        "modelType": "image_clustering",
        "modelVersion": "dinov2-small",
        "result": {
            "embedding": embedding,
            "embeddingEncoding": "float32_le",
            "embeddingDimensions": 384,
            "perceptualHash": "0123456789abcdef"
        }
    });
    let server = TestServer::new(application).expect("server");

    server
        .post("/api/v1/internal/llm/callback")
        .add_header("x-momento-callback-key", "test-callback-key")
        .json(&request)
        .await
        .assert_status_ok();

    let connection = pool.get().expect("connection");
    let capture_time_seconds: i64 = connection
        .query_row(
            "SELECT capture_time_seconds FROM media_similarity_index WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("capture timestamp");
    assert_eq!(capture_time_seconds, 1_705_314_600);
}

#[tokio::test]
async fn callback_returns_internal_database_detail_to_llm_service() {
    let (application, pool) = create_callback_test_app();
    let media_id = create_test_media(&pool, "callback-database-error.jpg");
    let connection = pool.get().expect("connection");
    connection.execute("INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES ('callback-database-error', ?, 'ocr', 'submitted', 1)", [media_id]).expect("job");
    connection
        .execute("DROP TABLE media_text", [])
        .expect("drop text table");
    drop(connection);
    let server = TestServer::new(application).expect("server");

    let response = server
        .post("/api/v1/internal/llm/callback")
        .add_header("x-momento-callback-key", "test-callback-key")
        .json(&callback_body("callback-database-error", media_id, 1))
        .await;

    response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    let body: serde_json::Value = response.json();
    assert!(body["detail"]
        .as_str()
        .expect("error detail")
        .contains("no such table: media_text"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn callback_waits_for_concurrent_writer_instead_of_returning_busy() {
    init_test_paths();
    let directory = TempDir::new().expect("database directory");
    let database_path = directory.path().join("database.sqlite");
    let pool = create_pool_at(&database_path).expect("database pool");
    let connection = pool.get().expect("connection");
    init_database(&connection).expect("database schema");
    drop(connection);
    let media_id = create_test_media(&pool, "callback-contention.jpg");
    let connection = pool.get().expect("connection");
    connection.execute("INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES ('callback-contention', ?, 'ocr', 'submitted', 1)", [media_id]).expect("job");
    drop(connection);
    let mut config = Config::default();
    config.llm.callback_key = "test-callback-key".to_string();
    let server = TestServer::new(create_app(Arc::new(config), pool)).expect("server");
    let (writer_ready, callback_ready) = std::sync::mpsc::sync_channel(1);
    let writer = std::thread::spawn(move || {
        let connection = rusqlite::Connection::open(database_path).expect("writer connection");
        connection
            .execute_batch("BEGIN IMMEDIATE")
            .expect("writer transaction");
        writer_ready.send(()).expect("writer ready");
        std::thread::sleep(Duration::from_millis(150));
        connection
            .execute_batch("ROLLBACK")
            .expect("writer rollback");
    });
    callback_ready.recv().expect("writer lock");

    server
        .post("/api/v1/internal/llm/callback")
        .add_header("x-momento-callback-key", "test-callback-key")
        .json(&callback_body("callback-contention", media_id, 1))
        .await
        .assert_status_ok();

    writer.join().expect("writer thread");
}

#[tokio::test]
async fn cancelled_job_acknowledges_late_callback_without_persisting() {
    let (application, pool) = create_callback_test_app();
    let media_id = create_test_media(&pool, "cancelled.jpg");
    let connection = pool.get().expect("connection");
    connection.execute("INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES ('callback-cancelled', ?, 'ocr', 'cancelled', 1)", [media_id]).expect("job");
    drop(connection);
    let server = TestServer::new(application).expect("server");
    server
        .post("/api/v1/internal/llm/callback")
        .add_header("x-momento-callback-key", "test-callback-key")
        .json(&callback_body("callback-cancelled", media_id, 1))
        .await
        .assert_status_ok();
    let connection = pool.get().expect("connection");
    let text_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_text WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("text count");
    assert_eq!(text_count, 0);
}

#[tokio::test]
async fn callback_rejects_non_terminal_status() {
    let (application, pool) = create_callback_test_app();
    let media_id = create_test_media(&pool, "invalid-status.jpg");
    let connection = pool.get().expect("connection");
    connection.execute("INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES ('callback-invalid-status', ?, 'ocr', 'submitted', 1)", [media_id]).expect("job");
    drop(connection);
    let server = TestServer::new(application).expect("server");
    let mut request = callback_body("callback-invalid-status", media_id, 1);
    request["status"] = serde_json::json!("running");
    server
        .post("/api/v1/internal/llm/callback")
        .add_header("x-momento-callback-key", "test-callback-key")
        .json(&request)
        .await
        .assert_status_bad_request();
}
