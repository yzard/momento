use momento_api::io::file::{NormalizedStoragePath, PathClaimMode, PathClaimScope, StorageRootId};
use momento_api::io::journal::{
    DirectoryCopyConstructionPlan, DirectoryCopyCursor, DirectoryCopyEntryCheckpoint,
    DirectoryCopyFinishedCheckpoint, FileEntryAction, FileEntryPlan, FileOperationPlan,
    FilePathClaimPlan, JournalPlanError, JournalSpaceReservationPlan,
};
use momento_api::io::space_budget::{DataDirSpaceBudget, FilesystemSpaceSnapshot};

fn path(value: &str) -> NormalizedStoragePath {
    NormalizedStoragePath::parse(value).expect("normalized path")
}

fn journal_reservation(reservation_id: &str) -> JournalSpaceReservationPlan {
    let budget = DataDirSpaceBudget::from_snapshot(FilesystemSpaceSnapshot {
        filesystem_id: "device-1".to_string(),
        total_bytes: 100 * 1024 * 1024 * 1024,
        free_bytes: 100 * 1024 * 1024 * 1024,
        fragment_size: 4096,
    })
    .expect("space budget");
    budget
        .begin_reconstruction()
        .publish()
        .expect("space reconstruction");
    budget.mark_running().expect("running space budget");
    let token = budget
        .reserve_journal(reservation_id.to_string(), 4096)
        .expect("space admission")
        .into_result()
        .expect("journal capacity");
    JournalSpaceReservationPlan::new(token).expect("journal reservation")
}

fn publish_plan() -> FileOperationPlan {
    FileOperationPlan {
        group_id: "group-1".to_string(),
        kind: "media_import".to_string(),
        owner_kind: "import_job".to_string(),
        owner_id: "job-1".to_string(),
        claim_token: None,
        product_target: Some("media".to_string()),
        product_version: Some(1),
        entries: vec![FileEntryPlan {
            action: FileEntryAction::Publish,
            storage_root: StorageRootId::Originals,
            source_path: None,
            temporary_path: Some(path("staging/photo.jpg")),
            destination_path: Some(path("photo.jpg")),
            tombstone_path: None,
            expected_size: Some(10),
            expected_sha256: Some([7; 32]),
            expected_version: None,
        }],
        claims: vec![
            FilePathClaimPlan {
                storage_root: StorageRootId::Originals,
                path: path("staging/photo.jpg"),
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Exact,
                role: "temporary".to_string(),
                expected_version: None,
            },
            FilePathClaimPlan {
                storage_root: StorageRootId::Originals,
                path: path("photo.jpg"),
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Exact,
                role: "destination".to_string(),
                expected_version: None,
            },
        ],
        space_reservation: Some(journal_reservation("reservation-1")),
    }
}

