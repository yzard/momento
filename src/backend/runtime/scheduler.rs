use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use crossbeam_channel::{Sender as ExecutorSender, TrySendError};
use tokio::sync::{mpsc, oneshot, Notify};

use crate::database::DbPool;
use crate::executor::{
    bootstrap_file_executor, complete_file_executor_bootstrap, recover_log_capacity,
    spawn_cpu_workers, spawn_file_workers, spawn_sqlite_workers, BootstrapDatabaseState,
    CpuCommand, CpuExecutorHandle, ExecutorError, FileCommand, FileIoExecutorHandle,
    FileWorkerContext, SqliteCommand, SqliteExecutorHandle,
};
use crate::io::file::MutationGateRegistry;
use crate::io::session::FileHandleRegistry;
use crate::io::space_budget::{
    DurableSpaceReservationRecord, SpaceBudgetHealth, SpaceReservationClass,
    MAX_SPACE_RECONSTRUCTION_PAGE,
};
use crate::runtime::RuntimeSizing;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubmissionMode {
    Try,
    Durable,
}

struct PendingReservation {
    pending: Arc<AtomicUsize>,
}

impl Drop for PendingReservation {
    fn drop(&mut self) {
        let previous = self.pending.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "scheduler pending reservation underflow");
    }
}

enum SchedulerCommand {
    Cpu {
        command: CpuCommand,
        mode: SubmissionMode,
        operation: &'static str,
        reservation: PendingReservation,
    },
    File {
        command: FileCommand,
        mode: SubmissionMode,
        operation: &'static str,
        reservation: PendingReservation,
    },
    Sqlite {
        command: SqliteCommand,
        mode: SubmissionMode,
        operation: &'static str,
        reservation: PendingReservation,
    },
    SchedulerControl {
        task: Pin<Box<dyn Future<Output = ()> + Send + 'static>>,
        reservation: PendingReservation,
    },
    Shutdown {
        reply: oneshot::Sender<()>,
    },
}

#[derive(Clone)]
pub(crate) struct SchedulerIngress {
    sender: mpsc::Sender<SchedulerCommand>,
    pending: Arc<AtomicUsize>,
    maximum_pending: usize,
}

impl SchedulerIngress {
    pub(crate) fn submit_cpu(
        &self,
        command: CpuCommand,
        mode: SubmissionMode,
        operation: &'static str,
    ) -> Result<(), ExecutorError> {
        let reservation = self.reserve(operation)?;
        self.submit(
            SchedulerCommand::Cpu {
                command,
                mode,
                operation,
                reservation,
            },
            operation,
        )
    }

    pub(crate) fn submit_file(
        &self,
        command: FileCommand,
        mode: SubmissionMode,
        operation: &'static str,
    ) -> Result<(), ExecutorError> {
        let reservation = self.reserve(operation)?;
        self.submit(
            SchedulerCommand::File {
                command,
                mode,
                operation,
                reservation,
            },
            operation,
        )
    }

    pub(crate) fn submit_sqlite(
        &self,
        command: SqliteCommand,
        mode: SubmissionMode,
        operation: &'static str,
    ) -> Result<(), ExecutorError> {
        let reservation = self.reserve(operation)?;
        self.submit(
            SchedulerCommand::Sqlite {
                command,
                mode,
                operation,
                reservation,
            },
            operation,
        )
    }

    pub(crate) fn submit_scheduler_control<Task>(&self, task: Task) -> Result<(), ExecutorError>
    where
        Task: Future<Output = ()> + Send + 'static,
    {
        const OPERATION: &str = "scheduler_control";
        let reservation = self.reserve(OPERATION)?;
        self.submit(
            SchedulerCommand::SchedulerControl {
                task: Box::pin(task),
                reservation,
            },
            OPERATION,
        )
    }

