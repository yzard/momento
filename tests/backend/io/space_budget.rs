use momento_api::io::space_budget::{
    DataDirSpaceBudget, DurableSpaceReservationRecord, FilesystemSpaceSnapshot, SpaceAdmission,
    SpaceBudgetError, SpaceBudgetHealth, SpaceBudgetMode, SpaceReservationClass,
    SqliteRecoveryFootprintSpec,
};

const GIBIBYTE: u64 = 1024 * 1024 * 1024;

fn recovery_ready_budget(free_bytes: u64) -> DataDirSpaceBudget {
    let budget = DataDirSpaceBudget::from_snapshot(FilesystemSpaceSnapshot {
        filesystem_id: "filesystem-1".to_string(),
        total_bytes: 100 * GIBIBYTE,
        free_bytes,
        fragment_size: 4096,
    })
    .expect("space budget");
    budget
        .begin_reconstruction()
        .publish()
        .expect("space reconstruction");
    budget
}

#[test]
fn shared_data_capacity_preserves_recovery_and_log_space() {
    let budget = recovery_ready_budget(100 * GIBIBYTE);
    budget.mark_running().expect("running budget");
    let snapshot = budget.snapshot().expect("ledger snapshot");

    assert_eq!(snapshot.mode, SpaceBudgetMode::Running);
    assert_eq!(snapshot.health, SpaceBudgetHealth::Healthy);
    assert_eq!(snapshot.recovery_floor_bytes, 5 * GIBIBYTE);
    assert_eq!(snapshot.sqlite_wal_limit_bytes, 2 * GIBIBYTE);
    assert_eq!(snapshot.log_quota_bytes, 95 * GIBIBYTE / 100);
    assert_eq!(
        snapshot.data_hard_limit_bytes,
        95 * GIBIBYTE - 95 * GIBIBYTE / 100
    );
    assert_eq!(
        budget
            .filesystem_entry_metadata_bytes()
            .expect("entry metadata bound"),
        16 * 1024
    );
}

#[test]
fn provisional_drop_restores_capacity_and_hard_limit_is_distinct() {
    let budget = recovery_ready_budget(100 * GIBIBYTE);
    budget.mark_running().expect("running budget");
    let first = budget
        .reserve_journal("first".to_string(), 60 * GIBIBYTE)
        .expect("first admission")
        .into_result()
        .expect("first reservation");
    assert!(matches!(
        budget
            .reserve_journal("second".to_string(), 40 * GIBIBYTE)
            .expect("second admission"),
        SpaceAdmission::TemporarilyUnavailable { .. }
    ));
    drop(first);
    assert!(matches!(
        budget
            .reserve_journal("second".to_string(), 40 * GIBIBYTE)
            .expect("repeated admission"),
        SpaceAdmission::Fits(_)
    ));
    assert!(matches!(
        budget
            .reserve_journal("impossible".to_string(), 95 * GIBIBYTE)
            .expect("hard-limit admission"),
        SpaceAdmission::ExceedsHardLimit { .. }
    ));
}

#[test]
fn durable_checkout_drop_keeps_obligation_and_allows_one_reacquisition() {
    let budget = recovery_ready_budget(100 * GIBIBYTE);
    budget.mark_running().expect("running budget");
    let token = budget
        .reserve_journal("durable-1".to_string(), GIBIBYTE)
        .expect("admission")
        .into_result()
        .expect("reservation");
    let checkout = token
        .commit_to_durable_owner(
            "file_operation".to_string(),
            "owner-1".to_string(),
            Some("group-1".to_string()),
        )
        .expect("durable promotion");
    assert_eq!(
        budget
            .snapshot()
            .expect("snapshot")
            .journal_outstanding_bytes,
        GIBIBYTE
    );

    let record = DurableSpaceReservationRecord {
        reservation_id: "durable-1".to_string(),
        class: SpaceReservationClass::Journal,
        owner_kind: "file_operation".to_string(),
        owner_id: "owner-1".to_string(),
        journal_group_id: Some("group-1".to_string()),
        filesystem_id: "filesystem-1".to_string(),
        reserved_peak_additional_bytes: GIBIBYTE,
        newly_allocated_blocks: 0,
        version: 1,
    };
    assert_eq!(
        budget
            .reacquire_durable(&record)
            .expect_err("live checkout must be exclusive"),
        SpaceBudgetError::ReservationAlreadyCheckedOut
    );
    drop(checkout);
    let reacquired = budget
        .reacquire_durable(&record)
        .expect("reacquire durable reservation");
    drop(reacquired);
    assert_eq!(
        budget
            .snapshot()
            .expect("snapshot")
            .journal_outstanding_bytes,
        GIBIBYTE
    );
}

