use super::*;

fn command(job_id: &str) -> SqliteCommand {
    SqliteCommand::new(
        SqliteOperation::PersistPreparedLlmResult(
            crate::processor::ai::result::PreparedQueuedResult::PermanentFailure {
                job_id: job_id.into(),
                claim_token: Some("00000000-0000-0000-0000-000000000001".into()),
                error: "invalid result".into(),
            },
        ),
        oneshot::channel().0,
    )
}

fn fixture() -> (SqliteWorkerContext, tempfile::TempDir) {
    use crate::io::space_budget::{DataDirSpaceBudget, FilesystemSpaceSnapshot};
    let directory = crate::temporary::tempdir().unwrap();
    let database_path = directory.path().join("database.sqlite");
    let pool = crate::database::create_pool_at(&database_path, 1).unwrap();
    let connection = pool.get().unwrap();
    connection
        .execute_batch(include_str!("../../../../src/backend/database/schema.sql"))
        .unwrap();
    connection
        .pragma_update(None, "foreign_keys", true)
        .unwrap();
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .unwrap();
    let budget = DataDirSpaceBudget::from_snapshot(FilesystemSpaceSnapshot {
        filesystem_id: "batch-test".into(),
        total_bytes: 100 << 30,
        free_bytes: 90 << 30,
        fragment_size: 4096,
    })
    .unwrap();
    let mut reconstruction = budget.begin_reconstruction();
    for job_id in ["a", "b", "c"] {
        connection.execute("INSERT INTO media(filename,original_filename,file_path,media_type) VALUES(?,?,?,'image')", [job_id, job_id, job_id]).unwrap();
        let media_id = connection.last_insert_rowid();
        connection.execute("INSERT INTO llm_jobs(id,media_id,task,status,attempts) VALUES(?,?,'ocr','submitted',1)", rusqlite::params![job_id,media_id]).unwrap();
        connection.execute("INSERT INTO file_operation_groups(id,kind,owner_kind,owner_id,state,entry_count,product_target) VALUES(?,'llm_result_receive','llm_result',?,'completed',1,'llm_result_inbox')", [job_id,job_id]).unwrap();
        connection.execute("INSERT INTO data_dir_space_reservations(id,class,owner_kind,owner_id,filesystem_id,reserved_peak_additional_bytes,state) VALUES(?,'sqlite','llm_result',?,'batch-test',1073741824,'active')", [job_id,job_id]).unwrap();
        connection.execute("INSERT INTO llm_result_receipts(job_id,attempt,job_version,media_id,task,result_status,encoding,record_count,byte_size,content_hash,journal_group_id,sqlite_reservation_id,inbox_path,receive_token,state,claim_token,result_product_version) VALUES(?,1,1,?,'ocr','failed','momento-result-records-v1',3,100,?,?,?,?,'00000000-0000-0000-0000-000000000002','processing','00000000-0000-0000-0000-000000000001',1)", rusqlite::params![job_id,media_id,"0".repeat(64),job_id,job_id,job_id]).unwrap();
        let record = load_result_sqlite_reservation(&connection, job_id, "test").unwrap();
        reconstruction.add_page(&[record]).unwrap();
    }
    reconstruction.publish().unwrap();
    budget.mark_running().unwrap();
    drop(connection);
    (
        SqliteWorkerContext {
            pool,
            capacity_wake: std::sync::Arc::new(Notify::new()),
            space_budget: budget,
            database_path,
            footprints: crate::database::result_footprint::SqliteFootprintRegistry::new(4096)
                .unwrap(),
        },
        directory,
    )
}

fn malformed_page() -> SqliteOperation {
    SqliteOperation::StageLlmResultPage(Box::new(operations::StageLlmResultPage {
        job_id: "b".into(),
        attempt: 1,
        claim_token: "00000000-0000-0000-0000-000000000001".into(),
        expected_record_sequence: 0,
        expected_byte_offset: 0,
        records: [0, 2]
            .into_iter()
            .map(|sequence| operations::StagedLlmResultRecord {
                record_sequence: sequence,
                input_sequence: None,
                kind: "failure".into(),
                byte_offset: u64::from(sequence) * 24,
                encoded_size: 24,
                normalized_payload: Vec::new(),
            })
            .collect(),
    }))
}

#[test]
fn ready_writes_share_one_commit_and_bad_record_rolls_back_only_itself() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let (context, _directory) = fixture();
    let commits = Arc::new(AtomicUsize::new(0));
    let observed = commits.clone();
    context.pool.get().unwrap().commit_hook(Some(move || {
        observed.fetch_add(1, Ordering::SeqCst);
        false
    }));
    let results = execute_batch(
        vec![
            command("a").operation,
            malformed_page(),
            command("c").operation,
        ],
        &context,
    )
    .unwrap();
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
    assert!(results[2].is_ok());
    assert_eq!(commits.load(Ordering::SeqCst), 1);
    let connection = context.pool.get().unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM llm_jobs WHERE status='failed'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        2
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM llm_result_staging", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT state FROM llm_result_receipts WHERE job_id='b'",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
        "processing"
    );
}

