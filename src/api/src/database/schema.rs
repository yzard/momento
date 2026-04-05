use crate::database::queries;
use crate::database::DbConn;
use crate::error::AppResult;

const SCHEMA: &str = include_str!("schema.sql");

pub mod sql {
    pub const PRAGMA_FOREIGN_KEYS_ON: &str = "PRAGMA foreign_keys = ON";
}

fn table_exists(conn: &DbConn, table: &str) -> AppResult<bool> {
    let count: i32 = conn.query_row(queries::schema::TABLE_EXISTS, [table], |row| row.get(0))?;
    Ok(count > 0)
}

pub fn init_database(conn: &DbConn) -> AppResult<()> {
    if table_exists(conn, "media")? {
        return Ok(());
    }
    conn.execute_batch(SCHEMA)?;
    Ok(())
}
