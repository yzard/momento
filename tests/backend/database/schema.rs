use crate::test_utils::{create_test_db, create_test_media};
use momento_api::database::queries;

#[test]
fn fresh_schema_records_the_source_owned_database_identity() {
    let pool = create_test_db();
    let connection = pool.get().expect("database connection");
    let application_id: i64 = connection
        .pragma_query_value(None, "application_id", |row| row.get(0))
        .expect("application ID");
    let schema_version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("schema version");

    assert_eq!(application_id, 0x4d4f_4d4f);
    assert_eq!(schema_version, 1);
}

#[test]
fn creates_active_media_access_index() {
    let pool = create_test_db();
    let connection = pool.get().expect("Failed to get database connection");
    let index_name: String = connection
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'index' AND name = ?",
            ["idx_media_access_user_active"],
            |row| row.get(0),
        )
        .expect("Failed to find active media access index");

    assert_eq!(index_name, "idx_media_access_user_active");
}

#[test]
fn creates_current_schema_without_removed_tables() {
    let pool = create_test_db();
    let connection = pool.get().expect("Failed to get database connection");

    for removed_table in [
        "image_text",
        "llm_job_results",
        "media_similarity_failures",
        "schema_version",
    ] {
        let exists: i64 = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?)",
                [removed_table],
                |row| row.get(0),
            )
            .expect("Failed to inspect database schema");
        assert_eq!(exists, 0, "removed table {removed_table} should not exist");
    }
}

#[test]
fn creates_lossless_backup_manifest_table() {
    let pool = create_test_db();
    let connection = pool.get().expect("database connection");
    let table_name: String = connection
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?",
            ["backup_asset_manifests"],
            |row| row.get(0),
        )
        .expect("lossless backup manifest table");

    assert_eq!(table_name, "backup_asset_manifests");
}

#[test]
fn creates_raw_metadata_source_table() {
    let pool = create_test_db();
    let connection = pool.get().expect("database connection");
    let table_name: String = connection
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?",
            ["media_metadata_sources"],
            |row| row.get(0),
        )
        .expect("raw metadata source table");

    assert_eq!(table_name, "media_metadata_sources");
}

#[test]
fn creates_durable_metadata_and_ai_job_tables() {
    let pool = create_test_db();
    let connection = pool.get().expect("Failed to get database connection");

    let table_name: String = connection
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?",
            ["llm_jobs"],
            |row| row.get(0),
        )
        .expect("Failed to find LLM jobs table");
    let metadata_table: String = connection
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?",
            ["media_metadata_jobs"],
            |row| row.get(0),
        )
        .expect("Failed to find metadata jobs table");
    let cancellation_table: String = connection
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?",
            ["llm_job_cancellations"],
            |row| row.get(0),
        )
        .expect("Failed to find LLM cancellation table");
    let cancellation_scope_table: String = connection
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?",
            ["llm_cancellation_scopes"],
            |row| row.get(0),
        )
        .expect("Failed to find LLM cancellation scope table");
    let aesthetics_table: String = connection
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?",
            ["media_aesthetics"],
            |row| row.get(0),
        )
        .expect("Failed to find media aesthetics table");
    let aesthetic_inputs_table: String = connection
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?",
            ["media_aesthetic_inputs"],
            |row| row.get(0),
        )
        .expect("Failed to find media aesthetic inputs table");

    assert_eq!(table_name, "llm_jobs");
    assert_eq!(metadata_table, "media_metadata_jobs");
    assert_eq!(cancellation_table, "llm_job_cancellations");
    assert_eq!(cancellation_scope_table, "llm_cancellation_scopes");
    assert_eq!(aesthetics_table, "media_aesthetics");
    assert_eq!(aesthetic_inputs_table, "media_aesthetic_inputs");
    for classifier_table in [
        "media_screenshot_classifications",
        "media_screenshot_classification_inputs",
        "media_document_classifications",
        "media_document_classification_inputs",
    ] {
        let exists: i64 = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?)",
                [classifier_table],
                |row| row.get(0),
            )
            .expect("Failed to inspect classifier table");
        assert_eq!(exists, 1, "{classifier_table} should exist");
    }
}

#[test]
fn llm_job_state_transitions_advance_the_fencing_version() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "llm-state-version.jpg");
    let connection = pool.get().expect("database connection");
    connection
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status) VALUES ('versioned-job', ?, 'ocr', 'queued')",
            [media_id],
        )
        .expect("queued job");
    let initial: i64 = connection
        .query_row(
            "SELECT state_version FROM llm_jobs WHERE id = 'versioned-job'",
            [],
            |row| row.get(0),
        )
        .expect("initial version");
    assert_eq!(initial, 1);

    assert_eq!(
        connection
            .execute(queries::ai_jobs::CLAIM, ["versioned-job"])
            .expect("claim job"),
        1
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT state_version FROM llm_jobs WHERE id = 'versioned-job'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("claimed version"),
        2
    );

    assert_eq!(
        connection
            .execute(queries::ai_jobs::CANCEL_FOR_TASK, ["ocr"])
            .expect("cancel job"),
        1
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT state_version FROM llm_jobs WHERE id = 'versioned-job'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("cancelled version"),
        3
    );
}

