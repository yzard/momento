use crate::test_utils::{create_test_app, create_test_media, create_test_user};
use axum::http::header::AUTHORIZATION;
use axum_test::TestServer;
use momento_api::auth::create_access_token;
use momento_api::config::Config;
use momento_api::database::operations::ResetMetadataOutcome;
use momento_api::processor::ai;

#[tokio::test]
async fn metadata_generate_requires_administrator() {
    let (application, pool) = create_test_app();
    let user_id = create_test_user(&pool, "metadata-user", "metadata-user@example.com");
    let token = create_access_token(user_id, "metadata-user", "user", &Config::default(), None)
        .expect("token");
    let server = TestServer::new(application).expect("server");
    server
        .post("/api/v1/metadata/generate")
        .add_header(AUTHORIZATION, format!("Bearer {token}"))
        .json(&serde_json::json!({}))
        .await
        .assert_status_forbidden();
}

#[tokio::test]
async fn metadata_status_returns_complete_failure_diagnostics() {
    let (application, pool) = create_test_app();
    let administrator_id = create_test_user(
        &pool,
        "metadata-status-admin",
        "metadata-status-admin@example.com",
    );
    let media_id = create_test_media(&pool, "broken-metadata.heic");
    let diagnostic = "exiftool could not read metadata from /data/originals/broken-metadata.heic: exiftool exited with exit status: 1; stderr: File format error";
    let connection = pool.get().expect("database connection");
    connection
        .execute(
            "UPDATE users SET role = 'admin' WHERE id = ?",
            [administrator_id],
        )
        .expect("administrator role");
    connection
        .execute(
            "INSERT INTO media_metadata_jobs (media_id, status, last_error) VALUES (?, 'failed', ?)",
            rusqlite::params![media_id, diagnostic],
        )
        .expect("failed metadata job");
    drop(connection);
    let token = create_access_token(
        administrator_id,
        "metadata-status-admin",
        "admin",
        &Config::default(),
        None,
    )
    .expect("token");
    let server = TestServer::new(application).expect("server");

    let response = server
        .post("/api/v1/metadata/status")
        .add_header(AUTHORIZATION, format!("Bearer {token}"))
        .json(&serde_json::json!({}))
        .await;

    response.assert_status_ok();
    let status = response.json::<serde_json::Value>();
    assert_eq!(status["errors"], serde_json::json!([diagnostic]));
}

#[tokio::test]
async fn metadata_reset_clears_durable_ai_input_records() {
    let (_application, pool) = create_test_app();
    let media_id = create_test_user(&pool, "metadata-reset", "metadata-reset@example.com");
    let connection = pool.get().expect("connection");
    connection.execute("INSERT INTO media (user_id, filename, original_filename, file_path, media_type, import_state, import_source) VALUES (?, 'test.jpg', 'test.jpg', 'test.jpg', 'image', 'imported', 'local')", [media_id]).expect("media");
    let imported_media_id = connection.last_insert_rowid();
    connection.execute("INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, 'ocr', 0, 'image', 'previews', 'ai/test.jpg', 'test.jpg', 'image/jpeg', 1, 'hash')", [imported_media_id]).expect("input");
    drop(connection);
    let outcome = crate::test_utils::test_executor_handles(pool.clone())
        .sqlite
        .reset_metadata_request("metadata-reset-inputs".to_string())
        .await
        .expect("reset");
    assert_eq!(outcome, ResetMetadataOutcome::Reset { media_count: 1 });
    let connection = pool.get().expect("connection");
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_ai_inputs WHERE media_id = ?",
            [imported_media_id],
            |row| row.get(0),
        )
        .expect("count");
    assert_eq!(count, 0);
}

