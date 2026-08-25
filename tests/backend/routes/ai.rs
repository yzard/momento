use crate::test_utils::{
    create_test_app, create_test_db, create_test_media, create_test_user, init_test_paths,
};
use axum::http::{header::AUTHORIZATION, StatusCode};
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

fn action_affected_jobs(body: &Value, feature: &str) -> i64 {
    body["results"]
        .as_array()
        .expect("action results")
        .iter()
        .find(|result| result["feature"] == feature)
        .expect("feature action result")["affectedJobs"]
        .as_i64()
        .expect("affected job count")
}

fn task_status<'a>(body: &'a Value, task: &str) -> &'a Value {
    body["tasks"]
        .as_array()
        .expect("task statuses")
        .iter()
        .find(|status| status["task"] == task)
        .expect("task status")
}

#[tokio::test]
async fn aggregate_status_requires_an_administrator() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "ai-viewer", "ai-viewer@example.com");
    let token = create_access_token(user_id, "ai-viewer", "user", &Config::default(), None)
        .expect("user token");
    let server = TestServer::new(app).expect("server");

    server
        .post("/api/v1/ai/status")
        .add_header(AUTHORIZATION, format!("Bearer {token}"))
        .await
        .assert_status_forbidden();
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
        .await;

    response.assert_status_ok();
    let body: Value = response.json();
    assert_eq!(action_affected_jobs(&body, "ocr"), 1);
    let connection = pool.get().expect("Failed to get connection");
    let cancelled_jobs: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM llm_jobs WHERE status = 'cancelled'",
            [],
            |row| row.get(0),
        )
        .expect("Failed to count cancelled AI jobs");
    assert_eq!(cancelled_jobs, 1);
    let pending_cancellations: i64 = connection
        .query_row("SELECT COUNT(*) FROM llm_job_cancellations", [], |row| {
            row.get(0)
        })
        .expect("Failed to count pending cancellations");
    assert_eq!(pending_cancellations, 2);
    let cancellation_scopes: Vec<(String, String)> = connection
        .prepare("SELECT scope, task FROM llm_cancellation_scopes")
        .expect("cancellation scope query")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("cancellation scopes")
        .collect::<Result<_, _>>()
        .expect("cancellation scope rows");
    assert_eq!(cancellation_scopes, [("all".to_string(), String::new())]);
    let failed_jobs: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM llm_jobs WHERE status = 'failed'",
            [],
            |row| row.get(0),
        )
        .expect("Failed to count failed AI jobs");
    assert_eq!(failed_jobs, 1);
}

#[tokio::test]
async fn cancelling_an_idle_feature_does_not_create_an_empty_scope() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "idle-cancel-admin", "idle-cancel@example.com");
    pool.get()
        .expect("database connection")
        .execute("UPDATE users SET role = 'admin' WHERE id = ?", [user_id])
        .expect("administrator");
    let server = TestServer::new(app).expect("server");

    let response = server
        .post("/api/v1/ai/ocr/cancel")
        .add_header(AUTHORIZATION, format!("Bearer {}", admin_token(user_id)))
        .await;

    response.assert_status_ok();
    assert_eq!(
        response.json::<Value>()["results"][0]["outcome"],
        "noActiveWork"
    );
    let scope_count: i64 = pool
        .get()
        .expect("database connection")
        .query_row("SELECT COUNT(*) FROM llm_cancellation_scopes", [], |row| {
            row.get(0)
        })
        .expect("cancellation scope count");
    assert_eq!(scope_count, 0);
}

#[tokio::test]
async fn face_cancel_reports_an_active_downstream_grouping_run() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "face-run-admin", "face-run@example.com");
    let connection = pool.get().expect("database connection");
    connection
        .execute("UPDATE users SET role = 'admin' WHERE id = ?", [user_id])
        .expect("administrator");
    connection
        .execute(
            "INSERT INTO face_grouping_runs (status) VALUES ('running')",
            [],
        )
        .expect("face grouping run");
    drop(connection);
    let server = TestServer::new(app).expect("server");

    let response = server
        .post("/api/v1/ai/face_detection/cancel")
        .add_header(AUTHORIZATION, format!("Bearer {}", admin_token(user_id)))
        .await;

    response.assert_status_ok();
    let body = response.json::<Value>();
    assert_eq!(body["results"][0]["outcome"], "cancellationRequested");
    assert_eq!(body["results"][0]["affectedJobs"], 0);
    let run_status: String = pool
        .get()
        .expect("database connection")
        .query_row("SELECT status FROM face_grouping_runs", [], |row| {
            row.get(0)
        })
        .expect("face grouping run status");
    assert_eq!(run_status, "cancelling");
}