#[test]
fn creates_bounded_generic_file_operation_journal_schema() {
    let pool = create_test_db();
    let connection = pool.get().expect("database connection");

    for table in [
        "file_operation_groups",
        "file_operation_entries",
        "file_operation_path_claims",
        "data_dir_space_reservations",
        "file_operation_retry_requests",
    ] {
        let exists: i64 = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?)",
                [table],
                |row| row.get(0),
            )
            .expect("journal table lookup");
        assert_eq!(exists, 1, "{table} should exist");
    }
    connection
        .execute(
            "INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, state, entry_count) VALUES ('group-1', 'test', 'test', 'owner-1', 'prepared', 1)",
            [],
        )
        .expect("journal group");
    connection
        .execute(
            "INSERT INTO file_operation_entries (group_id, sequence, action, storage_root, temporary_path, destination_path) VALUES ('group-1', 0, 'publish', 'journal', 'staging/a', 'final/a')",
            [],
        )
        .expect("journal entry");
    connection
        .execute(
            "INSERT INTO file_operation_path_claims (group_id, sequence, storage_root, relative_path, path_key, mode, scope, role) VALUES ('group-1', 0, 'originals', 'a', X'000161', 'write', 'exact', 'destination')",
            [],
        )
        .expect("journal path claim");
    assert!(connection
        .execute(
            "INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, state, entry_count) VALUES ('too-large', 'test', 'test', 'owner-2', 'prepared', 257)",
            [],
        )
        .is_err());
}

#[test]
fn creates_bounded_token_fenced_llm_result_receipt_schema() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "result-receipt-schema.jpg");
    let connection = pool.get().expect("database connection");
    connection
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES ('receipt-job', ?, 'ocr', 'submitted', 1)",
            [media_id],
        )
        .expect("submitted job");
    connection
        .execute(
            "INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, state, product_target, product_version, entry_count) VALUES ('receipt-group', 'llm_result_receive', 'llm_result', 'receipt-job', 'prepared', 'llm_result_inbox', 1, 1)",
            [],
        )
        .expect("result receive group");
    connection
        .execute_batch(
            "INSERT INTO data_dir_space_reservations (id, class, owner_kind, owner_id, filesystem_id, reserved_peak_additional_bytes, state) VALUES ('receipt-sqlite', 'sqlite', 'llm_result', 'receipt-job', 'test', 4096, 'active');",
        )
        .expect("result SQLite reservation");
    connection
        .execute(
            "INSERT INTO llm_result_receipts (job_id, attempt, job_version, media_id, task, result_status, model_type, model_version, encoding, record_count, byte_size, content_hash, journal_group_id, sqlite_reservation_id, inbox_path, receive_token, state, result_product_version) VALUES ('receipt-job', 1, 1, ?, 'ocr', 'completed', 'ocr', 'test', 'momento-result-records-v1', 3, 72, ?, 'receipt-group', 'receipt-sqlite', 'llm-results/receipt-job/1.bin', '00000000-0000-0000-0000-000000000001', 'receiving', 1)",
            rusqlite::params![media_id, "0".repeat(64)],
        )
        .expect("result receipt");
    connection
        .execute(
            "INSERT INTO llm_result_staging (job_id, attempt, record_sequence, input_sequence, kind, byte_offset, encoded_size, normalized_payload) VALUES ('receipt-job', 1, 0, 0, 'ocr_text_continuation', 0, 24, X'')",
            [],
        )
        .expect("bounded staging record");

    assert!(connection
        .execute(
            "INSERT INTO llm_result_staging (job_id, attempt, record_sequence, input_sequence, kind, byte_offset, encoded_size, normalized_payload) VALUES ('receipt-job', 1, 1, 0, 'ocr_text', 24, 1048600, zeroblob(1048577))",
            [],
        )
        .is_err());
    assert!(connection
        .execute(
            "INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, state, product_target, product_version, entry_count) VALUES ('failed-receipt-group', 'llm_result_receive', 'llm_result', 'failed-receipt-job', 'prepared', 'llm_result_inbox', 1, 1)",
            [],
        )
        .is_ok());
    connection
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES ('failed-receipt-job', ?, 'image_tagging', 'submitted', 1)",
            [media_id],
        )
        .expect("second submitted job");
    connection
        .execute_batch(
            "INSERT INTO data_dir_space_reservations (id, class, owner_kind, owner_id, filesystem_id, reserved_peak_additional_bytes, state) VALUES ('failed-receipt-sqlite', 'sqlite', 'llm_result', 'failed-receipt-job', 'test', 4096, 'active');",
        )
        .expect("failed result SQLite reservation");
    assert!(connection
        .execute(
            "INSERT INTO llm_result_receipts (job_id, attempt, job_version, media_id, task, result_status, encoding, record_count, byte_size, content_hash, journal_group_id, sqlite_reservation_id, inbox_path, receive_token, state, result_product_version) VALUES ('failed-receipt-job', 1, 1, ?, 'image_tagging', 'completed', 'momento-result-records-v1', 1, 24, ?, 'failed-receipt-group', 'failed-receipt-sqlite', 'llm-results/failed-receipt-job/1.bin', '00000000-0000-0000-0000-000000000002', 'receiving', 1)",
            rusqlite::params![media_id, "0".repeat(64)],
        )
        .is_err());
}

