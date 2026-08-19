use crate::test_utils::{create_test_db, create_test_media};
use momento_api::database::init_database;

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

    for removed_table in ["image_text", "media_similarity_failures", "schema_version"] {
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
fn rerunning_schema_recreates_missing_table_and_index() {
    let pool = create_test_db();
    let connection = pool.get().expect("Failed to get database connection");
    connection
        .execute_batch("DROP INDEX idx_llm_jobs_claim; DROP TABLE llm_jobs;")
        .expect("Failed to remove LLM job schema objects");

    init_database(&connection).expect("Schema should be safe to rerun");
    init_database(&connection).expect("Repeated schema initialization should succeed");

    for (object_type, object_name) in [("table", "llm_jobs"), ("index", "idx_llm_jobs_claim")] {
        let exists: i64 = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = ? AND name = ?)",
                [object_type, object_name],
                |row| row.get(0),
            )
            .expect("Failed to inspect recreated schema object");
        assert_eq!(exists, 1, "{object_type} {object_name} should be recreated");
    }
}
