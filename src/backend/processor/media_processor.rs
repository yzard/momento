use chrono::{DateTime, Utc};
use filetime::{set_file_times, FileTime};
use geohash::{encode, Coord};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use uuid::Uuid;

use crate::config::{LlmConfig, ReverseGeocodingConfig, ThumbnailConfig};
use crate::constants::{paths, IMAGE_EXTENSIONS, VIDEO_EXTENSIONS};
use crate::database::{execute_query, fetch_one, queries, DbConn, DbPool};
use crate::processor::metadata::{
    apply_supplemental_metadata, extract_image_metadata, extract_video_metadata,
    load_supplemental_metadata, normalize_gps_coordinates, MediaMetadata,
};
use crate::processor::thumbnails::{generate_image_thumbnail, generate_video_thumbnail};
use crate::utils::hash::calculate_file_hash;

#[derive(Clone, Copy)]
pub struct SourceFileTimes {
    pub accessed: FileTime,
    pub modified: FileTime,
}

pub fn capture_file_times(source_path: &Path) -> std::io::Result<SourceFileTimes> {
    let metadata = fs::metadata(source_path)?;
    Ok(SourceFileTimes {
        accessed: FileTime::from_last_access_time(&metadata),
        modified: FileTime::from_last_modification_time(&metadata),
    })
}

pub fn apply_file_times(
    destination_path: &Path,
    file_times: SourceFileTimes,
) -> std::io::Result<()> {
    set_file_times(destination_path, file_times.accessed, file_times.modified)
}

#[derive(Clone)]
pub struct MediaProcessingContext {
    pub user_id: i64,
    pub thumbnails: ThumbnailConfig,
    pub reverse_geocoding: Option<ReverseGeocodingConfig>,
    pub llm: LlmConfig,
    pub pool: DbPool,
}

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

fn save_original_file(source_path: &Path) -> std::io::Result<(PathBuf, PathBuf, String)> {
    let ext = source_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin");
    let temporary_filename = format!("pending_{}.{}", Uuid::new_v4(), ext);

    let relative_path = PathBuf::from(&temporary_filename);
    let dest_path = paths().originals.join(&relative_path);

    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::copy(source_path, &dest_path)?;

    Ok((dest_path, relative_path, temporary_filename))
}

fn finalize_original_file(
    temporary_path: &Path,
    media_id: i64,
    source_path: &Path,
    file_times: SourceFileTimes,
) -> std::io::Result<(PathBuf, PathBuf, String)> {
    let new_filename = build_original_filename(media_id, source_path);
    let relative_path = PathBuf::from(&new_filename);
    let destination = paths().originals.join(&relative_path);
    fs::rename(temporary_path, &destination)?;
    apply_file_times(&destination, file_times)?;
    Ok((destination, relative_path, new_filename))
}

pub fn build_original_filename(media_id: i64, source_path: &Path) -> String {
    let original_stem = source_path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown");
    let extension = source_path
        .extension()
        .and_then(|extension| extension.to_str());
    match extension {
        Some(extension) => format!("{}_{}.{}", media_id, original_stem, extension),
        None => format!("{}_{}", media_id, original_stem),
    }
}

