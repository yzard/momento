use std::sync::Arc;
use std::time::Duration;

use momento_api::io::file::NormalizedStoragePath;
use momento_api::io::file::StorageRootId;
use momento_api::{
    config::Config,
    database::{create_pool_at, init_database},
    processor::import::{
        import_staged_file, run_local_import, run_webdav_import_cycle, CreateImportJobOutcome,
        ImportSettings, ImportSource, StagedImportCleanup, StagedImportFile,
    },
};

use crate::test_utils::{create_test_db, create_test_user, lock_webdav_test, QOI_FIXTURE};

fn staged_file(
    data_directory: &std::path::Path,
    storage_root: StorageRootId,
    relative_path: &str,
) -> (StagedImportFile, std::path::PathBuf) {
    let path = NormalizedStoragePath::parse(relative_path).expect("normalized staged path");
    let absolute = data_directory
        .join(storage_root.directory_name())
        .join(path.relative_path());
    std::fs::create_dir_all(absolute.parent().expect("staged parent"))
        .expect("staged parent directory");
    (StagedImportFile { storage_root, path }, absolute)
}

fn mark_webdav_file_ready(pool: &momento_api::database::DbPool, user_id: i64, file_path: &str) {
    pool.get()
        .expect("database")
        .execute(
            momento_api::database::queries::webdav_ready::UPSERT,
            rusqlite::params![user_id, file_path],
        )
        .expect("ready WebDAV file");
}

async fn finish_deferred_import(
    mut outcome: momento_api::processor::import::ImportStagedFileOutcome,
    executors: &momento_api::runtime::ExecutorHandles,
    admission: &momento_api::runtime::DurableAdmission,
) -> i64 {
    loop {
        match outcome {
            momento_api::processor::import::ImportStagedFileOutcome::Completed(media_id) => {
                return media_id;
            }
            momento_api::processor::import::ImportStagedFileOutcome::Deferred(prepared) => {
                tokio::task::yield_now().await;
                outcome = momento_api::processor::import::resume_staged_file_import(
                    *prepared, executors, admission,
                )
                .await
                .expect("retry deferred import");
            }
        }
    }
}

