use crate::test_utils::create_test_db;
use momento_api::database::DbConn;
use momento_api::processor::media_processor::{
    calculate_geohash, delete_from_rtree, insert_into_rtree,
};

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
