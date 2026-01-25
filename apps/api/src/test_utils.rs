#![cfg(test)]

use crate::app::create_app;
use crate::config::Config;
use crate::database::{init_database, DbPool};
use axum::Router;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

static MEDIA_ID_COUNTER: AtomicI64 = AtomicI64::new(1);
static USER_ID_COUNTER: AtomicI64 = AtomicI64::new(1);

/// Create an in-memory SQLite database pool with full schema applied
pub fn create_test_db() -> DbPool {
    let manager = SqliteConnectionManager::memory().with_init(|conn| {
        conn.execute_batch("PRAGMA foreign_keys = ON")?;
        Ok(())
    });

    let pool = Pool::builder()
        .max_size(5)
        .build(manager)
        .expect("Failed to create test database pool");

    let conn = pool.get().expect("Failed to get connection from pool");
    init_database(&conn).expect("Failed to initialize test database schema");

    pool
}

/// Create a test app with in-memory database
pub fn create_test_app() -> (Router, DbPool) {
    let pool = create_test_db();
    let config = Arc::new(Config::default());
    let app = create_app(config, pool.clone());
    (app, pool)
}

/// Test fixture: Create a user in the test database
pub fn create_test_user(pool: &DbPool, username: &str, email: &str) -> i64 {
    let conn = pool.get().expect("Failed to get connection");
    let user_id = USER_ID_COUNTER.fetch_add(1, Ordering::SeqCst);

    conn.execute(
        "INSERT INTO users (id, username, email, hashed_password, role, must_change_password, is_active) 
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        rusqlite::params![user_id, username, email, "hashed_password_placeholder", "user", 0, 1],
    )
    .expect("Failed to insert test user");

    user_id
}

/// Test fixture: Create media with GPS coordinates
pub fn create_test_media_with_gps(
    pool: &DbPool,
    filename: &str,
    latitude: f64,
    longitude: f64,
) -> i64 {
    create_test_media_with_gps_and_date(pool, filename, latitude, longitude, "2024-01-15T10:30:00")
}

pub fn create_test_media_with_gps_and_date(
    pool: &DbPool,
    filename: &str,
    latitude: f64,
    longitude: f64,
    date_taken: &str,
) -> i64 {
    let conn = pool.get().expect("Failed to get connection");
    let media_id = MEDIA_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
    let file_path = format!("/test/media/{}", filename);
    let content_hash = format!("hash_{}", media_id);

    let geohash = geohash::encode(
        geohash::Coord {
            x: longitude,
            y: latitude,
        },
        9,
    )
    .ok();

    conn.execute(
        "INSERT INTO media (
            id, filename, original_filename, file_path, media_type, mime_type,
            width, height, file_size, date_taken, gps_latitude, gps_longitude,
            content_hash, geohash, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'))",
        rusqlite::params![
            media_id,
            filename,
            filename,
            file_path,
            "image",
            "image/jpeg",
            1920,
            1080,
            1024000,
            date_taken,
            latitude,
            longitude,
            content_hash,
            geohash,
        ],
    )
    .expect("Failed to insert test media");

    media_id
}

pub fn grant_media_access(pool: &DbPool, media_id: i64, user_id: i64) {
    let conn = pool.get().expect("Failed to get connection");
    conn.execute(
        "INSERT OR IGNORE INTO media_access (media_id, user_id, access_level) VALUES (?, ?, 1)",
        rusqlite::params![media_id, user_id],
    )
    .expect("Failed to grant media access");
}

/// Test fixture: Create media without GPS coordinates
pub fn create_test_media(pool: &DbPool, filename: &str) -> i64 {
    let conn = pool.get().expect("Failed to get connection");
    let media_id = MEDIA_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
    let file_path = format!("/test/media/{}", filename);
    let content_hash = format!("hash_{}", media_id);

    conn.execute(
        "INSERT INTO media (
            id, filename, original_filename, file_path, media_type, mime_type,
            width, height, file_size, date_taken, content_hash, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'))",
        rusqlite::params![
            media_id,
            filename,
            filename,
            file_path,
            "image",
            "image/jpeg",
            1920,
            1080,
            1024000,
            "2024-01-15T10:30:00",
            content_hash,
        ],
    )
    .expect("Failed to insert test media");

    media_id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_test_db() {
        let pool = create_test_db();
        let conn = pool.get().expect("Failed to get connection");

        let result: Result<i64, _> = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='users'",
            [],
            |row| row.get(0),
        );

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1);
    }

    #[test]
    fn test_create_test_app() {
        let (_app, _pool) = create_test_app();
    }

    #[test]
    fn test_create_test_user() {
        let pool = create_test_db();
        let user_id = create_test_user(&pool, "testuser", "test@example.com");

        let conn = pool.get().expect("Failed to get connection");
        let result: Result<String, _> = conn.query_row(
            "SELECT username FROM users WHERE id = ?",
            [user_id],
            |row| row.get(0),
        );

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "testuser");
    }

    #[test]
    fn test_create_test_media_with_gps() {
        let pool = create_test_db();
        let media_id = create_test_media_with_gps(&pool, "photo.jpg", 40.7128, -74.0060);

        let conn = pool.get().expect("Failed to get connection");
        let result: Result<(f64, f64), _> = conn.query_row(
            "SELECT gps_latitude, gps_longitude FROM media WHERE id = ?",
            [media_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        );

        assert!(result.is_ok());
        let (lat, lon) = result.unwrap();
        assert_eq!(lat, 40.7128);
        assert_eq!(lon, -74.0060);
    }

    #[test]
    fn test_create_test_media() {
        let pool = create_test_db();
        let media_id = create_test_media(&pool, "photo.jpg");

        let conn = pool.get().expect("Failed to get connection");
        let result: Result<String, _> = conn.query_row(
            "SELECT filename FROM media WHERE id = ?",
            [media_id],
            |row| row.get(0),
        );

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "photo.jpg");
    }

    #[test]
    fn test_sequential_id_generation() {
        let pool = create_test_db();

        let id1 = create_test_media(&pool, "photo1.jpg");
        let id2 = create_test_media(&pool, "photo2.jpg");
        let id3 = create_test_media(&pool, "photo3.jpg");

        assert!(id1 < id2);
        assert!(id2 < id3);
    }

    #[test]
    fn test_multiple_media_with_gps() {
        let pool = create_test_db();

        let id1 = create_test_media_with_gps(&pool, "photo1.jpg", 40.7128, -74.0060);
        let id2 = create_test_media_with_gps(&pool, "photo2.jpg", 51.5074, -0.1278);

        let conn = pool.get().expect("Failed to get connection");

        let (lat1, lon1): (f64, f64) = conn
            .query_row(
                "SELECT gps_latitude, gps_longitude FROM media WHERE id = ?",
                [id1],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("Failed to query first media");

        assert_eq!(lat1, 40.7128);
        assert_eq!(lon1, -74.0060);

        let (lat2, lon2): (f64, f64) = conn
            .query_row(
                "SELECT gps_latitude, gps_longitude FROM media WHERE id = ?",
                [id2],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("Failed to query second media");

        assert_eq!(lat2, 51.5074);
        assert_eq!(lon2, -0.1278);
    }
}
