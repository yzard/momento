use crate::database::DbConn;
use crate::error::AppResult;

const SCHEMA: &str = include_str!("schema.sql");
const DELETE_REMOVED_OBJECT_DETECTION: &str =
    "DELETE FROM image_text WHERE model_type = 'object_detection'";

pub mod sql {
    pub const PRAGMA_FOREIGN_KEYS_ON: &str = "PRAGMA foreign_keys = ON";
}

type LegacyImageTextRow = (i64, String, String);

pub fn init_database(conn: &DbConn) -> AppResult<()> {
    let (has_legacy_table, legacy_rows) = load_legacy_image_text(conn)?;
    if has_legacy_table {
        conn.execute_batch(
            "DROP TRIGGER IF EXISTS delete_image_text_after_media_delete;
             DROP TABLE image_text;",
        )?;
    }
    conn.execute_batch(SCHEMA)?;
    for (image_id, model_type, text) in legacy_rows {
        conn.execute(
            "INSERT INTO image_text (image_id, model_type, model_version, string)
             VALUES (?1, ?2, 'legacy', ?3)",
            rusqlite::params![image_id, model_type, text],
        )?;
    }
    conn.execute(DELETE_REMOVED_OBJECT_DETECTION, [])?;
    Ok(())
}

fn load_legacy_image_text(conn: &DbConn) -> AppResult<(bool, Vec<LegacyImageTextRow>)> {
    let has_plugin_id = conn
        .prepare("PRAGMA table_info(image_text)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .any(|column| column == "plugin_id");
    if !has_plugin_id {
        return Ok((false, Vec::new()));
    }

    let mut statement = conn.prepare(
        "SELECT image_id, 'ocr', string
           FROM image_text
          WHERE plugin_id = 1",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get::<_, String>(1)?, row.get(2)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok((true, rows))
}
