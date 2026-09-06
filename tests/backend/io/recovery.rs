use momento_api::io::recovery::{
    discard_incomplete_file_products_after_restart, recover_generic_file_operations,
    recover_startup_critical_file_operations,
};
use sha2::Digest;

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
    assert!(recover_generic_file_operations(&executors).await.is_err());
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
            let roots = [
                "thumbnails",
                "tiny_thumbnails",
                "place_thumbnails",
                "previews",
            ];
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
