use crate::test_utils::{
    assert_failed_ai_job_restarted, create_test_app, create_test_config_manager, create_test_db,
    create_test_media, create_test_user, prepare_failed_ai_job,
};
use axum::http::{header::AUTHORIZATION, StatusCode};
use axum_test::TestServer;
use momento_api::app::create_app;
use momento_api::auth::create_access_token;
use momento_api::config::{Config, ConfigManager};
use momento_api::processor::ai::operation::AiFeature;
use serde_json::{json, Value};
use tempfile::TempDir;

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

fn feature_schedule<'a>(body: &'a Value, feature: &str) -> &'a Value {
    body["schedules"]
        .as_array()
        .expect("feature schedules")
        .iter()
        .find(|schedule| schedule["feature"] == feature)
        .expect("feature schedule")
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
    server
        .post("/api/v1/ai/schedule/update")
        .add_header(AUTHORIZATION, format!("Bearer {token}"))
        .json(&json!({"feature": "ocr", "cronExpression": "0 3 * * *"}))
        .await
        .assert_status_forbidden();
}

#[tokio::test]
async fn administrator_updates_a_live_ai_schedule_and_persists_config() {
    let directory = TempDir::new().expect("temporary config directory");
    let config_path = directory.path().join("config.toml");
    let config = Config::default();
    std::fs::write(
        &config_path,
        format!(
            "# retain schedule comment\n{}",
            toml::to_string(&config).expect("serialize config")
        ),
    )
    .expect("write config");
    let pool = create_test_db();
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    let loaded =
        momento_api::config::load_config_with_identity(&config_path).expect("load config identity");
    let config_manager = ConfigManager::new(loaded, &executors);
    let app = create_app(
        config_manager.clone(),
        crate::test_utils::test_app_dependencies(pool.clone(), None),
    );
    let user_id = create_test_user(&pool, "schedule-admin", "schedule-admin@example.com");
    pool.get()
        .expect("database")
        .execute("UPDATE users SET role = 'admin' WHERE id = ?", [user_id])
        .expect("administrator");
    let authorization = format!("Bearer {}", admin_token(user_id));
    let server = TestServer::new(app).expect("server");

    let initial_status = server
        .post("/api/v1/ai/status")
        .add_header(AUTHORIZATION, authorization.clone())
        .await;
    initial_status.assert_status_ok();
    let initial_status = initial_status.json::<Value>();
    assert_eq!(initial_status["schedules"].as_array().unwrap().len(), 7);
    assert_eq!(
        feature_schedule(&initial_status, "ocr")["cronExpression"],
        config_manager.current().llm.ocr_cron
    );

    let update = server
        .post("/api/v1/ai/schedule/update")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({"feature": "ocr", "cronExpression": " 15  4 * * 1-5 "}))
        .await;
    update.assert_status_ok();
    assert_eq!(update.json::<Value>()["cronExpression"], "15 4 * * 1-5");
    assert_eq!(config_manager.current().llm.ocr_cron, "15 4 * * 1-5");
    let saved_config = std::fs::read_to_string(&config_path).expect("saved config");
    assert!(saved_config.contains("# retain schedule comment"));
    assert!(saved_config.contains("ocr_cron = \"15 4 * * 1-5\""));

    let updated_status = server
        .post("/api/v1/ai/status")
        .add_header(AUTHORIZATION, authorization.clone())
        .await;
    updated_status.assert_status_ok();
    assert_eq!(
        feature_schedule(&updated_status.json::<Value>(), "ocr")["cronExpression"],
        "15 4 * * 1-5"
    );

    server
        .post("/api/v1/ai/schedule/update")
        .add_header(AUTHORIZATION, authorization)
        .json(&json!({"feature": "ocr", "cronExpression": "invalid"}))
        .await
        .assert_status_bad_request();
    assert_eq!(config_manager.current().llm.ocr_cron, "15 4 * * 1-5");
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
    let pool = create_test_db();
    let mut config = Config::default();
    config.llm.enabled = true;
    let config_manager = create_test_config_manager(config);
    let app = create_app(
        config_manager,
        crate::test_utils::test_app_dependencies(pool.clone(), None),
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
async fn failed_jobs_can_be_cleaned_and_restarted_for_every_ai_feature() {
    for (case_index, (feature, task)) in [
        ("ocr", "ocr"),
        ("image_tagging", "image_tagging"),
        ("image_aesthetics", "image_aesthetics"),
        ("screenshot_detection", "screenshot_detection"),
        ("document_detection", "document_detection"),
        ("face_detection", "face_detection"),
        ("deduplicate", "image_clustering"),
    ]
    .into_iter()
    .enumerate()
    {
        let pool = create_test_db();
        let mut config = Config::default();
        config.llm.enabled = true;
        let app = create_app(
            create_test_config_manager(config),
            crate::test_utils::test_app_dependencies(pool.clone(), None),
        );
        let user_id = create_test_user(
            &pool,
            &format!("failed-clean-{case_index}"),
            &format!("failed-clean-{case_index}@example.com"),
        );
        let media_id = create_test_media(&pool, &format!("failed-clean-{case_index}.jpg"));
        prepare_failed_ai_job(&pool, media_id, task, &format!("{case_index:032x}"));
        let connection = pool.get().expect("database connection");
        connection
            .execute("UPDATE users SET role = 'admin' WHERE id = ?", [user_id])
            .expect("administrator");
        match feature {
            "face_detection" => {
                connection
                    .execute(
                        "INSERT INTO media_face_detection_results (media_id, model_type, model_version) VALUES (?, 'face_detection', 'test')",
                        [media_id],
                    )
                    .expect("face detection result");
            }
            "deduplicate" => {
                connection
                    .execute(
                        "INSERT INTO media_similarity_index (media_id, content_hash, model_version, preprocessing_version, embedding, perceptual_hash, processing_status) VALUES (?, 'hash', 'test', 'test', X'00000000', 1, 1)",
                        [media_id],
                    )
                    .expect("similarity result");
                connection
                    .execute(
                        "INSERT INTO media_similarity_hash_bands (media_id, band_index, band_value) VALUES (?, 0, 1)",
                        [media_id],
                    )
                    .expect("similarity hash band");
            }
            "ocr" | "image_tagging" => {
                connection
                    .execute(
                        "INSERT INTO media_text (media_id, model_type, model_version, string) VALUES (?, ?, 'test', 'result')",
                        rusqlite::params![media_id, task],
                    )
                    .expect("text result");
                connection
                    .execute(
                        "INSERT INTO media_text_inputs (media_id, model_type, sequence, model_version, string) VALUES (?, ?, 0, 'test', 'result')",
                        rusqlite::params![media_id, task],
                    )
                    .expect("text input result");
            }
            "image_aesthetics" => {
                connection.execute("INSERT INTO media_aesthetics (media_id, model_type, model_version, aesthetic_score, scenic_score, simplicity_score, landscape_score, technical_quality_score) VALUES (?, 'image_aesthetics', 'test', 0.5, 0.5, 0.5, 0.5, 0.5)", [media_id]).expect("aesthetic result");
                connection.execute("INSERT INTO media_aesthetic_inputs (media_id, sequence, model_type, model_version, aesthetic_score, scenic_score, simplicity_score, landscape_score, technical_quality_score) VALUES (?, 0, 'image_aesthetics', 'test', 0.5, 0.5, 0.5, 0.5, 0.5)", [media_id]).expect("aesthetic input result");
            }
            "screenshot_detection" => {
                connection.execute("INSERT INTO media_screenshot_classifications (media_id, model_type, model_version, is_screenshot, confidence) VALUES (?, 'screenshot_detection', 'test', 1, 0.9)", [media_id]).expect("screenshot result");
                connection.execute("INSERT INTO media_screenshot_classification_inputs (media_id, sequence, model_type, model_version, is_screenshot, confidence) VALUES (?, 0, 'screenshot_detection', 'test', 1, 0.9)", [media_id]).expect("screenshot input result");
            }
            "document_detection" => {
                connection.execute("INSERT INTO media_document_classifications (media_id, model_type, model_version, is_document, confidence) VALUES (?, 'document_detection', 'test', 1, 0.8)", [media_id]).expect("document result");
                connection.execute("INSERT INTO media_document_classification_inputs (media_id, sequence, model_type, model_version, is_document, confidence) VALUES (?, 0, 'document_detection', 'test', 1, 0.8)", [media_id]).expect("document input result");
            }
            _ => {
                unreachable!("all failed-job cleanup feature fixtures must be explicit")
            }
        }
        drop(connection);

        let server = TestServer::new(app).expect("server");
        let authorization = format!("Bearer {}", admin_token(user_id));
        let clean = server
            .post(&format!("/api/v1/ai/{feature}/clean"))
            .add_header(AUTHORIZATION, authorization.clone())
            .json(&json!({}))
            .await;
        clean.assert_status_ok();
        assert_eq!(action_affected_jobs(&clean.json::<Value>(), feature), 1);

        let remaining_jobs: i64 = pool
            .get()
            .expect("database connection")
            .query_row(
                "SELECT COUNT(*) FROM llm_jobs WHERE task = ?",
                [task],
                |row| row.get(0),
            )
            .expect("remaining failed job count");
        assert_eq!(remaining_jobs, 0, "{feature} failed job should be cleaned");

        let start = server
            .post(&format!("/api/v1/ai/{feature}/start"))
            .add_header(AUTHORIZATION, authorization)
            .json(&json!({}))
            .await;
        start.assert_status_ok();
        assert_eq!(action_affected_jobs(&start.json::<Value>(), feature), 1);
    }
}

#[tokio::test]
async fn manual_start_queues_a_new_attempt_after_failure_for_every_ai_feature() {
    let cases = [
        ("ocr", "ocr"),
        ("image_tagging", "image_tagging"),
        ("image_aesthetics", "image_aesthetics"),
        ("screenshot_detection", "screenshot_detection"),
        ("document_detection", "document_detection"),
        ("face_detection", "face_detection"),
        ("deduplicate", "image_clustering"),
    ];
    assert_eq!(
        cases,
        AiFeature::ALL.map(|feature| (feature.name(), feature.inference_task()))
    );

    for (case_index, (feature, task)) in cases.into_iter().enumerate() {
        let pool = create_test_db();
        let mut config = Config::default();
        config.llm.enabled = true;
        let app = create_app(
            create_test_config_manager(config),
            crate::test_utils::test_app_dependencies(pool.clone(), None),
        );
        let user_id = create_test_user(
            &pool,
            &format!("failed-retry-{case_index}"),
            &format!("failed-retry-{case_index}@example.com"),
        );
        pool.get()
            .expect("database connection")
            .execute("UPDATE users SET role = 'admin' WHERE id = ?", [user_id])
            .expect("administrator");
        let media_id = create_test_media(&pool, &format!("failed-retry-{case_index}.jpg"));
        prepare_failed_ai_job(&pool, media_id, task, &format!("{:032x}", case_index + 100));
        let server = TestServer::new(app).expect("server");

        let start = server
            .post(&format!("/api/v1/ai/{feature}/start"))
            .add_header(AUTHORIZATION, format!("Bearer {}", admin_token(user_id)))
            .json(&json!({}))
            .await;

        start.assert_status_ok();
        assert_eq!(action_affected_jobs(&start.json::<Value>(), feature), 1);
        assert_failed_ai_job_restarted(&pool, task);
    }
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
    let pool = create_test_db();
    let mut config = Config::default();
    config.llm.enabled = true;
    let config_manager = create_test_config_manager(config);
    let app = create_app(
        config_manager,
        crate::test_utils::test_app_dependencies(pool.clone(), None),
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
    let pool = create_test_db();
    let mut config = Config::default();
    config.llm.enabled = true;
    let config_manager = create_test_config_manager(config);
    let app = create_app(
        config_manager,
        crate::test_utils::test_app_dependencies(pool.clone(), None),
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
    let pool = create_test_db();
    let mut config = Config::default();
    config.llm.enabled = true;
    let config_manager = create_test_config_manager(config);
    let app = create_app(
        config_manager,
        crate::test_utils::test_app_dependencies(pool.clone(), None),
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
    connection.execute("INSERT INTO media_faces (media_id, input_sequence, face_index, x, y, width, height, confidence, face_size_score, frontality_score, visibility_score, feature_clarity_score, embedding, crop_path) VALUES (?, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, X'00000000', 'faces/test.jpg')", [media_id]).expect("face");
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
            "INSERT INTO face_group_members (face_group_id, face_id, manual_anchor) VALUES (?, ?, 0)",
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
    momento_api::processor::face_detection::finalize_ready_runs(
        &crate::test_utils::test_executor_handles(pool.clone()),
        &Config::default().face_group,
    )
    .await
    .expect("finalize cancel");
    let connection = pool.get().expect("connection");
    connection
        .execute("DELETE FROM llm_job_cancellations", [])
        .expect("acknowledge cancellations");
    connection
        .execute("DELETE FROM llm_cancellation_scopes", [])
        .expect("acknowledge cancellation scope");
    connection
        .execute(
            "INSERT INTO face_grouping_runs (status, completed_at) VALUES ('completed', datetime('now'))",
            [],
        )
        .expect("completed face grouping run");
    let completed_run_id = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO face_group_generations (run_id, status, published_at) VALUES (?, 'active', datetime('now'))",
            [completed_run_id],
        )
        .expect("active face generation");
    let active_generation_id = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO face_group_generation_state (id, active_generation_id) VALUES (1, ?)",
            [active_generation_id],
        )
        .expect("active face generation state");
    connection
        .execute(
            "UPDATE face_groups SET automatic_generation_id = ? WHERE id = ?",
            [active_generation_id, face_group_id],
        )
        .expect("published automatic face group");
    connection
        .execute(
            "UPDATE face_group_members SET automatic_generation_id = ? WHERE face_group_id = ?",
            [active_generation_id, face_group_id],
        )
        .expect("published automatic face group member");
    connection
        .execute(
            "INSERT INTO face_group_representatives (generation_id, face_group_id, face_id) VALUES (?, ?, ?)",
            [active_generation_id, face_group_id, face_id],
        )
        .expect("published face group representative");
    connection
        .execute(
            "INSERT INTO face_group_manual_state (id, revision) VALUES (1, 0)",
            [],
        )
        .expect("face manual state");
    drop(connection);
    server
        .post("/api/v1/ai/face_detection/clean")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({}))
        .await
        .assert_status_ok();

    let connection = pool.get().expect("connection");
    for table in [
        "face_grouping_runs",
        "face_group_generations",
        "face_group_generation_state",
        "face_group_manual_state",
        "face_group_representatives",
        "face_group_members",
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
    let transaction = connection
        .unchecked_transaction()
        .expect("face cleanup inspection transaction");
    let cleanup: (String, String, i64) = transaction
        .query_row(
            "SELECT state, kind, COUNT(*) OVER () FROM file_operation_groups WHERE kind = 'face_detection_clean'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("face cleanup journal");
    assert!(
        matches!(
            cleanup.0.as_str(),
            "prepared" | "publishing" | "files_committed" | "cleanup_pending" | "cleaned"
        ),
        "face cleanup should remain on the successful journal path: {cleanup:?}"
    );
    assert_eq!(cleanup.1, "face_detection_clean");
    assert_eq!(cleanup.2, 1);
    if cleanup.0 == "cleaned" {
        let claims = transaction
            .query_row(
                "SELECT COUNT(*) FROM file_operation_path_claims WHERE role = 'face_crop_tree'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("released face cleanup claim count");
        assert_eq!(claims, 0);
    } else {
        let claim: (String, String, String, String) = transaction
            .query_row(
                "SELECT storage_root, relative_path, mode, scope FROM file_operation_path_claims WHERE role = 'face_crop_tree'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("face cleanup path claim");
        assert_eq!(
            claim,
            (
                "previews".to_string(),
                "faces".to_string(),
                "write".to_string(),
                "subtree".to_string()
            )
        );
    }
    transaction.commit().expect("face cleanup inspection");
    drop(connection);

    let regenerate = server
        .post("/api/v1/ai/face_detection/start")
        .add_header(AUTHORIZATION, authorization)
        .json(&json!({}))
        .await;
    regenerate.assert_status_ok();
    assert_eq!(
        action_affected_jobs(&regenerate.json::<Value>(), "face_detection"),
        1
    );
}

#[tokio::test]
async fn different_ai_features_start_independently() {
    let pool = create_test_db();
    let mut config = Config::default();
    config.llm.enabled = true;
    let config_manager = create_test_config_manager(config);
    let app = create_app(
        config_manager,
        crate::test_utils::test_app_dependencies(pool.clone(), None),
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
    let pool = create_test_db();
    let mut config = Config::default();
    config.llm.enabled = true;
    let config_manager = create_test_config_manager(config);
    let app = create_app(
        config_manager,
        crate::test_utils::test_app_dependencies(pool.clone(), None),
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
    for task in [
        "ocr",
        "image_tagging",
        "image_aesthetics",
        "screenshot_detection",
        "document_detection",
        "face_detection",
    ] {
        assert_eq!(task_status(&body, task)["enabled"], true);
    }
    assert_eq!(body["deduplicate"]["ensembledMedia"], 1);
}

#[tokio::test]
async fn aggregate_status_reports_only_each_media_tasks_latest_job() {
    let pool = create_test_db();
    let mut config = Config::default();
    config.llm.enabled = true;
    let config_manager = create_test_config_manager(config);
    let app = create_app(
        config_manager,
        crate::test_utils::test_app_dependencies(pool.clone(), None),
    );
    let user_id = create_test_user(&pool, "latest-status-admin", "latest-status@example.com");
    let recovered_media_id = create_test_media(&pool, "recovered-face.jpg");
    let failed_media_id = create_test_media(&pool, "failed-face.jpg");
    let connection = pool.get().expect("database connection");
    connection
        .execute("UPDATE users SET role = 'admin' WHERE id = ?", [user_id])
        .expect("administrator");
    connection
        .execute(
            "INSERT INTO face_grouping_runs (id, status) VALUES (1, 'completed')",
            [],
        )
        .expect("face grouping run");
    for (job_id, media_id, status, error) in [
        (
            "face-old-failure",
            recovered_media_id,
            "failed",
            Some("runtime startup failed"),
        ),
        ("face-recovered", recovered_media_id, "completed", None),
        (
            "face-current-failure",
            failed_media_id,
            "failed",
            Some("image could not be decoded"),
        ),
    ] {
        connection
            .execute(
                "INSERT INTO llm_jobs (id, media_id, face_grouping_run_id, task, status, last_error) VALUES (?, ?, 1, 'face_detection', ?, ?)",
                rusqlite::params![job_id, media_id, status, error],
            )
            .expect("face detection job");
    }
    drop(connection);
    let server = TestServer::new(app).expect("server");

    let response = server
        .post("/api/v1/ai/status")
        .add_header(AUTHORIZATION, format!("Bearer {}", admin_token(user_id)))
        .await;

    response.assert_status_ok();
    let body: Value = response.json();
    let face_status = task_status(&body, "face_detection");
    assert_eq!(face_status["jobs"]["completed"], 1);
    assert_eq!(face_status["jobs"]["failed"], 1);
    assert_eq!(face_status["errors"], json!(["image could not be decoded"]));
}

#[tokio::test]
async fn deduplicate_control_uses_the_complete_pipeline_and_shared_contract() {
    let pool = create_test_db();
    let mut config = Config::default();
    config.llm.enabled = true;
    let config_manager = create_test_config_manager(config);
    let app = create_app(
        config_manager,
        crate::test_utils::test_app_dependencies(pool.clone(), None),
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
    let finalization_executors = crate::test_utils::test_executor_handles(pool.clone());
    momento_api::processor::deduplicator::finalize_ready_runs(&finalization_executors)
        .await
        .expect("finalize cancellation");
    let connection = pool.get().expect("database connection");
    connection
        .execute("DELETE FROM llm_job_cancellations", [])
        .expect("acknowledge cancellations");
    connection
        .execute("DELETE FROM llm_cancellation_scopes", [])
        .expect("acknowledge cancellation scope");
    let cancelled_run_id: i64 = connection
        .query_row("SELECT id FROM media_similarity_runs", [], |row| row.get(0))
        .expect("cancelled deduplicate run");
    connection
        .execute(
            "UPDATE llm_jobs SET status = 'failed', last_error = 'inference failed' WHERE task = 'image_clustering'",
            [],
        )
        .expect("failed clustering job");
    connection
        .execute(
            "INSERT INTO media_similarity_generations (run_id, status, published_at) VALUES (?, 'active', datetime('now'))",
            [cancelled_run_id],
        )
        .expect("active similarity generation");
    let generation_id = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO media_similarity_generation_state (singleton, active_generation_id) VALUES (1, ?)",
            [generation_id],
        )
        .expect("active similarity generation state");
    connection
        .execute(
            "INSERT INTO media_similarity_finalizations (run_id, generation_id, phase) VALUES (?, ?, 'cleanup')",
            [cancelled_run_id, generation_id],
        )
        .expect("similarity finalization");
    connection
        .execute(
            "INSERT INTO media_similarity_finalization_dirty (run_id, media_id, marked_at) VALUES (?, ?, datetime('now'))",
            [cancelled_run_id, media_id],
        )
        .expect("similarity finalization dirty row");
    connection
        .execute(
            "INSERT INTO media_similarity_labels (run_id, kind, media_id, component_label) VALUES (?, 'near_duplicate', ?, ?)",
            [cancelled_run_id, media_id, media_id],
        )
        .expect("similarity label");
    connection
        .execute(
            "INSERT INTO media_similarity_clusters (generation_id, kind, representative_media_id) VALUES (?, 'near_duplicate', ?)",
            [generation_id, media_id],
        )
        .expect("similarity cluster");
    let cluster_id = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO media_similarity_cluster_members (cluster_id, media_id, cosine_similarity, perceptual_hash_distance) VALUES (?, ?, 1.0, 0)",
            [cluster_id, media_id],
        )
        .expect("similarity cluster member");
    drop(connection);

    let clean = server
        .post("/api/v1/ai/deduplicate/clean")
        .add_header(AUTHORIZATION, authorization.clone())
        .await;
    clean.assert_status_ok();
    assert_eq!(
        action_affected_jobs(&clean.json::<Value>(), "deduplicate"),
        1
    );
    let connection = pool.get().expect("database connection");
    for table in [
        "llm_jobs",
        "media_similarity_cluster_members",
        "media_similarity_clusters",
        "media_similarity_finalization_dirty",
        "media_similarity_labels",
        "media_similarity_finalizations",
        "media_similarity_generation_state",
        "media_similarity_generations",
        "media_similarity_runs",
    ] {
        let count: i64 = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("deduplicate table count");
        assert_eq!(count, 0, "{table} should be empty");
    }
    let dirty_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM media_similarity_dirty", [], |row| {
            row.get(0)
        })
        .expect("dirty media count");
    assert_eq!(dirty_count, 1);
    drop(connection);

    let restart = server
        .post("/api/v1/ai/deduplicate/start")
        .add_header(AUTHORIZATION, authorization)
        .await;
    restart.assert_status_ok();
    assert_eq!(
        action_affected_jobs(&restart.json::<Value>(), "deduplicate"),
        1
    );
}