#[tokio::test]
async fn start_succeeds_when_deduplicate_is_already_running() {
    init_test_paths();
    let pool = create_test_db();
    let mut config = Config::default();
    config.llm.enabled = true;
    config.llm.deduplicate_enabled = true;
    let app = create_app(
        Arc::new(config),
        pool.clone(),
        Default::default(),
        Arc::new(tokio::sync::Semaphore::new(16)),
        None,
    );
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
        .post("/api/v1/ai/start")
        .add_header(AUTHORIZATION, format!("Bearer {}", admin_token(user_id)))
        .await;

    response.assert_status_ok();
    let body: Value = response.json();
    assert_eq!(body["action"], "start");
    assert_eq!(body["results"].as_array().expect("results").len(), 7);
    assert_eq!(
        body["results"]
            .as_array()
            .expect("results")
            .iter()
            .find(|result| result["feature"] == "deduplicate")
            .expect("deduplicate result")["outcome"],
        "noWork"
    );
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
async fn clean_rejects_an_active_feature() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "active-clean-admin", "active-clean@example.com");
    let media_id = create_test_media(&pool, "active-clean.jpg");
    let connection = pool.get().expect("database connection");
    connection
        .execute("UPDATE users SET role = 'admin' WHERE id = ?", [user_id])
        .expect("administrator");
    connection
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status) VALUES ('99887766554433221100aabbccddeeff', ?, 'ocr', 'submitted')",
            [media_id],
        )
        .expect("active OCR job");
    drop(connection);
    let server = TestServer::new(app).expect("server");

    server
        .post("/api/v1/ai/ocr/clean")
        .add_header(AUTHORIZATION, format!("Bearer {}", admin_token(user_id)))
        .await
        .assert_status(StatusCode::CONFLICT);
}

#[tokio::test]
async fn image_aesthetics_admin_controls_queue_report_and_clean_results() {
    init_test_paths();
    let pool = create_test_db();
    let mut config = Config::default();
    config.llm.enabled = true;
    config.llm.image_aesthetics_enabled = true;
    let app = create_app(
        Arc::new(config),
        pool.clone(),
        Default::default(),
        Arc::new(tokio::sync::Semaphore::new(16)),
        None,
    );
    let user_id = create_test_user(&pool, "aesthetics-admin", "aesthetics-admin@example.com");
    let media_id = create_test_media(&pool, "aesthetics.jpg");
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
    connection.execute("INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, 'image_aesthetics', 0, 'image', 'previews', 'ai/aesthetics.jpg', 'aesthetics.jpg', 'image/jpeg', 4, 'hash')", [media_id]).expect("aesthetics input");
    drop(connection);
    let server = TestServer::new(app).expect("server");
    let authorization = format!("Bearer {}", admin_token(user_id));

    let trigger = server
        .post("/api/v1/ai/image_aesthetics/start")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({}))
        .await;
    trigger.assert_status_ok();
    assert_eq!(
        action_affected_jobs(&trigger.json::<Value>(), "image_aesthetics"),
        1
    );
    let status = server
        .post("/api/v1/ai/status")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({}))
        .await;
    status.assert_status_ok();
    let status_body = status.json::<Value>();
    assert_eq!(
        task_status(&status_body, "image_aesthetics")["jobs"]["queued"],
        1
    );
    server
        .post("/api/v1/ai/image_aesthetics/cancel")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({}))
        .await
        .assert_status_ok();
    let connection = pool.get().expect("connection");
    connection
        .execute("DELETE FROM llm_job_cancellations", [])
        .expect("acknowledge cancellations");
    connection
        .execute("DELETE FROM llm_cancellation_scopes", [])
        .expect("acknowledge cancellation scope");
    connection.execute("INSERT INTO media_aesthetics (media_id, model_type, model_version, aesthetic_score, scenic_score, simplicity_score, landscape_score, technical_quality_score) VALUES (?, 'image_aesthetics', 'test', 0.5, 0.5, 0.5, 0.5, 0.5)", [media_id]).expect("aesthetics result");
    drop(connection);
    server
        .post("/api/v1/ai/image_aesthetics/clean")
        .add_header(AUTHORIZATION, authorization)
        .json(&json!({}))
        .await
        .assert_status_ok();

    let connection = pool.get().expect("connection");
    let result_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM media_aesthetics", [], |row| {
            row.get(0)
        })
        .expect("aesthetics result count");
    let job_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM llm_jobs WHERE task = 'image_aesthetics'",
            [],
            |row| row.get(0),
        )
        .expect("aesthetics job count");
    assert_eq!(result_count, 0);
    assert_eq!(job_count, 0);
}

