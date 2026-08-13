use crate::test_utils::{create_test_app, create_test_media};
use momento_api::constants::{IMAGE_TAGGING_MODEL_TYPE, OCR_MODEL_TYPE};
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
}
