use super::*;

fn running_budget(directory: &std::path::Path) -> DataDirSpaceBudget {
    let budget = DataDirSpaceBudget::from_directory(File::open(directory).unwrap()).unwrap();
    let mut reconstruction = budget.begin_reconstruction();
    reconstruction.set_allocated_bytes(0, 0);
    reconstruction.publish().unwrap();
    budget.mark_running().unwrap();
    budget
}

#[test]
fn sqlite_logical_extents_are_charged_before_physical_blocks_materialize() {
    use std::io::Write;
    let directory = crate::temporary::tempdir().unwrap();
    let path = directory.path().join("database.sqlite");
    let budget = running_budget(directory.path());
    let token = budget
        .reserve_sqlite("writer".into(), 4 * 1024 * 1024)
        .unwrap()
        .into_result()
        .unwrap();
    let mut file = File::create(&path).unwrap();
    file.set_len(4 * 1024 * 1024).unwrap();
    let sparse = inspect_sqlite_allocation(&path).unwrap();
    assert_eq!(sparse.logical_bytes, 4 * 1024 * 1024);
    assert_eq!(sparse.charged_bytes, 4 * 1024 * 1024);
    token
        .publish_ephemeral_sqlite_allocation(&path, "writer_commit")
        .unwrap();
    let connection = budget
        .reserve_sqlite("connection".into(), 4096)
        .unwrap()
        .into_result()
        .unwrap();
    for _ in 0..1024 {
        file.write_all(&[42; 4096]).unwrap();
    }
    file.sync_all().unwrap();
    connection
        .publish_ephemeral_sqlite_allocation(&path, "sqlite_connection_open")
        .unwrap();
    assert_eq!(
        budget.snapshot().unwrap().sqlite_allocated_bytes,
        4 * 1024 * 1024
    );
    assert_eq!(budget.snapshot().unwrap().sqlite_outstanding_bytes, 0);
}

#[test]
fn later_publication_remeasures_instead_of_overwriting_with_a_stale_baseline() {
    let directory = crate::temporary::tempdir().unwrap();
    let path = directory.path().join("database.sqlite");
    let budget = running_budget(directory.path());
    let connection = budget
        .reserve_sqlite("connection".into(), 4096)
        .unwrap()
        .into_result()
        .unwrap();
    let stale = measure_sqlite_allocation(&path).unwrap();
    let writer = budget
        .reserve_sqlite("writer".into(), 8192)
        .unwrap()
        .into_result()
        .unwrap();
    File::create(&path).unwrap().set_len(8192).unwrap();
    writer
        .publish_ephemeral_sqlite_allocation(&path, "writer_commit")
        .unwrap();
    connection
        .publish_ephemeral_sqlite_allocation(&path, "sqlite_connection_open")
        .unwrap();
    assert_eq!(stale, 0);
    assert_eq!(budget.snapshot().unwrap().sqlite_allocated_bytes, 8192);
}

#[test]
fn real_growth_overrun_reports_operation_and_all_budget_numbers() {
    let directory = crate::temporary::tempdir().unwrap();
    let path = directory.path().join("database.sqlite");
    let budget = running_budget(directory.path());
    let token = budget
        .reserve_sqlite("small-reservation".into(), 4096)
        .unwrap()
        .into_result()
        .unwrap();
    File::create(&path).unwrap().set_len(8192).unwrap();
    let error = token
        .publish_ephemeral_sqlite_allocation(&path, "sqlite_connection_open")
        .unwrap_err();
    assert!(matches!(
        &error,
        SpaceBudgetError::SqliteGrowthExceeded {
            operation: "sqlite_connection_open",
            baseline_bytes: 0,
            outstanding_bytes: 4096,
            logical_bytes: 8192,
            charged_bytes: 8192,
            excess_bytes: 4096,
            ..
        }
    ));
    let message = error.to_string();
    for field in [
        "operation=sqlite_connection_open",
        "reservation_id=small-reservation",
        "baseline_bytes=0",
        "outstanding_bytes=4096",
        "physical_bytes=",
        "logical_bytes=8192",
        "charged_bytes=8192",
        "excess_bytes=4096",
    ] {
        assert!(message.contains(field), "{message}");
    }
    assert!(!message.contains("reconstruction"));
}

fn layout() -> BudgetLayout {
    BudgetLayout {
        filesystem_id: "filesystem-1".to_string(),
        total_bytes: 100 * GIBIBYTE,
        fragment_size: 4096,
        recovery_floor_bytes: 5 * GIBIBYTE,
        sqlite_wal_limit_bytes: 2 * GIBIBYTE,
        log_quota_bytes: GIBIBYTE,
        data_hard_limit_bytes: 94 * GIBIBYTE,
    }
}

#[test]
fn runtime_observation_accepts_total_capacity_changes() {
    let layout = layout();
    for (total_bytes, free_bytes) in [
        (80 * GIBIBYTE, 20 * GIBIBYTE),
        (120 * GIBIBYTE, 60 * GIBIBYTE),
    ] {
        validate_runtime_observation(
            &layout,
            &FilesystemSpaceSnapshot {
                filesystem_id: layout.filesystem_id.clone(),
                total_bytes,
                free_bytes,
                fragment_size: layout.fragment_size,
            },
        )
        .expect("same filesystem with changed capacity");
    }
}

#[test]
fn runtime_observation_rejects_filesystem_or_allocation_unit_changes() {
    let layout = layout();
    for (filesystem_id, fragment_size) in [("filesystem-2", 4096), ("filesystem-1", 8192)] {
        assert_eq!(
            validate_runtime_observation(
                &layout,
                &FilesystemSpaceSnapshot {
                    filesystem_id: filesystem_id.to_string(),
                    total_bytes: layout.total_bytes,
                    free_bytes: 50 * GIBIBYTE,
                    fragment_size,
                },
            ),
            Err(SpaceBudgetError::InvalidFilesystemSnapshot)
        );
    }
}
