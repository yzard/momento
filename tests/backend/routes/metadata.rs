use crate::test_utils::{create_test_app, create_test_media, create_test_user};
use axum::http::header::AUTHORIZATION;
use axum_test::TestServer;
use momento_api::auth::create_access_token;
use momento_api::config::Config;
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

#[test]
fn metadata_reset_clears_durable_ai_input_records() {
    let (_application, pool) = create_test_app();
    let media_id = create_test_user(&pool, "metadata-reset", "metadata-reset@example.com");
    let connection = pool.get().expect("connection");
    connection.execute("INSERT INTO media (user_id, filename, original_filename, file_path, media_type, import_state, import_source) VALUES (?, 'test.jpg', 'test.jpg', 'test.jpg', 'image', 'imported', 'local')", [media_id]).expect("media");
    let imported_media_id = connection.last_insert_rowid();
    connection.execute("INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, 'ocr', 0, 'image', 'ai/test.jpg', 'test.jpg', 'image/jpeg', 1, 'hash')", [imported_media_id]).expect("input");
    drop(connection);
    momento_api::processor::metadata_worker::reset_all(&pool).expect("reset");
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

#[test]
fn metadata_reset_removes_ai_jobs_results_and_similarity_index() {
    let (_application, pool) = create_test_app();
    let media_id = create_test_media(&pool, "metadata-reset-derived.jpg");
    let connection = pool.get().expect("connection");
    connection.execute("INSERT INTO llm_jobs (id, media_id, task, status) VALUES ('reset-job', ?, 'ocr', 'completed')", [media_id]).expect("job");
    connection.execute("INSERT INTO media_text (media_id, model_type, model_version, string) VALUES (?, 'ocr', 'test', 'text')", [media_id]).expect("text");
    connection.execute("INSERT INTO media_text_inputs (media_id, model_type, sequence, model_version, string) VALUES (?, 'ocr', 0, 'test', 'frame text')", [media_id]).expect("input text");
    connection.execute("INSERT INTO media_similarity_index (media_id, content_hash, model_version, preprocessing_version, embedding, perceptual_hash, processing_status) VALUES (?, 'hash', 'test', 'test', X'00000000', 0, 1)", [media_id]).expect("index");
    drop(connection);
    momento_api::processor::metadata_worker::reset_all(&pool).expect("reset");
    let connection = pool.get().expect("connection");
    for table in [
        "llm_jobs",
        "media_text",
        "media_text_inputs",
        "media_similarity_index",
    ] {
        let count: i64 = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("count");
        assert_eq!(count, 0, "{table} should be empty");
    }
}

#[test]
fn ocr_queueing_does_not_depend_on_image_tagging_configuration() {
    let (_application, pool) = create_test_app();
    let media_id = create_test_media(&pool, "ocr-input.jpg");
    let connection = pool.get().expect("connection");
    connection
        .execute(
            "INSERT INTO media_metadata_jobs (media_id, status) VALUES (?, 'completed')",
            [media_id],
        )
        .expect("metadata job");
    connection.execute("INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, 'ocr', 0, 'image', 'ai/input.jpg', 'input.jpg', 'image/jpeg', 1, 'hash')", [media_id]).expect("input");
    drop(connection);
    assert_eq!(ai::queue_task(&pool, "ocr", false).expect("queue ocr"), 1);
}
