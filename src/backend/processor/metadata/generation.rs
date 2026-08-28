use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::constants::{
    paths, DOCUMENT_DETECTION_MODEL_TYPE, FACE_DETECTION_MODEL_TYPE, IMAGE_AESTHETICS_MODEL_TYPE,
    IMAGE_TAGGING_MODEL_TYPE, OCR_MODEL_TYPE, SCREENSHOT_DETECTION_MODEL_TYPE,
};
use crate::database::{queries, DbPool};
use crate::processor::ai::input::AiInputStorage;
use crate::processor::media_processor::{calculate_geohash, generate_complete_metadata};
use crate::processor::thumbnails::{
    generate_image_preview, generate_image_thumbnail, generate_video_preview,
    generate_video_thumbnail,
};
use crate::utils::hash::calculate_file_hash;
use crate::utils::path::resolve_existing_storage_path;
use crate::utils::process::{process_limits, ExternalProcess};

pub async fn generate_media_metadata(
    pool: &DbPool,
    media_id: i64,
    config: &Config,
) -> Result<(), String> {
    let (file_path, media_type, stored_content_hash, original_filename, stored_mime_type): (
        String,
        String,
        Option<String>,
        String,
        Option<String>,
    ) = {
        let connection = pool.get().map_err(|error| error.to_string())?;
        connection
            .query_row(
                queries::metadata::SELECT_IMPORTED_MEDIA,
                [media_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .map_err(|error| error.to_string())?
    };
    let original_path = resolve_existing_storage_path(&paths().originals, &file_path)
        .await
        .map_err(|error| error.to_string())?;
    let content_hash = match stored_content_hash.filter(|content_hash| is_sha256(content_hash)) {
        Some(content_hash) => content_hash,
        None => calculate_file_hash(&original_path)
            .await
            .map_err(|error| error.to_string())?,
    };
    let complete_metadata =
        generate_complete_metadata(&original_path, &media_type, &config.media_process).await;
    let metadata = &complete_metadata.metadata;
    let thumbnail_relative = PathBuf::from(media_id.to_string()).join("thumbnail.jpg");
    let thumbnail_path = paths().thumbnails.join(&thumbnail_relative);
    let tiny_thumbnail_path = paths().thumbnails_tiny.join(&thumbnail_relative);
    let place_thumbnail_path = paths().thumbnails_places.join(&thumbnail_relative);
    let thumbnail_parent = thumbnail_path
        .parent()
        .ok_or_else(|| "thumbnail path has no parent".to_string())?;
    let tiny_thumbnail_parent = tiny_thumbnail_path
        .parent()
        .ok_or_else(|| "tiny thumbnail path has no parent".to_string())?;
    let place_thumbnail_parent = place_thumbnail_path
        .parent()
        .ok_or_else(|| "place thumbnail path has no parent".to_string())?;
    std::fs::create_dir_all(thumbnail_parent).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(tiny_thumbnail_parent).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(place_thumbnail_parent).map_err(|error| error.to_string())?;
    let thumbnail_generated = generate_thumbnail(
        &media_type,
        &original_path,
        &thumbnail_path,
        config.metadata.thumbnails_max_size,
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
        config.metadata.thumbnails_tiny_size,
        config,
    )
    .await;
    if !tiny_thumbnail_generated {
        return Err("tiny thumbnail generation failed".to_string());
    }
    let place_thumbnail_generated = if media_type == "image" {
        generate_image_preview(
            &original_path,
            &place_thumbnail_path,
            config.metadata.thumbnails_max_size,
            config.metadata.thumbnails_quality,
            &config.media_process,
        )
        .await
    } else {
        generate_video_preview(
            &original_path,
            &place_thumbnail_path,
            config.metadata.thumbnails_max_size,
            config.metadata.thumbnails_quality,
            config.metadata.thumbnails_video_frame_quality,
            &config.media_process,
        )
        .await
    };
    if !place_thumbnail_generated {
        return Err("place thumbnail generation failed".to_string());
    }
    let geohash = metadata
        .gps_latitude
        .zip(metadata.gps_longitude)
        .and_then(|(latitude, longitude)| calculate_geohash(latitude, longitude));
    {
        let connection = pool.get().map_err(|error| error.to_string())?;
        let transaction = connection
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        transaction
            .execute(queries::metadata::DELETE_SOURCES_FOR_MEDIA, [media_id])
            .map_err(|error| error.to_string())?;
        for source in &complete_metadata.sources {
            let payload_json =
                serde_json::to_string(&source.payload).map_err(|error| error.to_string())?;
            transaction
                .execute(
                    queries::metadata::INSERT_SOURCE,
                    rusqlite::params![media_id, source.source_type.as_str(), 1, payload_json],
                )
                .map_err(|error| error.to_string())?;
        }
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
    }
    prepare_ai_inputs(
        pool,
        media_id,
        &original_path,
        OriginalAiInput {
            relative_path: &file_path,
            filename: &original_filename,
            mime_type: stored_mime_type.as_deref(),
            content_hash: &content_hash,
        },
        &media_type,
        &config.media_process,
    )
    .await?;
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
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
            config.metadata.thumbnails_quality,
            &config.media_process,
        )
        .await;
    }
    generate_video_thumbnail(
        original_path,
        output_path,
        maximum_size,
        config.metadata.thumbnails_quality,
        config.metadata.thumbnails_video_frame_quality,
        &config.media_process,
    )
    .await
}