#[tokio::test]
async fn removed_image_aesthetics_reset_returns_not_found() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(
        &pool,
        "disabled-aesthetics-admin",
        "disabled-aesthetics-admin@example.com",
    );
    let media_id = create_test_media(&pool, "disabled-aesthetics.jpg");
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
    connection.execute("INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, 'image_aesthetics', 0, 'image', 'previews', 'ai/aesthetics.jpg', 'aesthetics.jpg', 'image/jpeg', 4, 'hash')", [media_id]).expect("aesthetics input");
    drop(connection);
    let server = TestServer::new(app).expect("server");

    let response = server
        .post("/api/v1/ai/image_aesthetics/reset")
        .add_header(AUTHORIZATION, format!("Bearer {}", admin_token(user_id)))
        .json(&json!({}))
        .await;
    response.assert_status_not_found();
    let jobs: i64 = pool
        .get()
        .expect("connection")
        .query_row(
            "SELECT COUNT(*) FROM llm_jobs WHERE task = 'image_aesthetics'",
            [],
            |row| row.get(0),
        )
        .expect("job count");
    assert_eq!(jobs, 0);
}

#[tokio::test]
async fn classifier_admin_controls_queue_report_cancel_and_clean_results() {
    init_test_paths();
    let pool = create_test_db();
    let mut config = Config::default();
    config.llm.enabled = true;
    config.llm.screenshot_detection_enabled = true;
    config.llm.document_detection_enabled = true;
    let app = create_app(
        Arc::new(config),
        pool.clone(),
        Default::default(),
        Arc::new(tokio::sync::Semaphore::new(16)),
        None,
    );
    let user_id = create_test_user(&pool, "classifier-admin", "classifier-admin@example.com");
    let media_id = create_test_media(&pool, "classifier-admin.jpg");
    let connection = pool.get().expect("database connection");
    connection
        .execute("UPDATE users SET role = 'admin' WHERE id = ?", [user_id])
        .expect("administrator");
    connection
        .execute(
            "INSERT INTO media_metadata_jobs (media_id, status) VALUES (?, 'completed')",
            [media_id],
        )
        .expect("metadata job");
    for task in ["screenshot_detection", "document_detection"] {
        connection
            .execute(
                "INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, ?, 0, 'image', 'previews', 'ai/classifier.jpg', 'classifier.jpg', 'image/jpeg', 4, 'hash')",
                rusqlite::params![media_id, task],
            )
            .expect("classifier input");
    }
    drop(connection);
    let server = TestServer::new(app).expect("server");
    let authorization = format!("Bearer {}", admin_token(user_id));

    server
        .post("/api/v1/ai/screenshot_detection/start")
        .json(&json!({}))
        .await
        .assert_status_unauthorized();
    for task in ["screenshot_detection", "document_detection"] {
        let start = server
            .post(&format!("/api/v1/ai/{task}/start"))
            .add_header(AUTHORIZATION, authorization.clone())
            .json(&json!({}))
            .await;
        start.assert_status_ok();
        assert_eq!(action_affected_jobs(&start.json::<Value>(), task), 1);
        let status = server
            .post("/api/v1/ai/status")
            .add_header(AUTHORIZATION, authorization.clone())
            .json(&json!({}))
            .await;
        status.assert_status_ok();
        let status_body = status.json::<Value>();
        assert_eq!(task_status(&status_body, task)["jobs"]["queued"], 1);
        server
            .post(&format!("/api/v1/ai/{task}/cancel"))
            .add_header(AUTHORIZATION, authorization.clone())
            .json(&json!({}))
            .await
            .assert_status_ok();
    }
    let connection = pool.get().expect("database connection");
    connection
        .execute("DELETE FROM llm_job_cancellations", [])
        .expect("acknowledge cancellations");
    connection
        .execute("DELETE FROM llm_cancellation_scopes", [])
        .expect("acknowledge cancellation scopes");
    connection.execute("INSERT INTO media_screenshot_classifications (media_id, model_type, model_version, is_screenshot, confidence) VALUES (?, 'screenshot_detection', 'test', 1, 0.9)", [media_id]).expect("screenshot result");
    connection.execute("INSERT INTO media_screenshot_classification_inputs (media_id, sequence, model_type, model_version, is_screenshot, confidence) VALUES (?, 0, 'screenshot_detection', 'test', 1, 0.9)", [media_id]).expect("screenshot input result");
    connection.execute("INSERT INTO media_document_classifications (media_id, model_type, model_version, is_document, confidence) VALUES (?, 'document_detection', 'test', 1, 0.8)", [media_id]).expect("document result");
    connection.execute("INSERT INTO media_document_classification_inputs (media_id, sequence, model_type, model_version, is_document, confidence) VALUES (?, 0, 'document_detection', 'test', 1, 0.8)", [media_id]).expect("document input result");
    drop(connection);
    for task in ["screenshot_detection", "document_detection"] {
        server
            .post(&format!("/api/v1/ai/{task}/clean"))
            .add_header(AUTHORIZATION, authorization.clone())
            .json(&json!({}))
            .await
            .assert_status_ok();
    }

    let connection = pool.get().expect("database connection");
    for table in [
        "media_screenshot_classifications",
        "media_screenshot_classification_inputs",
        "media_document_classifications",
        "media_document_classification_inputs",
    ] {
        let count: i64 = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("classifier row count");
        assert_eq!(count, 0, "{table} should be empty");
    }
}

