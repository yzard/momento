use crate::database::DbConn;
use crate::error::AppResult;

const SCHEMA: &str = include_str!("schema.sql");

pub mod sql {
    pub const PRAGMA_FOREIGN_KEYS_ON: &str = "PRAGMA foreign_keys = ON";
}

pub fn init_database(conn: &DbConn) -> AppResult<()> {
    conn.execute_batch(SCHEMA)?;
    Ok(())
}