#[tokio::test]
async fn qoi_import_uses_the_canonical_image_mime_type() {
    let _filesystem_test_guard = lock_webdav_test().await;
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "qoi-import", "qoi-import@example.com");
    let (executors, data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let (staged_source, source_path) = staged_file(
        &data_directory,
        StorageRootId::Imports,
        &format!("qoi-{}/lossless.QOI", uuid::Uuid::new_v4()),
    );
    std::fs::write(&source_path, QOI_FIXTURE).expect("QOI source");
    let admission = executors
        .scheduler
        .acquire_durable(
            momento_api::runtime::DurableSourceId::LocalImport,
            momento_api::runtime::SchedulerAdmissionKind::NewClaim,
        )
        .await
        .expect("import admission");

    let media_id = import_staged_file(
        staged_source,
        ImportSource::Local,
        user_id,
        &executors,
        StagedImportCleanup {
            source: false,
            supplemental_metadata: true,
        },
        &admission,
    )
    .await
    .expect("QOI import")
    .completed_media_id()
    .expect("QOI import was not deferred");

    let (media_type, mime_type, metadata_status): (String, String, String) = pool
        .get()
        .expect("database")
        .query_row(
            "SELECT m.media_type, m.mime_type, j.status FROM media m JOIN media_metadata_jobs j ON j.media_id = m.id WHERE m.id = ?",
            [media_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("imported QOI");
    assert_eq!(media_type, "image");
    assert_eq!(mime_type, "image/qoi");
    assert_eq!(metadata_status, "queued");
}

#[tokio::test]
async fn test_local_import_uses_canonical_staged_file_import() {
    let _filesystem_test_guard = lock_webdav_test().await;
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "local-import", "local-import@example.com");
    let (executors, data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let source_directory = data_directory
        .join("imports")
        .join(format!("processor-tests/{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&source_directory).expect("local import directory");
    let source_path = source_directory.join("local-photo.jpg");
    std::fs::write(&source_path, b"local import bytes").expect("local import source");
    let CreateImportJobOutcome::Created(job_id) = executors
        .sqlite
        .create_import_job_request(ImportSource::Local)
        .await
        .expect("local import job")
    else {
        panic!("local import job was already running");
    };
    run_local_import(
        ImportSettings {
            user_id,
            executors: executors.clone(),
            scheduler: crate::test_utils::test_scheduler(pool.clone()),
        },
        job_id,
    )
    .await;
    momento_api::io::recovery::recover_generic_file_operations(&executors)
        .await
        .expect("source cleanup");

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
    let product_group: (String, String, Option<String>, i64) = connection
        .query_row(
            "SELECT state, completion_outcome, product_target, entry_count FROM file_operation_groups WHERE kind = 'import_media_publication' AND owner_id = CAST(? AS TEXT)",
            [media_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("import product group");
    assert_eq!(import_state, "imported");
    assert_eq!(import_source, "local");
    assert_eq!(metadata_status, "queued");
    assert_eq!(access_count, 1);
    assert_eq!(
        product_group,
        ("cleaned".to_string(), "published".to_string(), None, 1)
    );

    std::fs::remove_file(data_directory.join("originals").join(media_path))
        .expect("remove imported original");
    std::fs::remove_dir_all(source_directory).expect("remove local import directory");
}

#[tokio::test]
async fn test_local_import_absorbs_duplicates_and_removes_duplicate_sources() {
    let _filesystem_test_guard = lock_webdav_test().await;
    let pool = create_test_db();
    let user_id = create_test_user(
        &pool,
        "local-duplicate-import",
        "local-duplicate-import@example.com",
    );
    let (executors, data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let source_directory = data_directory
        .join("imports")
        .join(format!("duplicate-tests/{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&source_directory).expect("local duplicate directory");
    let first_source_path = source_directory.join("first.jpg");
    let duplicate_source_path = source_directory.join("duplicate.jpg");
    let duplicate_sidecar_path = source_directory.join("duplicate.jpg.supplemental-metadata.json");
    std::fs::write(&first_source_path, b"identical local import bytes")
        .expect("first local source");
    std::fs::write(&duplicate_source_path, b"identical local import bytes")
        .expect("duplicate local source");
    std::fs::write(
        &duplicate_sidecar_path,
        b"{\"description\":\"absorbed duplicate\"}",
    )
    .expect("duplicate sidecar");
    let CreateImportJobOutcome::Created(job_id) = executors
        .sqlite
        .create_import_job_request(ImportSource::Local)
        .await
        .expect("local import job")
    else {
        panic!("local import job was already running");
    };
    run_local_import(
        ImportSettings {
            user_id,
            executors: executors.clone(),
            scheduler: crate::test_utils::test_scheduler(pool.clone()),
        },
        job_id,
    )
    .await;
    momento_api::io::recovery::recover_generic_file_operations(&executors)
        .await
        .expect("source cleanup");

    assert!(!first_source_path.exists());
    assert!(!duplicate_source_path.exists());
    assert!(!duplicate_sidecar_path.exists());
    let connection = pool.get().expect("database");
    let (media_id, canonical_relative_path): (i64, String) = connection
        .query_row(
            "SELECT id, file_path FROM media WHERE content_hash IS NOT NULL",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("absorbed media");
    let media_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM media", [], |row| row.get(0))
        .expect("media count");
    let job: (i64, i64, i64) = connection
        .query_row(
            "SELECT processed_files, successful_imports, failed_imports FROM import_jobs WHERE id = ?",
            [job_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("local import progress");
    assert_eq!(media_count, 1);
    assert_eq!(job, (2, 2, 0));
    let canonical_path = data_directory
        .join("originals")
        .join(&canonical_relative_path);
    assert_eq!(
        std::fs::read(format!(
            "{}.supplemental-metadata.json",
            canonical_path.display()
        ))
        .expect("absorbed canonical sidecar"),
        b"{\"description\":\"absorbed duplicate\"}"
    );

    drop(connection);
    std::fs::remove_file(&canonical_path).expect("remove canonical original");
    std::fs::remove_file(format!(
        "{}.supplemental-metadata.json",
        canonical_path.display()
    ))
    .expect("remove canonical sidecar");
    std::fs::remove_dir_all(source_directory).expect("remove local duplicate directory");
    assert!(media_id > 0);
}

#[tokio::test]
async fn test_webdav_import_waits_for_active_uploads_before_claiming() {
    let _webdav_test_guard = lock_webdav_test().await;
    let pool = create_test_db();
    let username = format!("import-gate-{}", uuid::Uuid::new_v4());
    let user_id = create_test_user(&pool, &username, "import-gate@example.com");
    let (executors, data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let user_root = data_directory.join("webdav").join(&username);
    std::fs::create_dir_all(&user_root).expect("WebDAV user directory");
    let source_path = user_root.join("photo.jpg");
    std::fs::write(&source_path, b"incomplete upload").expect("staged upload");

    let mut config = Config::default();
    config.webdav.stable_file_age_seconds = 0;
    let gate = Arc::new(tokio::sync::RwLock::new(()));
    let upload_permit = gate.read().await;
    let scheduler = crate::test_utils::test_scheduler(pool.clone());
    run_webdav_import_cycle(&config, &executors, &gate, &scheduler).await;
    assert!(source_path.exists());

    std::fs::write(&source_path, b"complete upload").expect("complete staged upload");
    mark_webdav_file_ready(&pool, user_id, "photo.jpg");
    let cycle_config = config.clone();
    let cycle_gate = Arc::clone(&gate);
    let cycle_scheduler = scheduler.clone();
    let cycle_executors = executors.clone();
    let cycle = tokio::spawn(async move {
        run_webdav_import_cycle(
            &cycle_config,
            &cycle_executors,
            &cycle_gate,
            &cycle_scheduler,
        )
        .await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!cycle.is_finished());
    drop(upload_permit);
    tokio::time::timeout(Duration::from_secs(5), cycle)
        .await
        .expect("import cycle timeout")
        .expect("import cycle");
    momento_api::io::recovery::recover_generic_file_operations(&executors)
        .await
        .expect("source cleanup");
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

    std::fs::remove_file(data_directory.join("originals").join(media_path))
        .expect("remove imported original");
    std::fs::remove_dir_all(user_root).expect("remove WebDAV test directory");
}

#[tokio::test]
async fn test_webdav_import_rechecks_readiness_after_waiting_for_uploads() {
    let _webdav_test_guard = lock_webdav_test().await;
    let pool = create_test_db();
    let username = format!("readiness-race-{}", uuid::Uuid::new_v4());
    let user_id = create_test_user(&pool, &username, "readiness-race@example.com");
    let (executors, data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let user_root = data_directory.join("webdav").join(&username);
    std::fs::create_dir_all(&user_root).expect("WebDAV user directory");
    let source_path = user_root.join("video.mp4");
    std::fs::write(&source_path, b"previous completed upload").expect("staged upload");
    mark_webdav_file_ready(&pool, user_id, "video.mp4");

    let mut config = Config::default();
    config.webdav.stable_file_age_seconds = 0;
    let gate = Arc::new(tokio::sync::RwLock::new(()));
    let upload_permit = gate.read().await;
    let cycle_config = config.clone();
    let cycle_gate = Arc::clone(&gate);
    let cycle_scheduler = crate::test_utils::test_scheduler(pool.clone());
    let cycle_executors = executors;
    let cycle = tokio::spawn(async move {
        run_webdav_import_cycle(
            &cycle_config,
            &cycle_executors,
            &cycle_gate,
            &cycle_scheduler,
        )
        .await;
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
    let pool = create_test_db();
    let first_user_id = create_test_user(
        &pool,
        "webdav-duplicate-owner",
        "webdav-duplicate-owner@example.com",
    );
    let username = format!("webdav-duplicate-{}", uuid::Uuid::new_v4());
    let second_user_id = create_test_user(&pool, &username, &format!("{username}@example.com"));
    let (seed_executors, data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let (first_staged_source, first_source_path) = staged_file(
        &data_directory,
        StorageRootId::Imports,
        &format!("webdav-seed-{}/original.jpg", uuid::Uuid::new_v4()),
    );
    std::fs::write(&first_source_path, b"shared WebDAV bytes").expect("first source");
    let seed_admission = seed_executors
        .scheduler
        .acquire_durable(
            momento_api::runtime::DurableSourceId::LocalImport,
            momento_api::runtime::SchedulerAdmissionKind::NewClaim,
        )
        .await
        .expect("seed import admission");
    let media_id = import_staged_file(
        first_staged_source,
        ImportSource::Local,
        first_user_id,
        &seed_executors,
        StagedImportCleanup {
            source: false,
            supplemental_metadata: true,
        },
        &seed_admission,
    )
    .await
    .expect("first import")
    .completed_media_id()
    .expect("first import was not deferred");

    let user_root = data_directory.join("webdav").join(&username);
    std::fs::create_dir_all(&user_root).expect("WebDAV user directory");
    let duplicate_path = user_root.join("duplicate(2).jpg");
    std::fs::write(&duplicate_path, b"shared WebDAV bytes").expect("WebDAV duplicate");
    let duplicate_sidecar_path = user_root.join("duplicate.jpg.supplemental-metadata(2).json");
    std::fs::write(
        &duplicate_sidecar_path,
        b"{\"description\":\"WebDAV sidecar\"}",
    )
    .expect("WebDAV sidecar");
    mark_webdav_file_ready(&pool, second_user_id, "duplicate(2).jpg");
    mark_webdav_file_ready(
        &pool,
        second_user_id,
        "duplicate.jpg.supplemental-metadata(2).json",
    );
    let mut config = Config::default();
    config.webdav.stable_file_age_seconds = 0;
    let gate = Arc::new(tokio::sync::RwLock::new(()));

    run_webdav_import_cycle(
        &config,
        &seed_executors,
        &gate,
        &crate::test_utils::test_scheduler(pool.clone()),
    )
    .await;
    momento_api::io::recovery::recover_generic_file_operations(&seed_executors)
        .await
        .expect("source cleanup");

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
            data_directory
                .join("originals")
                .join(format!("{canonical_file_path}.supplemental-metadata.json"))
        )
        .expect("canonical sidecar"),
        b"{\"description\":\"WebDAV sidecar\"}"
    );

    std::fs::remove_dir_all(user_root).expect("remove WebDAV test directory");
}

#[tokio::test]
async fn test_concurrent_matching_hash_imports_create_one_media_row() {
    let _filesystem_test_guard = lock_webdav_test().await;
    let database_directory = tempfile::tempdir().expect("database directory");
    let pool = create_pool_at(&database_directory.path().join("database.sqlite"), 2)
        .expect("database pool");
    init_database(&pool.get().expect("connection")).expect("database schema");
    let user_id = create_test_user(
        &pool,
        "concurrent-duplicate",
        "concurrent-duplicate@example.com",
    );
    let (executors, data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let unique_name = uuid::Uuid::new_v4();
    let (first_staged_source, first_source_path) = staged_file(
        &data_directory,
        StorageRootId::Imports,
        &format!("concurrent-{unique_name}/first.jpg"),
    );
    let (second_staged_source, second_source_path) = staged_file(
        &data_directory,
        StorageRootId::Imports,
        &format!("concurrent-{unique_name}/second.jpg"),
    );
    std::fs::write(&first_source_path, b"concurrent identical bytes").expect("first source");
    std::fs::write(&second_source_path, b"concurrent identical bytes").expect("second source");
    let first_admission = executors
        .scheduler
        .acquire_durable(
            momento_api::runtime::DurableSourceId::LocalImport,
            momento_api::runtime::SchedulerAdmissionKind::NewClaim,
        )
        .await
        .expect("first import admission");
    let second_admission = executors
        .scheduler
        .acquire_durable(
            momento_api::runtime::DurableSourceId::LocalImport,
            momento_api::runtime::SchedulerAdmissionKind::NewClaim,
        )
        .await
        .expect("second import admission");

    let (first_result, second_result) = tokio::join!(
        import_staged_file(
            first_staged_source,
            ImportSource::Local,
            user_id,
            &executors,
            StagedImportCleanup {
                source: false,
                supplemental_metadata: true,
            },
            &first_admission,
        ),
        import_staged_file(
            second_staged_source,
            ImportSource::Local,
            user_id,
            &executors,
            StagedImportCleanup {
                source: false,
                supplemental_metadata: true,
            },
            &second_admission,
        ),
    );

    let first_media_id = finish_deferred_import(
        first_result.expect("first import"),
        &executors,
        &first_admission,
    )
    .await;
    let second_media_id = finish_deferred_import(
        second_result.expect("second import"),
        &executors,
        &second_admission,
    )
    .await;
    let media_count: i64 = pool
        .get()
        .expect("connection")
        .query_row("SELECT COUNT(*) FROM media", [], |row| row.get(0))
        .expect("media count");
    assert_eq!(first_media_id, second_media_id);
    assert_eq!(media_count, 1);
}

#[tokio::test]
async fn test_webdav_import_handles_nested_ready_source() {
    let _webdav_test_guard = lock_webdav_test().await;
    let pool = create_test_db();
    let username = format!("nested-import-{}", uuid::Uuid::new_v4());
    let user_id = create_test_user(&pool, &username, "nested-import@example.com");
    let (executors, data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let source_directory = data_directory
        .join("webdav")
        .join(&username)
        .join("Camera Roll");
    std::fs::create_dir_all(&source_directory).expect("source directory");
    std::fs::write(source_directory.join("photo.jpg"), b"photo bytes").expect("source file");
    mark_webdav_file_ready(&pool, user_id, "Camera Roll/photo.jpg");
    let mut config = Config::default();
    config.webdav.stable_file_age_seconds = 0;
    let gate = Arc::new(tokio::sync::RwLock::new(()));
    run_webdav_import_cycle(
        &config,
        &executors,
        &gate,
        &crate::test_utils::test_scheduler(pool.clone()),
    )
    .await;
    momento_api::io::recovery::recover_generic_file_operations(&executors)
        .await
        .expect("source cleanup");
    assert!(!source_directory.join("photo.jpg").exists());
    let media_count: i64 = pool
        .get()
        .expect("database")
        .query_row("SELECT COUNT(*) FROM media", [], |row| row.get(0))
        .expect("media count");
    assert_eq!(media_count, 1);
    std::fs::remove_dir_all(data_directory.join("webdav").join(username))
        .expect("remove WebDAV test directory");
}