#[tokio::test]
async fn face_admin_start_cancel_and_clean_use_a_durable_grouping_run() {
    init_test_paths();
    let pool = create_test_db();
    let mut config = Config::default();
    config.llm.enabled = true;
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
    connection.execute("INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, 'face_detection', 0, 'image', 'previews', 'ai/face.jpg', 'face.jpg', 'image/jpeg', 4, 'hash')", [media_id]).expect("face input");
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
        .post("/api/v1/ai/face_detection/start")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({}))
        .await;
    start.assert_status_ok();
    assert_eq!(
        action_affected_jobs(&start.json::<Value>(), "face_detection"),
        1
    );
    let status = server
        .post("/api/v1/ai/status")
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
        .post("/api/v1/ai/face_detection/cancel")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({}))
        .await
        .assert_status_ok();
    momento_api::processor::face_detection::finalize_ready_runs(&pool, 0.55)
        .expect("finalize cancel");
    let connection = pool.get().expect("connection");
    connection
        .execute("DELETE FROM llm_job_cancellations", [])
        .expect("acknowledge cancellations");
    connection
        .execute("DELETE FROM llm_cancellation_scopes", [])
        .expect("acknowledge cancellation scope");
    drop(connection);
    server
        .post("/api/v1/ai/face_detection/clean")
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