#[tokio::test]
async fn directory_copy_cursor_checkpoints_survive_independent_sqlite_operations() {
    let pool = crate::test_utils::create_test_db();
    let executors = crate::test_utils::test_executor_handles(pool);
    let group_id = "directory-copy-cursor".to_string();
    let source = path("alice/source");
    let temporary = path("alice/.copy.tmp");
    let destination = path("alice/copied");
    let reservation = executors
        .file_io
        .reserve_journal_space(group_id.clone(), 4096)
        .expect("journal reservation")
        .into_result()
        .expect("journal capacity");
    let plan = FileOperationPlan {
        group_id: group_id.clone(),
        kind: "webdav_directory_copy".to_string(),
        owner_kind: "webdav".to_string(),
        owner_id: "copy-cursor".to_string(),
        claim_token: None,
        product_target: None,
        product_version: None,
        entries: vec![FileEntryPlan {
            action: FileEntryAction::Publish,
            storage_root: StorageRootId::WebDav,
            source_path: None,
            temporary_path: Some(temporary.clone()),
            destination_path: Some(destination.clone()),
            tombstone_path: None,
            expected_size: None,
            expected_sha256: None,
            expected_version: None,
        }],
        claims: vec![
            FilePathClaimPlan {
                storage_root: StorageRootId::WebDav,
                path: source.clone(),
                mode: PathClaimMode::Read,
                scope: PathClaimScope::Subtree,
                role: "source".to_string(),
                expected_version: None,
            },
            FilePathClaimPlan {
                storage_root: StorageRootId::WebDav,
                path: temporary.clone(),
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Subtree,
                role: "temporary".to_string(),
                expected_version: None,
            },
            FilePathClaimPlan {
                storage_root: StorageRootId::WebDav,
                path: destination,
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Subtree,
                role: "destination".to_string(),
                expected_version: None,
            },
        ],
        space_reservation: Some(
            JournalSpaceReservationPlan::new(reservation).expect("reservation plan"),
        ),
    };
    let fingerprint = [9_u8; 32];
    executors
        .sqlite
        .prepare_directory_copy_operation_durable(
            plan,
            DirectoryCopyConstructionPlan {
                storage_root: StorageRootId::WebDav,
                source_root: source.clone(),
                temporary_root: temporary.clone(),
                expected_file_bytes: 0,
                expected_entry_count: 1,
                expected_fingerprint: fingerprint,
            },
        )
        .await
        .expect("prepare directory copy");

    assert!(executors
        .sqlite
        .checkpoint_directory_copy_entry_durable(DirectoryCopyEntryCheckpoint {
            group_id: group_id.clone(),
            depth: 0,
            expected_resume_offset: 0,
            next_resume_offset: 44,
            file_bytes: 0,
            fingerprint,
            child: Some(DirectoryCopyCursor {
                depth: 1,
                source_path: path("alice/source/nested"),
                temporary_path: path("alice/.copy.tmp/nested"),
                resume_offset: 0,
            }),
        })
        .await
        .expect("checkpoint child"));
    let resumed = executors
        .sqlite
        .load_directory_copy_durable(Some(group_id.clone()))
        .await
        .expect("load durable cursor")
        .expect("directory copy construction");
    assert_eq!(resumed.copied_entry_count, 1);
    assert_eq!(resumed.copied_fingerprint, fingerprint);
    assert_eq!(resumed.cursors.len(), 2);
    assert_eq!(resumed.cursors[0].resume_offset, 44);

    assert!(executors
        .sqlite
        .checkpoint_directory_copy_finished_durable(DirectoryCopyFinishedCheckpoint {
            group_id: group_id.clone(),
            depth: 1,
            expected_resume_offset: 0,
        })
        .await
        .expect("finish child"));
    assert!(executors
        .sqlite
        .checkpoint_directory_copy_finished_durable(DirectoryCopyFinishedCheckpoint {
            group_id: group_id.clone(),
            depth: 0,
            expected_resume_offset: 44,
        })
        .await
        .expect("finish root"));
    let completed = executors
        .sqlite
        .load_directory_copy_durable(Some(group_id))
        .await
        .expect("load completed construction")
        .expect("completed construction");
    assert!(completed.complete);
    assert!(completed.cursors.is_empty());
}

#[test]
fn journal_plan_requires_bounded_action_specific_paths_and_space() {
    let mut plan = publish_plan();
    assert_eq!(plan.validate(), Ok(()));
    plan.space_reservation = None;
    assert_eq!(plan.validate(), Err(JournalPlanError::InvalidReservation));

    let mut plan = publish_plan();
    plan.entries[0].temporary_path = None;
    assert_eq!(plan.validate(), Err(JournalPlanError::InvalidEntry));
}

#[test]
fn journal_plan_rejects_internally_conflicting_claims() {
    let mut plan = publish_plan();
    plan.claims.push(FilePathClaimPlan {
        storage_root: StorageRootId::Originals,
        path: path("photo.jpg"),
        mode: PathClaimMode::Read,
        scope: PathClaimScope::Exact,
        role: "source".to_string(),
        expected_version: None,
    });
    assert_eq!(plan.validate(), Err(JournalPlanError::ConflictingClaims));
}

#[test]
fn journal_plan_rejects_every_unclaimed_mutation_path() {
    let mut plan = publish_plan();
    plan.claims.remove(0);
    assert_eq!(
        plan.validate(),
        Err(JournalPlanError::UnclaimedMutationPath)
    );
}

