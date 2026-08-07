use crate::test_utils::create_test_db;

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