    fn submit(
        &self,
        command: SchedulerCommand,
        operation: &'static str,
    ) -> Result<(), ExecutorError> {
        self.sender.try_send(command).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => ExecutorError::new(
                crate::executor::ExecutorErrorKind::Internal,
                operation,
                "reserved scheduler ingress credit was unavailable",
            ),
            mpsc::error::TrySendError::Closed(_) => ExecutorError::shutting_down(operation),
        })
    }

    fn reserve(&self, operation: &'static str) -> Result<PendingReservation, ExecutorError> {
        self.pending
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |pending| {
                (pending < self.maximum_pending).then_some(pending + 1)
            })
            .map_err(|_| ExecutorError::overloaded(operation))?;
        Ok(PendingReservation {
            pending: Arc::clone(&self.pending),
        })
    }
}

#[derive(Clone)]
pub struct ExecutorHandles {
    pub cpu: CpuExecutorHandle,
    pub file_io: FileIoExecutorHandle,
    pub sqlite: SqliteExecutorHandle,
    pub scheduler: crate::runtime::SchedulerHandle,
}

pub struct ExecutorRuntime {
    ingress: SchedulerIngress,
    scheduler_thread: Option<JoinHandle<()>>,
    worker_threads: Vec<JoinHandle<()>>,
    _data_directory_lock: std::fs::File,
}

impl ExecutorRuntime {
    pub fn start(
        sizing: &RuntimeSizing,
        pool: DbPool,
        config_identity: crate::runtime::ConfigFileIdentity,
        data_dir: std::path::PathBuf,
        static_dir: Option<std::path::PathBuf>,
    ) -> Result<(Self, ExecutorHandles), String> {
        validate_injected_pool_path(&pool, &data_dir.join("database.sqlite"))?;
        let (runtime, handles) =
            Self::start_with_pool_factory(sizing, config_identity, data_dir, static_dir, || {
                Ok(pool)
            })?;
        Ok((runtime, handles))
    }

