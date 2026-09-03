use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use crate::database::schema::sql;
use crate::error::{AppError, AppResult};
use crate::io::session::rename_descriptor_entry;
use r2d2::{ManageConnection, Pool, PooledConnection};
use rusqlite::{Connection, Row};
use scheduled_thread_pool::ScheduledThreadPool;

#[derive(Clone, Debug)]
pub struct DbPool {
    pool: Pool<BudgetedSqliteConnectionManager>,
    connection_budget: Arc<RwLock<Option<SqliteConnectionBudget>>>,
}

impl std::ops::Deref for DbPool {
    type Target = Pool<BudgetedSqliteConnectionManager>;

    fn deref(&self) -> &Self::Target {
        &self.pool
    }
}

impl DbPool {
    pub(crate) fn set_connection_budget(
        &self,
        budget: crate::io::space_budget::DataDirSpaceBudget,
        database_path: PathBuf,
    ) -> AppResult<()> {
        let fragment_size = budget.filesystem_fragment_size();
        let maximum_page_bytes = 65_536_u64;
        let wal_activation_bytes = round_up_bootstrap(
            32_u64
                .checked_add(24)
                .and_then(|value| value.checked_add(maximum_page_bytes))
                .ok_or_else(|| {
                    AppError::Internal("SQLite connection WAL footprint overflowed".to_string())
                })?,
            fragment_size,
        )?;
        let shm_bytes = round_up_bootstrap(32 * 1024, fragment_size)?;
        let directory_metadata_bytes = fragment_size.checked_mul(8).ok_or_else(|| {
            AppError::Internal("SQLite connection metadata footprint overflowed".to_string())
        })?;
        let peak_additional_bytes = wal_activation_bytes
            .checked_add(shm_bytes)
            .and_then(|value| value.checked_add(directory_metadata_bytes))
            .ok_or_else(|| {
                AppError::Internal("SQLite connection footprint overflowed".to_string())
            })?;
        *self.connection_budget.write().map_err(|_| {
            AppError::Internal("SQLite connection budget lock was poisoned".to_string())
        })? = Some(SqliteConnectionBudget {
            budget,
            database_path,
            peak_additional_bytes,
        });
        Ok(())
    }
}

pub type DbConn = PooledConnection<BudgetedSqliteConnectionManager>;

#[derive(Debug, Clone)]
struct SqliteConnectionBudget {
    budget: crate::io::space_budget::DataDirSpaceBudget,
    database_path: PathBuf,
    peak_additional_bytes: u64,
}

#[derive(Debug)]
pub struct BudgetedSqliteConnectionManager {
    database_path: PathBuf,
    connection_budget: Arc<RwLock<Option<SqliteConnectionBudget>>>,
}

impl ManageConnection for BudgetedSqliteConnectionManager {
    type Connection = Connection;
    type Error = rusqlite::Error;

    fn connect(&self) -> Result<Self::Connection, Self::Error> {
        let connection_budget = self
            .connection_budget
            .read()
            .map_err(|_| sqlite_capacity_error("SQLite connection budget lock was poisoned"))?
            .clone();
        let capacity_token = connection_budget
            .as_ref()
            .map(|connection_budget| {
                connection_budget
                    .budget
                    .reserve_sqlite(
                        format!("sqlite-connection-{}", uuid::Uuid::new_v4().simple()),
                        connection_budget.peak_additional_bytes,
                    )
                    .and_then(|admission| admission.into_result())
                    .map_err(sqlite_capacity_error)
            })
            .transpose()?;
        let mut connection = Connection::open_with_flags(
            &self.database_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        configure_connection(&mut connection)?;
        connection.query_row("SELECT 1", [], |_| Ok(()))?;
        if let (Some(token), Some(connection_budget)) = (capacity_token, connection_budget) {
            let allocated = crate::io::space_budget::measure_sqlite_allocation(
                &connection_budget.database_path,
            )
            .map_err(sqlite_capacity_error)?;
            token
                .publish_ephemeral_sqlite_allocation(allocated)
                .map_err(sqlite_capacity_error)?;
        }
        Ok(connection)
    }

    fn is_valid(&self, connection: &mut Self::Connection) -> Result<(), Self::Error> {
        connection.execute_batch("")
    }

    fn has_broken(&self, _connection: &mut Self::Connection) -> bool {
        false
    }
}

fn sqlite_capacity_error(error: impl std::fmt::Display) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_FULL),
        Some(error.to_string()),
    )
}