#[test]
fn ai_input_tables_record_the_momento_storage_root() {
    let pool = create_test_db();
    let connection = pool.get().expect("database connection");

    for table in ["media_ai_inputs", "llm_job_inputs"] {
        let storage_root_column: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info(?) WHERE name = 'storage_root' AND \"notnull\" = 1",
                [table],
                |row| row.get(0),
            )
            .expect("storage_root column");
        assert_eq!(storage_root_column, 1, "{table} storage root");
    }
}

#[test]
fn backup_schema_enforces_device_ownership_and_statuses() {
    let pool = create_test_db();
    let user_id =
        crate::test_utils::create_test_user(&pool, "backup-schema", "backup-schema@example.com");
    let connection = pool.get().expect("database connection");

    assert!(connection
        .execute(
            "INSERT INTO backup_assets (user_id, device_id, client_asset_id, operation_id, original_filename, mime_type, byte_size, source_modified_at, status, staged_path) VALUES (?, 'unknown', 'asset', 'operation', 'photo.jpg', 'image/jpeg', 1, '2024-01-01T00:00:00Z', 'uploading', 'asset.part')",
            [user_id],
        )
        .is_err());
    connection
        .execute(
            "INSERT INTO backup_devices (user_id, device_id, device_name) VALUES (?, 'known', 'Known device')",
            [user_id],
        )
        .expect("backup device");
    assert!(connection
        .execute(
            "INSERT INTO backup_assets (user_id, device_id, client_asset_id, operation_id, original_filename, mime_type, byte_size, source_modified_at, status, staged_path) VALUES (?, 'known', 'asset', 'operation', 'photo.jpg', 'image/jpeg', 1, '2024-01-01T00:00:00Z', 'writing', 'asset.part')",
            [user_id],
        )
        .is_err());
}

#[test]
fn classifier_tables_enforce_boolean_and_confidence_ranges() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "classifier-constraints.jpg");
    let connection = pool.get().expect("database connection");

    assert!(connection
        .execute(
            "INSERT INTO media_screenshot_classifications (media_id, model_type, model_version, is_screenshot, confidence) VALUES (?, 'screenshot_detection', 'test', 2, 0.5)",
            [media_id],
        )
        .is_err());
    assert!(connection
        .execute(
            "INSERT INTO media_document_classifications (media_id, model_type, model_version, is_document, confidence) VALUES (?, 'document_detection', 'test', 1, 1.1)",
            [media_id],
        )
        .is_err());
}

#[test]
fn face_schema_stores_independent_bounded_quality_scores() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "face-score-constraints.jpg");
    let connection = pool.get().expect("database connection");

    for invalid_scores in [
        (1.1_f64, 1.0_f64, 1.0_f64, 1.0_f64, 1.0_f64),
        (1.0, -0.1, 1.0, 1.0, 1.0),
        (1.0, 1.0, 1.1, 1.0, 1.0),
        (1.0, 1.0, 1.0, -0.1, 1.0),
        (1.0, 1.0, 1.0, 1.0, 1.1),
    ] {
        assert!(connection
            .execute(
                queries::faces::INSERT_FACE,
                rusqlite::params![
                    media_id,
                    0,
                    0,
                    0.0,
                    0.0,
                    1.0,
                    1.0,
                    invalid_scores.0,
                    invalid_scores.1,
                    invalid_scores.2,
                    invalid_scores.3,
                    invalid_scores.4,
                    [0_u8; 4],
                    "faces/invalid.jpg"
                ],
            )
            .is_err());
    }
}
