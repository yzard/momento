use std::sync::Arc;
use std::time::Duration;

use momento_api::io::file::NormalizedStoragePath;
use momento_api::io::file::StorageRootId;
use momento_api::{
    config::Config,
    database::{create_pool_at, init_database},
    processor::import::{
        import_staged_file, import_staged_file_to_completion, run_local_import,
        run_webdav_import_cycle, CreateImportJobOutcome, ImportSettings, ImportSource,
        StagedImportCleanup, StagedImportFile, StagedImportRequest,
    },
};
use sha2::{Digest, Sha256};

use crate::test_utils::{create_test_db, create_test_user, lock_webdav_test, QOI_FIXTURE};

#[tokio::test]
async fn restart_recopies_persisted_source_and_reuses_media_id() {
    for (root, import_source) in [
        (StorageRootId::Imports, "local"),
        (StorageRootId::WebDav, "webdav"),
        (StorageRootId::Backups, "mobile_backup"),
    ] {
        assert_restart_recopies_source(root, import_source).await;
    }
}

async fn assert_restart_recopies_source(root: StorageRootId, import_source: &str) {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "recopy", "recopy@example.com");
    let (executors, directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let (_, source) = staged_file(&directory, root, "nested/photo.jpg");
    let bytes = b"complete source bytes";
    let connection = pool.get().unwrap();
    connection.execute("INSERT INTO media (user_id, filename, original_filename, file_path, media_type, import_state, import_source, import_source_root, import_source_path) VALUES (?, '.importing', 'photo.jpg', '.importing/interrupted', 'image', 'importing', ?, ?, 'nested/photo.jpg')", rusqlite::params![user_id, import_source, root.directory_name()]).unwrap();
    let media_id = connection.last_insert_rowid();
    drop(connection);
    momento_api::processor::import::recover_interrupted_imports(&executors)
        .await
        .unwrap();
    // A temporarily unavailable mount/source is not a terminal business failure.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let state: String = pool
        .get()
        .unwrap()
        .query_row(
            "SELECT import_state FROM media WHERE id=?",
            [media_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "recovering");
    std::fs::write(&source, bytes).unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let state: String = pool
                .get()
                .unwrap()
                .query_row(
                    "SELECT import_state FROM media WHERE id=?",
                    [media_id],
                    |row| row.get(0),
                )
                .unwrap();
            if state == "imported" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("automatic source replay");
    let path: String = pool
        .get()
        .unwrap()
        .query_row(
            "SELECT file_path FROM media WHERE id=?",
            [media_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        std::fs::read(directory.join("originals").join(path)).unwrap(),
        bytes
    );
    assert_eq!(std::fs::read(source).unwrap(), bytes);
    let count: i64 = pool
        .get()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM media_metadata_jobs WHERE media_id=?",
            [media_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn recovery_marks_explicit_unsupported_media_failure() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "rejected", "rejected@example.com");
    let (executors, _directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let connection = pool.get().unwrap();
    connection.execute("INSERT INTO media (user_id, filename, original_filename, file_path, media_type, import_state, import_source_root, import_source_path) VALUES (?, 'source.txt', 'source.txt', '.importing/interrupted', 'image', 'importing', 'imports', 'source.txt')", [user_id]).unwrap();
    let media_id = connection.last_insert_rowid();
    drop(connection);
    momento_api::processor::import::recover_interrupted_imports(&executors)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let (state, error): (String, Option<String>) = pool
                .get()
                .unwrap()
                .query_row(
                    "SELECT import_state, import_error FROM media WHERE id=?",
                    [media_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            if state == "failed" {
                assert!(error.unwrap().contains("unsupported media file"));
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("business rejection is terminal");
}

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

#[tokio::test]
async fn committed_import_reuses_pending_cleanup_without_republishing_sidecar() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "cleanup-replay", "cleanup-replay@example.com");
    let (executors, directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let (source, path) = staged_file(&directory, StorageRootId::Imports, "replay/photo.jpg");
    std::fs::write(&path, b"immutable original").unwrap();
    let sidecar = path.with_file_name("photo.jpg.supplemental-metadata.json");
    std::fs::write(&sidecar, br#"{"description":"same sidecar"}"#).unwrap();
    let admission = executors
        .scheduler
        .acquire_durable(
            momento_api::runtime::DurableSourceId::LocalImport,
            momento_api::runtime::SchedulerAdmissionKind::NewClaim,
        )
        .await
        .unwrap();
    let cleanup = StagedImportCleanup {
        source: true,
        supplemental_metadata: true,
    };
    let media_id = import_staged_file(
        source.clone(),
        ImportSource::Local,
        user_id,
        &executors,
        cleanup,
        &admission,
    )
    .await
    .unwrap()
    .completed_media_id()
    .unwrap();
    let journal_count = || {
        pool.get()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM file_operation_groups", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap()
    };
    let initial_count = journal_count();
    for _ in 0..40 {
        assert_eq!(
            import_staged_file(
                source.clone(),
                ImportSource::Local,
                user_id,
                &executors,
                cleanup,
                &admission
            )
            .await
            .unwrap()
            .completed_media_id(),
            Some(media_id)
        );
    }
    assert_eq!(
        journal_count(),
        initial_count,
        "replays must not publish or queue cleanup again"
    );
    assert!(path.exists());
    assert!(sidecar.exists());
    // A changed sidecar at the same path is not the previous committed operation.
    std::fs::write(&sidecar, br#"{"description":"changed sidecar bytes"}"#).unwrap();
    assert!(matches!(
        import_staged_file(
            source,
            ImportSource::Local,
            user_id,
            &executors,
            cleanup,
            &admission
        )
        .await
        .unwrap(),
        momento_api::processor::import::ImportStagedFileOutcome::SourceCleanupBusy
    ));
    assert_eq!(
        journal_count(),
        initial_count,
        "blocked sources must not publish a sidecar"
    );
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
            momento_api::processor::import::ImportStagedFileOutcome::SourceCleanupBusy
            | momento_api::processor::import::ImportStagedFileOutcome::RecoveryPending(_) => {
                panic!("unexpected source cleanup conflict in fixture")
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
    let expected_source_path = staged_source.path.relative_path().to_string();
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
    let source_descriptor: (String, String, bool, bool) = pool.get().unwrap().query_row(
        "SELECT import_source_root, import_source_path, import_cleanup_source, import_cleanup_sidecar FROM media WHERE id=?",
        [media_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).unwrap();
    assert_eq!(
        source_descriptor,
        ("imports".to_string(), expected_source_path, false, true)
    );
}

#[tokio::test]
async fn staged_import_retries_a_busy_database_without_failing_the_media() {
    let _filesystem_test_guard = lock_webdav_test().await;
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "busy-import-retry", "busy-import-retry@example.com");
    let (executors, data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let (staged_source, source_path) = staged_file(
        &data_directory,
        StorageRootId::Imports,
        &format!("busy-retry-{}/photo.jpg", uuid::Uuid::new_v4()),
    );
    std::fs::write(&source_path, b"retry database busy import").expect("import source");
    let admission = executors
        .scheduler
        .acquire_durable(
            momento_api::runtime::DurableSourceId::LocalImport,
            momento_api::runtime::SchedulerAdmissionKind::NewClaim,
        )
        .await
        .expect("import admission");
    let database_path = data_directory.join("database.sqlite");
    let writer = rusqlite::Connection::open(&database_path).expect("busy writer");
    writer
        .execute_batch("BEGIN IMMEDIATE")
        .expect("hold SQLite write lock");

    let import_executors = executors.clone();
    let import_task = tokio::spawn(async move {
        import_staged_file_to_completion(
            StagedImportRequest {
                source: staged_source,
                import_source: ImportSource::Local,
                user_id,
                cleanup: StagedImportCleanup {
                    source: false,
                    supplemental_metadata: false,
                },
                durable_source: momento_api::runtime::DurableSourceId::LocalImport,
            },
            &import_executors,
            &import_executors.scheduler,
            admission,
        )
        .await
    });
    tokio::time::sleep(Duration::from_secs(6)).await;
    writer.execute_batch("COMMIT").expect("release SQLite lock");

    let media_id = import_task
        .await
        .expect("import task")
        .expect("busy database retry import");
    let (import_state, failure): (String, Option<String>) = pool
        .get()
        .expect("database")
        .query_row(
            "SELECT import_state, import_error FROM media WHERE id = ?",
            [media_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("imported media");
    assert_eq!(import_state, "imported");
    assert_eq!(failure, None);
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
async fn duplicate_import_retries_cleanup_path_conflicts_without_failing_or_committing_access() {
    use momento_api::processor::import::ImportStagedFileOutcome;
    use momento_api::runtime::{DurableSourceId, SchedulerAdmissionKind};
    for (root, import_source, durable_source) in [
        (
            StorageRootId::Imports,
            ImportSource::Local,
            DurableSourceId::LocalImport,
        ),
        (
            StorageRootId::WebDav,
            ImportSource::Webdav,
            DurableSourceId::WebDavImport,
        ),
    ] {
        let pool = create_test_db();
        let owner = create_test_user(&pool, "owner", "owner@example.com");
        let importing_user = create_test_user(&pool, "importer", "importer@example.com");
        let (executors, directory) =
            crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
        let (first, first_path) = staged_file(&directory, root, "first.jpg");
        std::fs::write(&first_path, b"duplicate content").unwrap();
        let admission = executors
            .scheduler
            .acquire_durable(durable_source, SchedulerAdmissionKind::NewClaim)
            .await
            .unwrap();
        let media_id = import_staged_file(
            first,
            import_source,
            owner,
            &executors,
            StagedImportCleanup {
                source: false,
                supplemental_metadata: false,
            },
            &admission,
        )
        .await
        .unwrap()
        .completed_media_id()
        .unwrap();
        drop(admission);
        let (duplicate, duplicate_path) = staged_file(&directory, root, "duplicate.jpg");
        std::fs::write(&duplicate_path, b"duplicate content").unwrap();
        {
            let connection = pool.get().unwrap();
            connection.execute("INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, state, entry_count) VALUES ('held-path', 'test', 'test', 'test', 'prepared', 1)", []).unwrap();
            connection.execute("INSERT INTO file_operation_path_claims (group_id, sequence, storage_root, relative_path, path_key, mode, scope, role) VALUES ('held-path', 0, ?, ?, ?, 'read', 'exact', 'source')",
                rusqlite::params![root.as_str(), duplicate.path.relative_path(), duplicate.path.path_key()]).unwrap();
        }
        let admission = executors
            .scheduler
            .acquire_durable(durable_source, SchedulerAdmissionKind::NewClaim)
            .await
            .unwrap();
        let outcome = import_staged_file(
            duplicate.clone(),
            import_source,
            importing_user,
            &executors,
            StagedImportCleanup {
                source: true,
                supplemental_metadata: false,
            },
            &admission,
        )
        .await
        .expect("conflict is not a database error");
        assert!(matches!(
            outcome,
            ImportStagedFileOutcome::SourceCleanupBusy
        ));
        assert!(duplicate_path.exists());
        {
            let connection = pool.get().unwrap();
            let access: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM media_access WHERE media_id = ? AND user_id = ?",
                    rusqlite::params![media_id, importing_user],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(access, 0, "conflicting transaction must roll back");
            let claims: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM import_content_hash_claims",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                claims, 0,
                "content-hash owner must be released before retry"
            );
        }
        let retry = import_staged_file_to_completion(
            StagedImportRequest {
                source: duplicate,
                import_source,
                user_id: importing_user,
                cleanup: StagedImportCleanup {
                    source: true,
                    supplemental_metadata: false,
                },
                durable_source,
            },
            &executors,
            &executors.scheduler,
            admission,
        );
        tokio::pin!(retry);
        tokio::select! {
            result = &mut retry => panic!("path conflict should wait: {result:?}"),
            () = tokio::time::sleep(Duration::from_millis(100)) => {}
        }
        pool.get()
            .unwrap()
            .execute(
                "DELETE FROM file_operation_groups WHERE id = 'held-path'",
                [],
            )
            .unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), retry)
                .await
                .unwrap()
                .unwrap(),
            media_id
        );
        momento_api::io::recovery::recover_generic_file_operations(&executors)
            .await
            .unwrap();
        assert!(!duplicate_path.exists());
        let connection = pool.get().unwrap();
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM media", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
        let access: i64 = connection.query_row("SELECT COUNT(*) FROM media_access WHERE media_id = ? AND user_id = ? AND deleted_at IS NULL", rusqlite::params![media_id, importing_user], |row| row.get(0)).unwrap();
        assert_eq!(access, 1);
    }
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
    let database_directory = crate::temporary::tempdir().expect("database directory");
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
async fn duplicate_import_waits_for_the_existing_content_hash_owner_before_replacing_sidecar() {
    let _filesystem_test_guard = lock_webdav_test().await;
    let pool = create_test_db();
    let user_id = create_test_user(
        &pool,
        "serialized-duplicate",
        "serialized-duplicate@example.com",
    );
    let (executors, data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let directory_name = format!("serialized-{}", uuid::Uuid::new_v4());
    let source_bytes = b"serialized duplicate bytes";
    let (first_source, first_path) = staged_file(
        &data_directory,
        StorageRootId::Imports,
        &format!("{directory_name}/first.jpg"),
    );
    std::fs::write(&first_path, source_bytes).expect("first source");
    std::fs::write(
        first_path.with_file_name("first.jpg.supplemental-metadata.json"),
        b"{\"description\":\"first\"}",
    )
    .expect("first sidecar");
    let first_admission = executors
        .scheduler
        .acquire_durable(
            momento_api::runtime::DurableSourceId::LocalImport,
            momento_api::runtime::SchedulerAdmissionKind::NewClaim,
        )
        .await
        .expect("first import admission");
    let media_id = import_staged_file(
        first_source,
        ImportSource::Local,
        user_id,
        &executors,
        StagedImportCleanup {
            source: false,
            supplemental_metadata: false,
        },
        &first_admission,
    )
    .await
    .expect("first import")
    .completed_media_id()
    .expect("first import was not deferred");

    let content_hash = Sha256::digest(source_bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let competing_claim_token = uuid::Uuid::new_v4().to_string();
    pool.get()
        .expect("database")
        .execute(
            momento_api::database::queries::import::INSERT_CONTENT_HASH_CLAIM,
            rusqlite::params![content_hash, competing_claim_token, "local"],
        )
        .expect("competing content-hash claim");

    let (second_source, second_path) = staged_file(
        &data_directory,
        StorageRootId::Imports,
        &format!("{directory_name}/second.jpg"),
    );
    std::fs::write(&second_path, source_bytes).expect("second source");
    std::fs::write(
        second_path.with_file_name("second.jpg.supplemental-metadata.json"),
        b"{\"description\":\"second\"}",
    )
    .expect("second sidecar");
    let second_admission = executors
        .scheduler
        .acquire_durable(
            momento_api::runtime::DurableSourceId::LocalImport,
            momento_api::runtime::SchedulerAdmissionKind::NewClaim,
        )
        .await
        .expect("second import admission");
    let deferred = import_staged_file(
        second_source,
        ImportSource::Local,
        user_id,
        &executors,
        StagedImportCleanup {
            source: false,
            supplemental_metadata: false,
        },
        &second_admission,
    )
    .await
    .expect("deferred duplicate import");
    let momento_api::processor::import::ImportStagedFileOutcome::Deferred(prepared) = deferred
    else {
        panic!("duplicate import bypassed its active content-hash owner");
    };

    pool.get()
        .expect("database")
        .execute(
            momento_api::database::queries::import::RELEASE_CONTENT_HASH_CLAIM,
            rusqlite::params![content_hash, competing_claim_token],
        )
        .expect("release competing content-hash claim");
    let resumed_media_id = finish_deferred_import(
        momento_api::processor::import::resume_staged_file_import(
            *prepared,
            &executors,
            &second_admission,
        )
        .await
        .expect("resume duplicate import"),
        &executors,
        &second_admission,
    )
    .await;
    assert_eq!(resumed_media_id, media_id);

    let canonical_path: String = pool
        .get()
        .expect("database")
        .query_row(
            "SELECT file_path FROM media WHERE id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("canonical path");
    assert_eq!(
        std::fs::read(
            data_directory
                .join("originals")
                .join(format!("{canonical_path}.supplemental-metadata.json")),
        )
        .expect("canonical sidecar"),
        b"{\"description\":\"second\"}"
    );
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