    pub(crate) fn start_with_pool_factory<F>(
        sizing: &RuntimeSizing,
        config_identity: crate::runtime::ConfigFileIdentity,
        data_dir: std::path::PathBuf,
        static_dir: Option<std::path::PathBuf>,
        create_pool: F,
    ) -> Result<(Self, ExecutorHandles), String>
    where
        F: FnOnce() -> Result<DbPool, String>,
    {
        sizing
            .validate_pre_spawn_environment()
            .map_err(|error| error.to_string())?;
        let file_bootstrap = bootstrap_file_executor(config_identity, data_dir.clone())?;
        let data_directory_lock = file_bootstrap.data_directory_lock;
        let space_budget = file_bootstrap.space_budget;
        let log_allocated_bytes = file_bootstrap.log_allocated_bytes;
        let database_path = data_dir.join("database.sqlite");
        let pool = match file_bootstrap.database_state {
            BootstrapDatabaseState::Fresh => {
                let mut reconstruction = space_budget.begin_reconstruction();
                reconstruction.set_allocated_bytes(0, log_allocated_bytes);
                let snapshot = reconstruction
                    .publish()
                    .map_err(|error| error.to_string())?;
                if !matches!(
                    snapshot.health,
                    SpaceBudgetHealth::Healthy | SpaceBudgetHealth::LogOverQuota
                ) {
                    return Err(format!(
                        "fresh data-directory space budget is not healthy: {:?}",
                        snapshot.health
                    ));
                }
                recover_log_capacity(&data_dir, &space_budget)?;
                let bootstrap_spec = crate::database::SqliteBootstrapFootprintSpec::derive(
                    space_budget.filesystem_fragment_size(),
                )
                .map_err(|error| error.to_string())?;
                let token = space_budget
                    .reserve_sqlite(
                        "sqlite-fresh-bootstrap".to_string(),
                        bootstrap_spec.peak_additional_bytes,
                    )
                    .map_err(|error| error.to_string())?
                    .into_result()
                    .map_err(|error| error.to_string())?;
                let pool = create_pool()?;
                let allocated = crate::io::space_budget::measure_sqlite_allocation(&database_path)
                    .map_err(|error| error.to_string())?;
                token
                    .publish_ephemeral_sqlite_allocation(allocated)
                    .map_err(|error| error.to_string())?;
                pool
            }
            BootstrapDatabaseState::Existing => {
                let recovery_spec = crate::io::space_budget::SqliteRecoveryFootprintSpec::inspect(
                    &database_path,
                    space_budget.filesystem_fragment_size(),
                )
                .map_err(|error| error.to_string())?;
                let read_only = match crate::database::open_existing_database_read_only(
                    &database_path,
                ) {
                    Ok(connection) => connection,
                    Err(first_error) if recovery_spec.wal_frame_count > 0 => {
                        let baseline =
                            crate::io::space_budget::measure_sqlite_allocation(&database_path)
                                .map_err(|error| error.to_string())?;
                        let recovery = space_budget
                            .reserve_recovery_sqlite(recovery_spec.peak_additional_bytes)
                            .map_err(|error| error.to_string())?
                            .into_result()
                            .map_err(|error| error.to_string())?;
                        crate::database::recover_existing_database(&database_path).map_err(
                                |recovery_error| {
                                    format!(
                                        "existing SQLite read-only probe failed ({first_error}); bounded recovery failed: {recovery_error}"
                                    )
                                },
                            )?;
                        let recovered =
                            crate::io::space_budget::measure_sqlite_allocation(&database_path)
                                .map_err(|error| error.to_string())?;
                        recovery
                            .publish_sqlite_recovery(baseline, recovered)
                            .map_err(|error| error.to_string())?;
                        crate::database::open_existing_database_read_only(&database_path)
                                .map_err(|second_error| {
                                    format!(
                                        "existing SQLite read-only probe still failed after bounded recovery: {second_error}"
                                    )
                                })?
                    }
                    Err(error) => {
                        return Err(format!(
                                "existing SQLite read-only probe failed without recoverable WAL frames: {error}"
                            ));
                    }
                };
                reconstruct_space_budget(
                    &space_budget,
                    &read_only,
                    &database_path,
                    log_allocated_bytes,
                    true,
                )?;
                drop(read_only);
                recover_log_capacity(&data_dir, &space_budget)?;
                let pool_token = space_budget
                    .reserve_sqlite(
                        "sqlite-existing-pool-bootstrap".to_string(),
                        recovery_spec.peak_additional_bytes,
                    )
                    .map_err(|error| error.to_string())?
                    .into_result()
                    .map_err(|error| error.to_string())?;
                let pool = create_pool()?;
                let allocated = crate::io::space_budget::measure_sqlite_allocation(&database_path)
                    .map_err(|error| error.to_string())?;
                pool_token
                    .publish_ephemeral_sqlite_allocation(allocated)
                    .map_err(|error| error.to_string())?;
                pool
            }
        };
        let sqlite_page_size =
            configure_sqlite_capacity(&pool, &space_budget, sizing.sqlite_workers, &database_path)?;
        let sqlite_footprints =
            crate::database::result_footprint::SqliteFootprintRegistry::new(sqlite_page_size)
                .map_err(|error| error.to_string())?;
        let bootstrapped_roots =
            complete_file_executor_bootstrap(data_dir.clone(), static_dir, space_budget.clone())?;
        space_budget.mark_running().map_err(|error| {
            format!("data-directory space budget could not enter running mode: {error}")
        })?;

        let capacity_wake = Arc::new(Notify::new());
        let (cpu_sender, cpu_receiver) = crossbeam_channel::bounded(sizing.cpu_queue_capacity);
        let (file_sender, file_receiver) = crossbeam_channel::bounded(sizing.file_queue_capacity);
        let (sqlite_sender, sqlite_receiver) =
            crossbeam_channel::bounded(sizing.sqlite_queue_capacity);
        let storage_roots = Arc::new(std::sync::OnceLock::new());
        storage_roots
            .set(bootstrapped_roots)
            .map_err(|_| "storage-root registry was already published".to_string())?;
        let mutation_gates = Arc::new(
            MutationGateRegistry::new(sizing.file_queue_capacity)
                .map_err(|error| error.to_string())?,
        );
        let (file_close_sender, file_close_receiver) = crossbeam_channel::bounded(1);
        let (log_event_producer, log_event_consumer) =
            crate::io::log::bounded_log_ring(sizing.log_event_capacity)
                .map_err(|error| error.to_string())?;
        let log_event_consumer = Arc::new(log_event_consumer);
        let log_writer = Arc::new(std::sync::Mutex::new(
            crate::io::log::RuntimeLogWriter::new(),
        ));
        let file_handles = Arc::new(
            FileHandleRegistry::new(sizing.file_registry_capacity, file_close_sender)
                .map_err(|error| error.to_string())?,
        );
        let mut worker_threads = spawn_file_workers(
            sizing.file_workers,
            file_receiver,
            FileWorkerContext {
                capacity_wake: Arc::clone(&capacity_wake),
                storage_roots: Arc::clone(&storage_roots),
                mutation_gates: Arc::clone(&mutation_gates),
                file_handles,
                close_receiver: file_close_receiver,
                log_consumer: log_event_consumer,
                log_writer,
                space_budget: space_budget.clone(),
            },
        )
        .map_err(|error| error.to_string())?;
        let sqlite_workers = match spawn_sqlite_workers(
            sizing.sqlite_workers,
            pool.clone(),
            sqlite_receiver,
            Arc::clone(&capacity_wake),
            space_budget.clone(),
            database_path,
            sqlite_footprints,
        ) {
            Ok(workers) => workers,
            Err(error) => {
                drop(file_sender);
                drop(sqlite_sender);
                join_started_workers(worker_threads)?;
                return Err(error.to_string());
            }
        };
        worker_threads.extend(sqlite_workers);
        let cpu_workers =
            match spawn_cpu_workers(sizing.cpu_workers, cpu_receiver, Arc::clone(&capacity_wake)) {
                Ok(workers) => workers,
                Err(error) => {
                    drop(cpu_sender);
                    drop(file_sender);
                    drop(sqlite_sender);
                    join_started_workers(worker_threads)?;
                    return Err(error.to_string());
                }
            };
        worker_threads.extend(cpu_workers);

        let (sender, receiver) = mpsc::channel(sizing.scheduler_ingress_capacity);
        let ingress = SchedulerIngress {
            sender,
            pending: Arc::new(AtomicUsize::new(0)),
            maximum_pending: sizing.scheduler_ingress_capacity,
        };
        let scheduler_thread = match std::thread::Builder::new()
            .name("momento-scheduler".to_string())
            .stack_size(crate::runtime::WORKER_STACK_BYTES as usize)
            .spawn(move || {
                let scheduler_runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
                    .expect("scheduler runtime");
                scheduler_runtime.block_on(run_scheduler(
                    receiver,
                    cpu_sender,
                    file_sender,
                    sqlite_sender,
                    capacity_wake,
                ));
            }) {
            Ok(thread) => thread,
            Err(error) => {
                drop(ingress);
                join_started_workers(worker_threads)?;
                return Err(error.to_string());
            }
        };

        let handles = ExecutorHandles {
            cpu: CpuExecutorHandle::new(ingress.clone()),
            file_io: FileIoExecutorHandle::new(
                ingress.clone(),
                mutation_gates,
                space_budget,
                log_event_producer,
            ),
            sqlite: SqliteExecutorHandle::new(ingress.clone()),
            scheduler: crate::runtime::SchedulerHandle::new(sizing, ingress.clone()),
        };
        Ok((
            Self {
                ingress,
                scheduler_thread: Some(scheduler_thread),
                worker_threads,
                _data_directory_lock: data_directory_lock,
            },
            handles,
        ))
    }

