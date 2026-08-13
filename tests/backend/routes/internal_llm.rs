use crate::test_utils::{create_test_db, create_test_media, init_test_paths};
use axum::http::StatusCode;
use axum_test::TestServer;
use momento_api::app::create_app;
use momento_api::config::Config;
use std::sync::Arc;

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
