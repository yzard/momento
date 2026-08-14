use crate::test_utils::{create_test_app, create_test_media, create_test_user};
use axum::http::header::AUTHORIZATION;
use axum_test::TestServer;
use momento_api::auth::create_access_token;
use momento_api::config::Config;
use serde_json::{json, Value};

fn admin_token(user_id: i64) -> String {
    create_access_token(user_id, "admin", "admin", &Config::default())
        .expect("Failed to create token")
}

#[tokio::test]
async fn cancel_cancels_all_active_ai_jobs() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "ai-admin", "ai-admin@example.com");
    let media_id = create_test_media(&pool, "ai.jpg");
    let connection = pool.get().expect("Failed to get connection");
    connection
        .execute("UPDATE users SET role = 'admin' WHERE id = ?", [user_id])
        .expect("Failed to grant administrator role");
    connection
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status) VALUES ('0123456789abcdef0123456789abcdef', ?, 'ocr', 'queued')",
            [media_id],
        )
        .expect("Failed to insert AI job");
    drop(connection);
    let server = TestServer::new(app).expect("Failed to create server");

    let response = server
        .post("/api/v1/ai/cancel")
        .add_header(AUTHORIZATION, format!("Bearer {}", admin_token(user_id)))
        .json(&json!({}))
        .await;

    response.assert_status_ok();
    let body: Value = response.json();
    assert_eq!(body["queuedJobs"], 1);
    let connection = pool.get().expect("Failed to get connection");
    let status: String = connection
        .query_row("SELECT status FROM llm_jobs", [], |row| row.get(0))
        .expect("Failed to load AI job status");
    assert_eq!(status, "cancelled");
}

#[tokio::test]
async fn trigger_succeeds_when_image_clustering_is_already_running() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "trigger-admin", "trigger-admin@example.com");
    let connection = pool.get().expect("Failed to get connection");
    connection
        .execute("UPDATE users SET role = 'admin' WHERE id = ?", [user_id])
        .expect("Failed to grant administrator role");
    connection
        .execute(
            "INSERT INTO media_similarity_runs (trigger, status) VALUES ('manual', 'running')",
            [],
        )
        .expect("Failed to create active clustering run");
    drop(connection);
    let server = TestServer::new(app).expect("Failed to create server");

    let response = server
        .post("/api/v1/ai/trigger")
        .add_header(AUTHORIZATION, format!("Bearer {}", admin_token(user_id)))
        .json(&json!({}))
        .await;

    response.assert_status_ok();
    let body: Value = response.json();
    assert_eq!(body["message"], "AI processing queued");
}

#[tokio::test]
async fn clean_ocr_removes_ocr_results_and_jobs() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "clean-admin", "clean-admin@example.com");
    let media_id = create_test_media(&pool, "clean.jpg");
    let connection = pool.get().expect("Failed to get connection");
    connection
        .execute("UPDATE users SET role = 'admin' WHERE id = ?", [user_id])
        .expect("Failed to grant administrator role");
    connection
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status) VALUES ('abcdef0123456789abcdef0123456789', ?, 'ocr', 'completed')",
            [media_id],
        )
        .expect("Failed to insert OCR job");
    connection
        .execute(
            "INSERT INTO media_text (media_id, model_type, model_version, string) VALUES (?, 'ocr', 'test', 'text')",
            [media_id],
        )
        .expect("Failed to insert OCR result");
    drop(connection);
    let server = TestServer::new(app).expect("Failed to create server");

    server
        .post("/api/v1/ai/ocr/clean")
        .add_header(AUTHORIZATION, format!("Bearer {}", admin_token(user_id)))
        .json(&json!({}))
        .await
        .assert_status_ok();

    let connection = pool.get().expect("Failed to get connection");
    let jobs: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM llm_jobs WHERE task = 'ocr'",
            [],
            |row| row.get(0),
        )
        .expect("Failed to count OCR jobs");
    let results: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_text WHERE model_type = 'ocr'",
            [],
            |row| row.get(0),
        )
        .expect("Failed to count OCR results");
    assert_eq!(jobs, 0);
    assert_eq!(results, 0);
}
