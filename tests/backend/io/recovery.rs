use momento_api::io::recovery::{
    discard_incomplete_file_products_after_restart, recover_generic_file_operations,
    recover_startup_critical_file_operations,
};
use sha2::Digest;

#[tokio::test]
async fn interrupted_import_rolls_back_only_owned_partial_files() {
    for (owned, temporary, role, can_rollback) in [
        (
            true,
            ".importing/.importing-interrupted",
            "temporary_original",
            true,
        ),
        (
            true,
            ".importing/sidecar-interrupted",
            "temporary_sidecar",
            true,
        ),
        (
            false,
            ".importing/.importing-interrupted",
            "temporary_original",
            false,
        ),
        (true, "unrelated-file", "temporary_original", false),
    ] {
        let pool = crate::test_utils::create_test_db();
        let (executors, directory) =
            crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
        std::fs::create_dir_all(directory.join("originals/.importing")).unwrap();
        std::fs::write(directory.join("originals").join(temporary), b"partial").unwrap();
        std::fs::write(directory.join("imports/source.mp4"), b"complete original").unwrap();
        std::fs::write(
            directory.join("originals/canonical.mp4"),
            b"existing canonical",
        )
        .unwrap();
        let connection = pool.get().unwrap();
        connection.execute("INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, state, entry_count) VALUES ('partial-import', 'import_media_publication', 'import', '1', 'prepared', 1)", []).unwrap();
        connection.execute("INSERT INTO file_operation_entries (group_id, sequence, action, storage_root, temporary_path, destination_path, expected_size, expected_sha256) VALUES ('partial-import', 0, 'publish', 'originals', ?, 'canonical.mp4', 1000, zeroblob(32))", [temporary]).unwrap();
        if owned {
            connection.execute("INSERT INTO file_operation_path_claims (group_id, sequence, storage_root, relative_path, path_key, mode, scope, role) VALUES ('partial-import', 0, 'originals', ?, ?, 'write', 'exact', ?)", rusqlite::params![temporary, temporary, role]).unwrap();
        }
        drop(connection);
        momento_api::io::recovery::rollback_prepared_file_operations_after_restart(&executors)
            .await
            .unwrap();
        let recovery = recover_startup_critical_file_operations(&executors).await;
        if can_rollback {
            assert_eq!(recovery.unwrap(), 1);
            assert!(!directory.join("originals").join(temporary).exists());
            assert_eq!(
                recover_startup_critical_file_operations(&executors)
                    .await
                    .unwrap(),
                0
            );
        } else {
            assert!(recovery.is_err());
            assert!(directory.join("originals").join(temporary).exists());
        }
        assert_eq!(
            std::fs::read(directory.join("imports/source.mp4")).unwrap(),
            b"complete original"
        );
        assert_eq!(
            std::fs::read(directory.join("originals/canonical.mp4")).unwrap(),
            b"existing canonical"
        );
    }
}