    pub async fn shutdown(mut self) -> Result<(), String> {
        let (reply, response) = oneshot::channel();
        self.ingress
            .sender
            .send(SchedulerCommand::Shutdown { reply })
            .await
            .map_err(|_| "scheduler shutdown command could not be submitted".to_string())?;
        response
            .await
            .map_err(|_| "scheduler stopped before acknowledging shutdown".to_string())?;
        if self
            .scheduler_thread
            .take()
            .expect("scheduler thread exists")
            .join()
            .is_err()
        {
            return Err("scheduler thread panicked".to_string());
        }
        for worker_thread in self.worker_threads.drain(..) {
            if worker_thread.join().is_err() {
                return Err("executor worker thread panicked".to_string());
            }
        }
        Ok(())
    }
}

fn validate_injected_pool_path(
    pool: &DbPool,
    expected_database_path: &std::path::Path,
) -> Result<(), String> {
    let connection = pool
        .get()
        .map_err(|error| format!("could not inspect injected SQLite pool: {error}"))?;
    let configured_path = connection
        .query_row(
            "SELECT file FROM pragma_database_list WHERE name = 'main'",
            [],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| format!("could not read injected SQLite pool path: {error}"))?;
    if configured_path.is_empty() {
        return Err("executor runtime requires a file-backed SQLite pool".to_string());
    }
    let configured_path = std::fs::canonicalize(&configured_path)
        .map_err(|error| format!("could not canonicalize injected SQLite pool path: {error}"))?;
    let expected_database_path = std::fs::canonicalize(expected_database_path)
        .map_err(|error| format!("could not canonicalize expected SQLite path: {error}"))?;
    if configured_path != expected_database_path {
        return Err(format!(
            "injected SQLite pool path {} does not match runtime database {}",
            configured_path.display(),
            expected_database_path.display()
        ));
    }
    Ok(())
}

