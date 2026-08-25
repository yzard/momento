use chrono::{DateTime, Utc};
use filetime::{set_file_times, FileTime};
use geohash::{encode, Coord};
use std::fs;
use std::path::Path;

use crate::constants::paths;
use crate::database::{queries, DbConn};
use crate::processor::metadata::{
    apply_supplemental_metadata, extract_image_metadata, extract_video_metadata,
    load_supplemental_metadata, normalize_gps_coordinates, reverse_geocoding::reverse_geocode,
    supplemental_metadata_path, MediaMetadata,
};
use crate::utils::path::resolve_storage_path;

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

pub async fn generate_complete_metadata(source_path: &Path, media_type: &str) -> MediaMetadata {
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

    if metadata.location_city.is_some()
        && metadata.location_state.is_some()
        && metadata.location_country.is_some()
    {
        return metadata;
    }

    let Some((latitude, longitude)) = metadata.gps_latitude.zip(metadata.gps_longitude) else {
        return metadata;
    };
    if let Ok(Some(location)) = reverse_geocode(latitude, longitude) {
        if metadata.location_city.is_none() {
            metadata.location_city = Some(location.city);
        }
        if metadata.location_state.is_none() {
            metadata.location_state = location.state;
        }
        if metadata.location_country.is_none() {
            metadata.location_country = Some(location.country);
        }
    }

    metadata
}

pub fn delete_media_files(media_id: i64, file_path: &str, thumbnail_path: Option<&str>) {
    let Ok(raw_file) = resolve_storage_path(&paths().originals, file_path) else {
        tracing::warn!(file_path, "refusing to delete an invalid stored media path");
        return;
    };
    if let Some(sidecar_path) = supplemental_metadata_path(&raw_file) {
        let _ = fs::remove_file(sidecar_path);
    }
    if raw_file.exists() {
        let _ = fs::remove_file(&raw_file);
    }

    if let Some(thumb_path) = thumbnail_path {
        for root in [
            &paths().thumbnails,
            &paths().thumbnails_tiny,
            &paths().thumbnails_places,
        ] {
            if let Ok(thumbnail_file) = resolve_storage_path(root, thumb_path) {
                if thumbnail_file.exists() {
                    let _ = fs::remove_file(thumbnail_file);
                }
            }
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

    let _ = fs::remove_dir_all(paths().previews.join("faces").join(media_id.to_string()));
    let _ = fs::remove_dir_all(paths().previews.join("ai").join(media_id.to_string()));
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