#[tokio::test]
async fn busy_journal_head_is_deferred_without_blocking_following_cleanup() {
    let pool = crate::test_utils::create_test_db();
    let (executors, directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    for (id, position) in [("busy", 1), ("ready", 2)] {
        std::fs::write(directory.join("journal").join(id), b"cleanup").unwrap();
        let connection = pool.get().unwrap();
        connection.execute("INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, state, completion_outcome, entry_count, recovery_order) VALUES (?, 'test', 'test', '1', 'cleanup_pending', 'published', 1, ?)", rusqlite::params![id, position]).unwrap();
        connection.execute("INSERT INTO file_operation_entries (group_id, sequence, action, storage_root, source_path) VALUES (?, 0, 'cleanup', 'journal', ?)", rusqlite::params![id, id]).unwrap();
    }
    let held = executors
        .file_io
        .reserve_journal_mutation("busy", 1)
        .unwrap();
    let grant = executors
        .sqlite
        .verify_file_operation_cleanup_durable(&held, 1)
        .await
        .unwrap()
        .unwrap();
    let held = held.acquire(grant).unwrap();
    assert_eq!(
        recover_generic_file_operations(&executors).await.unwrap(),
        1
    );
    assert!(directory.join("journal/busy").exists());
    assert!(!directory.join("journal/ready").exists());
    let version: i64 = pool
        .get()
        .unwrap()
        .query_row(
            "SELECT version FROM file_operation_groups WHERE id='busy'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, 2);
    let delay = executors
        .sqlite
        .journal_retry_delay()
        .await
        .unwrap()
        .unwrap();
    drop(held);
    tokio::time::sleep(delay).await;
    assert_eq!(
        recover_generic_file_operations(&executors).await.unwrap(),
        1
    );
    assert!(!directory.join("journal/busy").exists());
    assert!(executors
        .sqlite
        .journal_retry_delay()
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn source_cleanup_permission_failure_is_logged_without_losing_the_source() {
    use std::os::unix::fs::PermissionsExt;
    use tracing::instrument::WithSubscriber;

    let pool = crate::test_utils::create_test_db();
    let (executors, directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let parent = directory.join("imports/locked");
    std::fs::create_dir(&parent).unwrap();
    let source = parent.join("photo.jpg");
    std::fs::write(&source, b"original").unwrap();
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o555)).unwrap();
    pool.get().unwrap().execute("INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, state, completion_outcome, entry_count, version) VALUES ('permission-cleanup', 'import_source_cleanup', 'import', '1', 'cleanup_pending', 'published', 1, 2)", []).unwrap();
    pool.get().unwrap().execute("INSERT INTO file_operation_entries (group_id, sequence, action, storage_root, source_path) VALUES ('permission-cleanup', 0, 'cleanup', 'imports', 'locked/photo.jpg')", []).unwrap();
    let buffer = crate::test_utils::LogBuffer::default();
    let writer = buffer.clone();
    let subscriber = std::sync::Arc::new(
        tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish(),
    );
    let recovered = recover_generic_file_operations(&executors)
        .with_subscriber(subscriber.clone())
        .await;
    // Restore fixture access before asserting so a failed test can clean up.
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
    recovered.unwrap();
    assert_eq!(std::fs::read(&source).unwrap(), b"original");
    let state: String = pool
        .get()
        .unwrap()
        .query_row(
            "SELECT state FROM file_operation_groups WHERE id='permission-cleanup'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "cleanup_failed");
    // A failed group must not be selected and logged again on every sweep.
    assert_eq!(
        recover_generic_file_operations(&executors)
            .with_subscriber(subscriber.clone())
            .await
            .unwrap(),
        0
    );
    let log = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    for text in [
        "WARN",
        "Journal cleanup failed",
        "permission-cleanup",
        "locked/photo.jpg",
        "Imports",
        "FilePermission",
        "Permission denied",
    ] {
        assert!(log.contains(text), "missing {text}: {log}");
    }
    assert_eq!(log.matches("Journal cleanup failed").count(), 1);
}

#[tokio::test]
async fn completed_result_cleanup_does_not_block_startup() {
    let pool = crate::test_utils::create_test_db();
    let (executors, directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let connection = pool.get().unwrap();
    connection.execute("INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, state, completion_outcome, entry_count, version) VALUES ('finished', 'llm_result_receive', 'llm_result', '1', 'cleanup_pending', 'discarded', 1, 2)", []).unwrap();
    connection.execute("INSERT INTO file_operation_entries (group_id, sequence, action, storage_root, source_path) VALUES ('finished', 0, 'cleanup', 'journal', 'finished.records')", []).unwrap();
    drop(connection);
    std::fs::write(directory.join("journal/finished.records"), b"processed").unwrap();
    assert_eq!(
        recover_startup_critical_file_operations(&executors)
            .await
            .unwrap(),
        0
    );
    assert!(directory.join("journal/finished.records").exists());
    assert_eq!(
        recover_generic_file_operations(&executors).await.unwrap(),
        1
    );
    assert!(!directory.join("journal/finished.records").exists());
}

#[tokio::test]
async fn failed_discard_keeps_metadata_queued_until_cleanup_can_retry() {
    let pool = crate::test_utils::create_test_db();
    let media_id = crate::test_utils::create_test_media(&pool, "retry.jpg");
    let (executors, directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let connection = pool.get().unwrap();
    connection
        .execute(
            "INSERT INTO media_metadata_jobs (media_id, status) VALUES (?, 'queued')",
            [media_id],
        )
        .unwrap();
    connection.execute("INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, state, product_target, product_version, entry_count, version) VALUES ('retry', 'metadata_artifacts', 'metadata_generation', ?, 'publishing', 'metadata_artifacts', 1, 1, 2)", [media_id.to_string()]).unwrap();
    connection.execute("INSERT INTO file_operation_entries (group_id, sequence, action, storage_root, temporary_path, destination_path) VALUES ('retry', 0, 'publish', 'thumbnails', 'partial', 'parent/derived')", []).unwrap();
    drop(connection);
    std::fs::write(directory.join("thumbnails/partial"), b"broken").unwrap();
    std::fs::write(directory.join("thumbnails/parent"), b"obstruction").unwrap();
    discard_incomplete_file_products_after_restart(&executors)
        .await
        .unwrap();
    assert_eq!(
        recover_generic_file_operations(&executors).await.unwrap(),
        0
    );
    assert!(executors
        .sqlite
        .claim_next_metadata_job_durable()
        .await
        .unwrap()
        .is_none());
    let state: String = pool
        .get()
        .unwrap()
        .query_row(
            "SELECT state FROM file_operation_groups WHERE id = 'retry'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "cleanup_pending");
    assert!(!directory.join("thumbnails/partial").exists());
    std::fs::remove_file(directory.join("thumbnails/parent")).unwrap();
    std::fs::create_dir(directory.join("thumbnails/parent")).unwrap();
    std::fs::write(directory.join("thumbnails/parent/derived"), b"broken").unwrap();
    tokio::time::sleep(
        executors
            .sqlite
            .journal_retry_delay()
            .await
            .unwrap()
            .unwrap(),
    )
    .await;
    assert_eq!(
        recover_generic_file_operations(&executors).await.unwrap(),
        1
    );
    assert!(executors
        .sqlite
        .claim_next_metadata_job_durable()
        .await
        .unwrap()
        .is_some());
    assert!(!directory.join("thumbnails/parent/derived").exists());
}

#[tokio::test]
async fn recovery_verifies_only_the_entry_it_will_apply() {
    let pool = crate::test_utils::create_test_db();
    let (executors, directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let connection = pool.get().unwrap();
    connection.execute("INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, state, entry_count, version) VALUES ('ordered', 'test', 'test', '1', 'publishing', 2, 2)", []).unwrap();
    for sequence in 0..2 {
        let hash = if sequence == 0 {
            sha2::Sha256::digest(b"valid").to_vec()
        } else {
            vec![0; 32]
        };
        connection.execute("INSERT INTO file_operation_entries (group_id, sequence, action, storage_root, temporary_path, destination_path, expected_sha256) VALUES ('ordered', ?, 'publish', 'thumbnails', ?, ?, ?)", rusqlite::params![sequence, format!("pending-{sequence}"), format!("final-{sequence}"), hash]).unwrap();
        std::fs::write(
            directory.join(format!("thumbnails/pending-{sequence}")),
            b"valid",
        )
        .unwrap();
    }
    drop(connection);
    assert_eq!(
        recover_generic_file_operations(&executors).await.unwrap(),
        1
    );
    assert!(directory.join("thumbnails/final-0").exists());
    assert!(!directory.join("thumbnails/final-1").exists());
    let failed_sequence: i64 = pool.get().unwrap().query_row("SELECT sequence FROM file_operation_entries WHERE group_id = 'ordered' AND last_error IS NOT NULL", [], |row| row.get(0)).unwrap();
    assert_eq!(failed_sequence, 1);
}

#[tokio::test]
async fn discarded_metadata_is_requeued_only_after_cleanup_and_preserves_successful_generation() {
    for succeeded in [false, true] {
        let pool = crate::test_utils::create_test_db();
        let media_id = crate::test_utils::create_test_media(&pool, "rerun.jpg");
        let (executors, directory) =
            crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
        let connection = pool.get().unwrap();
        connection
            .execute(
                "INSERT INTO media_metadata_jobs (media_id, status) VALUES (?, ?)",
                rusqlite::params![media_id, if succeeded { "completed" } else { "failed" }],
            )
            .unwrap();
        if succeeded {
            connection
                .execute(
                    "UPDATE media_metadata SET artifact_version = 1, thumbnail_path = 'derived' WHERE media_id = ?",
                    [media_id],
                )
                .unwrap();
        }
        connection.execute("INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, state, product_target, product_version, entry_count, version) VALUES ('rerun', 'metadata_artifacts', 'metadata_generation', ?, 'publication_failed', 'metadata_artifacts', 1, 1, 2)", [media_id.to_string()]).unwrap();
        connection.execute("INSERT INTO file_operation_entries (group_id, sequence, action, storage_root, temporary_path, destination_path) VALUES ('rerun', 0, 'publish', 'thumbnails', 'partial', 'derived')", []).unwrap();
        std::fs::write(directory.join("thumbnails/partial"), b"broken").unwrap();
        std::fs::write(directory.join("thumbnails/derived"), b"published").unwrap();
        drop(connection);
        discard_incomplete_file_products_after_restart(&executors)
            .await
            .unwrap();
        assert!(executors
            .sqlite
            .claim_next_metadata_job_durable()
            .await
            .unwrap()
            .is_none());
        recover_startup_critical_file_operations(&executors)
            .await
            .unwrap();
        let job = executors
            .sqlite
            .claim_next_metadata_job_durable()
            .await
            .unwrap();
        assert_eq!(job.is_some(), !succeeded);
        assert!(!directory.join("thumbnails/partial").exists());
        assert_eq!(directory.join("thumbnails/derived").exists(), succeeded);
    }
}

#[tokio::test]
async fn interrupted_products_discard_corrupt_partial_files_without_the_expired_owner() {
    for (kind, owner, target) in [
        (
            "metadata_artifacts",
            "metadata_generation",
            "metadata_artifacts",
        ),
        (
            "llm_result_artifacts",
            "llm_result",
            "llm_result_face_crops",
        ),
        ("llm_result_receive", "llm_result", "llm_result_inbox"),
    ] {
        for detached in [false, true] {
            let pool = crate::test_utils::create_test_db();
            let (executors, directory) =
                crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
            let connection = pool.get().unwrap();
            connection.execute(
                "INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, claim_token, state, product_target, cancel_requested, entry_count, version) VALUES ('interrupted', ?, ?, '1', '00000000-0000-0000-0000-000000000091', 'publishing', ?, ?, 4, 6)",
                rusqlite::params![kind, owner, if detached { None } else { Some(target) }, detached],
            ).unwrap();
            let roots = ["thumbnails", "tiny_thumbnails", "previews", "previews"];
            for (sequence, root) in roots.iter().enumerate() {
                connection.execute(
                    "INSERT INTO file_operation_entries (group_id, sequence, action, storage_root, temporary_path, destination_path, state, expected_size, expected_sha256) VALUES ('interrupted', ?, 'publish', ?, ?, ?, ?, 500, zeroblob(32))",
                    rusqlite::params![sequence, root, format!("partial-{sequence}"), format!("derived-{sequence}"), if sequence < 3 { "committed" } else { "prepared" }],
                ).unwrap();
                // Both paths may exist after an interrupted publication. Neither
                // the partial size nor hash need match to discard owned output.
                let root = momento_api::io::file::StorageRootId::try_from(*root)
                    .unwrap()
                    .directory_name();
                std::fs::write(
                    directory.join(format!("{root}/partial-{sequence}")),
                    b"broken",
                )
                .unwrap();
                std::fs::write(
                    directory.join(format!("{root}/derived-{sequence}")),
                    b"broken",
                )
                .unwrap();
            }
            std::fs::write(directory.join("originals/original.jpg"), b"original").unwrap();
            std::fs::write(directory.join("thumbnails/successful.jpg"), b"published").unwrap();
            drop(connection);

            assert_eq!(
                discard_incomplete_file_products_after_restart(&executors)
                    .await
                    .unwrap(),
                1
            );
            let recovered = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                recover_startup_critical_file_operations(&executors),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(recovered, 4);
            assert_eq!(
                recover_generic_file_operations(&executors).await.unwrap(),
                0
            );
            let state: (String, String, String) = pool.get().unwrap().query_row(
                "SELECT state, completion_outcome, finalization_error_kind FROM file_operation_groups WHERE id = 'interrupted'", [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            ).unwrap();
            assert_eq!(
                state,
                (
                    "cleaned".into(),
                    "discarded".into(),
                    "InterruptedProduct".into()
                )
            );
            for (sequence, root) in roots.iter().enumerate() {
                let root = momento_api::io::file::StorageRootId::try_from(*root)
                    .unwrap()
                    .directory_name();
                assert!(!directory
                    .join(format!("{root}/partial-{sequence}"))
                    .exists());
                assert!(!directory
                    .join(format!("{root}/derived-{sequence}"))
                    .exists());
            }
            assert_eq!(
                std::fs::read(directory.join("originals/original.jpg")).unwrap(),
                b"original"
            );
            assert_eq!(
                std::fs::read(directory.join("thumbnails/successful.jpg")).unwrap(),
                b"published"
            );
        }
    }
}

#[tokio::test]
async fn interrupted_video_frame_cleanup_preserves_current_and_snapshotted_inputs() {
    for reference in ["none", "media", "job"] {
        for state in [
            "publishing",
            "publication_failed",
            "files_committed",
            "finalize_failed",
        ] {
            let pool = crate::test_utils::create_test_db();
            let media_id = crate::test_utils::create_test_media(&pool, "video.mp4");
            let (executors, directory) =
                crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
            let connection = pool.get().unwrap();
            connection
                .execute(
                    "INSERT INTO media_metadata_jobs (media_id, status) VALUES (?, 'completed')",
                    [media_id],
                )
                .unwrap();
            connection.execute("INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, claim_token, state, entry_count, version) VALUES ('frame', 'video_ai_frame', 'generated_artifact', 'frame', '00000000-0000-0000-0000-000000000091', ?, 1, 2)", [state]).unwrap();
            connection.execute("INSERT INTO file_operation_entries (group_id, sequence, action, storage_root, temporary_path, destination_path, expected_size, expected_sha256) VALUES ('frame', 0, 'publish', 'previews', 'partial-frame', 'frame.png', 500, zeroblob(32))", []).unwrap();
            if reference == "media" {
                connection.execute("INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, 'ocr', 0, 'video_frame', 'previews', 'frame.png', 'frame.png', 'image/png', 5, 'hash')", [media_id]).unwrap();
            } else if reference == "job" {
                connection.execute("INSERT INTO llm_jobs (id, media_id, task, status) VALUES ('job', ?, 'ocr', 'submitted')", [media_id]).unwrap();
                connection.execute("INSERT INTO llm_job_inputs (job_id, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES ('job', 0, 'video_frame', 'previews', 'frame.png', 'frame.png', 'image/png', 5, 'hash')", []).unwrap();
            }
            drop(connection);
            std::fs::write(directory.join("previews/partial-frame"), b"partial").unwrap();
            std::fs::write(directory.join("previews/frame.png"), b"valid").unwrap();
            std::fs::write(directory.join("originals/video.mp4"), b"original").unwrap();
            assert_eq!(
                discard_incomplete_file_products_after_restart(&executors)
                    .await
                    .unwrap(),
                1
            );
            assert_eq!(
                recover_startup_critical_file_operations(&executors)
                    .await
                    .unwrap(),
                1
            );
            assert_eq!(
                recover_startup_critical_file_operations(&executors)
                    .await
                    .unwrap(),
                0
            );
            assert!(!directory.join("previews/partial-frame").exists());
            assert_eq!(
                directory.join("previews/frame.png").exists(),
                reference != "none"
            );
            if reference != "none" {
                assert_eq!(
                    std::fs::read(directory.join("previews/frame.png")).unwrap(),
                    b"valid"
                );
            }
            assert_eq!(
                std::fs::read(directory.join("originals/video.mp4")).unwrap(),
                b"original"
            );
            let connection = pool.get().unwrap();
            let status: String = connection
                .query_row(
                    "SELECT status FROM media_metadata_jobs WHERE media_id = ?",
                    [media_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(status, "completed");
            let status: String = connection
                .query_row(
                    "SELECT state FROM file_operation_groups WHERE id = 'frame'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(status, "cleaned");
        }
    }
}

#[tokio::test]
async fn unknown_stale_owner_does_not_spin_or_delete_originals() {
    let pool = crate::test_utils::create_test_db();
    let (executors, directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    pool.get().unwrap().execute(
        "INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, claim_token, state, entry_count, version) VALUES ('stale', 'import_media_publication', 'import', '1', '00000000-0000-0000-0000-000000000091', 'publishing', 1, 2)", [],
    ).unwrap();
    std::fs::write(directory.join("originals/original.jpg"), b"original").unwrap();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        recover_startup_critical_file_operations(&executors),
    )
    .await
    .unwrap();
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("journal recovery made no progress: stale version 2"));
    assert!(directory.join("originals/original.jpg").exists());
}
