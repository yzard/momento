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
        ("lens_make", "TEXT"),
        ("lens_model", "TEXT"),
        ("content_hash", "TEXT"),
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

    let _ = conn.execute("CREATE UNIQUE INDEX IF NOT EXISTS idx_media_content_hash ON media (content_hash) WHERE content_hash IS NOT NULL", []);

    Ok(())
}

pub fn ensure_access_control_setup(conn: &DbConn) -> AppResult<()> {
    conn.execute_batch(r#"
    CREATE TABLE IF NOT EXISTS media_access (
        media_id INTEGER NOT NULL
      , user_id INTEGER NOT NULL
      , access_level INTEGER NOT NULL
      , created_at TEXT DEFAULT (datetime('now'))
      , deleted_at TEXT DEFAULT NULL
      , PRIMARY KEY (media_id, user_id)
      , FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
      , FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS album_access (
        album_id INTEGER NOT NULL
      , user_id INTEGER NOT NULL
      , access_level INTEGER NOT NULL
      , created_at TEXT DEFAULT (datetime('now'))
      , PRIMARY KEY (album_id, user_id)
      , FOREIGN KEY (album_id) REFERENCES albums(id) ON DELETE CASCADE
      , FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
    );

    CREATE INDEX IF NOT EXISTS idx_media_access_user_deleted ON media_access(user_id, deleted_at) WHERE deleted_at IS NOT NULL;
    CREATE INDEX IF NOT EXISTS idx_media_access_media ON media_access(media_id);
    CREATE INDEX IF NOT EXISTS idx_album_access_user ON album_access(user_id);
    "#)?;

    Ok(())
}