async fn prepare_ai_inputs(
    pool: &DbPool,
    media_id: i64,
    original_path: &Path,
    original: OriginalAiInput<'_>,
    media_type: &str,
    process_config: &crate::config::MediaProcessConfig,
) -> Result<(), String> {
    let output_directory = paths().previews.join("ai").join(media_id.to_string());
    let input = if media_type == "video" {
        let filename = format!("{}.png", original.content_hash);
        let frame_path = output_directory.join("frames").join(&filename);
        if !frame_path.is_file() {
            extract_first_video_frame(original_path, &frame_path, process_config).await?;
        }
        let byte_size = std::fs::metadata(&frame_path)
            .map_err(|error| error.to_string())?
            .len();
        let content_hash = calculate_file_hash(&frame_path)
            .await
            .map_err(|error| error.to_string())?;
        AiInputDescriptor {
            storage: AiInputStorage::Previews,
            file_path: PathBuf::from("ai")
                .join(media_id.to_string())
                .join("frames")
                .join(&filename)
                .to_string_lossy()
                .into_owned(),
            filename,
            mime_type: "image/png".to_string(),
            byte_size,
            content_hash,
            input_kind: "video_frame",
            frame_timestamp_ms: Some(0),
        }
    } else {
        let mime_type = original
            .mime_type
            .filter(|mime_type| mime_type.starts_with("image/"))
            .ok_or_else(|| "canonical original has no supported image MIME type".to_string())?;
        let byte_size = std::fs::metadata(original_path)
            .map_err(|error| error.to_string())?
            .len();
        if byte_size == 0 {
            return Err("canonical original is empty".to_string());
        }
        AiInputDescriptor {
            storage: AiInputStorage::Originals,
            file_path: original.relative_path.to_string(),
            filename: original.filename.to_string(),
            mime_type: mime_type.to_string(),
            byte_size,
            content_hash: original.content_hash.to_string(),
            input_kind: "image",
            frame_timestamp_ms: None,
        }
    };
    let mut tasks = vec![
        OCR_MODEL_TYPE,
        IMAGE_TAGGING_MODEL_TYPE,
        "image_clustering",
        IMAGE_AESTHETICS_MODEL_TYPE,
        FACE_DETECTION_MODEL_TYPE,
    ];
    if media_type == "image" {
        tasks.extend([
            SCREENSHOT_DETECTION_MODEL_TYPE,
            DOCUMENT_DETECTION_MODEL_TYPE,
        ]);
    }
    let connection = pool.get().map_err(|error| error.to_string())?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    for task in tasks {
        transaction
            .execute(
                queries::metadata::DELETE_AI_INPUTS_FOR_TASK,
                rusqlite::params![media_id, task],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                queries::metadata::INSERT_AI_INPUT,
                rusqlite::params![
                    media_id,
                    task,
                    0,
                    input.input_kind,
                    input.storage.as_str(),
                    input.file_path,
                    input.filename,
                    input.mime_type,
                    i64::try_from(input.byte_size)
                        .map_err(|_| "AI input byte size exceeds SQLite range".to_string())?,
                    input.content_hash,
                    input.frame_timestamp_ms
                ],
            )
            .map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())
}

struct OriginalAiInput<'a> {
    relative_path: &'a str,
    filename: &'a str,
    mime_type: Option<&'a str>,
    content_hash: &'a str,
}

struct AiInputDescriptor {
    storage: AiInputStorage,
    file_path: String,
    filename: String,
    mime_type: String,
    byte_size: u64,
    content_hash: String,
    input_kind: &'static str,
    frame_timestamp_ms: Option<i64>,
}

async fn extract_first_video_frame(
    original_path: &std::path::Path,
    output_path: &std::path::Path,
    process_config: &crate::config::MediaProcessConfig,
) -> Result<(), String> {
    let frames_directory = output_path
        .parent()
        .ok_or_else(|| "video frame path has no parent".to_string())?;
    std::fs::create_dir_all(frames_directory).map_err(|error| error.to_string())?;
    let temporary_path = frames_directory.join(format!(".frame-{}.png", uuid::Uuid::new_v4()));
    let (timeout, termination_grace, maximum_stderr_bytes) = process_limits(process_config);
    let output = ExternalProcess::new(
        "ffmpeg",
        vec![
            OsString::from("-nostdin"),
            OsString::from("-y"),
            OsString::from("-ss"),
            OsString::from("0"),
            OsString::from("-i"),
            original_path.as_os_str().to_os_string(),
            OsString::from("-map"),
            OsString::from("0:v:0"),
            OsString::from("-frames:v"),
            OsString::from("1"),
            OsString::from("-c:v"),
            OsString::from("png"),
            temporary_path.as_os_str().to_os_string(),
        ],
        timeout,
        termination_grace,
        0,
        maximum_stderr_bytes,
    )
    .run()
    .await
    .map_err(|error| error.to_string())?;
    if !output.status.success() || !temporary_path.is_file() {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(format!(
            "FFmpeg frame extraction failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    if let Err(error) = std::fs::rename(&temporary_path, output_path) {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(error.to_string());
    }
    Ok(())
}