const DATABASE_BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const WAL_AUTOCHECKPOINT_PAGES: u32 = 1_000;
const SQLITE_PAGE_CACHE_KIBIBYTES: i64 = 8 * 1024;
const SQLITE_PREPARED_STATEMENTS_PER_CONNECTION: usize = 64;
const R2D2_MAINTENANCE_THREADS: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SqliteBootstrapFootprintSpec {
    pub page_size_bytes: u64,
    pub schema_page_count: u64,
    pub private_database_bytes: u64,
    pub rollback_journal_peak_bytes: u64,
    pub wal_activation_peak_bytes: u64,
    pub shm_recreation_peak_bytes: u64,
    pub peak_additional_bytes: u64,
}

impl SqliteBootstrapFootprintSpec {
    pub fn derive(fragment_size: u64) -> AppResult<Self> {
        if fragment_size == 0 || !fragment_size.is_power_of_two() {
            return Err(AppError::Internal(
                "filesystem fragment size is invalid for SQLite bootstrap".to_string(),
            ));
        }
        let connection = Connection::open_in_memory()?;
        connection.pragma_update(None, "journal_mode", "DELETE")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        connection.pragma_update(None, "temp_store", "MEMORY")?;
        connection.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
        crate::database::schema::init_database(&connection)?;
        let page_size_bytes =
            connection.query_row("PRAGMA page_size", [], |row| row.get::<_, u64>(0))?;
        let schema_page_count =
            connection.query_row("PRAGMA page_count", [], |row| row.get::<_, u64>(0))?;
        if page_size_bytes == 0 || !page_size_bytes.is_power_of_two() || schema_page_count == 0 {
            return Err(AppError::Internal(
                "SQLite bootstrap schema reported invalid geometry".to_string(),
            ));
        }
        let private_database_bytes = round_up_bootstrap(
            page_size_bytes
                .checked_mul(schema_page_count)
                .ok_or_else(|| AppError::Internal("SQLite schema size overflowed".to_string()))?,
            fragment_size,
        )?;
        let rollback_journal_peak_bytes = private_database_bytes;
        let wal_activation_peak_bytes = round_up_bootstrap(
            32_u64
                .checked_add(24)
                .and_then(|value| value.checked_add(page_size_bytes))
                .ok_or_else(|| AppError::Internal("SQLite WAL peak overflowed".to_string()))?,
            fragment_size,
        )?;
        let shm_recreation_peak_bytes = round_up_bootstrap(32 * 1024, fragment_size)?;
        let directory_metadata_peak = fragment_size
            .checked_mul(8)
            .ok_or_else(|| AppError::Internal("SQLite metadata peak overflowed".to_string()))?;
        let peak_additional_bytes = private_database_bytes
            .checked_add(rollback_journal_peak_bytes)
            .and_then(|value| value.checked_add(wal_activation_peak_bytes))
            .and_then(|value| value.checked_add(shm_recreation_peak_bytes))
            .and_then(|value| value.checked_add(directory_metadata_peak))
            .ok_or_else(|| AppError::Internal("SQLite bootstrap peak overflowed".to_string()))?;
        Ok(Self {
            page_size_bytes,
            schema_page_count,
            private_database_bytes,
            rollback_journal_peak_bytes,
            wal_activation_peak_bytes,
            shm_recreation_peak_bytes,
            peak_additional_bytes,
        })
    }
}

fn round_up_bootstrap(value: u64, alignment: u64) -> AppResult<u64> {
    let mask = alignment
        .checked_sub(1)
        .ok_or_else(|| AppError::Internal("SQLite bootstrap alignment overflowed".to_string()))?;
    value
        .checked_add(mask)
        .map(|rounded| rounded & !mask)
        .ok_or_else(|| AppError::Internal("SQLite bootstrap rounding overflowed".to_string()))
}