#[test]
fn failed_batch_commit_returns_no_success_and_keeps_jobs_retryable() {
    let (context, _directory) = fixture();
    context.pool.get().unwrap().commit_hook(Some(|| true));
    assert!(execute_batch(
        vec![command("a").operation, command("c").operation],
        &context
    )
    .is_err());
    let connection = context.pool.get().unwrap();
    connection.commit_hook(None::<fn() -> bool>);
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM llm_jobs WHERE status='submitted'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        3
    );
    drop(connection);
    assert!(execute_batch(
        vec![command("a").operation, command("c").operation],
        &context
    )
    .unwrap()
    .iter()
    .all(Result::is_ok));
}

#[test]
fn sqlite_busy_preserves_every_result_for_a_later_batch() {
    let (context, _directory) = fixture();
    context
        .pool
        .get()
        .unwrap()
        .busy_timeout(Duration::from_millis(5))
        .unwrap();
    let competing_writer = rusqlite::Connection::open(&context.database_path).unwrap();
    competing_writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let error = execute_batch(
        vec![command("a").operation, command("c").operation],
        &context,
    )
    .err()
    .expect("writer contention");
    assert_eq!(error.kind, ExecutorErrorKind::DatabaseBusy);
    competing_writer.execute_batch("ROLLBACK").unwrap();
    assert!(execute_batch(
        vec![command("a").operation, command("c").operation],
        &context
    )
    .unwrap()
    .iter()
    .all(Result::is_ok));
}

#[test]
fn large_staging_pages_end_the_batch_at_the_byte_bound() {
    let footprints = crate::database::result_footprint::SqliteFootprintRegistry::new(4096).unwrap();
    let (sender, receiver) = crossbeam_channel::unbounded();
    let mut pages = (0..5).map(|index| {
        let SqliteOperation::StageLlmResultPage(mut page) = malformed_page() else {
            unreachable!()
        };
        page.job_id = index.to_string();
        SqliteCommand::new(
            SqliteOperation::StageLlmResultPage(page),
            oneshot::channel().0,
        )
    });
    let first = pages.next().unwrap();
    for page in pages {
        sender.send(page).unwrap();
    }
    let (batch, pending) = collect(first, &receiver, &footprints, &Notify::new());
    assert_eq!(batch.len(), 3);
    assert!(pending.is_some());
    assert_eq!(receiver.len(), 1);
}

#[test]
fn batching_stops_at_fifo_barrier_without_skipping_it() {
    let footprints = crate::database::result_footprint::SqliteFootprintRegistry::new(4096).unwrap();
    let (sender, receiver) = crossbeam_channel::unbounded();
    sender.send(command("b")).unwrap();
    sender
        .send(SqliteCommand::new(
            SqliteOperation::PrepareLlmSubmissionCycle,
            oneshot::channel().0,
        ))
        .unwrap();
    sender.send(command("c")).unwrap();
    let (batch, pending) = collect(command("a"), &receiver, &footprints, &Notify::new());
    assert_eq!(batch.len(), 2);
    assert!(matches!(
        pending.unwrap().operation,
        SqliteOperation::PrepareLlmSubmissionCycle
    ));
    assert_eq!(receiver.len(), 1);
}

#[test]
fn batching_is_bounded_and_does_not_wait_for_more_work() {
    let footprints = crate::database::result_footprint::SqliteFootprintRegistry::new(4096).unwrap();
    let (sender, receiver) = crossbeam_channel::unbounded();
    let (batch, pending) = collect(command("a"), &receiver, &footprints, &Notify::new());
    assert_eq!(batch.len(), 1);
    assert!(pending.is_none());
    for index in 0..40 {
        sender.send(command(&index.to_string())).unwrap();
    }
    let (batch, _) = collect(command("first"), &receiver, &footprints, &Notify::new());
    assert!(batch.len() <= MAX_COMMANDS);
    assert!(!receiver.is_empty());
}

#[test]
fn same_owner_and_unclaimed_results_are_not_coalesced() {
    let footprints = crate::database::result_footprint::SqliteFootprintRegistry::new(4096).unwrap();
    let (sender, receiver) = crossbeam_channel::unbounded();
    sender.send(command("a")).unwrap();
    let (batch, pending) = collect(command("a"), &receiver, &footprints, &Notify::new());
    assert_eq!(batch.len(), 1);
    assert!(pending.is_some());
    let unclaimed = SqliteOperation::PersistPreparedLlmResult(
        crate::processor::ai::result::PreparedQueuedResult::PermanentFailure {
            job_id: "a".into(),
            claim_token: None,
            error: "invalid".into(),
        },
    );
    assert!(!eligible(&unclaimed, &footprints));
}

#[test]
fn detached_result_payloads_are_bounded_without_relaxing_runtime_memory() {
    let SqliteOperation::PersistPreparedLlmResult(mut prepared) = command("a").operation else {
        unreachable!()
    };
    assert!(prepared.can_detach_sqlite_write());
    if let crate::processor::ai::result::PreparedQueuedResult::PermanentFailure { error, .. } =
        &mut prepared
    {
        *error = "x".repeat(8192);
    }
    assert!(!prepared.can_detach_sqlite_write());
}