fn join_started_workers(workers: Vec<JoinHandle<()>>) -> Result<(), String> {
    for worker in workers {
        if worker.join().is_err() {
            return Err("executor worker panicked during failed startup cleanup".to_string());
        }
    }
    Ok(())
}

fn reconstruct_space_budget(
    budget: &crate::io::space_budget::DataDirSpaceBudget,
    connection: &rusqlite::Connection,
    database_path: &std::path::Path,
    log_allocated_bytes: u64,
    require_reservation_table: bool,
) -> Result<(), String> {
    use rusqlite::params;
    let sqlite_allocated_bytes = crate::io::space_budget::measure_sqlite_allocation(database_path)
        .map_err(|error| error.to_string())?;

    crate::database::schema::validate_database_schema(connection)
        .map_err(|error| format!("database schema validation failed: {error}"))?;
    let reservation_table_exists = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'data_dir_space_reservations')",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|error| format!("could not inspect the space reservation schema: {error}"))?;
    if require_reservation_table && !reservation_table_exists {
        return Err(
            "existing database is missing the required data_dir_space_reservations table"
                .to_string(),
        );
    }
    let mut reconstruction = budget.begin_reconstruction();
    reconstruction.set_allocated_bytes(sqlite_allocated_bytes, log_allocated_bytes);
    if reservation_table_exists {
        let mut after_id = String::new();
        loop {
            let mut statement = connection
                .prepare(
                    "SELECT id, class, owner_kind, owner_id, journal_group_id, filesystem_id, reserved_peak_additional_bytes, newly_allocated_blocks, version FROM data_dir_space_reservations WHERE state IN ('active', 'releasing') AND id > ? ORDER BY id LIMIT ?",
                )
                .map_err(|error| format!("could not prepare space reconstruction: {error}"))?;
            let rows = statement
                .query_map(
                    params![after_id, MAX_SPACE_RECONSTRUCTION_PAGE as i64],
                    |row| {
                        let class_text = row.get::<_, String>(1)?;
                        let class = SpaceReservationClass::try_from(class_text.as_str())
                            .map_err(|_| rusqlite::Error::InvalidQuery)?;
                        let reserved = row.get::<_, i64>(6)?;
                        let allocated = row.get::<_, i64>(7)?;
                        let version = row.get::<_, i64>(8)?;
                        Ok(DurableSpaceReservationRecord {
                            reservation_id: row.get(0)?,
                            class,
                            owner_kind: row.get(2)?,
                            owner_id: row.get(3)?,
                            journal_group_id: row.get(4)?,
                            filesystem_id: row.get(5)?,
                            reserved_peak_additional_bytes: u64::try_from(reserved).map_err(
                                |_| rusqlite::Error::IntegralValueOutOfRange(6, reserved),
                            )?,
                            newly_allocated_blocks: u64::try_from(allocated).map_err(|_| {
                                rusqlite::Error::IntegralValueOutOfRange(7, allocated)
                            })?,
                            version: u64::try_from(version).map_err(|_| {
                                rusqlite::Error::IntegralValueOutOfRange(8, version)
                            })?,
                        })
                    },
                )
                .map_err(|error| format!("could not read space reconstruction page: {error}"))?;
            let page = rows
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|error| format!("could not map space reconstruction page: {error}"))?;
            if page.is_empty() {
                break;
            }
            let Some(last) = page.last() else {
                return Err("space reconstruction page unexpectedly became empty".to_string());
            };
            after_id = last.reservation_id.clone();
            reconstruction
                .add_page(&page)
                .map_err(|error| error.to_string())?;
            if page.len() < MAX_SPACE_RECONSTRUCTION_PAGE {
                break;
            }
        }
    }
    let snapshot = reconstruction
        .publish()
        .map_err(|error| error.to_string())?;
    if !matches!(
        snapshot.health,
        SpaceBudgetHealth::Healthy | SpaceBudgetHealth::LogOverQuota
    ) {
        return Err(format!(
            "data-directory space budget is not healthy after reconstruction: {:?}",
            snapshot.health
        ));
    }
    Ok(())
}

