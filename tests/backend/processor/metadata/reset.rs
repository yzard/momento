use crate::test_utils::{create_test_app, create_test_media, create_test_user};
use momento_api::constants::{
    DOCUMENT_DETECTION_MODEL_TYPE, IMAGE_TAGGING_MODEL_TYPE, OCR_MODEL_TYPE,
    SCREENSHOT_DETECTION_MODEL_TYPE,
};
use momento_api::database::operations::{
    reset_metadata_page, ResetMetadataOutcome, ResetMetadataStepOutcome,
};
use momento_api::database::queries;
use momento_api::processor::ai::operation::AiFeature;
use rusqlite::params;

#[tokio::test]
async fn clearing_metadata_also_clears_llm_text_models() {
    let (_app, pool) = create_test_app();
    let media_id = create_test_media(&pool, "metadata.jpg");
    let conn = pool.get().expect("Failed to get database connection");
    for model_type in [OCR_MODEL_TYPE, IMAGE_TAGGING_MODEL_TYPE] {
        conn.execute(
            queries::media_text::INSERT,
            params![media_id, model_type, "test-version", "generated text"],
        )
        .expect("Failed to insert LLM text");
    }
    conn.execute("INSERT INTO media_screenshot_classifications (media_id, model_type, model_version, is_screenshot, confidence) VALUES (?, 'screenshot_detection', 'test', 1, 0.9)", [media_id]).expect("screenshot result");
    conn.execute("INSERT INTO media_screenshot_classification_inputs (media_id, sequence, model_type, model_version, is_screenshot, confidence) VALUES (?, 0, 'screenshot_detection', 'test', 1, 0.9)", [media_id]).expect("screenshot input result");
    conn.execute("INSERT INTO media_document_classifications (media_id, model_type, model_version, is_document, confidence) VALUES (?, 'document_detection', 'test', 1, 0.8)", [media_id]).expect("document result");
    conn.execute("INSERT INTO media_document_classification_inputs (media_id, sequence, model_type, model_version, is_document, confidence) VALUES (?, 0, 'document_detection', 'test', 1, 0.8)", [media_id]).expect("document input result");
    conn.execute(
        "INSERT INTO media_metadata_sources (media_id, source_type, schema_version, payload_json) VALUES (?, 'exiftool', 1, '{}')",
        [media_id],
    )
    .expect("raw metadata source");
    drop(conn);

    let outcome = crate::test_utils::test_executor_handles(pool.clone())
        .sqlite
        .reset_metadata_request("metadata-reset-models".to_string())
        .await
        .expect("metadata reset should succeed");

    assert_eq!(outcome, ResetMetadataOutcome::Reset { media_count: 1 });
    let conn = pool.get().expect("Failed to get database connection");
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM media_text WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("Failed to query cleared LLM text");
    assert_eq!(count, 0);
    let source_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM media_metadata_sources WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("raw metadata source count");
    assert_eq!(source_count, 0);
    for (table, task) in [
        (
            "media_screenshot_classifications",
            SCREENSHOT_DETECTION_MODEL_TYPE,
        ),
        (
            "media_screenshot_classification_inputs",
            SCREENSHOT_DETECTION_MODEL_TYPE,
        ),
        (
            "media_document_classifications",
            DOCUMENT_DETECTION_MODEL_TYPE,
        ),
        (
            "media_document_classification_inputs",
            DOCUMENT_DETECTION_MODEL_TYPE,
        ),
    ] {
        let count: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE media_id = ?"),
                [media_id],
                |row| row.get(0),
            )
            .expect("classifier result count");
        assert_eq!(count, 0, "{task} rows should be cleared");
    }
}

#[tokio::test]
async fn metadata_reset_pages_large_tables_and_resumes_the_existing_operation() {
    let (_app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "metadata-reset-page", "reset-page@example.com");
    let connection = pool.get().expect("database connection");
    for index in 0..300 {
        connection
            .execute(
                "INSERT INTO media (user_id, filename, original_filename, file_path, media_type, import_state, import_source) VALUES (?, ?, ?, ?, 'image', 'imported', 'local')",
                params![user_id, format!("reset-{index}.jpg"), format!("reset-{index}.jpg"), format!("reset-{index}.jpg")],
            )
            .expect("media");
        let media_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO media_metadata_sources (media_id, source_type, schema_version, payload_json) VALUES (?, 'exiftool', 1, '{}')",
                [media_id],
            )
            .expect("metadata source");
    }
    drop(connection);

    let mut connection = pool.get().expect("database connection");
    assert_eq!(
        reset_metadata_page(&mut connection, Some("metadata-reset-resume"))
            .expect("initialize reset"),
        ResetMetadataStepOutcome::Progressed
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT state FROM file_operation_groups WHERE id = 'metadata-reset-resume'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("journal group"),
        "prepared"
    );
    drop(connection);

    let executors = crate::test_utils::test_executor_handles(pool.clone());
    for feature in [AiFeature::FaceDetection, AiFeature::Deduplicate] {
        assert_eq!(
            executors
                .sqlite
                .start_ai_feature_request(feature, "manual".to_string(), None)
                .await
                .expect("AI start while reset is active"),
            0
        );
    }
    let mut connection = pool.get().expect("database connection");
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM face_grouping_runs", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("face run count"),
        0
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM media_similarity_runs", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("deduplicate run count"),
        0
    );

    while connection
        .query_row(
            "SELECT phase FROM metadata_reset_operations WHERE id = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("reset phase")
        != "metadata_sources"
    {
        assert_eq!(
            reset_metadata_page(&mut connection, None).expect("advance reset"),
            ResetMetadataStepOutcome::Progressed
        );
    }
    assert_eq!(
        reset_metadata_page(&mut connection, None).expect("delete first source page"),
        ResetMetadataStepOutcome::Progressed
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM media_metadata_sources", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("remaining source rows"),
        44
    );
    drop(connection);

    let outcome = crate::test_utils::test_executor_handles(pool.clone())
        .sqlite
        .reset_metadata_request("unused-new-reset-id".to_string())
        .await
        .expect("resume reset");
    assert_eq!(outcome, ResetMetadataOutcome::Reset { media_count: 300 });

    let connection = pool.get().expect("database connection");
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM metadata_reset_operations",
                [],
                |row| { row.get::<_, i64>(0) }
            )
            .expect("reset state count"),
        0
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT state FROM file_operation_groups WHERE id = 'metadata-reset-resume'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("journal state"),
        "cleanup_pending"
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM file_operation_groups WHERE id = 'unused-new-reset-id'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("replacement group count"),
        0
    );
}