#[test]
fn reconstruction_requires_recovery_space_but_accepts_large_existing_sqlite_files() {
    let budget = recovery_ready_budget(GIBIBYTE);
    let snapshot = budget.snapshot().expect("deficit snapshot");
    assert_eq!(snapshot.health, SpaceBudgetHealth::ExternalDeficit);
    assert_eq!(
        budget.mark_running().expect_err("deficit cannot run"),
        SpaceBudgetError::LedgerNotHealthy(SpaceBudgetHealth::ExternalDeficit)
    );

    let large_database = DataDirSpaceBudget::from_snapshot(FilesystemSpaceSnapshot {
        filesystem_id: "filesystem-2".to_string(),
        total_bytes: 100 * GIBIBYTE,
        free_bytes: 20 * GIBIBYTE,
        fragment_size: 4096,
    })
    .expect("space budget");
    let mut reconstruction = large_database.begin_reconstruction();
    reconstruction.set_allocated_bytes(80 * GIBIBYTE, 0);
    let snapshot = reconstruction
        .publish()
        .expect("large SQLite reconstruction");
    assert_eq!(snapshot.health, SpaceBudgetHealth::Healthy);
    assert_eq!(snapshot.sqlite_allocated_bytes, 80 * GIBIBYTE);
}

#[test]
fn reconstruction_restores_only_the_unconsumed_sqlite_parent_capacity() {
    let budget = DataDirSpaceBudget::from_snapshot(FilesystemSpaceSnapshot {
        filesystem_id: "filesystem-result-restart".to_string(),
        total_bytes: 100 * GIBIBYTE,
        free_bytes: 98 * GIBIBYTE,
        fragment_size: 4096,
    })
    .expect("space budget");
    let mut reconstruction = budget.begin_reconstruction();
    reconstruction.set_allocated_bytes(GIBIBYTE, 0);
    reconstruction
        .add_page(&[DurableSpaceReservationRecord {
            reservation_id: "sqlite-result-parent".to_string(),
            class: SpaceReservationClass::Sqlite,
            owner_kind: "llm_result".to_string(),
            owner_id: "abcdef".to_string(),
            journal_group_id: None,
            filesystem_id: "filesystem-result-restart".to_string(),
            reserved_peak_additional_bytes: GIBIBYTE,
            newly_allocated_blocks: GIBIBYTE / 4,
            version: 7,
        }])
        .expect("reconstruct result parent");

    let snapshot = reconstruction.publish().expect("publish reconstruction");
    assert_eq!(snapshot.sqlite_allocated_bytes, GIBIBYTE);
    assert_eq!(snapshot.sqlite_outstanding_bytes, 3 * GIBIBYTE / 4);
}

#[test]
fn durable_reservations_survive_a_filesystem_identity_change_across_restart() {
    let budget = DataDirSpaceBudget::from_snapshot(FilesystemSpaceSnapshot {
        filesystem_id: "filesystem-after-zfs-remount".to_string(),
        total_bytes: 100 * GIBIBYTE,
        free_bytes: 98 * GIBIBYTE,
        fragment_size: 4096,
    })
    .expect("space budget");
    let record = DurableSpaceReservationRecord {
        reservation_id: "journal-before-zfs-remount".to_string(),
        class: SpaceReservationClass::Journal,
        owner_kind: "file_operation".to_string(),
        owner_id: "owner-before-zfs-remount".to_string(),
        journal_group_id: Some("group-before-zfs-remount".to_string()),
        filesystem_id: "filesystem-before-zfs-remount".to_string(),
        reserved_peak_additional_bytes: GIBIBYTE,
        newly_allocated_blocks: GIBIBYTE / 4,
        version: 3,
    };
    let mut reconstruction = budget.begin_reconstruction();
    reconstruction
        .add_page(std::slice::from_ref(&record))
        .expect("reconstruct reservation created before remount");
    let snapshot = reconstruction.publish().expect("publish reconstruction");
    assert_eq!(snapshot.journal_outstanding_bytes, 3 * GIBIBYTE / 4);

    let checkout = budget
        .reacquire_durable(&record)
        .expect("reacquire reservation created before remount");
    drop(checkout);
}

