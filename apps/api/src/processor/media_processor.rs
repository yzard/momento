use chrono::{DateTime, Utc};
use geohash::{encode, Coord};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use uuid::Uuid;

use crate::config::ReverseGeocodingConfig;
use crate::constants::{
    IMAGE_EXTENSIONS, ORIGINALS_DIR, THUMBNAILS_DIR, THUMBNAILS_TINY_DIR, VIDEO_EXTENSIONS,
};
use crate::database::{execute_query, fetch_one, insert_returning_id, queries, DbConn, DbPool};
use crate::processor::metadata::{extract_image_metadata, extract_video_metadata, MediaMetadata};
use crate::processor::thumbnails::{generate_image_thumbnail, generate_video_thumbnail};
use crate::utils::hash::calculate_file_hash;

pub fn get_media_type(file_path: &Path) -> Option<&'static str> {
    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_lowercase()))?;

    if IMAGE_EXTENSIONS.contains(ext.as_str()) {
        Some("image")
    } else if VIDEO_EXTENSIONS.contains(ext.as_str()) {
        Some("video")
    } else {
        None
    }
}

fn get_media_date(metadata: &MediaMetadata, source_path: &Path) -> DateTime<Utc> {
    if let Some(dt) = metadata.date_taken {
        return dt;
    }

    source_path
        .metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .map(DateTime::<Utc>::from)
        .unwrap_or_else(Utc::now)
}

fn save_original_file(
    source_path: &Path,
    date_taken: DateTime<Utc>,
) -> std::io::Result<(PathBuf, PathBuf, String)> {
    let year_month = date_taken.format("%Y-%m").to_string();
    let unique_id = &Uuid::new_v4().to_string()[..12];
    let ext = source_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("jpg")
        .to_lowercase();
    let new_filename = format!(
        "{}_{}.{}",
        date_taken.format("%Y%m%d_%H%M%S"),
        unique_id,
        ext
    );

    let relative_path = PathBuf::from(&year_month).join(&new_filename);
    let dest_path = ORIGINALS_DIR.join(&relative_path);

    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::copy(source_path, &dest_path)?;

    Ok((dest_path, relative_path, new_filename))
}

pub async fn generate_thumbnails(
    dest_path: &Path,
    media_type: &str,
    thumbnail_max_size: u32,
    tiny_thumbnail_size: u32,
    thumbnail_quality: u8,
    video_frame_quality: u8,
) -> (Option<String>, Option<String>) {
    let thumbnail_filename = format!(
        "{}.jpg",
        dest_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("thumb")
    );

    let parent_name = dest_path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let thumbnail_relative = PathBuf::from(parent_name).join(&thumbnail_filename);

    let thumbnail_path = THUMBNAILS_DIR.join(&thumbnail_relative);
    if let Some(parent) = thumbnail_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let normal_success = if media_type == "image" {
        generate_image_thumbnail(
            dest_path,
            &thumbnail_path,
            thumbnail_max_size,
            thumbnail_quality,
        )
        .await
    } else {
        generate_video_thumbnail(
            dest_path,
            &thumbnail_path,
            thumbnail_max_size,
            thumbnail_quality,
            video_frame_quality,
        )
        .await
    };

    let tiny_thumbnail_path = THUMBNAILS_TINY_DIR.join(&thumbnail_relative);
    if let Some(parent) = tiny_thumbnail_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let tiny_success = if media_type == "image" {
        generate_image_thumbnail(
            dest_path,
            &tiny_thumbnail_path,
            tiny_thumbnail_size,
            thumbnail_quality,
        )
        .await
    } else {
        generate_video_thumbnail(
            dest_path,
            &tiny_thumbnail_path,
            tiny_thumbnail_size,
            thumbnail_quality,
            video_frame_quality,
        )
        .await
    };

    let normal_relative = if normal_success {
        Some(thumbnail_relative.to_string_lossy().to_string())
    } else {
        None
    };

    let tiny_relative = if tiny_success {
        Some(thumbnail_relative.to_string_lossy().to_string())
    } else {
        None
    };

    (normal_relative, tiny_relative)
}

