use momento_api::io::file::StorageRootId;
use momento_api::{
    database::queries,
    processor::{
        backup::{recover, run_cycle},
        import::{import_staged_file, ImportSource, StagedImportFile},
    },
};
use sha2::{Digest, Sha256};

use crate::test_utils::{
    create_test_db, create_test_user, test_executor_handles_with_data_directory,
};

fn test_file_hash(path: &std::path::Path) -> String {
    let bytes = std::fs::read(path).expect("read test file for hashing");
    format!("{:x}", Sha256::digest(bytes))
}

fn insert_lossless_manifest(connection: &rusqlite::Connection, asset_id: i64, content_hash: &str) {
    connection
        .execute(
            queries::backup::INSERT_MANIFEST,
            rusqlite::params![
                asset_id,
                2_i64,
                content_hash,
                r#"{"momentoBackup":{"schemaVersion":2,"source":"androidMediaStore"}}"#,
            ],
        )
        .expect("lossless manifest");
}

#[tokio::test]
async fn recovery_truncates_writing_file_to_durable_offset() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "backup-recovery", "backup-recovery@example.com");
    let staged_path = format!("processor-tests/{}/recovery.part", uuid::Uuid::new_v4());
    let (executors, executor_data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let file_path = executor_data_directory.join("backups").join(&staged_path);
    std::fs::create_dir_all(file_path.parent().expect("parent")).expect("staging parent");
    std::fs::write(&file_path, b"123456789").expect("staged bytes");

    let connection = pool.get().expect("database connection");
    connection
        .execute(
            queries::backup::UPSERT_DEVICE,
            rusqlite::params![user_id, "backup_device", "Backup device"],
        )
        .expect("device");
    connection
        .execute(
            queries::backup::INSERT_ASSET,
            rusqlite::params![
                user_id,
                "backup_device",
                "asset_001",
                "operation_001",
                "backup.jpg",
                "image/jpeg",
                9_i64,
                "2024-01-02T03:04:05Z",
                staged_path
            ],
        )
        .expect("asset");
    let asset_id = connection.last_insert_rowid();
    insert_lossless_manifest(
        &connection,
        asset_id,
        "15e2b0d3c33891ebb0f1ef609ec419420c20e320ce94c65fbc8c3312448eb225",
    );
    connection
        .execute(
            queries::backup::INSERT_SESSION,
            rusqlite::params!["recovery_upload", asset_id, user_id, 9_i64, "+24 hours"],
        )
        .expect("session");
    connection.execute("UPDATE backup_upload_sessions SET status = 'writing', uploaded_size = 4 WHERE upload_id = 'recovery_upload'", []).expect("writing session");

    recover(&executors).await.expect("recover backup uploads");

    assert_eq!(
        std::fs::metadata(&file_path)
            .expect("staged metadata")
            .len(),
        4
    );
    let session_status: String = connection
        .query_row(
            "SELECT status FROM backup_upload_sessions WHERE upload_id = 'recovery_upload'",
            [],
            |row| row.get(0),
        )
        .expect("recovered status");
    assert_eq!(session_status, "uploading");
    std::fs::remove_file(file_path).expect("remove staged file");
}