#[tokio::test]
async fn metadata_reset_retires_result_journals_before_removing_receipts() {
    let (_app, pool) = create_test_app();
    let media_id = create_test_media(&pool, "metadata-reset-result.jpg");
    let connection = pool.get().expect("database connection");
    connection
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES ('metadata-reset-result-job', ?, 'ocr', 'submitted', 1)",
            [media_id],
        )
        .expect("submitted job");
    connection
        .execute(
            "INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, state, product_target, product_version, completion_outcome, entry_count) VALUES ('metadata-reset-result-group', 'llm_result_receive', 'llm_result', 'metadata-reset-result-job', 'completed', 'llm_result_inbox', 1, 'published', 1)",
            [],
        )
        .expect("result group");
    connection
        .execute(
            "INSERT INTO file_operation_entries (group_id, sequence, action, storage_root, temporary_path, destination_path, expected_size, state) VALUES ('metadata-reset-result-group', 0, 'publish', 'journal', 'llm-results/result.tmp', 'llm-results/result.records', 24, 'committed')",
            [],
        )
        .expect("result entry");
    connection
        .execute_batch(
            "INSERT INTO data_dir_space_reservations (id, class, owner_kind, owner_id, journal_group_id, filesystem_id, reserved_peak_additional_bytes, state) VALUES ('metadata-reset-result-journal-space', 'journal', 'llm_result', 'metadata-reset-result-job', 'metadata-reset-result-group', 'test', 4096, 'active');
             INSERT INTO data_dir_space_reservations (id, class, owner_kind, owner_id, filesystem_id, reserved_peak_additional_bytes, state) VALUES ('metadata-reset-result-sqlite-space', 'sqlite', 'llm_result', 'metadata-reset-result-job', 'test', 4096, 'active');",
        )
        .expect("result reservations");
    connection
        .execute(
            "INSERT INTO llm_result_receipts (job_id, attempt, job_version, media_id, task, result_status, model_type, model_version, encoding, record_count, byte_size, content_hash, journal_group_id, sqlite_reservation_id, inbox_path, receive_token, state, result_product_version, received_at) VALUES ('metadata-reset-result-job', 1, 1, ?, 'ocr', 'completed', 'ocr', 'test', 'momento-result-records-v1', 1, 24, ?, 'metadata-reset-result-group', 'metadata-reset-result-sqlite-space', 'llm-results/result.records', '00000000-0000-0000-0000-000000000006', 'received', 1, datetime('now'))",
            params![media_id, "0".repeat(64)],
        )
        .expect("result receipt");
    drop(connection);

    let outcome = crate::test_utils::test_executor_handles(pool.clone())
        .sqlite
        .reset_metadata_request("metadata-reset-result-cleanup".to_string())
        .await
        .expect("metadata reset");
    assert_eq!(outcome, ResetMetadataOutcome::Reset { media_count: 1 });

    let connection = pool.get().expect("database connection");
    let (state, target, outcome): (String, Option<String>, Option<String>) = connection
        .query_row(
            "SELECT state, product_target, completion_outcome FROM file_operation_groups WHERE id = 'metadata-reset-result-group'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("retired result group");
    assert_eq!(state, "cleaned");
    assert_eq!(target, None);
    assert_eq!(outcome.as_deref(), Some("discarded"));
    assert_eq!(
        connection
            .query_row(
                "SELECT cleanup_state FROM file_operation_entries WHERE group_id = 'metadata-reset-result-group'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("retired result entry"),
        "cleaned"
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM llm_result_receipts", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("receipt count"),
        0
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM data_dir_space_reservations WHERE owner_id = 'metadata-reset-result-job' AND state != 'released'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("active reservation count"),
        0
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM file_operation_entries WHERE group_id = 'metadata-reset-result-cleanup' AND storage_root = 'journal' AND source_path = 'llm-results'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("result inbox cleanup entry"),
        1
    );
}