#[tokio::test]
async fn metadata_claim_fences_journal_publication_after_ownership_changes() {
    let pool = crate::test_utils::create_test_db();
    let media_id = crate::test_utils::create_test_media(&pool, "journal-fence.jpg");
    pool.get()
        .expect("connection")
        .execute(
            "INSERT INTO media_metadata_jobs (media_id, status) VALUES (?, 'queued')",
            [media_id],
        )
        .expect("metadata job");
    let executors = crate::test_utils::test_executor_handles(pool);
    let claim = executors
        .sqlite
        .claim_next_metadata_job_durable()
        .await
        .expect("claim")
        .expect("claimed metadata job");
    let mut plan = publish_plan();
    plan.claim_token = Some(claim.claim_token);
    executors
        .sqlite
        .prepare_file_operation_durable(plan)
        .await
        .expect("prepare fenced publication");
    executors
        .sqlite
        .recover_metadata_claims_durable()
        .await
        .expect("change ownership");
    let ticket = executors
        .file_io
        .reserve_journal_mutation("group-1", 2)
        .expect("mutation ticket");

    let grant = executors
        .sqlite
        .begin_file_operation_publication_durable(&ticket, 1)
        .await
        .expect("publication decision");

    assert!(grant.is_none());
}

#[tokio::test]
async fn restart_recovery_detaches_interrupted_llm_result_products() {
    let pool = crate::test_utils::create_test_db();
    let (executors, data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let media_id = crate::test_utils::create_test_media(&pool, "interrupted-result.jpg");
    let connection = pool.get().expect("connection");
    connection
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES ('ab111111111111111111111111111111', ?, 'ocr', 'submitted', 1)",
            [media_id],
        )
        .expect("LLM job");
    connection
        .execute(
            "INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, state, product_target, product_version, entry_count, version) VALUES ('interrupted-result-group', 'llm_result_receive', 'llm_result', 'ab111111111111111111111111111111', 'prepared', 'llm_result_inbox', 1, 1, 1)",
            [],
        )
        .expect("result group");
    connection
        .execute(
            "INSERT INTO file_operation_entries (group_id, sequence, action, storage_root, temporary_path, destination_path, expected_size) VALUES ('interrupted-result-group', 0, 'publish', 'journal', '.result.tmp', 'result.records', 24)",
            [],
        )
        .expect("result entry");
    connection
        .execute(
            "INSERT INTO data_dir_space_reservations (id, class, owner_kind, owner_id, filesystem_id, reserved_peak_additional_bytes, state) VALUES ('interrupted-result-sqlite', 'sqlite', 'llm_result', 'ab111111111111111111111111111111', 'test', 4096, 'active')",
            [],
        )
        .expect("result SQLite reservation");
    connection
        .execute(
            "INSERT INTO llm_result_receipts (job_id, attempt, job_version, media_id, task, result_status, model_type, model_version, encoding, record_count, byte_size, content_hash, journal_group_id, sqlite_reservation_id, inbox_path, receive_token, state, result_product_version) VALUES ('ab111111111111111111111111111111', 1, 1, ?, 'ocr', 'completed', 'ocr', 'test', 'momento-result-records-v1', 1, 24, ?, 'interrupted-result-group', 'interrupted-result-sqlite', 'result.records', '00000000-0000-0000-0000-000000000004', 'receiving', 1)",
            rusqlite::params![media_id, "0".repeat(64)],
        )
        .expect("result receipt");
    drop(connection);
    std::fs::write(data_directory.join("journal/.result.tmp"), b"partial")
        .expect("partial result receipt");

    assert_eq!(
        momento_api::io::recovery::rollback_prepared_file_operations_after_restart(&executors)
            .await
            .expect("request interrupted result rollback"),
        1
    );
    momento_api::io::recovery::recover_generic_file_operations(&executors)
        .await
        .expect("finish interrupted result rollback");

    let connection = pool.get().expect("connection after recovery");
    let recovered: (String, i64, Option<String>) = connection
        .query_row(
            "SELECT state, cancel_requested, product_target FROM file_operation_groups WHERE id = 'interrupted-result-group'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("recovered result group");
    assert_eq!(recovered, ("rolled_back".to_string(), 1, None));
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM llm_result_receipts WHERE job_id = 'ab111111111111111111111111111111'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("replayable receipt count"),
        0,
        "an active submitted job must be able to receive the same result again"
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM data_dir_space_reservations WHERE id = 'interrupted-result-sqlite'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("retired interrupted receipt reservation"),
        0,
        "a replayable receipt must not leave a reservation that blocks the next receive"
    );
    assert!(!data_directory.join("journal/.result.tmp").exists());
}