#[tokio::test]
async fn recovery_completes_backup_after_import_commits_before_asset_completion() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "backup-crash", "backup-crash@example.com");
    let (executors, data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let staged_path = format!("processor-tests/{}/crash.jpg", uuid::Uuid::new_v4());
    let source_path = data_directory.join("backups").join(&staged_path);
    std::fs::create_dir_all(source_path.parent().expect("parent")).expect("staging parent");
    std::fs::write(&source_path, b"crash boundary image").expect("staged bytes");
    let content_hash = test_file_hash(&source_path);

    let connection = pool.get().expect("database connection");
    connection
        .execute(
            queries::backup::UPSERT_DEVICE,
            rusqlite::params![user_id, "backup_device", "Backup device"],
        )
        .expect("device");
    connection
        .execute(
            queries::backup::INSERT_ASSET,
            rusqlite::params![
                user_id,
                "backup_device",
                "asset_crash",
                "operation_crash",
                "crash.jpg",
                "image/jpeg",
                19_i64,
                "2020-01-02T03:04:05Z",
                staged_path,
            ],
        )
        .expect("asset");
    let asset_id = connection.last_insert_rowid();
    insert_lossless_manifest(&connection, asset_id, &content_hash);
    connection
        .execute(
            queries::backup::INSERT_SESSION,
            rusqlite::params!["crash_upload", asset_id, user_id, 19_i64, "+24 hours"],
        )
        .expect("session");
    connection
        .execute(
            "UPDATE backup_assets SET status = 'processing', content_hash = ? WHERE id = ?",
            rusqlite::params![content_hash, asset_id],
        )
        .expect("processing asset");
    connection
        .execute(
            "UPDATE backup_upload_sessions SET status = 'processing', uploaded_size = 19 WHERE upload_id = 'crash_upload'",
            [],
        )
        .expect("processing session");

    let admission = executors
        .scheduler
        .acquire_durable(
            momento_api::runtime::DurableSourceId::BackupImport,
            momento_api::runtime::SchedulerAdmissionKind::ExistingClaimCompletion,
        )
        .await
        .expect("backup import admission");

    import_staged_file(
        StagedImportFile {
            storage_root: StorageRootId::Backups,
            path: momento_api::io::file::NormalizedStoragePath::parse(&staged_path)
                .expect("staged source"),
        },
        ImportSource::MobileBackup,
        user_id,
        &executors,
        momento_api::processor::import::StagedImportCleanup {
            source: false,
            supplemental_metadata: false,
        },
        &admission,
    )
    .await
    .expect("durably finalize import")
    .completed_media_id()
    .expect("backup import was not deferred");

    let (executors, executor_data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let recovery_path = executor_data_directory.join("backups").join(&staged_path);
    std::fs::create_dir_all(recovery_path.parent().expect("recovery parent"))
        .expect("recovery directory");
    std::fs::write(&recovery_path, b"crash boundary image").expect("recovery staged bytes");
    recover(&executors)
        .await
        .expect("reconcile interrupted backup");
    momento_api::io::recovery::recover_generic_file_operations(&executors)
        .await
        .expect("cleanup recovered backup staging");
    assert!(
        !recovery_path.exists(),
        "reconciliation must remove the staged copy"
    );

    let (status, media_id): (String, Option<i64>) = connection
        .query_row(
            "SELECT status, media_id FROM backup_assets WHERE id = ?",
            [asset_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("reconciled asset");
    assert_eq!(status, "completed");
    let media_id = media_id.expect("recovered media ID");
    let (media_path, import_state, import_source): (String, String, String) = connection
        .query_row(
            "SELECT file_path, import_state, import_source FROM media WHERE id = ?",
            [media_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("imported media");
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
    assert_eq!(import_source, "mobile_backup");
    assert_eq!(metadata_status, "queued");
    assert_eq!(access_count, 1);
    if source_path.exists() {
        std::fs::remove_file(&source_path).expect("remove test staging source");
    }
    std::fs::remove_file(executor_data_directory.join("originals").join(media_path))
        .expect("remove imported original");
}

#[tokio::test]
async fn cancelled_queued_backup_is_never_claimed_for_finalization() {
    let pool = create_test_db();
    let (executors, executor_data_directory) =
        test_executor_handles_with_data_directory(pool.clone());
    let user_id = create_test_user(&pool, "backup-cancelled", "backup-cancelled@example.com");
    let staged_path = format!("processor-tests/{}/cancelled.jpg", uuid::Uuid::new_v4());
    let source_path = executor_data_directory.join("backups").join(&staged_path);
    std::fs::create_dir_all(source_path.parent().expect("parent")).expect("staging parent");
    std::fs::write(&source_path, b"cancelled bytes").expect("staged bytes");
    let connection = pool.get().expect("database connection");
    connection
        .execute(
            queries::backup::UPSERT_DEVICE,
            rusqlite::params![user_id, "backup_device", "Backup device"],
        )
        .expect("device");
    connection
        .execute(
            queries::backup::INSERT_ASSET,
            rusqlite::params![
                user_id,
                "backup_device",
                "asset_cancelled",
                "operation_cancelled",
                "cancelled.jpg",
                "image/jpeg",
                15_i64,
                "2020-01-02T03:04:05Z",
                staged_path,
            ],
        )
        .expect("asset");
    let asset_id = connection.last_insert_rowid();
    insert_lossless_manifest(
        &connection,
        asset_id,
        "7a0d202a03185ab2b795f149414ca69a55815793f4e4089e1e3cf6ee103b12d4",
    );
    connection
        .execute(
            queries::backup::INSERT_SESSION,
            rusqlite::params!["cancelled_upload", asset_id, user_id, 15_i64, "+24 hours"],
        )
        .expect("session");
    connection
        .execute(
            "UPDATE backup_assets SET status = 'queued' WHERE id = ?",
            [asset_id],
        )
        .expect("queued asset");
    connection
        .execute(
            "UPDATE backup_upload_sessions SET status = 'queued', uploaded_size = 15 WHERE asset_id = ?",
            [asset_id],
        )
        .expect("queued session");
    let transaction = connection
        .unchecked_transaction()
        .expect("cancel transaction");
    transaction
        .execute(
            queries::backup::CANCEL_SESSION,
            rusqlite::params!["cancelled_upload", user_id],
        )
        .expect("cancel session");
    transaction
        .execute(queries::backup::CANCEL_ASSET, [asset_id])
        .expect("cancel asset");
    transaction.commit().expect("commit cancellation");

    run_cycle(&executors).await.expect("worker cycle");

    let media_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM media", [], |row| row.get(0))
        .expect("media count");
    let statuses: (String, String) = connection
        .query_row(
            "SELECT backup_assets.status, backup_upload_sessions.status FROM backup_assets JOIN backup_upload_sessions ON backup_upload_sessions.asset_id = backup_assets.id WHERE backup_assets.id = ?",
            [asset_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("cancelled statuses");
    assert_eq!(media_count, 0);
    assert_eq!(statuses, ("cancelled".to_string(), "cancelled".to_string()));
    std::fs::remove_file(source_path).expect("remove staged file");
}

#[tokio::test]
async fn recovery_fails_missing_nonzero_staging_file_without_resuming() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "backup-missing", "backup-missing@example.com");
    let connection = pool.get().expect("database connection");
    connection
        .execute(
            queries::backup::UPSERT_DEVICE,
            rusqlite::params![user_id, "backup_device", "Backup device"],
        )
        .expect("device");
    connection
        .execute(
            queries::backup::INSERT_ASSET,
            rusqlite::params![
                user_id,
                "backup_device",
                "asset_missing",
                "operation_missing",
                "missing.jpg",
                "image/jpeg",
                10_i64,
                "2020-01-02T03:04:05Z",
                "processor-tests/missing.jpg",
            ],
        )
        .expect("asset");
    let asset_id = connection.last_insert_rowid();
    insert_lossless_manifest(
        &connection,
        asset_id,
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    connection
        .execute(
            queries::backup::INSERT_SESSION,
            rusqlite::params!["missing_upload", asset_id, user_id, 10_i64, "+24 hours"],
        )
        .expect("session");
    connection
        .execute(
            "UPDATE backup_upload_sessions SET uploaded_size = 5 WHERE upload_id = 'missing_upload'",
            [],
        )
        .expect("durable offset");

    let executors = crate::test_utils::test_executor_handles(pool.clone());
    recover(&executors).await.expect("recover backup uploads");

    let statuses: (String, String) = connection
        .query_row(
            "SELECT backup_assets.status, backup_upload_sessions.status FROM backup_assets JOIN backup_upload_sessions ON backup_upload_sessions.asset_id = backup_assets.id WHERE backup_assets.id = ?",
            [asset_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("failed backup");
    assert_eq!(statuses, ("failed".to_string(), "failed".to_string()));
}

#[tokio::test]
async fn worker_finalizes_queued_backup_and_records_media() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "backup-worker", "backup-worker@example.com");
    let staged_path = format!("processor-tests/{}/queued.jpg", uuid::Uuid::new_v4());
    let (executors, executor_data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let file_path = executor_data_directory.join("backups").join(&staged_path);
    std::fs::create_dir_all(file_path.parent().expect("parent")).expect("staging parent");
    std::fs::write(&file_path, b"backup image bytes").expect("staged bytes");
    let expected_content_hash = test_file_hash(&file_path);

    let connection = pool.get().expect("database connection");
    connection
        .execute(
            queries::backup::UPSERT_DEVICE,
            rusqlite::params![user_id, "backup_device", "Backup device"],
        )
        .expect("device");
    connection
        .execute(
            queries::backup::INSERT_ASSET,
            rusqlite::params![
                user_id,
                "backup_device",
                "asset_queued",
                "operation_queued",
                "queued.jpg",
                "image/jpeg",
                18_i64,
                "2020-01-02T03:04:05Z",
                staged_path,
            ],
        )
        .expect("asset");
    let asset_id = connection.last_insert_rowid();
    insert_lossless_manifest(&connection, asset_id, &expected_content_hash);
    connection
        .execute(
            queries::backup::INSERT_SESSION,
            rusqlite::params!["queued_upload", asset_id, user_id, 18_i64, "+24 hours"],
        )
        .expect("session");
    connection
        .execute(
            "UPDATE backup_assets SET status = 'queued' WHERE id = ?",
            [asset_id],
        )
        .expect("queued asset");
    connection
        .execute(
            "UPDATE backup_upload_sessions SET status = 'queued', uploaded_size = 18 WHERE upload_id = 'queued_upload'",
            [],
        )
        .expect("queued session");

    std::fs::write(
        file_path.with_file_name(format!(
            "{}.supplemental-metadata.json",
            file_path
                .file_name()
                .and_then(|name| name.to_str())
                .expect("staged filename")
        )),
        r#"{"momentoBackup":{"schemaVersion":2,"source":"androidMediaStore"}}"#,
    )
    .expect("mirrored test sidecar");
    run_cycle(&executors).await.expect("worker cycle");
    momento_api::io::recovery::recover_generic_file_operations(&executors)
        .await
        .expect("backup staging cleanup");

    let (status, media_id): (String, Option<i64>) = connection
        .query_row(
            "SELECT status, media_id FROM backup_assets WHERE id = ?",
            [asset_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("completed backup");
    assert_eq!(status, "completed");
    let media_id = media_id.expect("recorded media ID");
    let (media_path, import_state, import_source, created_at): (String, String, String, String) =
        connection
            .query_row(
                "SELECT file_path, import_state, import_source, created_at FROM media WHERE id = ?",
                [media_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("imported media");
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
    assert!(!file_path.exists());
    assert_eq!(import_state, "imported");
    assert_eq!(import_source, "mobile_backup");
    assert_eq!(created_at, "2020-01-02 03:04:05");
    assert_eq!(metadata_status, "queued");
    assert_eq!(access_count, 1);
    let imported_path = executor_data_directory.join("originals").join(media_path);
    let imported_filename = imported_path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("imported filename");
    let metadata_path =
        imported_path.with_file_name(format!("{imported_filename}.supplemental-metadata.json"));
    assert_eq!(
        std::fs::read_to_string(&metadata_path).expect("preserved Android metadata"),
        r#"{"momentoBackup":{"schemaVersion":2,"source":"androidMediaStore"}}"#,
    );
    std::fs::remove_file(imported_path).expect("remove imported original");
    std::fs::remove_file(metadata_path).expect("remove imported metadata");
}