pub fn create_pool_at(database_path: &Path, sqlite_workers: usize) -> AppResult<DbPool> {
    let pool_size = u32::try_from(sqlite_workers)
        .map_err(|_| AppError::Internal("sqlite worker count exceeds r2d2 capacity".to_string()))?;
    prepare_database_file(database_path)?;
    initialize_database_file(database_path)?;
    let connection_budget = Arc::new(RwLock::new(None));
    let manager = create_connection_manager(database_path, Arc::clone(&connection_budget));

    let pool = Pool::builder()
        .max_size(pool_size)
        .min_idle(Some(pool_size))
        .connection_timeout(DATABASE_BUSY_TIMEOUT)
        .thread_pool(Arc::new(
            ScheduledThreadPool::builder()
                .num_threads(R2D2_MAINTENANCE_THREADS)
                .thread_name_pattern("momento-r2d2-maintenance")
                .build(),
        ))
        .build(manager)
        .map_err(|error| AppError::Internal(format!("Failed to create database pool: {error}")))?;

    let mut warmed_connections = Vec::new();
    warmed_connections
        .try_reserve_exact(sqlite_workers)
        .map_err(|error| AppError::Internal(format!("Failed to reserve SQLite warmup: {error}")))?;
    for _ in 0..sqlite_workers {
        let connection = pool.get().map_err(AppError::Pool)?;
        connection.query_row("SELECT 1", [], |_| Ok(()))?;
        warmed_connections.push(connection);
    }
    drop(warmed_connections);
    Ok(DbPool {
        pool,
        connection_budget,
    })
}

fn create_connection_manager(
    database_path: &Path,
    connection_budget: Arc<RwLock<Option<SqliteConnectionBudget>>>,
) -> BudgetedSqliteConnectionManager {
    BudgetedSqliteConnectionManager {
        database_path: database_path.to_path_buf(),
        connection_budget,
    }
}

pub(crate) fn open_existing_database_read_only(database_path: &Path) -> AppResult<Connection> {
    use rusqlite::OpenFlags;

    let connection = Connection::open_with_flags(
        database_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(DATABASE_BUSY_TIMEOUT)?;
    connection.pragma_update(None, "query_only", true)?;
    connection.pragma_update(None, "temp_store", "MEMORY")?;
    connection.pragma_update(None, "mmap_size", 0)?;
    connection.pragma_update(None, "cache_size", -SQLITE_PAGE_CACHE_KIBIBYTES)?;
    connection.pragma_update(None, "threads", 0)?;
    connection.set_prepared_statement_cache_capacity(SQLITE_PREPARED_STATEMENTS_PER_CONNECTION);
    crate::database::schema::validate_database_schema(&connection)?;
    connection
        .query_row(
            "SELECT 1 FROM data_dir_space_reservations LIMIT 1",
            [],
            |_| Ok(()),
        )
        .or_else(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => Ok(()),
            other => Err(other),
        })?;
    Ok(connection)
}

