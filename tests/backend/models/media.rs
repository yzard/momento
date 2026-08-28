use momento_api::models::{map_media_response, map_media_response_with_content_hash};

const MEDIA_COLUMNS_WITHOUT_HASH: &str = "
    SELECT
        7, 'stored.jpg', 'original.jpg', 'image/jpeg', NULL, NULL, NULL, NULL, NULL,
        '2026-08-28T10:00:00', NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL,
        NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, '2026-08-28T11:00:00'
";

const MEDIA_COLUMNS_WITH_HASH: &str = "
    SELECT
        7, 'stored.jpg', 'original.jpg', 'image/jpeg', NULL, NULL, NULL, NULL, NULL,
        '2026-08-28T10:00:00', NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL,
        NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, 'abc123', '2026-08-28T11:00:00'
";

#[test]
fn shared_media_row_mapping_handles_queries_with_and_without_content_hash() {
    let connection = rusqlite::Connection::open_in_memory().expect("in-memory database");
    let without_hash = connection
        .query_row(MEDIA_COLUMNS_WITHOUT_HASH, [], map_media_response)
        .expect("media row without content hash");
    let with_hash = connection
        .query_row(
            MEDIA_COLUMNS_WITH_HASH,
            [],
            map_media_response_with_content_hash,
        )
        .expect("media row with content hash");

    assert_eq!(without_hash.id, 7);
    assert_eq!(without_hash.content_hash, None);
    assert_eq!(without_hash.created_at, "2026-08-28T11:00:00");
    assert_eq!(with_hash.id, 7);
    assert_eq!(with_hash.content_hash.as_deref(), Some("abc123"));
    assert_eq!(with_hash.created_at, "2026-08-28T11:00:00");
}