#[tokio::test]
async fn metadata_reset_removes_ai_jobs_results_and_similarity_index() {
    let (_application, pool) = create_test_app();
    let media_id = create_test_media(&pool, "metadata-reset-derived.jpg");
    let connection = pool.get().expect("connection");
    connection.execute("INSERT INTO llm_jobs (id, media_id, task, status) VALUES ('reset-job', ?, 'ocr', 'completed')", [media_id]).expect("job");
    connection.execute("INSERT INTO media_text (media_id, model_type, model_version, string) VALUES (?, 'ocr', 'test', 'text')", [media_id]).expect("text");
    connection.execute("INSERT INTO media_text_inputs (media_id, model_type, sequence, model_version, string) VALUES (?, 'ocr', 0, 'test', 'frame text')", [media_id]).expect("input text");
    connection.execute("INSERT INTO media_similarity_index (media_id, content_hash, model_version, preprocessing_version, embedding, perceptual_hash, processing_status) VALUES (?, 'hash', 'test', 'test', X'00000000', 0, 1)", [media_id]).expect("index");
    connection.execute("INSERT INTO media_aesthetics (media_id, model_type, model_version, aesthetic_score, scenic_score, simplicity_score, landscape_score, technical_quality_score) VALUES (?, 'image_aesthetics', 'test', 0.5, 0.5, 0.5, 0.5, 0.5)", [media_id]).expect("aesthetics");
    drop(connection);
    let outcome = crate::test_utils::test_executor_handles(pool.clone())
        .sqlite
        .reset_metadata_request("metadata-reset-derived".to_string())
        .await
        .expect("reset");
    assert_eq!(outcome, ResetMetadataOutcome::Reset { media_count: 1 });
    let connection = pool.get().expect("connection");
    for table in [
        "llm_jobs",
        "media_text",
        "media_text_inputs",
        "media_similarity_index",
        "media_aesthetics",
    ] {
        let count: i64 = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("count");
        assert_eq!(count, 0, "{table} should be empty");
    }
}

#[tokio::test]
async fn metadata_reset_commits_derived_tree_cleanup_to_the_generic_journal() {
    let (_application, pool) = create_test_app();
    let (executors, data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let derived_files = [
        data_directory.join("thumbnails/media/1/thumbnail.jpg"),
        data_directory.join("thumbnails_tiny/media/1/thumbnail.jpg"),
        data_directory.join("thumbnails_places/media/1/thumbnail.jpg"),
        data_directory.join("previews/faces/1/face.jpg"),
        data_directory.join("previews/ai/1/frame.png"),
        data_directory.join("previews/media/1/preview.jpg"),
    ];
    for path in &derived_files {
        std::fs::create_dir_all(path.parent().expect("derived parent")).expect("derived directory");
        std::fs::write(path, b"derived").expect("derived file");
    }

    assert_eq!(
        executors
            .sqlite
            .reset_metadata_request("metadata-reset-files".to_string())
            .await
            .expect("metadata reset"),
        ResetMetadataOutcome::Reset { media_count: 0 }
    );
    assert_eq!(
        pool.get()
            .expect("database")
            .query_row(
                "SELECT state FROM file_operation_groups WHERE id = 'metadata-reset-files'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("journal state"),
        "cleanup_pending"
    );

    for _ in 0..8 {
        momento_api::io::recovery::recover_generic_file_operations(&executors)
            .await
            .expect("journal recovery");
        if derived_files.iter().all(|path| !path.exists()) {
            break;
        }
    }
    assert!(derived_files.iter().all(|path| !path.exists()));
    assert_eq!(
        pool.get()
            .expect("database")
            .query_row(
                "SELECT state FROM file_operation_groups WHERE id = 'metadata-reset-files'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("journal state"),
        "cleaned"
    );
}

#[tokio::test]
async fn start_all_queues_only_features_with_prepared_work() {
    let (_application, pool) = create_test_app();
    let media_id = create_test_media(&pool, "ocr-input.jpg");
    let connection = pool.get().expect("connection");
    connection
        .execute(
            "INSERT INTO media_metadata_jobs (media_id, status) VALUES (?, 'completed')",
            [media_id],
        )
        .expect("metadata job");
    connection.execute("INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, 'ocr', 0, 'image', 'previews', 'ai/input.jpg', 'input.jpg', 'image/jpeg', 1, 'hash')", [media_id]).expect("input");
    drop(connection);
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    let mut queued = 0;
    for feature in ai::operation::AiFeature::ALL {
        queued += executors
            .sqlite
            .start_ai_feature_request(feature, "manual".to_string(), None)
            .await
            .expect("start feature");
    }
    assert_eq!(queued, 1);
}

#[tokio::test]
async fn image_aesthetics_queueing_uses_its_result_table_and_explicit_eligibility() {
    let (_application, pool) = create_test_app();
    let media_id = create_test_media(&pool, "aesthetics-input.jpg");
    let connection = pool.get().expect("connection");
    connection
        .execute(
            "INSERT INTO media_metadata_jobs (media_id, status) VALUES (?, 'completed')",
            [media_id],
        )
        .expect("metadata job");
    connection.execute("INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, 'image_aesthetics', 0, 'image', 'previews', 'ai/aesthetics.jpg', 'aesthetics.jpg', 'image/jpeg', 1, 'hash')", [media_id]).expect("input");
    drop(connection);

    assert_eq!(
        crate::test_utils::test_executor_handles(pool)
            .sqlite
            .start_ai_feature_request(
                ai::operation::AiFeature::ImageAesthetics,
                "manual".to_string(),
                None,
            )
            .await
            .expect("queue aesthetics"),
        1
    );
}