#[test]
fn reconstructed_sqlite_work_competes_with_journal_work_for_shared_capacity() {
    let budget = DataDirSpaceBudget::from_snapshot(FilesystemSpaceSnapshot {
        filesystem_id: "filesystem-shared-data".to_string(),
        total_bytes: 100 * GIBIBYTE,
        free_bytes: 50 * GIBIBYTE,
        fragment_size: 4096,
    })
    .expect("space budget");
    let mut reconstruction = budget.begin_reconstruction();
    reconstruction
        .add_page(&[DurableSpaceReservationRecord {
            reservation_id: "sqlite-shared-parent".to_string(),
            class: SpaceReservationClass::Sqlite,
            owner_kind: "llm_result".to_string(),
            owner_id: "shared-capacity".to_string(),
            journal_group_id: None,
            filesystem_id: "filesystem-shared-data".to_string(),
            reserved_peak_additional_bytes: 30 * GIBIBYTE,
            newly_allocated_blocks: 0,
            version: 1,
        }])
        .expect("reconstruct SQLite reservation");
    reconstruction.publish().expect("publish reconstruction");
    budget.mark_running().expect("running budget");

    assert!(matches!(
        budget
            .reserve_journal("journal-too-large".to_string(), 15 * GIBIBYTE)
            .expect("Journal admission"),
        SpaceAdmission::TemporarilyUnavailable { .. }
    ));
    assert!(matches!(
        budget
            .reserve_journal("journal-fits".to_string(), 14 * GIBIBYTE)
            .expect("Journal admission"),
        SpaceAdmission::Fits(_)
    ));
}

#[test]
fn recovery_log_cleanup_only_decreases_allocation_and_restores_health() {
    let budget = DataDirSpaceBudget::from_snapshot(FilesystemSpaceSnapshot {
        filesystem_id: "filesystem-log-cleanup".to_string(),
        total_bytes: 100 * GIBIBYTE,
        free_bytes: 100 * GIBIBYTE,
        fragment_size: 4096,
    })
    .expect("space budget");
    let log_quota = 95 * GIBIBYTE / 100;
    let mut reconstruction = budget.begin_reconstruction();
    reconstruction.set_allocated_bytes(0, log_quota + 4096);
    let over_quota = reconstruction.publish().expect("log reconstruction");
    assert_eq!(over_quota.health, SpaceBudgetHealth::LogOverQuota);
    assert_eq!(
        budget
            .mark_running()
            .expect_err("over-quota logs cannot enter running mode"),
        SpaceBudgetError::LedgerNotHealthy(SpaceBudgetHealth::LogOverQuota)
    );
    assert!(matches!(
        budget.publish_log_cleanup_allocation(log_quota + 8192),
        Err(SpaceBudgetError::InvalidReconstruction(_))
    ));
    let healthy = budget
        .publish_log_cleanup_allocation(log_quota)
        .expect("publish pruned log allocation");
    assert_eq!(healthy.health, SpaceBudgetHealth::Healthy);
    budget.mark_running().expect("healthy budget can run");
}

#[test]
fn sqlite_recovery_footprint_is_derived_from_real_wal_frames() {
    let directory = tempfile::tempdir().expect("SQLite recovery directory");
    let database_path = directory.path().join("database.sqlite");
    let connection = rusqlite::Connection::open(&database_path).expect("SQLite database");
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .expect("WAL mode");
    connection
        .pragma_update(None, "wal_autocheckpoint", 0)
        .expect("disable automatic checkpoint");
    connection
        .execute("CREATE TABLE recovery_fixture (payload BLOB NOT NULL)", [])
        .expect("recovery table");
    let transaction = connection
        .unchecked_transaction()
        .expect("recovery transaction");
    for _ in 0..32 {
        transaction
            .execute(
                "INSERT INTO recovery_fixture (payload) VALUES (?)",
                [vec![5_u8; 16 * 1024]],
            )
            .expect("recovery row");
    }
    transaction.commit().expect("recovery commit");

    let spec = SqliteRecoveryFootprintSpec::inspect(&database_path, 4096)
        .expect("SQLite recovery footprint");
    assert!(spec.page_size_bytes.is_power_of_two());
    assert!(spec.main_allocated_bytes > 0);
    assert!(spec.wal_allocated_bytes > 0);
    assert!(spec.wal_frame_count > 0);
    assert!(spec.peak_additional_bytes >= spec.wal_frame_count * spec.page_size_bytes);
}

#[test]
fn sqlite_recovery_footprint_rejects_a_truncated_wal_header() {
    let directory = tempfile::tempdir().expect("SQLite recovery directory");
    let database_path = directory.path().join("database.sqlite");
    let connection = rusqlite::Connection::open(&database_path).expect("SQLite database");
    connection
        .execute("CREATE TABLE recovery_fixture (id INTEGER)", [])
        .expect("recovery table");
    drop(connection);
    std::fs::write(database_path.with_extension("sqlite-wal"), [0_u8; 31]).expect("truncated WAL");

    assert!(matches!(
        SqliteRecoveryFootprintSpec::inspect(&database_path, 4096),
        Err(SpaceBudgetError::InvalidReconstruction(
            "SQLite WAL header is truncated"
        ))
    ));
}
