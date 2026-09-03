use crate::test_utils::{
    create_test_db, create_test_user, test_executor_handles_with_data_directory,
};
use filetime::{set_file_times, FileTime};
use momento_api::config::MediaProcessConfig;
use momento_api::database::{queries, DbConn};
use momento_api::io::file::NormalizedStoragePath;
use momento_api::io::file::StorageRootId;
use momento_api::processor::import::{
    recover_interrupted_imports, CreateImportJobOutcome, ImportSource, StagedImportCleanup,
    StagedImportFile,
};
use momento_api::processor::media_processor::{
    build_original_filename, calculate_geohash, generate_complete_metadata,
};
use std::fs;

fn staged_import_path(
    data_directory: &std::path::Path,
    relative_path: &str,
) -> (StagedImportFile, std::path::PathBuf) {
    let path = NormalizedStoragePath::parse(relative_path).expect("normalized staged path");
    let absolute = data_directory.join("imports").join(path.relative_path());
    fs::create_dir_all(absolute.parent().expect("staged parent")).expect("staged parent");
    (
        StagedImportFile {
            storage_root: StorageRootId::Imports,
            path,
        },
        absolute,
    )
}

#[tokio::test]
async fn complete_metadata_leaves_reverse_geocoding_to_the_cpu_executor() {
    let pool = create_test_db();
    momento_api::database::init_database(&pool.get().expect("schema connection")).expect("schema");
    let (executors, data_directory) = test_executor_handles_with_data_directory(pool);
    let relative_path = NormalizedStoragePath::parse("photo.jpg").expect("normalized media path");
    let media_path = data_directory
        .join("originals")
        .join(relative_path.relative_path());
    let sidecar_path = data_directory
        .join("originals")
        .join("photo.jpg.supplemental-metadata.json");
    image::RgbImage::new(2, 3)
        .save(&media_path)
        .expect("Failed to save image fixture");
    fs::write(
        sidecar_path,
        r#"{"geoData":{"latitude":40.759,"longitude":-73.9859}}"#,
    )
    .expect("Failed to save supplemental metadata fixture");
    let complete_metadata = generate_complete_metadata(
        &executors,
        StorageRootId::Originals,
        &relative_path,
        "image",
        &MediaProcessConfig::default(),
    )
    .await
    .expect("complete metadata");
    let metadata = complete_metadata.metadata;

    assert_eq!(metadata.gps_latitude, Some(40.759));
    assert_eq!(metadata.gps_longitude, Some(-73.9859));
    assert_eq!(metadata.location_city, None);
    assert_eq!(metadata.location_state, None);
    assert_eq!(metadata.location_country, None);
    assert!(complete_metadata
        .sources
        .iter()
        .any(|source| source.source_type.as_str() == "exiftool"));
    assert!(complete_metadata
        .sources
        .iter()
        .any(|source| source.source_type.as_str() == "supplemental_sidecar"));
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

fn insert_test_rtree(conn: &DbConn, media_id: i64, latitude: f64, longitude: f64) {
    conn.execute(
        queries::media::INSERT_RTREE,
        rusqlite::params![media_id, latitude, latitude, longitude, longitude],
    )
    .expect("test R-tree insert");
}

#[test]
fn duplicate_content_hashes_are_rejected_for_fresh_imports() {
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
}

#[tokio::test]
async fn import_status_is_durable_source_specific_and_prevents_concurrent_jobs() {
    let pool = create_test_db();
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    let CreateImportJobOutcome::Created(job_id) = executors
        .sqlite
        .create_import_job_request(ImportSource::Local)
        .await
        .expect("import job")
    else {
        panic!("local import job was already running");
    };
    assert!(job_id > 0);
    assert_eq!(
        executors
            .sqlite
            .create_import_job_request(ImportSource::Webdav)
            .await
            .expect("concurrent import outcome"),
        CreateImportJobOutcome::AlreadyRunning
    );
    pool.get()
        .expect("database")
        .execute(
            "UPDATE import_jobs SET status = 'completed', successful_imports = 2, completed_at = datetime('now') WHERE id = ?",
            [job_id],
        )
        .expect("complete local import");
    assert!(matches!(
        executors
            .sqlite
            .create_import_job_request(ImportSource::Webdav)
            .await
            .expect("WebDAV import job"),
        CreateImportJobOutcome::Created(_)
    ));

    let local_status = executors
        .sqlite
        .load_import_status_request(ImportSource::Local)
        .await
        .expect("local import status")
        .job;
    let webdav_status = executors
        .sqlite
        .load_import_status_request(ImportSource::Webdav)
        .await
        .expect("WebDAV import status")
        .job;
    assert_eq!(local_status.status, "completed");
    assert_eq!(local_status.successful_imports, 2);
    assert_eq!(webdav_status.status, "running");
    assert_eq!(webdav_status.successful_imports, 0);
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
    insert_test_rtree(&conn, 1, 40.7128, -74.0060);

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
    insert_test_rtree(&conn, 1, 40.7128, -74.0060);

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
    insert_test_rtree(&conn, 1, 40.7128, -74.0060);

    let count_before: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM media_rtree WHERE media_id = ?",
            [1],
            |row| row.get(0),
        )
        .expect("Query should succeed");
    assert_eq!(count_before, 1);

    conn.execute(queries::media::DELETE_RTREE, [1])
        .expect("R-tree delete should succeed");

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

    insert_test_rtree(&conn, 1, 40.7128, -74.0060);
    insert_test_rtree(&conn, 2, 51.5074, -0.1278);
    insert_test_rtree(&conn, 3, 35.6762, 139.6503);

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

    insert_test_rtree(&conn, media_id, latitude, longitude);

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

#[tokio::test]
async fn interrupted_import_recovery_rejects_an_unjournaled_original() {
    let pool = create_test_db();
    let connection = pool.get().expect("connection");
    connection.execute("INSERT INTO users (id, username, email, hashed_password, role, is_active) VALUES (9001, 'recovery', 'recovery@example.com', 'hash', 'admin', 1)", []).expect("user");
    connection
        .execute(
            "INSERT INTO import_jobs (source, status) VALUES ('local', 'running')",
            [],
        )
        .expect("interrupted import job");
    let original_filename = format!("recovery-{}.jpg", uuid::Uuid::new_v4());
    connection.execute("INSERT INTO media (user_id, filename, original_filename, file_path, media_type, import_state, import_source) VALUES (9001, '.importing', ?, '.importing/temporary', 'image', 'importing', 'local')", [&original_filename]).expect("media");
    let media_id = connection.last_insert_rowid();
    drop(connection);
    let final_filename =
        build_original_filename(media_id, std::path::Path::new(&original_filename));
    let (executors, data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let original_path = data_directory.join("originals").join(&final_filename);
    fs::write(&original_path, b"image").expect("original");
    recover_interrupted_imports(&executors)
        .await
        .expect("recovery");
    let connection = pool.get().expect("connection");
    let state: String = connection
        .query_row(
            "SELECT import_state FROM media WHERE id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("state");
    let metadata_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_metadata_jobs WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("metadata count");
    let recovered_filename: String = connection
        .query_row(
            "SELECT filename FROM media WHERE id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("filename");
    let access_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_access WHERE media_id = ? AND user_id = 9001",
            [media_id],
            |row| row.get(0),
        )
        .expect("access");
    let import_job: (String, Option<String>) = connection
        .query_row("SELECT status, last_error FROM import_jobs", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .expect("import job");
    assert_eq!(state, "failed");
    assert_eq!(metadata_count, 0);
    assert_eq!(recovered_filename, ".importing");
    assert_eq!(access_count, 0);
    assert!(original_path.is_file());
    assert_eq!(import_job.0, "failed");
    assert_eq!(
        import_job.1.as_deref(),
        Some("import interrupted by service restart")
    );
    drop(connection);
    let import_status = executors
        .sqlite
        .load_import_status_request(ImportSource::Local)
        .await
        .expect("import status")
        .job;
    assert_eq!(
        import_status.errors,
        vec!["import interrupted by service restart"]
    );
}

#[tokio::test]
async fn import_flattens_originals_and_moves_supplemental_metadata() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "flat-import", "flat-import@example.com");
    let (executors, data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let admission = executors
        .scheduler
        .acquire_durable(
            momento_api::runtime::DurableSourceId::LocalImport,
            momento_api::runtime::SchedulerAdmissionKind::NewClaim,
        )
        .await
        .expect("import admission");
    let directory_name = format!("flat-import-{}", uuid::Uuid::new_v4());
    let (staged_source, source_path) =
        staged_import_path(&data_directory, &format!("{directory_name}/camera(10).jpg"));
    let source_sidecar_path = data_directory
        .join("imports")
        .join(directory_name)
        .join("camera.jpg.supplemental-metadata(10).json");
    fs::write(&source_path, b"image").expect("media source");
    fs::write(&source_sidecar_path, "{}\n").expect("metadata source");
    let media_id = momento_api::processor::import::import_staged_file(
        staged_source,
        momento_api::processor::import::ImportSource::Local,
        user_id,
        &executors,
        StagedImportCleanup {
            source: false,
            supplemental_metadata: true,
        },
        &admission,
    )
    .await
    .expect("import")
    .completed_media_id()
    .expect("import was not deferred");
    momento_api::io::recovery::recover_generic_file_operations(&executors)
        .await
        .expect("sidecar cleanup");
    let final_filename = build_original_filename(media_id, &source_path);
    assert!(data_directory
        .join("originals")
        .join(&final_filename)
        .is_file());
    assert!(data_directory
        .join("originals")
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

#[tokio::test]
async fn duplicate_import_reuses_media_and_absorbs_earlier_time_and_sidecar() {
    let pool = create_test_db();
    let first_user_id = create_test_user(&pool, "duplicate-first", "duplicate-first@example.com");
    let second_user_id =
        create_test_user(&pool, "duplicate-second", "duplicate-second@example.com");
    let (executors, data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let admission = executors
        .scheduler
        .acquire_durable(
            momento_api::runtime::DurableSourceId::LocalImport,
            momento_api::runtime::SchedulerAdmissionKind::NewClaim,
        )
        .await
        .expect("import admission");
    let source_directory_name = format!("duplicate-import-{}", uuid::Uuid::new_v4());
    let unique_name = format!("duplicate-{}.jpg", uuid::Uuid::new_v4());
    let (first_staged_source, first_source_path) = staged_import_path(
        &data_directory,
        &format!("{source_directory_name}/{unique_name}"),
    );
    let (duplicate_staged_source, duplicate_source_path) = staged_import_path(
        &data_directory,
        &format!("{source_directory_name}/copy-{unique_name}"),
    );
    fs::write(&first_source_path, b"identical media bytes").expect("first media source");
    fs::write(&duplicate_source_path, b"identical media bytes").expect("duplicate media source");
    let first_modified = FileTime::from_unix_time(1_640_995_200, 0);
    set_file_times(&first_source_path, first_modified, first_modified).expect("first file time");

    let media_id = momento_api::processor::import::import_staged_file(
        first_staged_source,
        momento_api::processor::import::ImportSource::Local,
        first_user_id,
        &executors,
        StagedImportCleanup {
            source: false,
            supplemental_metadata: true,
        },
        &admission,
    )
    .await
    .expect("first import")
    .completed_media_id()
    .expect("first import was not deferred");
    let final_filename = build_original_filename(media_id, &first_source_path);
    let final_path = data_directory.join("originals").join(&final_filename);
    fs::write(&final_path, b"canonical original remains").expect("canonical marker");
    let duplicate_modified = FileTime::from_unix_time(1_577_836_800, 0);
    set_file_times(
        &duplicate_source_path,
        duplicate_modified,
        duplicate_modified,
    )
    .expect("duplicate file time");
    let duplicate_sidecar_path = duplicate_source_path.with_file_name(format!(
        "{}.supplemental-metadata.json",
        duplicate_source_path
            .file_name()
            .expect("duplicate filename")
            .to_string_lossy()
    ));
    fs::write(
        &duplicate_sidecar_path,
        r#"{"description":"updated metadata"}"#,
    )
    .expect("duplicate sidecar");
    let initial_created_at: String = pool
        .get()
        .expect("connection")
        .query_row(
            "SELECT created_at FROM media WHERE id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("initial created time");
    assert_eq!(initial_created_at, "2022-01-01 00:00:00");
    pool.get()
        .expect("connection")
        .execute(
            "UPDATE media_metadata_jobs SET status = 'completed' WHERE media_id = ?",
            [media_id],
        )
        .expect("completed metadata job");

    let duplicate_media_id = momento_api::processor::import::import_staged_file(
        duplicate_staged_source,
        momento_api::processor::import::ImportSource::Local,
        second_user_id,
        &executors,
        StagedImportCleanup {
            source: false,
            supplemental_metadata: true,
        },
        &admission,
    )
    .await
    .expect("duplicate import")
    .completed_media_id()
    .expect("duplicate import was not deferred");
    momento_api::io::recovery::recover_generic_file_operations(&executors)
        .await
        .expect("sidecar cleanup");

    assert_eq!(duplicate_media_id, media_id);
    assert_eq!(
        fs::read(&final_path).expect("canonical original"),
        b"canonical original remains"
    );
    assert!(duplicate_source_path.is_file());
    assert!(!duplicate_sidecar_path.exists());
    assert_eq!(
        fs::read_to_string(
            final_path.with_file_name(format!("{final_filename}.supplemental-metadata.json"))
        )
        .expect("absorbed sidecar"),
        r#"{"description":"updated metadata"}"#
    );
    let connection = pool.get().expect("connection");
    let media_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM media", [], |row| row.get(0))
        .expect("media count");
    let created_at: String = connection
        .query_row(
            "SELECT created_at FROM media WHERE id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("created time");
    let second_user_access: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_access WHERE media_id = ? AND user_id = ? AND deleted_at IS NULL",
            rusqlite::params![media_id, second_user_id],
            |row| row.get(0),
        )
        .expect("second user access");
    let metadata_status: String = connection
        .query_row(
            "SELECT status FROM media_metadata_jobs WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("metadata status");
    assert_eq!(media_count, 1);
    assert_eq!(created_at, "2020-01-01 00:00:00");
    assert_eq!(second_user_access, 1);
    assert_eq!(metadata_status, "queued");

    let (newer_staged_source, newer_duplicate_path) = staged_import_path(
        &data_directory,
        &format!("{source_directory_name}/newer-{unique_name}"),
    );
    fs::write(&newer_duplicate_path, b"identical media bytes").expect("newer duplicate");
    let newer_modified = FileTime::from_unix_time(1_672_531_200, 0);
    set_file_times(&newer_duplicate_path, newer_modified, newer_modified)
        .expect("newer duplicate time");
    let newer_media_id = momento_api::processor::import::import_staged_file(
        newer_staged_source,
        momento_api::processor::import::ImportSource::Local,
        first_user_id,
        &executors,
        StagedImportCleanup {
            source: false,
            supplemental_metadata: true,
        },
        &admission,
    )
    .await
    .expect("newer duplicate import")
    .completed_media_id()
    .expect("newer duplicate import was not deferred");
    let unchanged_created_at: String = pool
        .get()
        .expect("connection")
        .query_row(
            "SELECT created_at FROM media WHERE id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("unchanged created time");
    assert_eq!(newer_media_id, media_id);
    assert_eq!(unchanged_created_at, "2020-01-01 00:00:00");
}
