use crate::database::DbConn;
use crate::error::AppResult;
use crate::processor::media_processor::{calculate_geohash, insert_into_rtree};

pub fn backfill_geohash_and_rtree(conn: &DbConn) -> AppResult<(i64, i64)> {
    let geohash_count = backfill_geohash(conn)?;
    let rtree_count = backfill_rtree(conn)?;
    Ok((geohash_count, rtree_count))
}

pub fn backfill_geohash(conn: &DbConn) -> AppResult<i64> {
    let media_with_gps: Vec<(i64, f64, f64)> = {
        let mut stmt = conn.prepare(
            "SELECT id, gps_latitude, gps_longitude FROM media 
             WHERE gps_latitude IS NOT NULL 
               AND gps_longitude IS NOT NULL 
               AND geohash IS NULL",
        )?;

        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;

        rows.filter_map(|r| r.ok()).collect()
    };

    let mut updated_count = 0i64;
    for (media_id, lat, lon) in media_with_gps {
        if let Some(geohash) = calculate_geohash(lat, lon) {
            conn.execute(
                "UPDATE media SET geohash = ? WHERE id = ?",
                rusqlite::params![geohash, media_id],
            )?;
            updated_count += 1;
        }
    }

    Ok(updated_count)
}

pub fn backfill_rtree(conn: &DbConn) -> AppResult<i64> {
    let media_with_gps: Vec<(i64, f64, f64)> = {
        let mut stmt = conn.prepare(
            "SELECT m.id, m.gps_latitude, m.gps_longitude FROM media m
             LEFT JOIN media_rtree r ON m.id = r.media_id
             WHERE m.gps_latitude IS NOT NULL 
               AND m.gps_longitude IS NOT NULL 
               AND r.media_id IS NULL",
        )?;

        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;

        rows.filter_map(|r| r.ok()).collect()
    };

    let mut inserted_count = 0i64;
    for (media_id, lat, lon) in media_with_gps {
        if insert_into_rtree(conn, media_id, lat, lon).is_ok() {
            inserted_count += 1;
        }
    }

    Ok(inserted_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::create_test_db;

    fn insert_media_with_gps_no_geohash(conn: &DbConn, id: i64, lat: f64, lon: f64) {
        conn.execute(
            "INSERT INTO media (id, filename, original_filename, file_path, media_type, content_hash, gps_latitude, gps_longitude) 
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![id, format!("test{}.jpg", id), format!("test{}.jpg", id), format!("/path/test{}.jpg", id), "image", format!("hash{}", id), lat, lon],
        ).expect("Failed to insert test media");
    }

    fn insert_media_without_gps(conn: &DbConn, id: i64) {
        conn.execute(
            "INSERT INTO media (id, filename, original_filename, file_path, media_type, content_hash) 
             VALUES (?, ?, ?, ?, ?, ?)",
            rusqlite::params![id, format!("test{}.jpg", id), format!("test{}.jpg", id), format!("/path/test{}.jpg", id), "image", format!("hash{}", id)],
        ).expect("Failed to insert test media");
    }

    #[test]
    fn test_backfill_geohash_updates_media_with_gps() {
        let pool = create_test_db();
        let conn = pool.get().expect("Failed to get connection");

        insert_media_with_gps_no_geohash(&conn, 1, 40.7128, -74.0060);
        insert_media_with_gps_no_geohash(&conn, 2, 51.5074, -0.1278);
        insert_media_without_gps(&conn, 3);

        let updated = backfill_geohash(&conn).expect("Backfill should succeed");
        assert_eq!(updated, 2);

        let geohash1: Option<String> = conn
            .query_row("SELECT geohash FROM media WHERE id = 1", [], |row| {
                row.get(0)
            })
            .expect("Query should succeed");
        assert!(geohash1.is_some());
        assert!(geohash1.unwrap().starts_with("dr5r"));

        let geohash2: Option<String> = conn
            .query_row("SELECT geohash FROM media WHERE id = 2", [], |row| {
                row.get(0)
            })
            .expect("Query should succeed");
        assert!(geohash2.is_some());
        assert!(geohash2.unwrap().starts_with("gcpv"));

        let geohash3: Option<String> = conn
            .query_row("SELECT geohash FROM media WHERE id = 3", [], |row| {
                row.get(0)
            })
            .expect("Query should succeed");
        assert!(geohash3.is_none());
    }

    #[test]
    fn test_backfill_geohash_skips_already_populated() {
        let pool = create_test_db();
        let conn = pool.get().expect("Failed to get connection");

        conn.execute(
            "INSERT INTO media (id, filename, original_filename, file_path, media_type, content_hash, gps_latitude, gps_longitude, geohash) 
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![1, "test1.jpg", "test1.jpg", "/path/test1.jpg", "image", "hash1", 40.7128, -74.0060, "existing"],
        ).expect("Failed to insert test media");

        let updated = backfill_geohash(&conn).expect("Backfill should succeed");
        assert_eq!(updated, 0);

        let geohash: String = conn
            .query_row("SELECT geohash FROM media WHERE id = 1", [], |row| {
                row.get(0)
            })
            .expect("Query should succeed");
        assert_eq!(geohash, "existing");
    }

    #[test]
    fn test_backfill_rtree_inserts_missing_entries() {
        let pool = create_test_db();
        let conn = pool.get().expect("Failed to get connection");

        insert_media_with_gps_no_geohash(&conn, 1, 40.7128, -74.0060);
        insert_media_with_gps_no_geohash(&conn, 2, 51.5074, -0.1278);
        insert_media_without_gps(&conn, 3);

        let inserted = backfill_rtree(&conn).expect("Backfill should succeed");
        assert_eq!(inserted, 2);

        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM media_rtree", [], |row| row.get(0))
            .expect("Query should succeed");
        assert_eq!(count, 2);
    }

    #[test]
    fn test_backfill_rtree_skips_existing_entries() {
        let pool = create_test_db();
        let conn = pool.get().expect("Failed to get connection");

        insert_media_with_gps_no_geohash(&conn, 1, 40.7128, -74.0060);
        insert_into_rtree(&conn, 1, 40.7128, -74.0060).expect("Insert should succeed");

        let inserted = backfill_rtree(&conn).expect("Backfill should succeed");
        assert_eq!(inserted, 0);

        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM media_rtree", [], |row| row.get(0))
            .expect("Query should succeed");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_backfill_geohash_and_rtree_combined() {
        let pool = create_test_db();
        let conn = pool.get().expect("Failed to get connection");

        insert_media_with_gps_no_geohash(&conn, 1, 40.7128, -74.0060);
        insert_media_with_gps_no_geohash(&conn, 2, 51.5074, -0.1278);
        insert_media_with_gps_no_geohash(&conn, 3, 35.6762, 139.6503);
        insert_media_without_gps(&conn, 4);

        let (geohash_count, rtree_count) =
            backfill_geohash_and_rtree(&conn).expect("Backfill should succeed");
        assert_eq!(geohash_count, 3);
        assert_eq!(rtree_count, 3);

        let geohash_populated: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM media WHERE geohash IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .expect("Query should succeed");
        assert_eq!(geohash_populated, 3);

        let rtree_count: i32 = conn
            .query_row("SELECT COUNT(*) FROM media_rtree", [], |row| row.get(0))
            .expect("Query should succeed");
        assert_eq!(rtree_count, 3);
    }

    #[test]
    fn test_rtree_query_returns_correct_media_ids_in_bbox() {
        let pool = create_test_db();
        let conn = pool.get().expect("Failed to get connection");

        insert_media_with_gps_no_geohash(&conn, 1, 40.7128, -74.0060);
        insert_media_with_gps_no_geohash(&conn, 2, 40.7580, -73.9855);
        insert_media_with_gps_no_geohash(&conn, 3, 51.5074, -0.1278);

        backfill_rtree(&conn).expect("Backfill should succeed");

        let nyc_media: Vec<i64> = {
            let mut stmt = conn
                .prepare(
                    "SELECT media_id FROM media_rtree 
                 WHERE min_lat >= ? AND max_lat <= ? AND min_lon >= ? AND max_lon <= ?",
                )
                .expect("Prepare should succeed");

            let rows = stmt
                .query_map(rusqlite::params![40.0, 41.0, -75.0, -73.0], |row| {
                    row.get(0)
                })
                .expect("Query should succeed");

            rows.filter_map(|r| r.ok()).collect()
        };

        assert_eq!(nyc_media.len(), 2);
        assert!(nyc_media.contains(&1));
        assert!(nyc_media.contains(&2));
        assert!(!nyc_media.contains(&3));
    }
}
