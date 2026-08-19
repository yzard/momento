use crate::test_utils::{create_test_app, create_test_media};
use momento_api::constants::{
    DOCUMENT_DETECTION_MODEL_TYPE, IMAGE_TAGGING_MODEL_TYPE, OCR_MODEL_TYPE,
    SCREENSHOT_DETECTION_MODEL_TYPE,
};
use momento_api::database::queries;
use rusqlite::params;

#[test]
fn clearing_metadata_also_clears_llm_text_models() {
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
    drop(conn);

    let cleared = momento_api::processor::metadata_worker::reset_all(&pool)
        .expect("metadata reset should succeed");

    assert_eq!(cleared, 1);
    let conn = pool.get().expect("Failed to get database connection");
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM media_text WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("Failed to query cleared LLM text");
    assert_eq!(count, 0);
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
