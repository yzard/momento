use chrono::{DateTime, Utc};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::config::ReverseGeocodingConfig;
use crate::constants::{IMAGE_EXTENSIONS, ORIGINALS_DIR, THUMBNAILS_DIR, VIDEO_EXTENSIONS};
use crate::database::{insert_returning_id, DbPool};
use crate::processor::metadata::{extract_image_metadata, extract_video_metadata, MediaMetadata};
use crate::processor::thumbnails::{generate_image_thumbnail, generate_video_thumbnail};

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
    user_id: i64,
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

    let relative_path = PathBuf::from(user_id.to_string())
        .join(&year_month)
        .join(&new_filename);
    let dest_path = ORIGINALS_DIR.join(&relative_path);

    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::copy(source_path, &dest_path)?;

    Ok((dest_path, relative_path, new_filename))
}

fn generate_thumbnails(
    dest_path: &Path,
    user_id: i64,
    media_type: &str,
    thumbnail_max_size: u32,
    thumbnail_quality: u8,
    video_frame_quality: u8,
) -> Option<String> {
    let thumbnail_filename = format!(
        "{}.jpg",
        dest_path.file_stem().and_then(|s| s.to_str()).unwrap_or("thumb")
    );
    let thumbnail_relative = PathBuf::from(user_id.to_string()).join(&thumbnail_filename);
    let thumbnail_path = THUMBNAILS_DIR.join(&thumbnail_relative);

    let success = if media_type == "image" {
        generate_image_thumbnail(dest_path, &thumbnail_path, thumbnail_max_size, thumbnail_quality)
    } else {
        generate_video_thumbnail(
            dest_path,
            &thumbnail_path,
            thumbnail_max_size,
            thumbnail_quality,
            video_frame_quality,
        )
    };

    if success {
        Some(thumbnail_relative.to_string_lossy().to_string())
    } else {
        None
    }
}

pub fn reverse_geocode(
    config: &ReverseGeocodingConfig,
    latitude: f64,
    longitude: f64,
) -> (Option<String>, Option<String>) {
    if !config.enabled {
        return (None, None);
    }

    let url = format!(
        "{}?format=json&lat={}&lon={}&zoom=10&addressdetails=1",
        config.base_url, latitude, longitude
    );

    let client = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(config.timeout_seconds))
        .user_agent(&config.user_agent)
        .build()
    {
        Ok(c) => c,
        Err(_) => return (None, None),
    };

    let response = match client.get(&url).send() {
        Ok(r) => r,
        Err(_) => return (None, None),
    };

    let json: serde_json::Value = match response.json() {
        Ok(j) => j,
        Err(_) => return (None, None),
    };

    let address = json.get("address");
    if address.is_none() {
        return (None, None);
    }

    let address = address.unwrap();
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

    (state, country)
}

pub fn generate_complete_metadata(
    source_path: &Path,
    media_type: &str,
    reverse_geo_config: Option<&ReverseGeocodingConfig>,
) -> MediaMetadata {
    let mut metadata = if media_type == "image" {
        extract_image_metadata(source_path)
    } else {
        extract_video_metadata(source_path)
    };

    // Ensure date_taken is populated
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

    // Perform reverse geocoding if config matches and coordinates exist
    if let Some(geo_config) = reverse_geo_config {
        if geo_config.enabled
            && metadata.gps_latitude.is_some()
            && metadata.gps_longitude.is_some()
            && (metadata.location_state.is_none() || metadata.location_country.is_none())
        {
            let (state, country) = reverse_geocode(
                geo_config,
                metadata.gps_latitude.unwrap(),
                metadata.gps_longitude.unwrap(),
            );
            if state.is_some() {
                metadata.location_state = state;
            }
            if country.is_some() {
                metadata.location_country = country;
            }

            // Respect rate limit
            std::thread::sleep(std::time::Duration::from_secs_f64(geo_config.rate_limit_seconds));
        }
    }

    metadata
}

pub fn process_media_file(
    source_path: &Path,
    user_id: i64,
    thumbnail_max_size: u32,
    thumbnail_quality: u8,
    video_frame_quality: u8,
    reverse_geo_config: Option<&ReverseGeocodingConfig>,
    pool: &DbPool,
) -> Option<i64> {
    let media_type = get_media_type(source_path)?;
    if !source_path.exists() {
        return None;
    }

    let metadata = generate_complete_metadata(source_path, media_type, reverse_geo_config);
    let date_taken = get_media_date(&metadata, source_path);

    let (dest_path, relative_path, new_filename) =
        save_original_file(source_path, date_taken, user_id).ok()?;

    let thumbnail_relative = generate_thumbnails(
        &dest_path,
        user_id,
        media_type,
        thumbnail_max_size,
        thumbnail_quality,
        video_frame_quality,
    );

    let file_size = dest_path.metadata().ok().map(|m| m.len() as i64);

    let conn = pool.get().ok()?;

    let media_id = insert_returning_id(
        &conn,
        r#"
        INSERT INTO media (
            user_id, filename, original_filename, file_path, thumbnail_path,
            media_type, mime_type, width, height, file_size, duration_seconds,
            date_taken, gps_latitude, gps_longitude, camera_make, camera_model,
            iso, exposure_time, f_number, focal_length, gps_altitude,
            location_state, location_country, keywords
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
        &[
            &user_id,
            &new_filename,
            &source_path.file_name().and_then(|s| s.to_str()).unwrap_or("unknown"),
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
            &metadata.iso,
            &metadata.exposure_time,
            &metadata.f_number,
            &metadata.focal_length,
            &metadata.gps_altitude,
            &metadata.location_state,
            &metadata.location_country,
            &metadata.keywords,
        ],
    )
    .ok()?;

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
