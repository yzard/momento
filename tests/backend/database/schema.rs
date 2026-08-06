use crate::test_utils::create_test_db;
use momento_api::database::init_database;
use rusqlite::params;

#[test]
fn migrates_legacy_image_text_plugin_ids_to_model_metadata() {
    let pool = create_test_db();
    let conn = pool.get().expect("Failed to get database connection");
    conn.execute_batch(
        "DROP TRIGGER IF EXISTS delete_image_text_after_media_delete;
         DROP TABLE image_text;
         CREATE VIRTUAL TABLE image_text USING fts5 (
             image_id UNINDEXED,
             plugin_id UNINDEXED,
             string
         );",
    )
    .expect("Failed to create legacy image text table");
    conn.execute(
        "INSERT INTO image_text (image_id, plugin_id, string) VALUES (?, ?, ?)",
        params![7, 1, "ocr text"],
    )
    .expect("Failed to insert legacy image text");
    conn.execute(
        "INSERT INTO image_text (image_id, plugin_id, string) VALUES (?, ?, ?)",
        params![8, 2, "object detection text"],
    )
    .expect("Failed to insert legacy object detection text");

    init_database(&conn).expect("Failed to migrate database schema");

    let metadata: (String, String, String) = conn
        .query_row(
            "SELECT model_type, model_version, string FROM image_text WHERE image_id = ?",
            [7],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("Failed to query migrated image text");
    assert_eq!(
        metadata,
        (
            "ocr".to_string(),
            "legacy".to_string(),
            "ocr text".to_string()
        )
    );
    let object_detection_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM image_text WHERE image_id = ?",
            [8],
            |row| row.get(0),
        )
        .expect("Failed to query removed object detection text");
    assert_eq!(object_detection_count, 0);
}

#[test]
fn removes_object_detection_rows_from_current_image_text_schema() {
    let pool = create_test_db();
    let conn = pool.get().expect("Failed to get database connection");
    conn.execute(
        "INSERT INTO image_text (image_id, model_type, model_version, string)
         VALUES (?, 'object_detection', 'yolo11s.pt', ?)",
        params![7, "person"],
    )
    .expect("Failed to insert object detection text");

    init_database(&conn).expect("Failed to clean database schema");

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM image_text WHERE model_type = 'object_detection'",
            [],
            |row| row.get(0),
        )
        .expect("Failed to query removed object detection text");
    assert_eq!(count, 0);
}

#[test]
fn creates_active_media_access_index() {
    let pool = create_test_db();
    let conn = pool.get().expect("Failed to get database connection");
    let index_name: String = conn
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'index' AND name = ?",
            ["idx_media_access_user_active"],
            |row| row.get(0),
        )
        .expect("Failed to find active media access index");

    assert_eq!(index_name, "idx_media_access_user_active");
}
