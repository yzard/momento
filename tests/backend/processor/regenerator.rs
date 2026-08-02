use crate::test_utils::{create_test_app, create_test_media};
use momento_api::constants::{OBJECT_DETECTION_PLUGIN_ID, OCR_PLUGIN_ID};
use momento_api::database::queries;
use rusqlite::params;

#[test]
fn clearing_metadata_also_clears_llm_text_plugins() {
    let (_app, pool) = create_test_app();
    let media_id = create_test_media(&pool, "metadata.jpg");
    let conn = pool.get().expect("Failed to get database connection");
    for plugin_id in [OCR_PLUGIN_ID, OBJECT_DETECTION_PLUGIN_ID] {
        conn.execute(
            queries::image_text::INSERT,
            params![media_id, plugin_id, "generated text"],
        )
        .expect("Failed to insert LLM text");
    }
    drop(conn);

    let cleared = momento_api::processor::regenerator::clear_all_metadata_and_thumbnails(&pool);

    assert_eq!(cleared, 1);
    let conn = pool.get().expect("Failed to get database connection");
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM image_text WHERE image_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("Failed to query cleared LLM text");
    assert_eq!(count, 0);
}
