use crate::database::DbConn;
use crate::error::AppResult;

/// Current schema version
pub const CURRENT_SCHEMA_VERSION: i32 = 1;

/// SQL for schema version tracking table
const CREATE_SCHEMA_VERSION_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL
)
"#;

/// SQL for geohash column migration
const ADD_GEOHASH_COLUMN: &str = "ALTER TABLE media ADD COLUMN geohash TEXT";

/// SQL for geohash index
const CREATE_GEOHASH_INDEX: &str = "CREATE INDEX IF NOT EXISTS idx_media_geohash ON media(geohash)";

/// SQL for R-tree virtual table
const CREATE_MEDIA_RTREE: &str = r#"
CREATE VIRTUAL TABLE IF NOT EXISTS media_rtree USING rtree(
    media_id,
    min_lat, max_lat,
    min_lon, max_lon
)
"#;

/// Check if a column exists in a table
fn column_exists(conn: &DbConn, table: &str, column: &str) -> AppResult<bool> {
    let sql = format!("PRAGMA table_info({})", table);
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;

    for row in rows {
        if row? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Check if a table exists
fn table_exists(conn: &DbConn, table: &str) -> AppResult<bool> {
    let count: i32 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?",
        [table],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// Get current schema version from database
fn get_schema_version(conn: &DbConn) -> AppResult<i32> {
    if !table_exists(conn, "schema_version")? {
        return Ok(0);
    }

    let version: Option<i32> = conn
        .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
            row.get(0)
        })
        .ok();

    Ok(version.unwrap_or(0))
}

/// Record a migration as applied
fn record_migration(conn: &DbConn, version: i32) -> AppResult<()> {
    conn.execute(
        "INSERT INTO schema_version (version, applied_at) VALUES (?, datetime('now'))",
        [version],
    )?;
    Ok(())
}

/// Run all pending migrations
pub fn run_migrations(conn: &DbConn) -> AppResult<()> {
    // Ensure schema_version table exists
    conn.execute_batch(CREATE_SCHEMA_VERSION_TABLE)?;

    let current_version = get_schema_version(conn)?;

    // Migration 1: Add geohash column and R-tree table
    if current_version < 1 {
        migrate_v1(conn)?;
        record_migration(conn, 1)?;
    }

    Ok(())
}

/// Migration v1: Add geohash column, geohash index, and R-tree virtual table
fn migrate_v1(conn: &DbConn) -> AppResult<()> {
    // Add geohash column if it doesn't exist
    if !column_exists(conn, "media", "geohash")? {
        conn.execute(ADD_GEOHASH_COLUMN, [])?;
    }

    // Create geohash index (IF NOT EXISTS handles idempotency)
    conn.execute(CREATE_GEOHASH_INDEX, [])?;

    // Create R-tree virtual table (IF NOT EXISTS handles idempotency)
    conn.execute_batch(CREATE_MEDIA_RTREE)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::init_database;
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;

    /// Create a fresh test database with schema applied
    fn create_test_db() -> DbConn {
        let manager = SqliteConnectionManager::memory().with_init(|conn| {
            conn.execute_batch("PRAGMA foreign_keys = ON")?;
            Ok(())
        });

        let pool = Pool::builder()
            .max_size(1)
            .build(manager)
            .expect("Failed to create test database pool");

        let conn = pool.get().expect("Failed to get connection from pool");
        init_database(&conn).expect("Failed to initialize test database schema");
        conn
    }

    /// Create a test database simulating an "old" database without geohash column
    fn create_old_db_without_geohash() -> DbConn {
        let manager = SqliteConnectionManager::memory().with_init(|conn| {
            conn.execute_batch("PRAGMA foreign_keys = ON")?;
            Ok(())
        });

        let pool = Pool::builder()
            .max_size(1)
            .build(manager)
            .expect("Failed to create test database pool");

        let conn = pool.get().expect("Failed to get connection from pool");

        // Create a minimal media table WITHOUT geohash column (simulating old schema)
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS media (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                filename TEXT NOT NULL,
                original_filename TEXT NOT NULL,
                file_path TEXT NOT NULL,
                media_type TEXT NOT NULL,
                gps_latitude REAL,
                gps_longitude REAL,
                content_hash TEXT UNIQUE
            )
            "#,
        )
        .expect("Failed to create old media table");

        conn
    }

    #[test]
    fn test_migration_fresh_database_has_geohash_column() {
        let conn = create_test_db();

        // Run migrations
        run_migrations(&conn).expect("Migration should succeed");

        // Verify geohash column exists
        assert!(
            column_exists(&conn, "media", "geohash").unwrap(),
            "geohash column should exist after migration"
        );
    }

    #[test]
    fn test_migration_fresh_database_has_rtree_table() {
        let conn = create_test_db();

        // Run migrations
        run_migrations(&conn).expect("Migration should succeed");

        // Verify media_rtree table exists
        assert!(
            table_exists(&conn, "media_rtree").unwrap(),
            "media_rtree table should exist after migration"
        );
    }

    #[test]
    fn test_migration_fresh_database_has_schema_version() {
        let conn = create_test_db();

        // Run migrations
        run_migrations(&conn).expect("Migration should succeed");

        // Verify schema_version table exists and has version 1
        assert!(
            table_exists(&conn, "schema_version").unwrap(),
            "schema_version table should exist"
        );

        let version = get_schema_version(&conn).unwrap();
        assert_eq!(version, 1, "Schema version should be 1 after migration");
    }

    #[test]
    fn test_migration_upgrades_existing_database() {
        let conn = create_old_db_without_geohash();

        // Verify geohash column does NOT exist before migration
        assert!(
            !column_exists(&conn, "media", "geohash").unwrap(),
            "geohash column should NOT exist before migration"
        );

        // Run migrations
        run_migrations(&conn).expect("Migration should succeed on existing database");

        // Verify geohash column now exists
        assert!(
            column_exists(&conn, "media", "geohash").unwrap(),
            "geohash column should exist after migration"
        );

        // Verify R-tree table exists
        assert!(
            table_exists(&conn, "media_rtree").unwrap(),
            "media_rtree table should exist after migration"
        );
    }

    #[test]
    fn test_migration_is_idempotent() {
        let conn = create_test_db();

        // Run migrations twice
        run_migrations(&conn).expect("First migration should succeed");
        run_migrations(&conn).expect("Second migration should succeed (idempotent)");

        // Verify schema version is still 1 (not 2)
        let version = get_schema_version(&conn).unwrap();
        assert_eq!(
            version, 1,
            "Schema version should remain 1 after idempotent run"
        );
    }

    #[test]
    fn test_rtree_accepts_insert_and_select() {
        let conn = create_test_db();
        run_migrations(&conn).expect("Migration should succeed");

        // Insert into R-tree
        conn.execute(
            "INSERT INTO media_rtree (media_id, min_lat, max_lat, min_lon, max_lon) VALUES (?, ?, ?, ?, ?)",
            rusqlite::params![1, 40.7, 40.8, -74.1, -74.0],
        )
        .expect("R-tree INSERT should succeed");

        // Query R-tree with bounding box
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM media_rtree WHERE min_lat >= ? AND max_lat <= ? AND min_lon >= ? AND max_lon <= ?",
                rusqlite::params![40.0, 41.0, -75.0, -73.0],
                |row| row.get(0),
            )
            .expect("R-tree SELECT should succeed");

        assert_eq!(count, 1, "R-tree should contain 1 entry");
    }

    #[test]
    fn test_geohash_column_is_nullable() {
        let conn = create_test_db();
        run_migrations(&conn).expect("Migration should succeed");

        // Insert media without geohash (should succeed because column is nullable)
        conn.execute(
            "INSERT INTO media (filename, original_filename, file_path, media_type, content_hash) VALUES (?, ?, ?, ?, ?)",
            rusqlite::params!["test.jpg", "test.jpg", "/path/test.jpg", "image", "hash123"],
        )
        .expect("INSERT without geohash should succeed");

        // Verify geohash is NULL
        let geohash: Option<String> = conn
            .query_row(
                "SELECT geohash FROM media WHERE filename = ?",
                ["test.jpg"],
                |row| row.get(0),
            )
            .expect("SELECT should succeed");

        assert!(geohash.is_none(), "geohash should be NULL for new media");
    }

    #[test]
    fn test_geohash_index_exists() {
        let conn = create_test_db();
        run_migrations(&conn).expect("Migration should succeed");

        // Check if index exists
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_media_geohash'",
                [],
                |row| row.get(0),
            )
            .expect("Index query should succeed");

        assert_eq!(count, 1, "idx_media_geohash index should exist");
    }

    #[test]
    fn test_existing_gps_index_preserved() {
        let conn = create_test_db();
        run_migrations(&conn).expect("Migration should succeed");

        // Check that original idx_media_gps index still exists
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_media_gps'",
                [],
                |row| row.get(0),
            )
            .expect("Index query should succeed");

        assert_eq!(count, 1, "idx_media_gps index should be preserved");
    }
}
