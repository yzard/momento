use std::path::Path;
use std::time::Duration;

use crate::constants::paths;
use crate::database::schema::sql;
use crate::error::{AppError, AppResult};
use r2d2::{Pool, PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, Row};

pub type DbPool = Pool<SqliteConnectionManager>;
pub type DbConn = PooledConnection<SqliteConnectionManager>;

const DATABASE_BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const WAL_AUTOCHECKPOINT_PAGES: u32 = 1_000;

pub fn create_pool() -> AppResult<DbPool> {
    create_pool_at(&paths().database)
}

pub fn create_pool_at(database_path: &Path) -> AppResult<DbPool> {
    initialize_database_file(database_path)?;
    let manager = SqliteConnectionManager::file(database_path).with_init(configure_connection);

    Pool::builder()
        .max_size(10)
        .build(manager)
        .map_err(|e| AppError::Internal(format!("Failed to create database pool: {}", e)))
}

fn initialize_database_file(database_path: &Path) -> AppResult<()> {
    let connection = Connection::open(database_path)?;
    connection.busy_timeout(DATABASE_BUSY_TIMEOUT)?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.pragma_update(None, "wal_autocheckpoint", WAL_AUTOCHECKPOINT_PAGES)?;
    Ok(())
}

pub fn configure_connection(connection: &mut Connection) -> rusqlite::Result<()> {
    connection.execute_batch(sql::PRAGMA_FOREIGN_KEYS_ON)?;
    connection.busy_timeout(DATABASE_BUSY_TIMEOUT)?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.pragma_update(None, "wal_autocheckpoint", WAL_AUTOCHECKPOINT_PAGES)?;
    Ok(())
}

pub fn get_connection(pool: &DbPool) -> AppResult<DbConn> {
    pool.get().map_err(AppError::Pool)
}

pub fn fetch_one<T, F>(
    conn: &DbConn,
    sql: &str,
    params: &[&(dyn rusqlite::ToSql + '_)],
    mapper: F,
) -> AppResult<Option<T>>
where
    F: FnOnce(&Row<'_>) -> rusqlite::Result<T>,
{
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query(params)?;

    match rows.next()? {
        Some(row) => Ok(Some(mapper(row)?)),
        None => Ok(None),
    }
}

pub fn fetch_all<T, F>(
    conn: &DbConn,
    sql: &str,
    params: &[&(dyn rusqlite::ToSql + '_)],
    mapper: F,
) -> AppResult<Vec<T>>
where
    F: FnMut(&Row<'_>) -> rusqlite::Result<T>,
{
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, mapper)?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

pub fn execute_query(
    conn: &DbConn,
    sql: &str,
    params: &[&(dyn rusqlite::ToSql + '_)],
) -> AppResult<usize> {
    conn.execute(sql, params).map_err(AppError::from)
}

pub fn insert_returning_id(
    conn: &DbConn,
    sql: &str,
    params: &[&(dyn rusqlite::ToSql + '_)],
) -> AppResult<i64> {
    conn.execute(sql, params)?;
    Ok(conn.last_insert_rowid())
}

pub fn execute_many(
    conn: &DbConn,
    sql: &str,
    params_list: &[Vec<&(dyn rusqlite::ToSql + '_)>],
) -> AppResult<()> {
    let mut stmt = conn.prepare(sql)?;
    for params in params_list {
        stmt.execute(params.as_slice())?;
    }
    Ok(())
}