fn configure_sqlite_capacity(
    pool: &DbPool,
    budget: &crate::io::space_budget::DataDirSpaceBudget,
    sqlite_workers: usize,
    database_path: &std::path::Path,
) -> Result<u64, String> {
    use std::os::unix::fs::MetadataExt;

    pool.set_connection_budget(budget.clone(), database_path.to_path_buf())
        .map_err(|error| error.to_string())?;
    let wal_limit_bytes = budget.sqlite_wal_limit_bytes();
    let wal_path = database_path.with_extension("sqlite-wal");
    let wal_allocated_bytes = match std::fs::symlink_metadata(&wal_path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(format!(
                "SQLite WAL path is not a regular file: {}",
                wal_path.display()
            ));
        }
        Ok(metadata) => metadata
            .blocks()
            .checked_mul(512)
            .ok_or_else(|| "SQLite WAL allocation overflowed".to_string())?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => {
            return Err(format!(
                "could not measure SQLite WAL {}: {error}",
                wal_path.display()
            ));
        }
    };
    if wal_allocated_bytes > wal_limit_bytes {
        return Err(format!(
            "existing SQLite WAL allocation {wal_allocated_bytes} exceeds limit {wal_limit_bytes}"
        ));
    }

    let mut connections = Vec::new();
    connections
        .try_reserve_exact(sqlite_workers)
        .map_err(|_| "could not reserve SQLite capacity bootstrap handles".to_string())?;
    let mut configured_page_size = None;
    for _ in 0..sqlite_workers {
        let connection = pool
            .get()
            .map_err(|error| format!("could not configure SQLite capacity: {error}"))?;
        let page_size = connection
            .query_row("PRAGMA page_size", [], |row| row.get::<_, u64>(0))
            .map_err(|error| format!("could not read SQLite page size: {error}"))?;
        if page_size == 0 {
            return Err("SQLite reported a zero page size".to_string());
        }
        if configured_page_size.is_some_and(|configured| configured != page_size) {
            return Err("SQLite pool connections reported different page sizes".to_string());
        }
        configured_page_size = Some(page_size);
        connections.push(connection);
    }
    drop(connections);
    configured_page_size.ok_or_else(|| "SQLite pool has no configured connections".to_string())
}