pub async fn reverse_geocode(
    config: &ReverseGeocodingConfig,
    latitude: f64,
    longitude: f64,
) -> (Option<String>, Option<String>, Option<String>) {
    if !config.enabled {
        return (None, None, None);
    }

    let url = format!(
        "{}?format=json&lat={}&lon={}&zoom=10&addressdetails=1",
        config.base_url, latitude, longitude
    );

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(config.timeout_seconds))
        .user_agent(&config.user_agent)
        .build()
    {
        Ok(c) => c,
        Err(_) => return (None, None, None),
    };

    let response = match client.get(&url).send().await {
        Ok(r) => r,
        Err(_) => return (None, None, None),
    };

    let json: serde_json::Value = match response.json().await {
        Ok(j) => j,
        Err(_) => return (None, None, None),
    };

    let address = json.get("address");
    if address.is_none() {
        return (None, None, None);
    }

    let address = address.unwrap();
    let city = address
        .get("city")
        .or_else(|| address.get("town"))
        .or_else(|| address.get("village"))
        .or_else(|| address.get("hamlet"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let state = address
        .get("state")
        .or_else(|| address.get("region"))
        .or_else(|| address.get("province"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let country = address
        .get("country")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    (city, state, country)
}

pub async fn generate_complete_metadata(
    source_path: &Path,
    media_type: &str,
    reverse_geo_config: Option<&ReverseGeocodingConfig>,
) -> MediaMetadata {
    let mut metadata = if media_type == "image" {
        extract_image_metadata(source_path).await
    } else {
        extract_video_metadata(source_path).await
    };

    if metadata.date_taken.is_none() {
        metadata.date_taken = source_path
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .map(DateTime::<Utc>::from);
        if metadata.date_taken.is_none() {
            metadata.date_taken = Some(Utc::now());
        }
    }

    if let Some(geo_config) = reverse_geo_config {
        if geo_config.enabled
            && metadata.gps_latitude.is_some()
            && metadata.gps_longitude.is_some()
            && (metadata.location_state.is_none() || metadata.location_country.is_none())
        {
            let (city, state, country) = reverse_geocode(
                geo_config,
                metadata.gps_latitude.unwrap(),
                metadata.gps_longitude.unwrap(),
            )
            .await;
            if city.is_some() {
                metadata.location_city = city;
            }
            if state.is_some() {
                metadata.location_state = state;
            }
            if country.is_some() {
                metadata.location_country = country;
            }

            tokio::time::sleep(std::time::Duration::from_secs_f64(
                geo_config.rate_limit_seconds,
            ))
            .await;
        }
    }

    metadata
}

pub async fn process_media_file(
    source_path: &Path,
    user_id: i64,
    thumbnail_max_size: u32,
    tiny_thumbnail_size: u32,
    thumbnail_quality: u8,
    video_frame_quality: u8,
    reverse_geo_config: Option<&crate::config::ReverseGeocodingConfig>,
    pool: &DbPool,
) -> Option<i64> {
    let start_time = Instant::now();
    tracing::info!(
        "Media processing started for {} (user_id={})",
        source_path.display(),
        user_id
    );
    let media_type = get_media_type(source_path)?;

    let content_hash = match calculate_file_hash(source_path).await {
        Ok(h) => h,
        Err(e) => {
            tracing::error!(
                "Media processing failed for {} after {:?}: failed to hash file: {}",
                source_path.display(),
                start_time.elapsed(),
                e
            );
            return None;
        }
    };

    if let Ok(conn) = pool.get() {
        let existing_media_id: Option<i64> = fetch_one(
            &conn,
            queries::media::SELECT_BY_CONTENT_HASH,
            &[&content_hash],
            |row| row.get(0),
        )
        .ok()
        .flatten();

        if let Some(media_id) = existing_media_id {
            tracing::info!(
                "Found existing media {} for hash {}",
                media_id,
                content_hash
            );

            let has_access: Option<i32> = fetch_one(
                &conn,
                queries::access::CHECK_MEDIA_ACCESS,
                &[&media_id, &user_id],
                |row| row.get(0),
            )
            .ok()
            .flatten();

            if has_access.is_some() {
                tracing::info!("User {} already has access to media {}", user_id, media_id);

                let _ = execute_query(
                    &conn,
                    queries::access::RESTORE_MEDIA_ACCESS,
                    &[&media_id, &user_id],
                );

                tracing::info!(
                    "Media processing completed for {} in {:?}",
                    source_path.display(),
                    start_time.elapsed()
                );
                return Some(media_id);
            }

            let _ = execute_query(
                &conn,
                queries::access::INSERT_MEDIA_ACCESS,
                &[&media_id, &user_id, &2],
            );

            tracing::info!("Granted access to media {} for user {}", media_id, user_id);
            tracing::info!(
                "Media processing completed for {} in {:?}",
                source_path.display(),
                start_time.elapsed()
            );
            return Some(media_id);
        }
    }

    let metadata = generate_complete_metadata(source_path, media_type, reverse_geo_config).await;
    let date_taken = get_media_date(&metadata, source_path);

    let (dest_path, relative_path, new_filename) = match save_original_file(source_path, date_taken)
    {
        Ok(res) => res,
        Err(e) => {
            tracing::error!(
                "Media processing failed for {} after {:?}: failed to save original file: {}",
                source_path.display(),
                start_time.elapsed(),
                e
            );
            return None;
        }
    };

    let (thumbnail_relative, _tiny_thumbnail_relative) = generate_thumbnails(
        &dest_path,
        media_type,
        thumbnail_max_size,
        tiny_thumbnail_size,
        thumbnail_quality,
        video_frame_quality,
    )
    .await;

    let file_size = dest_path.metadata().ok().map(|m| m.len() as i64);
    let conn = match pool.get() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(
                "Media processing failed for {} after {:?}: failed to get DB connection: {}",
                source_path.display(),
                start_time.elapsed(),
                e
            );
            return None;
        }
    };

    let geohash = match (metadata.gps_latitude, metadata.gps_longitude) {
        (Some(lat), Some(lon)) => calculate_geohash(lat, lon),
        _ => None,
    };

    let media_id_result = insert_returning_id(
        &conn,
        queries::media::INSERT,
        &[
            &user_id,
            &new_filename,
            &source_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown"),
            &relative_path.to_string_lossy().to_string(),
            &thumbnail_relative,
            &media_type,
            &metadata.mime_type,
            &metadata.width,
            &metadata.height,
            &file_size,
            &metadata.duration_seconds,
            &metadata.date_taken.map(|dt| dt.to_rfc3339()),
            &metadata.gps_latitude,
            &metadata.gps_longitude,
            &metadata.camera_make,
            &metadata.camera_model,
            &metadata.lens_make,
            &metadata.lens_model,
            &metadata.iso,
            &metadata.exposure_time,
            &metadata.f_number,
            &metadata.focal_length,
            &metadata.focal_length_35mm,
            &metadata.gps_altitude,
            &metadata.location_city,
            &metadata.location_state,
            &metadata.location_country,
            &metadata.video_codec,
            &metadata.keywords,
            &content_hash,
            &geohash,
        ],
    );

    let media_id = match media_id_result {
        Ok(id) => id,
        Err(e) => {
            tracing::error!(
                "Media processing failed for {} after {:?}: failed to insert media into DB: {}",
                source_path.display(),
                start_time.elapsed(),
                e
            );
            return None;
        }
    };

    let _ = execute_query(
        &conn,
        queries::access::INSERT_MEDIA_ACCESS,
        &[&media_id, &user_id, &2],
    );

    if let (Some(lat), Some(lon)) = (metadata.gps_latitude, metadata.gps_longitude) {
        if let Err(e) = insert_into_rtree(&conn, media_id, lat, lon) {
            tracing::warn!(
                "Failed to insert media {} into R-tree: {}",
                media_id,
                e
            );
        }
    }

    tracing::info!(
        "Media processing completed for {} in {:?}",
        source_path.display(),
        start_time.elapsed()
    );
    Some(media_id)
}

pub fn delete_media_files(file_path: &str, thumbnail_path: Option<&str>) {
    let raw_file = ORIGINALS_DIR.join(file_path);
    if raw_file.exists() {
        let _ = fs::remove_file(&raw_file);
    }

    if let Some(thumb_path) = thumbnail_path {
        let thumb_file = THUMBNAILS_DIR.join(thumb_path);
        if thumb_file.exists() {
            let _ = fs::remove_file(&thumb_file);
        }
    }
}

pub fn calculate_geohash(lat: f64, lon: f64) -> Option<String> {
    let coord = Coord { x: lon, y: lat };
    encode(coord, 7).ok()
}

pub fn insert_into_rtree(conn: &DbConn, media_id: i64, lat: f64, lon: f64) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO media_rtree (media_id, min_lat, max_lat, min_lon, max_lon) VALUES (?, ?, ?, ?, ?)",
        rusqlite::params![media_id, lat, lat, lon, lon],
    )?;
    Ok(())
}