#[tokio::test]
async fn different_ai_features_start_independently() {
    init_test_paths();
    let pool = create_test_db();
    let mut config = Config::default();
    config.llm.enabled = true;
    config.llm.image_tagging_enabled = true;
    let app = create_app(
        Arc::new(config),
        pool.clone(),
        Default::default(),
        Arc::new(tokio::sync::Semaphore::new(16)),
        None,
    );
    let user_id = create_test_user(&pool, "independent-admin", "independent@example.com");
    let media_id = create_test_media(&pool, "independent.jpg");
    let connection = pool.get().expect("database connection");
    connection
        .execute("UPDATE users SET role = 'admin' WHERE id = ?", [user_id])
        .expect("administrator");
    connection
        .execute(
            "INSERT INTO media_metadata_jobs (media_id, status) VALUES (?, 'completed')",
            [media_id],
        )
        .expect("metadata job");
    for task in ["ocr", "image_tagging"] {
        connection
            .execute(
                "INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, ?, 0, 'image', 'previews', ?, ?, 'image/jpeg', 4, 'hash')",
                rusqlite::params![media_id, task, format!("ai/{task}.jpg"), format!("{task}.jpg")],
            )
            .expect("prepared input");
    }
    drop(connection);
    let server = TestServer::new(app).expect("server");
    let authorization = format!("Bearer {}", admin_token(user_id));

    server
        .post("/api/v1/ai/ocr/start")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({}))
        .await
        .assert_status_ok();
    pool.get()
        .expect("database connection")
        .execute(
            "UPDATE llm_jobs SET status = 'submitted' WHERE task = 'ocr'",
            [],
        )
        .expect("submitted OCR job");
    server
        .post("/api/v1/ai/image_tagging/start")
        .add_header(AUTHORIZATION, authorization)
        .json(&json!({}))
        .await
        .assert_status_ok();

    let connection = pool.get().expect("database connection");
    let ocr_status: String = connection
        .query_row(
            "SELECT status FROM llm_jobs WHERE task = 'ocr'",
            [],
            |row| row.get(0),
        )
        .expect("OCR status");
    let tagging_status: String = connection
        .query_row(
            "SELECT status FROM llm_jobs WHERE task = 'image_tagging'",
            [],
            |row| row.get(0),
        )
        .expect("tagging status");
    assert_eq!(ocr_status, "submitted");
    assert_eq!(tagging_status, "queued");
}

#[tokio::test]
async fn removed_trigger_and_image_clustering_routes_return_not_found() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "removed-route-admin", "removed-route@example.com");
    pool.get()
        .expect("database connection")
        .execute("UPDATE users SET role = 'admin' WHERE id = ?", [user_id])
        .expect("administrator");
    let server = TestServer::new(app).expect("server");
    let authorization = format!("Bearer {}", admin_token(user_id));

    for path in [
        "/api/v1/ai/trigger",
        "/api/v1/ai/ocr/trigger",
        "/api/v1/ai/image_tagging/trigger",
        "/api/v1/ai/image_aesthetics/trigger",
        "/api/v1/ai/screenshot_detection/trigger",
        "/api/v1/ai/document_detection/trigger",
        "/api/v1/ai/image_clustering/trigger",
        "/api/v1/ai/image_clustering/cancel",
        "/api/v1/ai/image_clustering/clean",
        "/api/v1/ai/ocr/status",
        "/api/v1/ai/image_tagging/status",
        "/api/v1/ai/image_aesthetics/status",
        "/api/v1/ai/screenshot_detection/status",
        "/api/v1/ai/document_detection/status",
        "/api/v1/ai/deduplicate/status",
        "/api/v1/ai/faces/start",
        "/api/v1/ai/faces/cancel",
        "/api/v1/ai/faces/clean",
        "/api/v1/ai/faces/status",
        "/api/v1/ai/deduplicate/groups",
        "/api/v1/ai/ocr/reset",
        "/api/v1/ai/image_tagging/reset",
        "/api/v1/ai/image_aesthetics/reset",
        "/api/v1/ai/screenshot_detection/reset",
        "/api/v1/ai/document_detection/reset",
    ] {
        server
            .post(path)
            .add_header(AUTHORIZATION, authorization.clone())
            .json(&json!({}))
            .await
            .assert_status_not_found();
    }
}

