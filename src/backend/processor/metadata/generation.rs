use std::path::PathBuf;

use sha2::Digest;

use crate::config::Config;
use crate::constants::paths;
use crate::database::{queries, DbPool};
use crate::processor::media_processor::{calculate_geohash, generate_complete_metadata};
use crate::processor::metadata::delete_supplemental_metadata;
use crate::processor::thumbnails::{generate_image_thumbnail, generate_video_thumbnail};
use crate::utils::hash::calculate_file_hash;

pub async fn generate_media_metadata(
    pool: &DbPool,
    media_id: i64,
    config: &Config,
) -> Result<(), String> {
    let (file_path, media_type): (String, String) = {
        let connection = pool.get().map_err(|error| error.to_string())?;
        connection
            .query_row(
                queries::metadata::SELECT_IMPORTED_MEDIA,
                [media_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|error| error.to_string())?
    };
    let original_path = paths().originals.join(&file_path);
    if !original_path.is_file() {
        return Err(format!(
            "original file is missing: {}",
            original_path.display()
        ));
    }
    let content_hash = calculate_file_hash(&original_path)
        .await
        .map_err(|error| error.to_string())?;
    let metadata =
        generate_complete_metadata(&original_path, &media_type, Some(&config.reverse_geocoding))
            .await;
    let thumbnail_relative = PathBuf::from(media_id.to_string()).join("thumbnail.jpg");
    let thumbnail_path = paths().thumbnails.join(&thumbnail_relative);
    let tiny_thumbnail_path = paths().thumbnails_tiny.join(&thumbnail_relative);
    let thumbnail_parent = thumbnail_path
        .parent()
        .ok_or_else(|| "thumbnail path has no parent".to_string())?;
    let tiny_thumbnail_parent = tiny_thumbnail_path
        .parent()
        .ok_or_else(|| "tiny thumbnail path has no parent".to_string())?;
    std::fs::create_dir_all(thumbnail_parent).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(tiny_thumbnail_parent).map_err(|error| error.to_string())?;
    let thumbnail_generated = generate_thumbnail(
        &media_type,
        &original_path,
        &thumbnail_path,
        config.thumbnails.max_size,
        config,
    )
    .await;
    if !thumbnail_generated {
        return Err("thumbnail generation failed".to_string());
    }
    let tiny_thumbnail_generated = generate_thumbnail(
        &media_type,
        &original_path,
        &tiny_thumbnail_path,
        config.thumbnails.tiny_size,
        config,
    )
    .await;
    if !tiny_thumbnail_generated {
        return Err("tiny thumbnail generation failed".to_string());
    }
    let geohash = metadata
        .gps_latitude
        .zip(metadata.gps_longitude)
        .and_then(|(latitude, longitude)| calculate_geohash(latitude, longitude));
    let connection = pool.get().map_err(|error| error.to_string())?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            queries::metadata::UPDATE_METADATA,
            rusqlite::params![
                media_id,
                metadata.width,
                metadata.height,
                metadata.date_taken.map(|date| date.to_rfc3339()),
                metadata.gps_latitude,
                metadata.gps_longitude,
                metadata.gps_altitude,
                metadata.camera_make,
                metadata.camera_model,
                metadata.lens_make,
                metadata.lens_model,
                metadata.iso,
                metadata.exposure_time,
                metadata.f_number,
                metadata.focal_length,
                metadata.focal_length_35mm,
                metadata.location_city,
                metadata.location_state,
                metadata.location_country,
                metadata.video_codec,
                metadata.keywords,
                metadata.duration_seconds
            ],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            queries::metadata::UPDATE_THUMBNAIL,
            rusqlite::params![thumbnail_relative.to_string_lossy(), media_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            queries::media::UPDATE_CONTENT_HASH,
            rusqlite::params![content_hash, media_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(queries::metadata::DELETE_RTREE_FOR_MEDIA, [media_id])
        .map_err(|error| error.to_string())?;
    if let (Some(latitude), Some(longitude)) = (metadata.gps_latitude, metadata.gps_longitude) {
        transaction
            .execute(
                queries::metadata::INSERT_RTREE,
                rusqlite::params![media_id, latitude, latitude, longitude, longitude],
            )
            .map_err(|error| error.to_string())?;
    }
    transaction
        .execute(
            queries::metadata::UPSERT_GEOHASH,
            rusqlite::params![media_id, geohash],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    delete_supplemental_metadata(&original_path).map_err(|error| error.to_string())?;
    prepare_ai_inputs(
        pool,
        media_id,
        &original_path,
        &thumbnail_path,
        &media_type,
        metadata.duration_seconds,
    )
}

async fn generate_thumbnail(
    media_type: &str,
    original_path: &std::path::Path,
    output_path: &std::path::Path,
    maximum_size: u32,
    config: &Config,
) -> bool {
    if media_type == "image" {
        return generate_image_thumbnail(
            original_path,
            output_path,
            maximum_size,
            config.thumbnails.quality,
        )
        .await;
    }
    generate_video_thumbnail(
        original_path,
        output_path,
        maximum_size,
        config.thumbnails.quality,
        config.thumbnails.video_frame_quality,
    )
    .await
}

fn prepare_ai_inputs(
    pool: &DbPool,
    media_id: i64,
    original_path: &std::path::Path,
    thumbnail_path: &std::path::Path,
    media_type: &str,
    _duration_seconds: Option<f64>,
) -> Result<(), String> {
    let output_directory = paths().previews.join("ai").join(media_id.to_string());
    let frames = if media_type == "video" {
        extract_first_video_frame(original_path, &output_directory)?
    } else {
        vec![(0, None, thumbnail_path.to_path_buf())]
    };
    for task in ["ocr", "image_tagging", "image_clustering"] {
        let task_directory = output_directory.join(task);
        std::fs::create_dir_all(&task_directory).map_err(|error| error.to_string())?;
        pool.get()
            .map_err(|error| error.to_string())?
            .execute(
                queries::metadata::DELETE_AI_INPUTS_FOR_TASK,
                rusqlite::params![media_id, task],
            )
            .map_err(|error| error.to_string())?;
        for (sequence, frame_timestamp_ms, source_path) in &frames {
            let filename = format!("{sequence:03}.jpg");
            let output_path = task_directory.join(&filename);
            std::fs::copy(source_path, &output_path).map_err(|error| error.to_string())?;
            let source_bytes = std::fs::read(&output_path).map_err(|error| error.to_string())?;
            let content_hash = format!("{:x}", sha2::Sha256::digest(&source_bytes));
            let relative_path = PathBuf::from("ai")
                .join(media_id.to_string())
                .join(task)
                .join(&filename);
            let input_kind = if media_type == "video" {
                "video_frame"
            } else {
                "image"
            };
            pool.get()
                .map_err(|error| error.to_string())?
                .execute(
                    queries::metadata::INSERT_AI_INPUT,
                    rusqlite::params![
                        media_id,
                        task,
                        sequence,
                        input_kind,
                        relative_path.to_string_lossy(),
                        filename,
                        source_bytes.len() as i64,
                        content_hash,
                        frame_timestamp_ms
                    ],
                )
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn extract_first_video_frame(
    original_path: &std::path::Path,
    output_directory: &std::path::Path,
) -> Result<Vec<(i64, Option<i64>, PathBuf)>, String> {
    let frames_directory = output_directory.join("frames");
    std::fs::create_dir_all(&frames_directory).map_err(|error| error.to_string())?;
    let output_path = frames_directory.join("000.jpg");
    let output = std::process::Command::new("ffmpeg")
        .args(["-y", "-ss", "0", "-i"])
        .arg(original_path)
        .args([
            "-frames:v",
            "1",
            "-vf",
            "scale='min(1920,iw)':-2",
            "-q:v",
            "2",
        ])
        .arg(&output_path)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() || !output_path.is_file() {
        return Err(format!(
            "FFmpeg frame extraction failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(vec![(0, Some(0), output_path)])
}
