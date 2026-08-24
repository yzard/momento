use std::sync::Arc;
use std::time::Duration;

use momento_api::{
    config::Config,
    constants::paths,
    database::{create_pool_at, init_database},
    processor::import::{
        create_import_job, import_staged_file, recover_import_claims, run_local_import,
        run_webdav_import_cycle, ImportSettings, ImportSource,
    },
};

use crate::test_utils::{create_test_db, create_test_user, init_test_paths, lock_webdav_test};

fn mark_webdav_file_ready(pool: &momento_api::database::DbPool, user_id: i64, file_path: &str) {
    pool.get()
        .expect("database")
        .execute(
            momento_api::database::queries::webdav_ready::UPSERT,
            rusqlite::params![user_id, file_path],
        )
        .expect("ready WebDAV file");
}

#[tokio::test]
async fn test_local_import_uses_canonical_staged_file_import() {
    init_test_paths();
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "local-import", "local-import@example.com");
    let source_directory = paths()
        .imports
        .join(format!("processor-tests/{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&source_directory).expect("local import directory");
    let source_path = source_directory.join("local-photo.jpg");
    std::fs::write(&source_path, b"local import bytes").expect("local import source");
    let job_id = create_import_job(&pool, ImportSource::Local).expect("local import job");

    run_local_import(
        ImportSettings {
            user_id,
            pool: pool.clone(),
            delete_after_import: true,
            concurrency: 1,
        },
        job_id,
    )
    .await;

    assert!(!source_path.exists());
    let connection = pool.get().expect("database");
    let (media_id, media_path, import_state, import_source): (i64, String, String, String) =
        connection
            .query_row(
                "SELECT id, file_path, import_state, import_source FROM media WHERE original_filename = 'local-photo.jpg'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("local imported media");
    let metadata_status: String = connection
        .query_row(
            "SELECT status FROM media_metadata_jobs WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("metadata job");
    let access_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_access WHERE media_id = ? AND user_id = ? AND deleted_at IS NULL",
            rusqlite::params![media_id, user_id],
            |row| row.get(0),
        )
        .expect("media access");
    assert_eq!(import_state, "imported");
    assert_eq!(import_source, "local");
    assert_eq!(metadata_status, "queued");
    assert_eq!(access_count, 1);

    std::fs::remove_file(paths().originals.join(media_path)).expect("remove imported original");
    std::fs::remove_dir_all(source_directory).expect("remove local import directory");
}

#[tokio::test]
async fn test_webdav_import_waits_for_active_uploads_before_claiming() {
    let _webdav_test_guard = lock_webdav_test().await;
    init_test_paths();
    let pool = create_test_db();
    let username = format!("import-gate-{}", uuid::Uuid::new_v4());
    let user_id = create_test_user(&pool, &username, "import-gate@example.com");
    let user_root = paths().webdav.join(&username);
    std::fs::create_dir_all(&user_root).expect("WebDAV user directory");
    let source_path = user_root.join("photo.jpg");
    std::fs::write(&source_path, b"incomplete upload").expect("staged upload");

    let mut config = Config::default();
    config.webdav.max_concurrent_requests = 1;
    config.webdav.stable_file_age_seconds = 0;
    let gate = Arc::new(tokio::sync::Semaphore::new(1));
    let upload_permit = gate.acquire().await.expect("upload permit");
    run_webdav_import_cycle(&config, &pool, &gate).await;
    assert!(source_path.exists());

    std::fs::write(&source_path, b"complete upload").expect("complete staged upload");
    mark_webdav_file_ready(&pool, user_id, "photo.jpg");
    let cycle_config = config.clone();
    let cycle_pool = pool.clone();
    let cycle_gate = Arc::clone(&gate);
    let cycle = tokio::spawn(async move {
        run_webdav_import_cycle(&cycle_config, &cycle_pool, &cycle_gate).await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!cycle.is_finished());
    drop(upload_permit);
    tokio::time::timeout(Duration::from_secs(5), cycle)
        .await
        .expect("import cycle timeout")
        .expect("import cycle");
    assert!(!source_path.exists());
    let (media_id, media_path, imported_size, import_state, import_source): (
        i64,
        String,
        i64,
        String,
        String,
    ) = pool
        .get()
        .expect("database")
        .query_row(
            "SELECT id, file_path, file_size, import_state, import_source FROM media WHERE original_filename = 'photo.jpg'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("imported media");
    assert_eq!(imported_size, b"complete upload".len() as i64);
    assert_eq!(import_state, "imported");
    assert_eq!(import_source, "webdav");
    let connection = pool.get().expect("database");
    let metadata_status: String = connection
        .query_row(
            "SELECT status FROM media_metadata_jobs WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("metadata job");
    let access_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_access WHERE media_id = ? AND user_id = ? AND deleted_at IS NULL",
            rusqlite::params![media_id, user_id],
            |row| row.get(0),
        )
        .expect("media access");
    assert_eq!(metadata_status, "queued");
    assert_eq!(access_count, 1);

    std::fs::remove_file(paths().originals.join(media_path)).expect("remove imported original");
    std::fs::remove_dir_all(user_root).expect("remove WebDAV test directory");
}

#[tokio::test]
async fn test_webdav_import_rechecks_readiness_after_waiting_for_uploads() {
    let _webdav_test_guard = lock_webdav_test().await;
    init_test_paths();
    let pool = create_test_db();
    let username = format!("readiness-race-{}", uuid::Uuid::new_v4());
    let user_id = create_test_user(&pool, &username, "readiness-race@example.com");
    let user_root = paths().webdav.join(&username);
    std::fs::create_dir_all(&user_root).expect("WebDAV user directory");
    let source_path = user_root.join("video.mp4");
    std::fs::write(&source_path, b"previous completed upload").expect("staged upload");
    mark_webdav_file_ready(&pool, user_id, "video.mp4");

    let mut config = Config::default();
    config.webdav.max_concurrent_requests = 1;
    config.webdav.stable_file_age_seconds = 0;
    let gate = Arc::new(tokio::sync::Semaphore::new(1));
    let upload_permit = gate.acquire().await.expect("upload permit");
    let cycle_config = config.clone();
    let cycle_pool = pool.clone();
    let cycle_gate = Arc::clone(&gate);
    let cycle = tokio::spawn(async move {
        run_webdav_import_cycle(&cycle_config, &cycle_pool, &cycle_gate).await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    pool.get()
        .expect("database")
        .execute(
            momento_api::database::queries::webdav_ready::DELETE,
            rusqlite::params![user_id, "video.mp4"],
        )
        .expect("invalidate readiness");
    std::fs::write(&source_path, b"incomplete replacement").expect("partial replacement");
    drop(upload_permit);
    cycle.await.expect("import cycle");

    assert_eq!(
        std::fs::read(&source_path).expect("unready source"),
        b"incomplete replacement"
    );
    let media_count: i64 = pool
        .get()
        .expect("database")
        .query_row("SELECT COUNT(*) FROM media", [], |row| row.get(0))
        .expect("media count");
    assert_eq!(media_count, 0);
    std::fs::remove_dir_all(user_root).expect("remove WebDAV test directory");
}

#[tokio::test]
async fn test_webdav_duplicate_reuses_existing_media() {
    let _webdav_test_guard = lock_webdav_test().await;
    init_test_paths();
    let pool = create_test_db();
    let first_user_id = create_test_user(
        &pool,
        "webdav-duplicate-owner",
        "webdav-duplicate-owner@example.com",
    );
    let username = format!("webdav-duplicate-{}", uuid::Uuid::new_v4());
    let second_user_id = create_test_user(&pool, &username, &format!("{username}@example.com"));
    let source_directory = tempfile::tempdir().expect("source directory");
    let first_source_path = source_directory.path().join("original.jpg");
    std::fs::write(&first_source_path, b"shared WebDAV bytes").expect("first source");
    let media_id = import_staged_file(
        &first_source_path,
        ImportSource::Local,
        first_user_id,
        &pool,
        false,
    )
    .await
    .expect("first import");

    let user_root = paths().webdav.join(&username);
    std::fs::create_dir_all(&user_root).expect("WebDAV user directory");
    let duplicate_path = user_root.join("duplicate.jpg");
    std::fs::write(&duplicate_path, b"shared WebDAV bytes").expect("WebDAV duplicate");
    let duplicate_sidecar_path = user_root.join("duplicate.jpg.supplemental-metadata(2).json");
    std::fs::write(
        &duplicate_sidecar_path,
        b"{\"description\":\"WebDAV sidecar\"}",
    )
    .expect("WebDAV sidecar");
    mark_webdav_file_ready(&pool, second_user_id, "duplicate.jpg");
    mark_webdav_file_ready(
        &pool,
        second_user_id,
        "duplicate.jpg.supplemental-metadata(2).json",
    );
    let mut config = Config::default();
    config.webdav.stable_file_age_seconds = 0;
    let gate = Arc::new(tokio::sync::Semaphore::new(
        config.webdav.max_concurrent_requests,
    ));

    run_webdav_import_cycle(&config, &pool, &gate).await;

    assert!(!duplicate_path.exists());
    assert!(!duplicate_sidecar_path.exists());
    let connection = pool.get().expect("database");
    let media_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM media", [], |row| row.get(0))
        .expect("media count");
    let access_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_access WHERE media_id = ? AND user_id = ? AND deleted_at IS NULL",
            rusqlite::params![media_id, second_user_id],
            |row| row.get(0),
        )
        .expect("duplicate user access");
    let ready_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM webdav_ready_files WHERE user_id = ?",
            [second_user_id],
            |row| row.get(0),
        )
        .expect("remaining ready files");
    assert_eq!(media_count, 1);
    assert_eq!(access_count, 1);
    assert_eq!(ready_count, 0);
    let canonical_file_path: String = connection
        .query_row(
            "SELECT file_path FROM media WHERE id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("canonical file path");
    assert_eq!(
        std::fs::read(
            paths()
                .originals
                .join(format!("{canonical_file_path}.supplemental-metadata.json"))
        )
        .expect("canonical sidecar"),
        b"{\"description\":\"WebDAV sidecar\"}"
    );

    std::fs::remove_dir_all(user_root).expect("remove WebDAV test directory");
}

#[tokio::test]
async fn test_concurrent_matching_hash_imports_create_one_media_row() {
    init_test_paths();
    let database_directory = tempfile::tempdir().expect("database directory");
    let pool =
        create_pool_at(&database_directory.path().join("database.sqlite")).expect("database pool");
    init_database(&pool.get().expect("connection")).expect("database schema");
    let user_id = create_test_user(
        &pool,
        "concurrent-duplicate",
        "concurrent-duplicate@example.com",
    );
    let source_directory = tempfile::tempdir().expect("source directory");
    let unique_name = uuid::Uuid::new_v4();
    let first_source_path = source_directory
        .path()
        .join(format!("first-{unique_name}.jpg"));
    let second_source_path = source_directory
        .path()
        .join(format!("second-{unique_name}.jpg"));
    std::fs::write(&first_source_path, b"concurrent identical bytes").expect("first source");
    std::fs::write(&second_source_path, b"concurrent identical bytes").expect("second source");

    let (first_result, second_result) = tokio::join!(
        import_staged_file(
            &first_source_path,
            ImportSource::Local,
            user_id,
            &pool,
            false,
        ),
        import_staged_file(
            &second_source_path,
            ImportSource::Local,
            user_id,
            &pool,
            false,
        ),
    );

    let first_media_id = first_result.expect("first import");
    let second_media_id = second_result.expect("second import");
    let media_count: i64 = pool
        .get()
        .expect("connection")
        .query_row("SELECT COUNT(*) FROM media", [], |row| row.get(0))
        .expect("media count");
    assert_eq!(first_media_id, second_media_id);
    assert_eq!(media_count, 1);
}

#[tokio::test]
async fn test_webdav_claim_recovery_restores_nested_source_file() {
    let _webdav_test_guard = lock_webdav_test().await;
    init_test_paths();
    let pool = create_test_db();
    let username = format!("claim-recovery-{}", uuid::Uuid::new_v4());
    let user_id = create_test_user(&pool, &username, "claim-recovery@example.com");
    let source_directory = paths().webdav.join(&username).join("Camera Roll");
    let claim_directory = source_directory.join(".processing/claim-id");
    std::fs::create_dir_all(&claim_directory).expect("claim directory");
    std::fs::write(claim_directory.join("photo.jpg"), b"photo bytes").expect("claimed file");
    mark_webdav_file_ready(&pool, user_id, "Camera Roll/photo.jpg");

    recover_import_claims(&paths().webdav).expect("recover WebDAV claims");

    assert_eq!(
        std::fs::read(source_directory.join("photo.jpg")).expect("restored source"),
        b"photo bytes"
    );
    assert!(!source_directory.join(".processing").exists());
    let mut config = Config::default();
    config.webdav.stable_file_age_seconds = 0;
    let gate = Arc::new(tokio::sync::Semaphore::new(
        config.webdav.max_concurrent_requests,
    ));
    run_webdav_import_cycle(&config, &pool, &gate).await;
    assert!(!source_directory.join("photo.jpg").exists());
    let media_count: i64 = pool
        .get()
        .expect("database")
        .query_row("SELECT COUNT(*) FROM media", [], |row| row.get(0))
        .expect("media count");
    assert_eq!(media_count, 1);
    std::fs::remove_dir_all(paths().webdav.join(username)).expect("remove WebDAV test directory");
}

#[tokio::test]
async fn test_import_claim_recovery_restores_local_source_file() {
    let directory = tempfile::tempdir().expect("import directory");
    let claim_directory = directory.path().join("album/.processing/claim-id");
    std::fs::create_dir_all(&claim_directory).expect("claim directory");
    std::fs::write(claim_directory.join("photo.jpg"), b"local claim").expect("claimed file");

    recover_import_claims(directory.path()).expect("recover local claims");

    assert_eq!(
        std::fs::read(directory.path().join("album/photo.jpg")).expect("restored local claim"),
        b"local claim"
    );
    assert!(!directory.path().join("album/.processing").exists());
}

#[tokio::test]
async fn test_webdav_claim_recovery_exposes_colliding_claim_in_recovered_directory() {
    let _webdav_test_guard = lock_webdav_test().await;
    init_test_paths();
    let username = format!("claim-collision-{}", uuid::Uuid::new_v4());
    let source_directory = paths().webdav.join(&username);
    let claim_directory = source_directory.join(".processing/claim-id");
    std::fs::create_dir_all(&claim_directory).expect("claim directory");
    std::fs::write(source_directory.join("photo.jpg"), b"new upload").expect("new source");
    std::fs::write(claim_directory.join("photo.jpg"), b"older claim").expect("claimed file");
    std::fs::write(
        claim_directory.join("photo.jpg.supplemental-metadata.json"),
        b"{}",
    )
    .expect("claimed supplemental metadata");

    recover_import_claims(&paths().webdav).expect("recover WebDAV claims");

    let recovered_directory = source_directory.join("recovered-claim-id");
    assert_eq!(
        std::fs::read(recovered_directory.join("photo.jpg")).expect("recovered claim"),
        b"older claim"
    );
    assert!(recovered_directory
        .join("photo.jpg.supplemental-metadata.json")
        .is_file());
    assert_eq!(
        std::fs::read(source_directory.join("photo.jpg")).expect("new source"),
        b"new upload"
    );
    std::fs::remove_dir_all(paths().webdav.join(username)).expect("remove WebDAV test directory");
}
