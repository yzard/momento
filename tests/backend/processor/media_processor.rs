use crate::test_utils::{create_test_db, create_test_user, init_test_paths};
use filetime::{set_file_times, FileTime};
use momento_api::config::MetadataConfig;
use momento_api::constants::paths;
use momento_api::database::{queries, DbConn};
use momento_api::processor::import::{
    create_import_job, get_import_status, recover_interrupted_imports, ImportSource,
};
use momento_api::processor::media_processor::{
    apply_file_times, build_original_filename, calculate_geohash, capture_file_times,
    delete_from_rtree, generate_complete_metadata, insert_into_rtree,
};
use std::fs;

#[tokio::test]
async fn complete_metadata_uses_combined_metadata_config() {
    let directory = tempfile::tempdir().expect("Failed to create temporary directory");
    let media_path = directory.path().join("photo.jpg");
    let sidecar_path = directory
        .path()
        .join("photo.jpg.supplemental-metadata.json");
    image::RgbImage::new(2, 3)
        .save(&media_path)
        .expect("Failed to save image fixture");
    fs::write(
        sidecar_path,
        r#"{"geoData":{"latitude":40.759,"longitude":-73.9859}}"#,
    )
    .expect("Failed to save supplemental metadata fixture");
    let config = MetadataConfig {
        reverse_geocoding_enabled: false,
        ..MetadataConfig::default()
    };

    let metadata = generate_complete_metadata(&media_path, "image", &config).await;

    assert_eq!(metadata.gps_latitude, Some(40.759));
    assert_eq!(metadata.gps_longitude, Some(-73.9859));
    assert_eq!(metadata.location_city, None);
    assert_eq!(metadata.location_state, None);
    assert_eq!(metadata.location_country, None);
}

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
fn duplicate_content_hashes_are_allowed_for_fresh_imports() {
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
    assert_eq!(second, 1);
}

#[test]
fn import_status_is_durable_and_prevents_concurrent_jobs() {
    let pool = create_test_db();
    let job_id = create_import_job(&pool, ImportSource::Local).expect("import job");
    assert!(job_id > 0);
    assert!(create_import_job(&pool, ImportSource::Webdav).is_err());
    let status = get_import_status(&pool).expect("import status");
    assert_eq!(status.status, "running");
    assert_eq!(status.total_files, 0);
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

#[test]
fn interrupted_import_recovery_restores_completed_media_and_queues_metadata() {
    init_test_paths();
    let pool = create_test_db();
    let connection = pool.get().expect("connection");
    connection.execute("INSERT INTO users (id, username, email, hashed_password, role, is_active) VALUES (9001, 'recovery', 'recovery@example.com', 'hash', 'admin', 1)", []).expect("user");
    connection.execute("INSERT INTO media (user_id, filename, original_filename, file_path, media_type, import_state, import_source) VALUES (9001, '.importing', 'camera.jpg', '42/camera.jpg', 'image', 'importing', 'local')", []).expect("media");
    let media_id = connection.last_insert_rowid();
    drop(connection);
    let original_path = paths().originals.join("camera.jpg");
    fs::write(&original_path, b"image").expect("original");
    recover_interrupted_imports(&pool).expect("recovery");
    let connection = pool.get().expect("connection");
    let state: String = connection
        .query_row(
            "SELECT import_state FROM media WHERE id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("state");
    let metadata_status: String = connection
        .query_row(
            "SELECT status FROM media_metadata_jobs WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("metadata job");
    assert_eq!(state, "imported");
    assert_eq!(metadata_status, "queued");
}

#[tokio::test]
async fn import_flattens_originals_and_moves_supplemental_metadata() {
    init_test_paths();
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "flat-import", "flat-import@example.com");
    let source_directory = tempfile::tempdir().expect("source directory");
    let source_path = source_directory.path().join("camera.jpg");
    let source_sidecar_path = source_directory
        .path()
        .join("camera.jpg.supplemental-metadata.json");
    fs::write(&source_path, b"image").expect("media source");
    fs::write(&source_sidecar_path, "{}\n").expect("metadata source");
    let media_id = momento_api::processor::import::finalize_staged_original(
        &source_path,
        momento_api::processor::import::ImportSource::Local,
        user_id,
        &pool,
    )
    .await
    .expect("import");
    let final_filename = build_original_filename(media_id, &source_path);
    assert!(paths().originals.join(&final_filename).is_file());
    assert!(paths()
        .originals
        .join(format!("{final_filename}.supplemental-metadata.json"))
        .is_file());
    assert!(!source_sidecar_path.exists());
    let connection = pool.get().expect("connection");
    let file_path: String = connection
        .query_row(
            "SELECT file_path FROM media WHERE id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("file path");
    assert_eq!(file_path, final_filename);
}
