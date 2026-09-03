use crate::error::AppResult;
use rusqlite::Connection;

const SCHEMA: &str = include_str!("schema.sql");
const MOMENTO_APPLICATION_ID: i64 = 0x4d4f_4d4f;
const MOMENTO_SCHEMA_VERSION: i64 = 1;

pub mod sql {
    pub const PRAGMA_FOREIGN_KEYS_ON: &str = "PRAGMA foreign_keys = ON";
}

pub fn init_database(conn: &Connection) -> AppResult<()> {
    let transaction = conn.unchecked_transaction()?;
    transaction.pragma_update(None, "application_id", MOMENTO_APPLICATION_ID)?;
    transaction.pragma_update(None, "user_version", MOMENTO_SCHEMA_VERSION)?;
    transaction.execute_batch(SCHEMA)?;
    transaction.commit().map_err(Into::into)
}

pub fn validate_database_schema(connection: &Connection) -> AppResult<()> {
    validate_database_identity(connection)?;
    let expected_connection = Connection::open_in_memory()?;
    init_database(&expected_connection)?;
    let expected = load_schema_manifest(&expected_connection)?;
    let actual = load_schema_manifest(connection)?;
    if actual != expected {
        return Err(crate::error::AppError::Internal(
            "database schema does not exactly match this Momento build; reset the database instead of migrating it"
                .to_string(),
        ));
    }
    Ok(())
}

fn validate_database_identity(connection: &Connection) -> AppResult<()> {
    let application_id =
        connection.pragma_query_value(None, "application_id", |row| row.get::<_, i64>(0))?;
    let schema_version =
        connection.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))?;
    if application_id != MOMENTO_APPLICATION_ID || schema_version != MOMENTO_SCHEMA_VERSION {
        return Err(crate::error::AppError::Internal(format!(
            "database identity does not match this Momento build: application_id={application_id}, user_version={schema_version}"
        )));
    }
    Ok(())
}

fn load_schema_manifest(
    connection: &Connection,
) -> AppResult<Vec<(String, String, String, String)>> {
    const MAX_SCHEMA_OBJECTS: usize = 512;
    const MAX_SCHEMA_MANIFEST_BYTES: usize = 1024 * 1024;

    let mut statement = connection.prepare(
        "SELECT type, name, tbl_name, COALESCE(sql, '') FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
    )?;
    let mut rows = statement.query([])?;
    let mut manifest = Vec::new();
    manifest
        .try_reserve_exact(MAX_SCHEMA_OBJECTS)
        .map_err(|_| {
            crate::error::AppError::Internal("could not reserve schema manifest".to_string())
        })?;
    let mut bytes = 0_usize;
    while let Some(row) = rows.next()? {
        if manifest.len() == MAX_SCHEMA_OBJECTS {
            return Err(crate::error::AppError::Internal(
                "database schema exceeds the bounded object manifest".to_string(),
            ));
        }
        let entry = (
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        );
        bytes = [&entry.0, &entry.1, &entry.2, &entry.3]
            .into_iter()
            .try_fold(bytes, |total, value| total.checked_add(value.len()))
            .ok_or_else(|| {
                crate::error::AppError::Internal("schema manifest size overflowed".to_string())
            })?;
        if bytes > MAX_SCHEMA_MANIFEST_BYTES {
            return Err(crate::error::AppError::Internal(
                "database schema exceeds the bounded manifest size".to_string(),
            ));
        }
        manifest.push(entry);
    }
    Ok(manifest)
}