#[tokio::test]
async fn aggregate_status_reports_exact_independent_job_states_without_a_body() {
    init_test_paths();
    let pool = create_test_db();
    let mut config = Config::default();
    config.llm.enabled = true;
    config.llm.image_tagging_enabled = true;
    let app = create_app(
        Arc::new(config),
        pool.clone(),
        Default::default(),
        Arc::new(tokio::sync::Semaphore::new(16)),
        None,
    );
    let user_id = create_test_user(&pool, "status-admin", "status-admin@example.com");
    let media_id = create_test_media(&pool, "status.jpg");
    let connection = pool.get().expect("database connection");
    connection
        .execute("UPDATE users SET role = 'admin' WHERE id = ?", [user_id])
        .expect("administrator");
    for (job_id, task, status) in [
        ("aa00112233445566778899aabbccddee", "ocr", "submitting"),
        (
            "bb00112233445566778899aabbccddee",
            "image_tagging",
            "submitted",
        ),
    ] {
        connection
            .execute(
                "INSERT INTO llm_jobs (id, media_id, task, status) VALUES (?, ?, ?, ?)",
                rusqlite::params![job_id, media_id, task, status],
            )
            .expect("AI job");
    }
    connection.execute("INSERT INTO media_similarity_index (media_id, content_hash, model_version, preprocessing_version, embedding, perceptual_hash, processing_status) VALUES (?, 'hash', 'model', 'prepared', X'00000000', 'hash', 1)", [media_id]).expect("similarity index");
    drop(connection);
    let server = TestServer::new(app).expect("server");

    let response = server
        .post("/api/v1/ai/status")
        .add_header(AUTHORIZATION, format!("Bearer {}", admin_token(user_id)))
        .await;

    response.assert_status_ok();
    let body: Value = response.json();
    assert_eq!(task_status(&body, "ocr")["state"], "submitting");
    assert_eq!(task_status(&body, "ocr")["jobs"]["submitting"], 1);
    assert_eq!(task_status(&body, "image_tagging")["state"], "submitted");
    assert_eq!(task_status(&body, "image_tagging")["jobs"]["submitted"], 1);
    assert_eq!(task_status(&body, "document_detection")["enabled"], false);
    assert_eq!(body["deduplicate"]["ensembledMedia"], 1);
}

#[tokio::test]
async fn deduplicate_control_uses_the_complete_pipeline_and_shared_contract() {
    init_test_paths();
    let pool = create_test_db();
    let mut config = Config::default();
    config.llm.enabled = true;
    config.llm.deduplicate_enabled = true;
    let app = create_app(
        Arc::new(config),
        pool.clone(),
        Default::default(),
        Arc::new(tokio::sync::Semaphore::new(16)),
        None,
    );
    let user_id = create_test_user(&pool, "dedup-admin", "dedup-admin@example.com");
    let media_id = create_test_media(&pool, "dedup.jpg");
    let connection = pool.get().expect("database connection");
    connection
        .execute("UPDATE users SET role = 'admin' WHERE id = ?", [user_id])
        .expect("administrator");
    connection
        .execute(
            "INSERT INTO media_metadata_jobs (media_id, status) VALUES (?, 'completed')",
            [media_id],
        )
        .expect("metadata job");
    connection.execute("INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, 'image_clustering', 0, 'image', 'previews', 'ai/cluster.jpg', 'cluster.jpg', 'image/jpeg', 4, 'hash')", [media_id]).expect("clustering input");
    drop(connection);
    let server = TestServer::new(app).expect("server");
    let authorization = format!("Bearer {}", admin_token(user_id));

    let start = server
        .post("/api/v1/ai/deduplicate/start")
        .add_header(AUTHORIZATION, authorization.clone())
        .await;
    start.assert_status_ok();
    assert_eq!(
        action_affected_jobs(&start.json::<Value>(), "deduplicate"),
        1
    );

    let cancel = server
        .post("/api/v1/ai/deduplicate/cancel")
        .add_header(AUTHORIZATION, authorization.clone())
        .await;
    cancel.assert_status_ok();
    assert_eq!(
        action_affected_jobs(&cancel.json::<Value>(), "deduplicate"),
        1
    );
    momento_api::processor::deduplicator::finalize_ready_runs(&pool)
        .expect("finalize cancellation");
    let connection = pool.get().expect("database connection");
    connection
        .execute("DELETE FROM llm_job_cancellations", [])
        .expect("acknowledge cancellations");
    connection
        .execute("DELETE FROM llm_cancellation_scopes", [])
        .expect("acknowledge cancellation scope");
    drop(connection);

    server
        .post("/api/v1/ai/deduplicate/clean")
        .add_header(AUTHORIZATION, authorization)
        .await
        .assert_status_ok();
    let run_count: i64 = pool
        .get()
        .expect("database connection")
        .query_row("SELECT COUNT(*) FROM media_similarity_runs", [], |row| {
            row.get(0)
        })
        .expect("run count");
    assert_eq!(run_count, 0);
}
