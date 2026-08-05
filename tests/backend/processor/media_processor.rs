use crate::test_utils::create_test_db;
use filetime::{set_file_times, FileTime};
use momento_api::database::{queries, DbConn};
use momento_api::processor::media_processor::{
    apply_file_times, build_original_filename, calculate_geohash, capture_file_times,
    delete_from_rtree, insert_into_rtree,
};
use std::fs;

#[test]
fn original_filename_preserves_stem_and_extension() {
    assert_eq!(
        build_original_filename(42, std::path::Path::new("IMG_2373.HEIC")),
        "42_IMG_2373.HEIC"
    );
    assert_eq!(
        build_original_filename(43, std::path::Path::new("holiday photo")),
        "43_holiday photo"
    );
}

#[test]
fn imported_file_times_match_the_source_file() {
    let directory = tempfile::tempdir().expect("Failed to create temporary directory");
    let source = directory.path().join("source.jpg");
    let destination = directory.path().join("destination.jpg");
    fs::write(&source, b"source").expect("Failed to write source fixture");
    fs::write(&destination, b"destination").expect("Failed to write destination fixture");
    let expected_accessed = FileTime::from_unix_time(1_700_000_001, 123_000_000);
    let expected_modified = FileTime::from_unix_time(1_700_000_002, 456_000_000);
    set_file_times(&source, expected_accessed, expected_modified)
        .expect("Failed to set source file times");

    let file_times = capture_file_times(&source).expect("Failed to capture source file times");
    apply_file_times(&destination, file_times).expect("Failed to apply source file times");
    let destination_metadata =
        fs::metadata(&destination).expect("Failed to read destination metadata");

    assert_eq!(
        FileTime::from_last_access_time(&destination_metadata),
        expected_accessed
    );
    assert_eq!(
        FileTime::from_last_modification_time(&destination_metadata),
        expected_modified
    );
}

fn insert_test_media(conn: &DbConn, id: i64, filename: &str) {
    conn.execute(
        "INSERT INTO media (id, filename, original_filename, file_path, media_type, content_hash) VALUES (?, ?, ?, ?, ?, ?)",
        rusqlite::params![
            id,
            filename,
            filename,
            format!("/path/{}", filename),
            "image",
            format!("hash{}", id)
        ],
    )
    .expect("Failed to insert test media");
}

#[test]
fn duplicate_content_hash_insert_is_ignored_atomically() {
    let pool = create_test_db();
    let conn = pool.get().expect("Failed to get connection");
    let first = conn
        .execute(
            queries::media::INSERT,
            rusqlite::params![
                1,
                "pending.jpg",
                "photo.jpg",
                "2024-01/pending.jpg",
                "image",
                "image/jpeg",
                10,
                "duplicate-hash"
            ],
        )
        .expect("First media insert should succeed");
    let second = conn
        .execute(
            queries::media::INSERT,
            rusqlite::params![
                1,
                "pending.jpg",
                "photo.jpg",
                "2024-01/pending.jpg",
                "image",
                "image/jpeg",
                10,
                "duplicate-hash"
            ],
        )
        .expect("Duplicate media insert should be ignored");

    assert_eq!(first, 1);
    assert_eq!(second, 0);
    assert_eq!(conn.last_insert_rowid(), 1);
}

#[test]
fn test_calculate_geohash_new_york() {
    let geohash = calculate_geohash(40.7128, -74.0060);
    assert!(geohash.is_some());

    let hash = geohash.unwrap();
    assert_eq!(hash.len(), 7);
    assert!(hash.starts_with("dr5r"));
}

#[test]
fn test_calculate_geohash_london() {
    let geohash = calculate_geohash(51.5074, -0.1278);
    assert!(geohash.is_some());

    let hash = geohash.unwrap();
    assert_eq!(hash.len(), 7);
    assert!(hash.starts_with("gcpv"));
}

#[test]
fn test_calculate_geohash_tokyo() {
    let geohash = calculate_geohash(35.6762, 139.6503);
    assert!(geohash.is_some());

    let hash = geohash.unwrap();
    assert_eq!(hash.len(), 7);
    assert!(hash.starts_with("xn7"));
}