#[tokio::test]
async fn restart_recovery_detaches_an_unpublished_metadata_generation() {
    let pool = crate::test_utils::create_test_db();
    let media_id = crate::test_utils::create_test_media(&pool, "interrupted-metadata.jpg");
    let claim_token = "00000000-0000-0000-0000-000000000091";
    let connection = pool.get().expect("connection");
    connection
        .execute(
            "INSERT INTO media_metadata_jobs (media_id, status, claim_token) VALUES (?, 'processing', ?)",
            rusqlite::params![media_id, claim_token],
        )
        .expect("metadata claim");
    connection
        .execute(
            "INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, claim_token, state, product_target, product_version, entry_count, version) VALUES ('interrupted-metadata-group', 'metadata_artifacts', 'metadata_generation', ?, ?, 'files_committed', 'metadata_artifacts', 1, 1, 3)",
            rusqlite::params![media_id.to_string(), claim_token],
        )
        .expect("metadata group");
    connection
        .execute(
            "INSERT INTO file_operation_entries (group_id, sequence, action, storage_root, temporary_path, destination_path, state) VALUES ('interrupted-metadata-group', 0, 'publish', 'thumbnails', '.metadata.tmp', 'media/1/v1/thumbnail.jpg', 'committed')",
            [],
        )
        .expect("metadata entry");
    drop(connection);
    let executors = crate::test_utils::test_executor_handles(pool.clone());

    assert_eq!(
        momento_api::io::recovery::discard_incomplete_file_products_after_restart(&executors)
            .await
            .expect("discard interrupted metadata"),
        1
    );
    let recovered: (String, i64, Option<String>) = pool
        .get()
        .expect("connection after recovery")
        .query_row(
            "SELECT state, cancel_requested, product_target FROM file_operation_groups WHERE id = 'interrupted-metadata-group'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("recovered metadata product");
    assert_eq!(recovered, ("files_committed".to_string(), 1, None));
}

#[tokio::test]
async fn restart_recovery_detaches_an_unfinalized_import_product() {
    let pool = crate::test_utils::create_test_db();
    let claim_token = "00000000-0000-0000-0000-000000000092";
    let connection = pool.get().expect("connection");
    connection
        .execute(
            "INSERT INTO import_content_hash_claims (content_hash, claim_token, import_source) VALUES (?, ?, 'local')",
            rusqlite::params!["1".repeat(64), claim_token],
        )
        .expect("import claim");
    connection
        .execute(
            "INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, claim_token, state, product_target, product_version, entry_count, version) VALUES ('interrupted-import-group', 'import_media_publication', 'import', '41', ?, 'files_committed', 'import_media', 1, 1, 3)",
            [claim_token],
        )
        .expect("import group");
    connection
        .execute(
            "INSERT INTO file_operation_entries (group_id, sequence, action, storage_root, temporary_path, destination_path, state) VALUES ('interrupted-import-group', 0, 'publish', 'originals', '.importing/source.tmp', '41.jpg', 'committed')",
            [],
        )
        .expect("import entry");
    drop(connection);
    let executors = crate::test_utils::test_executor_handles(pool.clone());

    assert_eq!(
        momento_api::io::recovery::discard_incomplete_file_products_after_restart(&executors)
            .await
            .expect("discard interrupted import"),
        1
    );
    let recovered: (String, i64, Option<String>) = pool
        .get()
        .expect("connection after recovery")
        .query_row(
            "SELECT state, cancel_requested, product_target FROM file_operation_groups WHERE id = 'interrupted-import-group'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("recovered import product");
    assert_eq!(recovered, ("files_committed".to_string(), 1, None));
}