fn grant_existing_media_access(conn: &DbConn, media_id: i64, user_id: i64) {
    let has_access: Option<i32> = fetch_one(
        conn,
        queries::access::CHECK_MEDIA_ACCESS,
        &[&media_id, &user_id],
        |row| row.get(0),
    )
    .ok()
    .flatten();

    if has_access.is_some() {
        let _ = execute_query(
            conn,
            queries::access::RESTORE_MEDIA_ACCESS,
            &[&media_id, &user_id],
        );
        return;
    }

    let _ = execute_query(
        conn,
        queries::access::INSERT_MEDIA_ACCESS,
        &[&media_id, &user_id, &2],
    );
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

    let thumbnail_path = paths().thumbnails.join(&thumbnail_relative);
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

    let tiny_thumbnail_path = paths().thumbnails_tiny.join(&thumbnail_relative);
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

    if let Some(supplemental_metadata) = load_supplemental_metadata(source_path) {
        apply_supplemental_metadata(&mut metadata, &supplemental_metadata);
    }
    normalize_gps_coordinates(&mut metadata);

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
            && (metadata.location_state.is_none() || metadata.location_country.is_none())
        {
            let Some((latitude, longitude)) = metadata.gps_latitude.zip(metadata.gps_longitude)
            else {
                return metadata;
            };
            let (city, state, country) = reverse_geocode(geo_config, latitude, longitude).await;
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
    context: &MediaProcessingContext,
) -> Option<i64> {
    let start_time = Instant::now();
    let user_id = context.user_id;
    tracing::info!(
        "Media processing started for {} (user_id={})",
        source_path.display(),
        user_id
    );
    let media_type = get_media_type(source_path)?;
    let source_file_times = match capture_file_times(source_path) {
        Ok(file_times) => file_times,
        Err(error) => {
            tracing::error!(
                "Media processing failed for {} after {:?}: failed to read source file times: {}",
                source_path.display(),
                start_time.elapsed(),
                error
            );
            return None;
        }
    };

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

    if let Ok(conn) = context.pool.get() {
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

            grant_existing_media_access(&conn, media_id, user_id);

            tracing::info!("Granted access to media {} for user {}", media_id, user_id);
            tracing::info!(
                "Media processing completed for {} in {:?}",
                source_path.display(),
                start_time.elapsed()
            );
            return Some(media_id);
        }
    }

    let metadata =
        generate_complete_metadata(source_path, media_type, context.reverse_geocoding.as_ref())
            .await;
    let (temporary_path, temporary_relative_path, temporary_filename) =
        match save_original_file(source_path) {
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

    let original_filename = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown");
    let conn = match context.pool.get() {
        Ok(c) => c,
        Err(e) => {
            let _ = fs::remove_file(&temporary_path);
            tracing::error!(
                "Media processing failed for {} after {:?}: failed to get DB connection: {}",
                source_path.display(),
                start_time.elapsed(),
                e
            );
            return None;
        }
    };

    let file_size = temporary_path.metadata().ok().map(|m| m.len() as i64);
    let geohash = match (metadata.gps_latitude, metadata.gps_longitude) {
        (Some(lat), Some(lon)) => calculate_geohash(lat, lon),
        _ => None,
    };

    let insert_result = conn.execute(
        queries::media::INSERT,
        rusqlite::params![
            user_id,
            temporary_filename,
            original_filename,
            temporary_relative_path.to_string_lossy().to_string(),
            media_type,
            &metadata.mime_type,
            &file_size,
            &content_hash,
        ],
    );

    let media_id = match insert_result {
        Ok(1) => conn.last_insert_rowid(),
        Ok(0) => {
            let existing_media_id: Option<i64> = fetch_one(
                &conn,
                queries::media::SELECT_BY_CONTENT_HASH,
                &[&content_hash],
                |row| row.get(0),
            )
            .ok()
            .flatten();

            let Some(media_id) = existing_media_id else {
                let _ = fs::remove_file(&temporary_path);
                tracing::error!(
                    "Media processing failed for {} after {:?}: duplicate content hash was not found after conflict",
                    source_path.display(),
                    start_time.elapsed()
                );
                return None;
            };

            let _ = fs::remove_file(&temporary_path);
            grant_existing_media_access(&conn, media_id, user_id);
            tracing::info!(
                "Reused media {} after concurrent content hash conflict for {}",
                media_id,
                source_path.display()
            );
            return Some(media_id);
        }
        Ok(rows) => {
            let _ = fs::remove_file(&temporary_path);
            tracing::error!(
                "Media processing failed for {} after {:?}: media insert affected {} rows",
                source_path.display(),
                start_time.elapsed(),
                rows
            );
            return None;
        }
        Err(e) => {
            let _ = fs::remove_file(&temporary_path);
            tracing::error!(
                "Media processing failed for {} after {:?}: failed to insert media into DB: {}",
                source_path.display(),
                start_time.elapsed(),
                e
            );
            return None;
        }
    };

    let (dest_path, relative_path, new_filename) =
        match finalize_original_file(&temporary_path, media_id, source_path, source_file_times) {
            Ok(result) => result,
            Err(error) => {
                let _ = execute_query(&conn, queries::trash::DELETE_PERMANENTLY, &[&media_id]);
                let _ = fs::remove_file(&temporary_path);
                tracing::error!(
                "Media processing failed for {} after {:?}: failed to finalize original file: {}",
                source_path.display(),
                start_time.elapsed(),
                error
            );
                return None;
            }
        };

    if execute_query(
        &conn,
        queries::media::UPDATE_FILE_LOCATION,
        &[
            &new_filename,
            &relative_path.to_string_lossy().to_string(),
            &media_id,
        ],
    )
    .is_err()
    {
        let _ = execute_query(&conn, queries::trash::DELETE_PERMANENTLY, &[&media_id]);
        let _ = fs::remove_file(&dest_path);
        tracing::error!(
            "Media processing failed for {} after {:?}: failed to update original file location",
            source_path.display(),
            start_time.elapsed()
        );
        return None;
    }

    drop(conn);

    let (thumbnail_relative, _tiny_thumbnail_relative) = generate_thumbnails(
        &dest_path,
        media_type,
        context.thumbnails.max_size,
        context.thumbnails.tiny_size,
        context.thumbnails.quality,
        context.thumbnails.video_frame_quality,
    )
    .await;

    let conn = match context.pool.get() {
        Ok(conn) => conn,
        Err(error) => {
            tracing::error!(
                "Media processing failed for {} after {:?}: failed to reacquire DB connection: {}",
                source_path.display(),
                start_time.elapsed(),
                error
            );
            return None;
        }
    };

    let _ = execute_query(
        &conn,
        queries::media::INSERT_METADATA,
        &[
            &media_id,
            &thumbnail_relative,
            &metadata.width,
            &metadata.height,
            &metadata.duration_seconds,
            &metadata.date_taken.map(|dt| dt.to_rfc3339()),
            &metadata.gps_latitude,
            &metadata.gps_longitude,
            &metadata.gps_altitude,
            &geohash,
            &metadata.location_city,
            &metadata.location_state,
            &metadata.location_country,
            &metadata.camera_make,
            &metadata.camera_model,
            &metadata.lens_make,
            &metadata.lens_model,
            &metadata.iso,
            &metadata.exposure_time,
            &metadata.f_number,
            &metadata.focal_length,
            &metadata.focal_length_35mm,
            &metadata.video_codec,
            &metadata.keywords,
        ],
    );

    let _ = execute_query(
        &conn,
        queries::access::INSERT_MEDIA_ACCESS,
        &[&media_id, &user_id, &2],
    );

    if let (Some(lat), Some(lon)) = (metadata.gps_latitude, metadata.gps_longitude) {
        if let Err(e) = insert_into_rtree(&conn, media_id, lat, lon) {
            tracing::warn!("Failed to insert media {} into R-tree: {}", media_id, e);
        }
    }

    tracing::info!(
        "Media processing completed for {} in {:?}",
        source_path.display(),
        start_time.elapsed()
    );
    Some(media_id)
}

pub fn delete_media_files(media_id: i64, file_path: &str, thumbnail_path: Option<&str>) {
    let raw_file = paths().originals.join(file_path);
    if raw_file.exists() {
        let _ = fs::remove_file(&raw_file);
    }

    if let Some(thumb_path) = thumbnail_path {
        let thumb_file = paths().thumbnails.join(thumb_path);
        if thumb_file.exists() {
            let _ = fs::remove_file(&thumb_file);
        }
        let tiny_thumbnail_file = paths().thumbnails_tiny.join(thumb_path);
        if tiny_thumbnail_file.exists() {
            let _ = fs::remove_file(&tiny_thumbnail_file);
        }
    }

    let original_stem = Path::new(file_path)
        .file_stem()
        .and_then(|stem| stem.to_str());
    if let Some(original_stem) = original_stem {
        let escaped_stem = glob::Pattern::escape(original_stem);
        let preview_pattern = paths()
            .previews
            .join("*")
            .join(format!("{escaped_stem}_preview.jpg"));
        if let Some(preview_pattern) = preview_pattern.to_str() {
            if let Ok(preview_paths) = glob::glob(preview_pattern) {
                for preview_path in preview_paths.flatten() {
                    let _ = fs::remove_file(preview_path);
                }
            }
        }
    }

    let clustering_frame = paths()
        .previews
        .join("deduplicate")
        .join(format!("{media_id}.jpg"));
    if clustering_frame.exists() {
        let _ = fs::remove_file(clustering_frame);
    }
}

pub fn calculate_geohash(lat: f64, lon: f64) -> Option<String> {
    let coord = Coord { x: lon, y: lat };
    encode(coord, 7).ok()
}

pub fn insert_into_rtree(
    conn: &DbConn,
    media_id: i64,
    lat: f64,
    lon: f64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        queries::media::INSERT_RTREE,
        rusqlite::params![media_id, lat, lat, lon, lon],
    )?;
    Ok(())
}

pub fn delete_from_rtree(conn: &DbConn, media_id: i64) -> Result<(), rusqlite::Error> {
    conn.execute(queries::media::DELETE_RTREE, rusqlite::params![media_id])?;
    Ok(())
}