async fn run_scheduler(
    mut receiver: mpsc::Receiver<SchedulerCommand>,
    cpu_sender: ExecutorSender<CpuCommand>,
    file_sender: ExecutorSender<FileCommand>,
    sqlite_sender: ExecutorSender<SqliteCommand>,
    capacity_wake: Arc<Notify>,
) {
    let mut cpu_waiters = VecDeque::new();
    let mut file_waiters = VecDeque::new();
    let mut sqlite_waiters = VecDeque::new();
    loop {
        tokio::select! {
            command = receiver.recv() => {
                let Some(command) = command else {
                    reject_all_waiters(&mut cpu_waiters, &mut file_waiters, &mut sqlite_waiters);
                    return;
                };
                match command {
                    SchedulerCommand::Cpu { command, mode, operation, reservation } => {
                        submit_cpu(command, mode, operation, reservation, &cpu_sender, &mut cpu_waiters);
                    }
                    SchedulerCommand::File { command, mode, operation, reservation } => {
                        submit_file(command, mode, operation, reservation, &file_sender, &mut file_waiters);
                    }
                    SchedulerCommand::Sqlite { command, mode, operation, reservation } => {
                        submit_sqlite(command, mode, operation, reservation, &sqlite_sender, &mut sqlite_waiters);
                    }
                    SchedulerCommand::SchedulerControl { task, reservation } => {
                        drop(reservation);
                        let _scheduler_control = tokio::spawn(task);
                    }
                    SchedulerCommand::Shutdown { reply } => {
                        reject_all_waiters(&mut cpu_waiters, &mut file_waiters, &mut sqlite_waiters);
                        let _ = reply.send(());
                        return;
                    }
                }
            }
            () = capacity_wake.notified() => {}
        }
        flush_cpu(&cpu_sender, &mut cpu_waiters);
        flush_file(&file_sender, &mut file_waiters);
        flush_sqlite(&sqlite_sender, &mut sqlite_waiters);
    }
}

fn submit_cpu(
    command: CpuCommand,
    mode: SubmissionMode,
    operation: &'static str,
    reservation: PendingReservation,
    sender: &ExecutorSender<CpuCommand>,
    waiters: &mut VecDeque<(CpuCommand, PendingReservation)>,
) {
    if !waiters.is_empty() {
        if mode == SubmissionMode::Try {
            command.reject(ExecutorError::overloaded(operation));
        } else {
            waiters.push_back((command, reservation));
        }
        return;
    }
    match sender.try_send(command) {
        Ok(()) => drop(reservation),
        Err(TrySendError::Full(command)) if mode == SubmissionMode::Durable => {
            waiters.push_back((command, reservation));
        }
        Err(TrySendError::Full(command)) => command.reject(ExecutorError::overloaded(operation)),
        Err(TrySendError::Disconnected(command)) => {
            command.reject(ExecutorError::shutting_down(operation));
        }
    }
}

