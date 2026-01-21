use crate::database::DbConn;
use crate::error::AppResult;

const SCHEMA: &str = include_str!("../../schema.sql");

pub fn init_database(conn: &DbConn) -> AppResult<()> {
    conn.execute_batch(SCHEMA)?;
    Ok(())
}

pub fn ensure_media_columns(conn: &DbConn) -> AppResult<()> {
    let existing: std::collections::HashSet<String> = {
        let mut stmt = conn.prepare("PRAGMA table_info(media)")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
        rows.filter_map(|r| r.ok()).collect()
    };

    let columns = [
        ("iso", "INTEGER"),
        ("exposure_time", "TEXT"),
        ("f_number", "REAL"),
        ("focal_length", "REAL"),
        ("focal_length_35mm", "REAL"),
        ("gps_altitude", "REAL"),
        ("location_city", "TEXT"),
        ("location_state", "TEXT"),
        ("location_country", "TEXT"),
        ("video_codec", "TEXT"),
        ("keywords", "TEXT"),
        ("deleted_at", "TEXT"),
        ("lens_make", "TEXT"),
        ("lens_model", "TEXT"),
    ];

    for (column_name, column_type) in columns {
        if !existing.contains(column_name) {
            conn.execute(
                &format!(
                    "ALTER TABLE media ADD COLUMN {} {}",
                    column_name, column_type
                ),
                [],
            )?;
        }
    }

    Ok(())
}