pub(crate) fn recover_existing_database(database_path: &Path) -> AppResult<()> {
    use rusqlite::OpenFlags;

    let connection = Connection::open_with_flags(
        database_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(DATABASE_BUSY_TIMEOUT)?;
    connection.pragma_update(None, "synchronous", "FULL")?;
    let (busy, _, _): (i64, i64, i64) =
        connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
    if busy != 0 {
        return Err(AppError::DatabaseBusy);
    }
    connection
        .close()
        .map_err(|(_, error)| AppError::Database(error))?;
    File::open(database_path)?.sync_all()?;
    for side_path in [
        database_path.with_extension("sqlite-wal"),
        database_path.with_extension("sqlite-shm"),
    ] {
        match File::open(&side_path) {
            Ok(file) => file.sync_all()?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let parent = database_path.parent().ok_or_else(|| {
        AppError::Internal("database path does not have a parent directory".to_string())
    })?;
    sync_directory(parent)
}

fn prepare_database_file(database_path: &Path) -> AppResult<()> {
    let parent = database_path.parent().ok_or_else(|| {
        AppError::Internal("database path does not have a parent directory".to_string())
    })?;
    let filename = database_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| AppError::Internal("database filename is not valid UTF-8".to_string()))?;
    let temporary_filename = format!(".{filename}.momento-bootstrap.tmp");
    let temporary_path = parent.join(&temporary_filename);
    let main_metadata = std::fs::symlink_metadata(database_path);
    let temporary_metadata = std::fs::symlink_metadata(&temporary_path);

    match (main_metadata, temporary_metadata) {
        (Ok(main), Ok(_)) => {
            if main.file_type().is_symlink() || !main.is_file() || main.len() == 0 {
                return Err(AppError::Internal(
                    "database.sqlite is not a complete regular file".to_string(),
                ));
            }
            return Err(AppError::Internal(format!(
                "official database and bootstrap temporary both exist: {}",
                temporary_path.display()
            )));
        }
        (Ok(main), Err(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            if main.file_type().is_symlink() || !main.is_file() || main.len() == 0 {
                return Err(AppError::Internal(
                    "database.sqlite is not a complete regular file".to_string(),
                ));
            }
            return Ok(());
        }
        (Ok(_), Err(error)) => return Err(error.into()),
        (Err(error), _) if error.kind() != std::io::ErrorKind::NotFound => {
            return Err(error.into());
        }
        (Err(_), Ok(temporary)) => {
            if temporary.file_type().is_symlink() || !temporary.is_file() {
                return Err(AppError::Internal(
                    "SQLite bootstrap temporary is not a regular file".to_string(),
                ));
            }
            validate_unpublished_bootstrap_file(&temporary_path)?;
            std::fs::remove_file(&temporary_path)?;
            sync_directory(parent)?;
        }
        (Err(_), Err(error)) if error.kind() != std::io::ErrorKind::NotFound => {
            return Err(error.into());
        }
        (Err(_), Err(_)) => {}
    }

    for side_file in [
        database_path.with_extension("sqlite-wal"),
        database_path.with_extension("sqlite-shm"),
    ] {
        match std::fs::symlink_metadata(&side_file) {
            Ok(_) => {
                return Err(AppError::Internal(format!(
                    "SQLite side file exists without the main database: {}",
                    side_file.display()
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }

    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary_path)?;
    let connection = Connection::open(&temporary_path)?;
    connection.busy_timeout(DATABASE_BUSY_TIMEOUT)?;
    connection.pragma_update(None, "journal_mode", "DELETE")?;
    connection.pragma_update(None, "synchronous", "FULL")?;
    connection.pragma_update(None, "temp_store", "MEMORY")?;
    connection.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
    crate::database::schema::init_database(&connection)?;
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(AppError::Internal(format!(
            "fresh SQLite schema failed integrity validation: {integrity}"
        )));
    }
    let reservations: i64 = connection.query_row(
        "SELECT COUNT(*) FROM data_dir_space_reservations",
        [],
        |row| row.get(0),
    )?;
    if reservations != 0 {
        return Err(AppError::Internal(
            "fresh SQLite schema unexpectedly contains space reservations".to_string(),
        ));
    }
    connection
        .close()
        .map_err(|(_, error)| AppError::Database(error))?;
    File::open(&temporary_path)?.sync_all()?;
    rename_without_replacement(parent, &temporary_filename, filename)?;
    sync_directory(parent)
}

fn validate_unpublished_bootstrap_file(temporary_path: &Path) -> AppResult<()> {
    use rusqlite::OpenFlags;

    let connection = Connection::open_with_flags(
        temporary_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| {
        AppError::Internal(format!(
            "SQLite bootstrap temporary is not a valid database: {error}"
        ))
    })?;
    connection.pragma_update(None, "query_only", true)?;
    crate::database::schema::validate_database_schema(&connection).map_err(|error| {
        AppError::Internal(format!(
            "SQLite bootstrap temporary does not match the current schema: {error}"
        ))
    })?;
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(AppError::Internal(format!(
            "SQLite bootstrap temporary failed integrity validation: {integrity}"
        )));
    }
    let mut table_names = Vec::new();
    table_names.try_reserve_exact(512).map_err(|_| {
        AppError::Internal("could not reserve bootstrap table validation".to_string())
    })?;
    let mut statement = connection
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name LIMIT 513")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    for table_name in rows {
        if table_names.len() == 512 {
            return Err(AppError::Internal(
                "SQLite bootstrap temporary exceeds the table validation bound".to_string(),
            ));
        }
        table_names.push(table_name?);
    }
    drop(statement);
    for table_name in table_names {
        if matches!(
            table_name.as_str(),
            "media_rtree_node" | "media_rtree_parent" | "media_rtree_rowid"
        ) {
            continue;
        }
        let quoted = table_name.replace('"', "\"\"");
        let query = format!("SELECT EXISTS(SELECT 1 FROM \"{quoted}\" LIMIT 1)");
        let has_rows = connection.query_row(&query, [], |row| row.get::<_, bool>(0))?;
        if has_rows {
            return Err(AppError::Internal(format!(
                "SQLite bootstrap temporary contains data in {table_name}"
            )));
        }
    }
    Ok(())
}

fn rename_without_replacement(parent: &Path, source: &str, destination: &str) -> AppResult<()> {
    let parent = File::open(parent)?;
    let source = CString::new(source)
        .map_err(|_| AppError::Internal("SQLite bootstrap source name is invalid".to_string()))?;
    let destination = CString::new(destination).map_err(|_| {
        AppError::Internal("SQLite bootstrap destination name is invalid".to_string())
    })?;
    rename_descriptor_entry(
        parent.as_raw_fd(),
        &source,
        parent.as_raw_fd(),
        &destination,
        libc::RENAME_NOREPLACE,
    )
    .map_err(Into::into)
}

fn sync_directory(path: &Path) -> AppResult<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}

fn initialize_database_file(database_path: &Path) -> AppResult<()> {
    let connection = Connection::open(database_path)?;
    connection.busy_timeout(DATABASE_BUSY_TIMEOUT)?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.pragma_update(None, "wal_autocheckpoint", WAL_AUTOCHECKPOINT_PAGES)?;
    connection.pragma_update(None, "temp_store", "MEMORY")?;
    connection.pragma_update(None, "mmap_size", 0)?;
    connection.pragma_update(None, "cache_size", -SQLITE_PAGE_CACHE_KIBIBYTES)?;
    connection.pragma_update(None, "threads", 0)?;
    connection.set_prepared_statement_cache_capacity(SQLITE_PREPARED_STATEMENTS_PER_CONNECTION);
    Ok(())
}

pub fn configure_connection(connection: &mut Connection) -> rusqlite::Result<()> {
    connection.execute_batch(sql::PRAGMA_FOREIGN_KEYS_ON)?;
    connection.busy_timeout(DATABASE_BUSY_TIMEOUT)?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.pragma_update(None, "wal_autocheckpoint", WAL_AUTOCHECKPOINT_PAGES)?;
    connection.pragma_update(None, "temp_store", "MEMORY")?;
    connection.pragma_update(None, "mmap_size", 0)?;
    connection.pragma_update(None, "cache_size", -SQLITE_PAGE_CACHE_KIBIBYTES)?;
    connection.pragma_update(None, "threads", 0)?;
    connection.set_prepared_statement_cache_capacity(SQLITE_PREPARED_STATEMENTS_PER_CONNECTION);
    Ok(())
}

pub fn fetch_one<T, F>(
    conn: &Connection,
    sql: &str,
    params: &[&(dyn rusqlite::ToSql + '_)],
    mapper: F,
) -> AppResult<Option<T>>
where
    F: FnOnce(&Row<'_>) -> rusqlite::Result<T>,
{
    let mut statement = conn.prepare(sql)?;
    let mut rows = statement.query(params)?;

    match rows.next()? {
        Some(row) => Ok(Some(mapper(row)?)),
        None => Ok(None),
    }
}

pub fn fetch_all<T, F>(
    conn: &Connection,
    sql: &str,
    params: &[&(dyn rusqlite::ToSql + '_)],
    mapper: F,
) -> AppResult<Vec<T>>
where
    F: FnMut(&Row<'_>) -> rusqlite::Result<T>,
{
    let mut statement = conn.prepare(sql)?;
    let rows = statement.query_map(params, mapper)?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

pub fn execute_query(
    conn: &Connection,
    sql: &str,
    params: &[&(dyn rusqlite::ToSql + '_)],
) -> AppResult<usize> {
    conn.execute(sql, params).map_err(AppError::from)
}

pub fn insert_returning_id(
    conn: &Connection,
    sql: &str,
    params: &[&(dyn rusqlite::ToSql + '_)],
) -> AppResult<i64> {
    conn.execute(sql, params)?;
    Ok(conn.last_insert_rowid())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::MetadataExt;

    use super::*;
    use r2d2::ManageConnection;

    #[derive(Debug, Eq, PartialEq)]
    struct FileSnapshot {
        device: u64,
        inode: u64,
        length: u64,
        blocks: u64,
        modified_seconds: i64,
        modified_nanoseconds: i64,
        changed_seconds: i64,
        changed_nanoseconds: i64,
        bytes: Vec<u8>,
    }

    #[test]
    fn replacement_connection_creation_checks_out_and_publishes_sqlite_capacity() {
        let directory = tempfile::tempdir().expect("temporary database directory");
        let database_path = directory.path().join("database.sqlite");
        prepare_database_file(&database_path).expect("prepare database");
        initialize_database_file(&database_path).expect("initialize database");
        let budget = crate::io::space_budget::DataDirSpaceBudget::from_directory(
            File::open(directory.path()).expect("data directory descriptor"),
        )
        .expect("space budget");
        let allocated = crate::io::space_budget::measure_sqlite_allocation(&database_path)
            .expect("SQLite allocation");
        let mut reconstruction = budget.begin_reconstruction();
        reconstruction.set_allocated_bytes(allocated, 0);
        reconstruction.publish().expect("publish reconstruction");
        budget.mark_running().expect("running budget");
        let connection_budget = Arc::new(RwLock::new(Some(SqliteConnectionBudget {
            budget: budget.clone(),
            database_path: database_path.clone(),
            peak_additional_bytes: 1024 * 1024,
        })));
        let manager = create_connection_manager(&database_path, connection_budget);

        let connection = manager.connect().expect("budgeted replacement connection");
        connection
            .query_row("SELECT 1", [], |_| Ok(()))
            .expect("validated replacement");
        let snapshot = budget.snapshot().expect("space budget snapshot");
        assert_eq!(snapshot.sqlite_outstanding_bytes, 0);
        assert!(snapshot.sqlite_allocated_bytes >= allocated);
    }

    fn snapshot(path: &Path) -> FileSnapshot {
        let metadata = std::fs::metadata(path).expect("SQLite file metadata");
        FileSnapshot {
            device: metadata.dev(),
            inode: metadata.ino(),
            length: metadata.len(),
            blocks: metadata.blocks(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
            changed_seconds: metadata.ctime(),
            changed_nanoseconds: metadata.ctime_nsec(),
            bytes: std::fs::read(path).expect("SQLite file bytes"),
        }
    }

    #[test]
    fn existing_database_read_only_probe_does_not_mutate_main_wal_or_shm() {
        let directory = tempfile::tempdir().expect("database directory");
        let database_path = directory.path().join("database.sqlite");
        prepare_database_file(&database_path).expect("fresh database");
        initialize_database_file(&database_path).expect("WAL activation");
        let mut writer = Connection::open(&database_path).expect("writer connection");
        configure_connection(&mut writer).expect("writer configuration");
        writer
            .execute(
                "INSERT INTO users (username, email, hashed_password) VALUES ('probe', 'probe@example.com', 'hash')",
                [],
            )
            .expect("WAL frame");
        writer
            .query_row("SELECT COUNT(*) FROM users", [], |row| row.get::<_, i64>(0))
            .expect("initialize shared memory");

        let paths = [
            database_path.clone(),
            database_path.with_extension("sqlite-wal"),
            database_path.with_extension("sqlite-shm"),
        ];
        let before = paths.iter().map(|path| snapshot(path)).collect::<Vec<_>>();

        let read_only =
            open_existing_database_read_only(&database_path).expect("read-only database probe");
        assert_eq!(
            read_only
                .query_row("SELECT COUNT(*) FROM users", [], |row| row.get::<_, i64>(0))
                .expect("read through probe"),
            1
        );
        drop(read_only);

        let after = paths.iter().map(|path| snapshot(path)).collect::<Vec<_>>();
        assert_eq!(after, before);
        drop(writer);
    }
}