#[test]
fn test_rtree_insert_and_query() {
    let pool = create_test_db();
    let conn = pool.get().expect("Failed to get connection");

    insert_test_media(&conn, 1, "test.jpg");
    insert_into_rtree(&conn, 1, 40.7128, -74.0060).expect("R-tree insert should succeed");

    let count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM media_rtree WHERE min_lat >= ? AND max_lat <= ? AND min_lon >= ? AND max_lon <= ?",
            rusqlite::params![40.0, 41.0, -75.0, -73.0],
            |row| row.get(0),
        )
        .expect("R-tree query should succeed");

    assert_eq!(count, 1);
}

#[test]
fn test_rtree_query_excludes_outside_bbox() {
    let pool = create_test_db();
    let conn = pool.get().expect("Failed to get connection");

    insert_test_media(&conn, 1, "test.jpg");
    insert_into_rtree(&conn, 1, 40.7128, -74.0060).expect("R-tree insert should succeed");

    let count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM media_rtree WHERE min_lat >= ? AND max_lat <= ? AND min_lon >= ? AND max_lon <= ?",
            rusqlite::params![51.0, 52.0, -1.0, 1.0],
            |row| row.get(0),
        )
        .expect("R-tree query should succeed");

    assert_eq!(count, 0);
}

#[test]
fn test_rtree_delete() {
    let pool = create_test_db();
    let conn = pool.get().expect("Failed to get connection");

    insert_test_media(&conn, 1, "test.jpg");
    insert_into_rtree(&conn, 1, 40.7128, -74.0060).expect("R-tree insert should succeed");

    let count_before: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM media_rtree WHERE media_id = ?",
            [1],
            |row| row.get(0),
        )
        .expect("Query should succeed");
    assert_eq!(count_before, 1);

    delete_from_rtree(&conn, 1).expect("R-tree delete should succeed");

    let count_after: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM media_rtree WHERE media_id = ?",
            [1],
            |row| row.get(0),
        )
        .expect("Query should succeed");
    assert_eq!(count_after, 0);
}

#[test]
fn test_rtree_multiple_entries() {
    let pool = create_test_db();
    let conn = pool.get().expect("Failed to get connection");

    for i in 1..=3 {
        insert_test_media(&conn, i, &format!("test{}.jpg", i));
    }

    insert_into_rtree(&conn, 1, 40.7128, -74.0060).expect("NYC insert should succeed");
    insert_into_rtree(&conn, 2, 51.5074, -0.1278).expect("London insert should succeed");
    insert_into_rtree(&conn, 3, 35.6762, 139.6503).expect("Tokyo insert should succeed");

    let count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM media_rtree WHERE min_lat >= ? AND max_lat <= ? AND min_lon >= ? AND max_lon <= ?",
            rusqlite::params![-90.0, 90.0, -180.0, 180.0],
            |row| row.get(0),
        )
        .expect("R-tree query should succeed");

    assert_eq!(count, 3);

    let nyc_count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM media_rtree WHERE min_lat >= ? AND max_lat <= ? AND min_lon >= ? AND max_lon <= ?",
            rusqlite::params![40.0, 41.0, -75.0, -73.0],
            |row| row.get(0),
        )
        .expect("R-tree query should succeed");

    assert_eq!(nyc_count, 1);
}

#[test]
fn test_new_media_populates_geohash_and_rtree() {
    let pool = create_test_db();
    let conn = pool.get().expect("Failed to get connection");

    let media_id = 1;
    let latitude = 40.7128;
    let longitude = -74.0060;
    let geohash = calculate_geohash(latitude, longitude).expect("Geohash should be calculated");

    conn.execute(
        "INSERT INTO media (id, filename, original_filename, file_path, media_type, content_hash) VALUES (?, ?, ?, ?, ?, ?)",
        rusqlite::params![
            media_id,
            "photo.jpg",
            "photo.jpg",
            "/path/photo.jpg",
            "image",
            "hash1"
        ],
    )
    .expect("Failed to insert media");

    conn.execute(
        "INSERT INTO media_metadata (media_id, gps_latitude, gps_longitude, geohash) VALUES (?, ?, ?, ?)",
        rusqlite::params![
            media_id,
            latitude,
            longitude,
            &geohash
        ],
    )
    .expect("Failed to insert media_metadata with geohash");

    insert_into_rtree(&conn, media_id, latitude, longitude).expect("R-tree insert should succeed");

    let stored_geohash: Option<String> = conn
        .query_row(
            "SELECT geohash FROM media_metadata WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("Failed to query geohash");

    assert_eq!(stored_geohash.as_deref(), Some(geohash.as_str()));

    let rtree_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM media_rtree WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("Failed to query rtree");

    assert_eq!(rtree_count, 1);
}