pub fn delete_from_rtree(conn: &DbConn, media_id: i64) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM media_rtree WHERE media_id = ?",
        rusqlite::params![media_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::create_test_db;

    #[test]
    fn test_calculate_geohash_new_york() {
        let geohash = calculate_geohash(40.7128, -74.0060);
        assert!(geohash.is_some());

        let hash = geohash.unwrap();
        assert_eq!(hash.len(), 7);
        assert!(hash.starts_with("dr5r"));
    }

    #[test]
    fn test_calculate_geohash_london() {
        let geohash = calculate_geohash(51.5074, -0.1278);
        assert!(geohash.is_some());

        let hash = geohash.unwrap();
        assert_eq!(hash.len(), 7);
        assert!(hash.starts_with("gcpv"));
    }

    #[test]
    fn test_calculate_geohash_tokyo() {
        let geohash = calculate_geohash(35.6762, 139.6503);
        assert!(geohash.is_some());

        let hash = geohash.unwrap();
        assert_eq!(hash.len(), 7);
        assert!(hash.starts_with("xn7"));
    }

    fn insert_test_media(conn: &DbConn, id: i64, filename: &str) {
        conn.execute(
            "INSERT INTO media (id, filename, original_filename, file_path, media_type, content_hash) VALUES (?, ?, ?, ?, ?, ?)",
            rusqlite::params![id, filename, filename, format!("/path/{}", filename), "image", format!("hash{}", id)],
        ).expect("Failed to insert test media");
    }

    #[test]
    fn test_rtree_insert_and_query() {
        let pool = create_test_db();
        let conn = pool.get().expect("Failed to get connection");

        insert_test_media(&conn, 1, "test.jpg");
        insert_into_rtree(&conn, 1, 40.7128, -74.0060).expect("R-tree insert should succeed");

        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM media_rtree WHERE min_lat >= ? AND max_lat <= ? AND min_lon >= ? AND max_lon <= ?",
                rusqlite::params![40.0, 41.0, -75.0, -73.0],
                |row| row.get(0),
            )
            .expect("R-tree query should succeed");

        assert_eq!(count, 1);
    }

    #[test]
    fn test_rtree_query_excludes_outside_bbox() {
        let pool = create_test_db();
        let conn = pool.get().expect("Failed to get connection");

        insert_test_media(&conn, 1, "test.jpg");
        insert_into_rtree(&conn, 1, 40.7128, -74.0060).expect("R-tree insert should succeed");

        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM media_rtree WHERE min_lat >= ? AND max_lat <= ? AND min_lon >= ? AND max_lon <= ?",
                rusqlite::params![51.0, 52.0, -1.0, 1.0],
                |row| row.get(0),
            )
            .expect("R-tree query should succeed");

        assert_eq!(count, 0);
    }

    #[test]
    fn test_rtree_delete() {
        let pool = create_test_db();
        let conn = pool.get().expect("Failed to get connection");

        insert_test_media(&conn, 1, "test.jpg");
        insert_into_rtree(&conn, 1, 40.7128, -74.0060).expect("R-tree insert should succeed");

        let count_before: i32 = conn
            .query_row("SELECT COUNT(*) FROM media_rtree WHERE media_id = ?", [1], |row| row.get(0))
            .expect("Query should succeed");
        assert_eq!(count_before, 1);

        delete_from_rtree(&conn, 1).expect("R-tree delete should succeed");

        let count_after: i32 = conn
            .query_row("SELECT COUNT(*) FROM media_rtree WHERE media_id = ?", [1], |row| row.get(0))
            .expect("Query should succeed");
        assert_eq!(count_after, 0);
    }

    #[test]
    fn test_rtree_multiple_entries() {
        let pool = create_test_db();
        let conn = pool.get().expect("Failed to get connection");

        for i in 1..=3 {
            insert_test_media(&conn, i, &format!("test{}.jpg", i));
        }

        insert_into_rtree(&conn, 1, 40.7128, -74.0060).expect("NYC insert should succeed");
        insert_into_rtree(&conn, 2, 51.5074, -0.1278).expect("London insert should succeed");
        insert_into_rtree(&conn, 3, 35.6762, 139.6503).expect("Tokyo insert should succeed");

        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM media_rtree WHERE min_lat >= ? AND max_lat <= ? AND min_lon >= ? AND max_lon <= ?",
                rusqlite::params![-90.0, 90.0, -180.0, 180.0],
                |row| row.get(0),
            )
            .expect("R-tree query should succeed");

        assert_eq!(count, 3);

        let nyc_count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM media_rtree WHERE min_lat >= ? AND max_lat <= ? AND min_lon >= ? AND max_lon <= ?",
                rusqlite::params![40.0, 41.0, -75.0, -73.0],
                |row| row.get(0),
            )
            .expect("R-tree query should succeed");

        assert_eq!(nyc_count, 1);
    }
}