fn submit_file(
    command: FileCommand,
    mode: SubmissionMode,
    operation: &'static str,
    reservation: PendingReservation,
    sender: &ExecutorSender<FileCommand>,
    waiters: &mut VecDeque<(FileCommand, PendingReservation)>,
) {
    if !waiters.is_empty() {
        if mode == SubmissionMode::Try {
            command.reject(ExecutorError::overloaded(operation));
        } else {
            waiters.push_back((command, reservation));
        }
        return;
    }
    match sender.try_send(command) {
        Ok(()) => drop(reservation),
        Err(TrySendError::Full(command)) if mode == SubmissionMode::Durable => {
            waiters.push_back((command, reservation));
        }
        Err(TrySendError::Full(command)) => command.reject(ExecutorError::overloaded(operation)),
        Err(TrySendError::Disconnected(command)) => {
            command.reject(ExecutorError::shutting_down(operation));
        }
    }
}

fn submit_sqlite(
    command: SqliteCommand,
    mode: SubmissionMode,
    operation: &'static str,
    reservation: PendingReservation,
    sender: &ExecutorSender<SqliteCommand>,
    waiters: &mut VecDeque<(SqliteCommand, PendingReservation)>,
) {
    if !waiters.is_empty() {
        if mode == SubmissionMode::Try {
            command.reject(ExecutorError::overloaded(operation));
        } else {
            waiters.push_back((command, reservation));
        }
        return;
    }
    match sender.try_send(command) {
        Ok(()) => drop(reservation),
        Err(TrySendError::Full(command)) if mode == SubmissionMode::Durable => {
            waiters.push_back((command, reservation));
        }
        Err(TrySendError::Full(command)) => command.reject(ExecutorError::overloaded(operation)),
        Err(TrySendError::Disconnected(command)) => {
            command.reject(ExecutorError::shutting_down(operation));
        }
    }
}

fn flush_cpu(
    sender: &ExecutorSender<CpuCommand>,
    waiters: &mut VecDeque<(CpuCommand, PendingReservation)>,
) {
    while let Some((command, reservation)) = waiters.pop_front() {
        match sender.try_send(command) {
            Ok(()) => drop(reservation),
            Err(TrySendError::Full(command)) => {
                waiters.push_front((command, reservation));
                return;
            }
            Err(TrySendError::Disconnected(command)) => {
                command.reject(ExecutorError::shutting_down("cpu_operation"));
            }
        }
    }
}

fn flush_file(
    sender: &ExecutorSender<FileCommand>,
    waiters: &mut VecDeque<(FileCommand, PendingReservation)>,
) {
    while let Some((command, reservation)) = waiters.pop_front() {
        match sender.try_send(command) {
            Ok(()) => drop(reservation),
            Err(TrySendError::Full(command)) => {
                waiters.push_front((command, reservation));
                return;
            }
            Err(TrySendError::Disconnected(command)) => {
                command.reject(ExecutorError::shutting_down("file_operation"));
            }
        }
    }
}

fn flush_sqlite(
    sender: &ExecutorSender<SqliteCommand>,
    waiters: &mut VecDeque<(SqliteCommand, PendingReservation)>,
) {
    while let Some((command, reservation)) = waiters.pop_front() {
        match sender.try_send(command) {
            Ok(()) => drop(reservation),
            Err(TrySendError::Full(command)) => {
                waiters.push_front((command, reservation));
                return;
            }
            Err(TrySendError::Disconnected(command)) => {
                command.reject(ExecutorError::shutting_down("sqlite_operation"));
            }
        }
    }
}

fn reject_all_waiters(
    cpu_waiters: &mut VecDeque<(CpuCommand, PendingReservation)>,
    file_waiters: &mut VecDeque<(FileCommand, PendingReservation)>,
    sqlite_waiters: &mut VecDeque<(SqliteCommand, PendingReservation)>,
) {
    for (command, _) in cpu_waiters.drain(..) {
        command.reject(ExecutorError::shutting_down("cpu_operation"));
    }
    for (command, _) in file_waiters.drain(..) {
        command.reject(ExecutorError::shutting_down("file_operation"));
    }
    for (command, _) in sqlite_waiters.drain(..) {
        command.reject(ExecutorError::shutting_down("sqlite_operation"));
    }
}
