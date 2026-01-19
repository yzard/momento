use crate::constants::DATABASE_PATH;
use crate::error::{AppError, AppResult};
use r2d2::{Pool, PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Row;

pub type DbPool = Pool<SqliteConnectionManager>;
pub type DbConn = PooledConnection<SqliteConnectionManager>;

pub fn create_pool() -> AppResult<DbPool> {
    let manager = SqliteConnectionManager::file(&*DATABASE_PATH)
        .with_init(|conn| {
            conn.execute_batch("PRAGMA foreign_keys = ON")?;
            Ok(())
        });

    Pool::builder()
        .max_size(10)
        .build(manager)
        .map_err(|e| AppError::Internal(format!("Failed to create database pool: {}", e)))
}

pub fn get_connection(pool: &DbPool) -> AppResult<DbConn> {
    pool.get().map_err(AppError::Pool)
}

pub fn fetch_one<T, F>(conn: &DbConn, sql: &str, params: &[&dyn rusqlite::ToSql], mapper: F) -> AppResult<Option<T>>
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

pub fn fetch_all<T, F>(conn: &DbConn, sql: &str, params: &[&dyn rusqlite::ToSql], mapper: F) -> AppResult<Vec<T>>
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

pub fn execute_query(conn: &DbConn, sql: &str, params: &[&dyn rusqlite::ToSql]) -> AppResult<usize> {
    conn.execute(sql, params).map_err(AppError::Database)
}

pub fn insert_returning_id(conn: &DbConn, sql: &str, params: &[&dyn rusqlite::ToSql]) -> AppResult<i64> {
    conn.execute(sql, params)?;
    Ok(conn.last_insert_rowid())
}

pub fn execute_many(conn: &DbConn, sql: &str, params_list: &[Vec<&dyn rusqlite::ToSql>]) -> AppResult<()> {
    let mut stmt = conn.prepare(sql)?;
    for params in params_list {
        stmt.execute(params.as_slice())?;
    }
    Ok(())
}
