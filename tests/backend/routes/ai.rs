use crate::test_utils::{
    create_test_app, create_test_db, create_test_media, create_test_user, init_test_paths,
};
use axum::http::header::AUTHORIZATION;
use axum_test::TestServer;
use momento_api::app::create_app;
use momento_api::auth::create_access_token;
use momento_api::config::Config;
use serde_json::{json, Value};
use std::sync::Arc;

fn admin_token(user_id: i64) -> String {
    create_access_token(user_id, "admin", "admin", &Config::default(), None)
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
    connection
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status) VALUES ('1123456789abcdef0123456789abcdef', ?, 'ocr', 'failed')",
            [media_id],
        )
        .expect("Failed to insert failed AI job");
    drop(connection);
    let server = TestServer::new(app).expect("Failed to create server");

    let response = server
        .post("/api/v1/ai/cancel")
        .add_header(AUTHORIZATION, format!("Bearer {}", admin_token(user_id)))
        .json(&json!({}))
        .await;

    response.assert_status_ok();
    let body: Value = response.json();
    assert_eq!(body["queuedJobs"], 2);
    let connection = pool.get().expect("Failed to get connection");
    let cancelled_jobs: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM llm_jobs WHERE status = 'cancelled'",
            [],
            |row| row.get(0),
        )
        .expect("Failed to count cancelled AI jobs");
    assert_eq!(cancelled_jobs, 2);
    let pending_cancellations: i64 = connection
        .query_row("SELECT COUNT(*) FROM llm_job_cancellations", [], |row| {
            row.get(0)
        })
        .expect("Failed to count pending cancellations");
    assert_eq!(pending_cancellations, 2);
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

#[tokio::test]
async fn face_admin_start_cancel_and_clean_use_a_durable_grouping_run() {
    init_test_paths();
    let pool = create_test_db();
    let mut config = Config::default();
    config.llm.face_detection_enabled = true;
    let app = create_app(
        Arc::new(config),
        pool.clone(),
        Default::default(),
        Arc::new(tokio::sync::Semaphore::new(16)),
        None,
    );
    let user_id = create_test_user(&pool, "face-ai-admin", "face-ai-admin@example.com");
    let media_id = create_test_media(&pool, "face-ai.jpg");
    let connection = pool.get().expect("connection");
    connection
        .execute("UPDATE users SET role = 'admin' WHERE id = ?", [user_id])
        .expect("administrator");
    connection
        .execute(
            "INSERT INTO media_metadata_jobs (media_id, status) VALUES (?, 'completed')",
            [media_id],
        )
        .expect("metadata job");
    connection.execute("INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, 'face_detection', 0, 'image', 'ai/face.jpg', 'face.jpg', 'image/jpeg', 4, 'hash')", [media_id]).expect("face input");
    connection.execute("INSERT INTO media_faces (media_id, input_sequence, face_index, x, y, width, height, confidence, quality, frontality, embedding, crop_path) VALUES (?, 0, 0, 0, 0, 1, 1, 1, 1, 1, X'00000000', 'faces/test.jpg')", [media_id]).expect("face");
    let face_id = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO face_groups (representative_face_id) VALUES (?)",
            [face_id],
        )
        .expect("face group");
    let face_group_id = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO face_group_members (face_group_id, face_id) VALUES (?, ?)",
            [face_group_id, face_id],
        )
        .expect("face group member");
    drop(connection);
    let server = TestServer::new(app).expect("server");
    let authorization = format!("Bearer {}", admin_token(user_id));

    let start = server
        .post("/api/v1/ai/faces/start")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({}))
        .await;
    start.assert_status_ok();
    assert_eq!(start.json::<Value>()["queuedJobs"], 1);
    let status = server
        .post("/api/v1/ai/faces/status")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({}))
        .await;
    status.assert_status_ok();
    assert_eq!(status.json::<Value>()["faceGroups"], 1);
    let connection = pool.get().expect("connection");
    let clustering_runs: i64 = connection
        .query_row("SELECT COUNT(*) FROM media_similarity_runs", [], |row| {
            row.get(0)
        })
        .expect("clustering run count");
    assert_eq!(clustering_runs, 0);
    drop(connection);
    server
        .post("/api/v1/ai/faces/cancel")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({}))
        .await
        .assert_status_ok();
    momento_api::processor::face_detection::finalize_ready_runs(&pool, 0.55)
        .expect("finalize cancel");
    server
        .post("/api/v1/ai/faces/clean")
        .add_header(AUTHORIZATION, authorization)
        .json(&json!({}))
        .await
        .assert_status_ok();

    let connection = pool.get().expect("connection");
    for table in [
        "face_grouping_runs",
        "media_face_detection_results",
        "media_faces",
        "face_groups",
    ] {
        let count: i64 = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("table count");
        assert_eq!(count, 0, "{table} should be empty");
    }
}
