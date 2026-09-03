use std::ffi::OsString;
use std::path::PathBuf;

use crate::config::Config;
use crate::constants::{
    DOCUMENT_DETECTION_MODEL_TYPE, FACE_DETECTION_MODEL_TYPE, IMAGE_AESTHETICS_MODEL_TYPE,
    IMAGE_TAGGING_MODEL_TYPE, OCR_MODEL_TYPE, SCREENSHOT_DETECTION_MODEL_TYPE,
};
use crate::database::operations::{
    MetadataAiInputWrite, MetadataSourceWrite, MetadataValuesWrite, PersistMetadataGeneration,
};
use crate::executor::process::{
    bounded_error_detail, ffmpeg_single_thread_arguments, validate_storage_image_dimensions,
};
use crate::processor::ai::input::AiInputStorage;
use crate::processor::artifact::ArtifactPublicationOwner;
use crate::processor::media_processor::generate_complete_metadata;
use crate::processor::thumbnails::{
    generate_image_preview_prepared, generate_image_thumbnail_prepared,
    generate_video_preview_prepared, generate_video_thumbnail_prepared, maximum_jpeg_output_bytes,
    StorageMediaFile,
};
use crate::runtime::ExecutorHandles;

pub async fn generate_media_metadata(
    executors: &ExecutorHandles,
    media_id: i64,
    claim_token: &str,
    config: &Config,
) -> Result<(), String> {
    let media = executors
        .sqlite
        .load_metadata_generation_media_durable(media_id)
        .await
        .map_err(|error| error.to_string())?;
    let artifact_version = media
        .artifact_version
        .checked_add(1)
        .ok_or_else(|| "metadata artifact version overflowed".to_string())?;
    let previous_thumbnail_path = media.thumbnail_path.clone();
    let previous_preview_path = media.preview_path.clone();
    let file_path = media.file_path;
    let media_type = media.media_type;
    let original_file = StorageMediaFile {
        storage_root: crate::io::file::StorageRootId::Originals,
        path: crate::io::file::NormalizedStoragePath::parse(&file_path)
            .map_err(|error| error.to_string())?,
    };
    if media_type == "image" {
        validate_storage_image_dimensions(
            &executors.cpu,
            &executors.file_io,
            crate::io::file::StorageRootId::Originals,
            crate::io::file::NormalizedStoragePath::parse(&file_path)
                .map_err(|error| error.to_string())?,
            &config.media_process,
        )
        .await
        .map_err(|error| format!("canonical image validation failed: {error}"))?;
    }
    let content_hash = match media
        .content_hash
        .filter(|content_hash| is_sha256(content_hash))
    {
        Some(content_hash) => content_hash,
        None => {
            hash_storage_file(
                executors,
                crate::io::file::StorageRootId::Originals,
                crate::io::file::NormalizedStoragePath::parse(&file_path)
                    .map_err(|error| error.to_string())?,
            )
            .await?
            .1
        }
    };
    let mut complete_metadata = generate_complete_metadata(
        executors,
        crate::io::file::StorageRootId::Originals,
        &crate::io::file::NormalizedStoragePath::parse(&file_path)
            .map_err(|error| error.to_string())?,
        &media_type,
        &config.media_process,
    )
    .await?;
    let derived_location = executors
        .cpu
        .derive_media_location_durable(
            complete_metadata.metadata.gps_latitude,
            complete_metadata.metadata.gps_longitude,
        )
        .await
        .map_err(|error| error.to_string())?;
    if complete_metadata.metadata.location_city.is_none() {
        complete_metadata.metadata.location_city = derived_location.city;
    }
    if complete_metadata.metadata.location_state.is_none() {
        complete_metadata.metadata.location_state = derived_location.state;
    }
    if complete_metadata.metadata.location_country.is_none() {
        complete_metadata.metadata.location_country = derived_location.country;
    }
    let geohash = derived_location.geohash;
    let ai_inputs = prepare_ai_inputs(
        executors,
        media_id,
        OriginalAiInput {
            relative_path: &file_path,
            filename: &media.original_filename,
            mime_type: media.mime_type.as_deref(),
            content_hash: &content_hash,
        },
        &media_type,
        (
            complete_metadata.metadata.width,
            complete_metadata.metadata.height,
        ),
        &config.media_process,
        claim_token,
    )
    .await?;
    let sources = std::mem::take(&mut complete_metadata.sources)
        .into_iter()
        .map(|source| MetadataSourceWrite {
            source_type: source.source_type.as_str().to_string(),
            payload_json: source.payload_json,
        })
        .collect();
    let metadata = &complete_metadata.metadata;
    let thumbnail_relative = PathBuf::from("media")
        .join(media_id.to_string())
        .join(format!("v{artifact_version}-{claim_token}"))
        .join("thumbnail.jpg");
    let preview_relative =
        if media_type == "image" && !is_web_compatible_image(media.mime_type.as_deref()) {
            let preview_path = PathBuf::from("media")
                .join(media_id.to_string())
                .join(format!("v{artifact_version}-{claim_token}"))
                .join("preview.jpg");
            Some(preview_path.to_string_lossy().into_owned())
        } else {
            None
        };
    let thumbnail_path =
        crate::io::file::NormalizedStoragePath::parse(&thumbnail_relative.to_string_lossy())
            .map_err(|error| error.to_string())?;
    let mut artifact_destinations = vec![
        (
            crate::io::file::StorageRootId::Thumbnails,
            thumbnail_path.clone(),
        ),
        (
            crate::io::file::StorageRootId::TinyThumbnails,
            thumbnail_path.clone(),
        ),
        (
            crate::io::file::StorageRootId::PlaceThumbnails,
            thumbnail_path,
        ),
    ];
    if let Some(path) = preview_relative.as_deref() {
        artifact_destinations.push((
            crate::io::file::StorageRootId::Previews,
            crate::io::file::NormalizedStoragePath::parse(path)
                .map_err(|error| error.to_string())?,
        ));
    }
    let artifact_output_limits = metadata_artifact_output_limits(
        config.metadata.thumbnails_max_size,
        config.metadata.thumbnails_tiny_size,
        preview_relative.is_some(),
        config.media_process.maximum_normalized_image_output_bytes as u64,
    )?;
    let maximum_artifact_batch_bytes = artifact_output_limits
        .iter()
        .try_fold(0_u64, |total, limit| total.checked_add(*limit))
        .ok_or_else(|| "metadata artifact reservation overflowed".to_string())?;
    let artifact_batch = crate::processor::artifact::prepare_metadata_artifact_batch(
        executors,
        artifact_destinations,
        maximum_artifact_batch_bytes,
        media_id,
        claim_token,
        artifact_version,
    )
    .await?;
    if let Err(error) = generate_metadata_artifact_batch(
        executors,
        &artifact_batch,
        &original_file,
        &media_type,
        preview_relative.is_some(),
        &artifact_output_limits,
        config,
    )
    .await
    {
        artifact_batch.cancel(executors).await;
        return Err(error);
    }
    let committed_artifacts = match artifact_batch.publish(executors, artifact_version).await {
        Ok(group) => group,
        Err(error) => {
            artifact_batch.cancel(executors).await;
            return Err(error);
        }
    };
    let persistence = executors
        .sqlite
        .persist_metadata_generation_durable(PersistMetadataGeneration {
            media_id,
            claim_token: claim_token.to_string(),
            metadata: MetadataValuesWrite {
                width: metadata.width,
                height: metadata.height,
                date_taken: metadata.date_taken.map(|date| date.to_rfc3339()),
                gps_latitude: metadata.gps_latitude,
                gps_longitude: metadata.gps_longitude,
                gps_altitude: metadata.gps_altitude,
                camera_make: metadata.camera_make.clone(),
                camera_model: metadata.camera_model.clone(),
                lens_make: metadata.lens_make.clone(),
                lens_model: metadata.lens_model.clone(),
                iso: metadata.iso,
                exposure_time: metadata.exposure_time.clone(),
                f_number: metadata.f_number,
                focal_length: metadata.focal_length,
                focal_length_35mm: metadata.focal_length_35mm,
                location_city: metadata.location_city.clone(),
                location_state: metadata.location_state.clone(),
                location_country: metadata.location_country.clone(),
                video_codec: metadata.video_codec.clone(),
                keywords: metadata.keywords.clone(),
                duration_seconds: metadata.duration_seconds,
            },
            sources,
            thumbnail_path: thumbnail_relative.to_string_lossy().into_owned(),
            preview_path: preview_relative,
            artifact_version: committed_artifacts.product_version,
            artifact_group_id: committed_artifacts.group_id.clone(),
            artifact_group_version: committed_artifacts.version,
            content_hash,
            geohash,
            ai_inputs,
        })
        .await
        .map_err(|error| error.to_string());
    if let Err(error) = persistence {
        artifact_batch.cancel(executors).await;
        return Err(error);
    }
    executors.scheduler.wake_journal_recovery();
    retire_previous_metadata_artifacts(
        executors,
        previous_thumbnail_path.as_deref(),
        previous_preview_path.as_deref(),
    )
    .await;
    Ok(())
}

async fn retire_previous_metadata_artifacts(
    executors: &ExecutorHandles,
    thumbnail_path: Option<&str>,
    preview_path: Option<&str>,
) {
    if let Some(path) = thumbnail_path {
        match crate::io::file::NormalizedStoragePath::parse(path) {
            Ok(path) => {
                for storage_root in [
                    crate::io::file::StorageRootId::Thumbnails,
                    crate::io::file::StorageRootId::TinyThumbnails,
                    crate::io::file::StorageRootId::PlaceThumbnails,
                ] {
                    if let Err(error) = crate::processor::artifact::retire_artifact(
                        executors,
                        storage_root,
                        path.clone(),
                    )
                    .await
                    {
                        tracing::error!(
                            path = path.relative_path(),
                            error,
                            "Could not journal cleanup for a replaced metadata thumbnail"
                        );
                    }
                }
            }
            Err(_) => tracing::error!(path, "Previous metadata thumbnail path is invalid"),
        }
    }
    if let Some(path) = preview_path {
        let Ok(path) = crate::io::file::NormalizedStoragePath::parse(path) else {
            tracing::error!(path, "Previous metadata preview path is invalid");
            return;
        };
        if let Err(error) = crate::processor::artifact::retire_artifact(
            executors,
            crate::io::file::StorageRootId::Previews,
            path.clone(),
        )
        .await
        {
            tracing::error!(
                path = path.relative_path(),
                error,
                "Could not journal cleanup for a replaced metadata preview"
            );
        }
    }
}

fn is_web_compatible_image(mime_type: Option<&str>) -> bool {
    matches!(
        mime_type,
        Some("image/jpeg" | "image/png" | "image/webp" | "image/gif")
    )
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

async fn generate_metadata_artifact_batch(
    executors: &ExecutorHandles,
    batch: &crate::processor::artifact::PreparedMetadataArtifactBatch,
    original: &StorageMediaFile,
    media_type: &str,
    include_web_preview: bool,
    output_limits: &[u64],
    config: &Config,
) -> Result<(), String> {
    let expected_outputs = if include_web_preview { 4 } else { 3 };
    if output_limits.len() != expected_outputs {
        return Err("metadata artifact output limits do not match the batch".to_string());
    }
    let target = |index: usize, name: &str| {
        batch
            .target(index)
            .map(|target| target.temporary_file())
            .ok_or_else(|| format!("metadata artifact batch is missing {name}"))
    };
    let thumbnail = target(0, "thumbnail")?;
    let tiny_thumbnail = target(1, "tiny thumbnail")?;
    let place_thumbnail = target(2, "place thumbnail")?;
    generate_prepared_thumbnail(
        executors,
        media_type,
        original,
        &thumbnail,
        config.metadata.thumbnails_max_size,
        output_limits[0],
        config,
    )
    .await
    .map_err(|error| format!("thumbnail generation failed: {error}"))?;
    generate_prepared_thumbnail(
        executors,
        media_type,
        original,
        &tiny_thumbnail,
        config.metadata.thumbnails_tiny_size,
        output_limits[1],
        config,
    )
    .await
    .map_err(|error| format!("tiny thumbnail generation failed: {error}"))?;
    if media_type == "image" {
        generate_image_preview_prepared(
            executors,
            original,
            &place_thumbnail,
            config.metadata.thumbnails_max_size,
            config.metadata.thumbnails_quality,
            output_limits[2],
            &config.media_process,
        )
        .await
        .map_err(|error| format!("place thumbnail generation failed: {error}"))?;
    } else {
        generate_video_preview_prepared(
            executors,
            original,
            &place_thumbnail,
            config.metadata.thumbnails_max_size,
            config.metadata.thumbnails_quality,
            output_limits[2],
            &config.media_process,
        )
        .await
        .map_err(|error| format!("place thumbnail generation failed: {error}"))?;
    }
    if include_web_preview {
        let preview = target(3, "web preview")?;
        generate_image_preview_prepared(
            executors,
            original,
            &preview,
            2048,
            90,
            output_limits[3],
            &config.media_process,
        )
        .await
        .map_err(|error| format!("web preview generation failed: {error}"))?;
    }
    Ok(())
}

async fn generate_prepared_thumbnail(
    executors: &ExecutorHandles,
    media_type: &str,
    original: &StorageMediaFile,
    output: &StorageMediaFile,
    maximum_size: u32,
    maximum_output_bytes: u64,
    config: &Config,
) -> Result<(), String> {
    if media_type == "image" {
        generate_image_thumbnail_prepared(
            executors,
            original,
            output,
            maximum_size,
            config.metadata.thumbnails_quality,
            maximum_output_bytes,
            &config.media_process,
        )
        .await
    } else {
        generate_video_thumbnail_prepared(
            executors,
            original,
            output,
            maximum_size,
            config.metadata.thumbnails_quality,
            maximum_output_bytes,
            &config.media_process,
        )
        .await
    }
}

fn metadata_artifact_output_limits(
    thumbnail_size: u32,
    tiny_thumbnail_size: u32,
    include_web_preview: bool,
    configured_maximum_bytes: u64,
) -> Result<Vec<u64>, String> {
    let thumbnail_limit = maximum_jpeg_output_bytes(thumbnail_size, configured_maximum_bytes)?;
    let mut limits = vec![
        thumbnail_limit,
        maximum_jpeg_output_bytes(tiny_thumbnail_size, configured_maximum_bytes)?,
        thumbnail_limit,
    ];
    if include_web_preview {
        limits.push(maximum_jpeg_output_bytes(2048, configured_maximum_bytes)?);
    }
    Ok(limits)
}

fn maximum_png_output_bytes(
    dimensions: (Option<i32>, Option<i32>),
    configured_maximum_bytes: u64,
) -> Result<u64, String> {
    const MAXIMUM_PNG_BYTES_PER_PIXEL: u64 = 16;
    const MAXIMUM_PNG_CONTAINER_OVERHEAD_BYTES: u64 = 1024 * 1024;

    if configured_maximum_bytes == 0 {
        return Err("PNG artifact limit must be positive".to_string());
    }
    let (Some(width), Some(height)) = dimensions else {
        return Ok(configured_maximum_bytes);
    };
    let (Ok(width), Ok(height)) = (u64::try_from(width), u64::try_from(height)) else {
        return Ok(configured_maximum_bytes);
    };
    if width == 0 || height == 0 {
        return Ok(configured_maximum_bytes);
    }
    let dimension_bound = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(MAXIMUM_PNG_BYTES_PER_PIXEL))
        .and_then(|bytes| bytes.checked_add(MAXIMUM_PNG_CONTAINER_OVERHEAD_BYTES))
        .ok_or_else(|| "PNG artifact byte bound overflowed".to_string())?;
    Ok(dimension_bound.min(configured_maximum_bytes))
}

async fn prepare_ai_inputs(
    executors: &ExecutorHandles,
    media_id: i64,
    original: OriginalAiInput<'_>,
    media_type: &str,
    media_dimensions: (Option<i32>, Option<i32>),
    process_config: &crate::config::MediaProcessConfig,
    claim_token: &str,
) -> Result<Vec<MetadataAiInputWrite>, String> {
    let input = if media_type == "video" {
        let filename = format!("{}.png", original.content_hash);
        let frame_relative = PathBuf::from("ai")
            .join(media_id.to_string())
            .join("frames")
            .join(&filename)
            .to_string_lossy()
            .into_owned();
        let frame_path = crate::io::file::NormalizedStoragePath::parse(&frame_relative)
            .map_err(|error| error.to_string())?;
        let frame_exists = match executors
            .file_io
            .open_storage_read_session_durable(
                crate::io::file::StorageRootId::Previews,
                frame_path.clone(),
            )
            .await
        {
            Ok((session, snapshot)) => {
                executors
                    .file_io
                    .close_storage_session_durable(session)
                    .await
                    .map_err(|error| error.to_string())?;
                snapshot.byte_size > 0
            }
            Err(error) if error.kind == crate::executor::ExecutorErrorKind::FileNotFound => false,
            Err(error) => return Err(error.to_string()),
        };
        if !frame_exists {
            let maximum_frame_bytes = maximum_png_output_bytes(
                media_dimensions,
                process_config.maximum_normalized_image_output_bytes as u64,
            )?;
            extract_first_video_frame(
                executors,
                original.relative_path,
                frame_path.clone(),
                maximum_frame_bytes,
                process_config,
                claim_token,
            )
            .await?;
        }
        let (byte_size, content_hash) = hash_storage_file(
            executors,
            crate::io::file::StorageRootId::Previews,
            frame_path,
        )
        .await?;
        AiInputDescriptor {
            storage: AiInputStorage::Previews,
            file_path: frame_relative,
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
        let original_path = crate::io::file::NormalizedStoragePath::parse(original.relative_path)
            .map_err(|error| error.to_string())?;
        let (session, snapshot) = executors
            .file_io
            .open_storage_read_session_durable(
                crate::io::file::StorageRootId::Originals,
                original_path,
            )
            .await
            .map_err(|error| error.to_string())?;
        executors
            .file_io
            .close_storage_session_durable(session)
            .await
            .map_err(|error| error.to_string())?;
        let byte_size = snapshot.byte_size;
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
    let byte_size = i64::try_from(input.byte_size)
        .map_err(|_| "AI input byte size exceeds SQLite range".to_string())?;
    Ok(tasks
        .into_iter()
        .map(|task| MetadataAiInputWrite {
            task: task.to_string(),
            sequence: 0,
            input_kind: input.input_kind.to_string(),
            storage_root: input.storage.as_str().to_string(),
            file_path: input.file_path.clone(),
            filename: input.filename.clone(),
            mime_type: input.mime_type.clone(),
            byte_size,
            content_hash: input.content_hash.clone(),
            frame_timestamp_ms: input.frame_timestamp_ms,
        })
        .collect())
}

async fn hash_storage_file(
    executors: &ExecutorHandles,
    storage_root: crate::io::file::StorageRootId,
    path: crate::io::file::NormalizedStoragePath,
) -> Result<(u64, String), String> {
    let (session, snapshot) = executors
        .file_io
        .open_storage_read_session_durable(storage_root, path)
        .await
        .map_err(|error| error.to_string())?;
    let mut session = Some(session);
    let mut hasher = Some(
        executors
            .cpu
            .start_sha256_session_durable()
            .await
            .map_err(|error| error.to_string())?,
    );
    let mut byte_count = 0_u64;
    loop {
        let (returned_session, bytes) = executors
            .file_io
            .read_storage_session_durable(
                session
                    .take()
                    .ok_or_else(|| "storage hash session is unavailable".to_string())?,
                crate::runtime::FILE_IO_CHUNK_BYTES as usize,
            )
            .await
            .map_err(|error| error.to_string())?;
        session = Some(returned_session);
        if bytes.is_empty() {
            break;
        }
        byte_count = byte_count
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| "storage hash byte count overflowed".to_string())?;
        if byte_count > snapshot.byte_size {
            return Err("storage file changed while hashing".to_string());
        }
        let (returned_hasher, _) = executors
            .cpu
            .update_sha256_session_durable(
                hasher
                    .take()
                    .ok_or_else(|| "storage hash state is unavailable".to_string())?,
                bytes,
            )
            .await
            .map_err(|error| error.to_string())?;
        hasher = Some(returned_hasher);
    }
    if byte_count != snapshot.byte_size {
        return Err("storage file changed while hashing".to_string());
    }
    let content_hash = executors
        .cpu
        .finish_sha256_session_durable(
            hasher
                .take()
                .ok_or_else(|| "storage hash state is unavailable".to_string())?,
        )
        .await
        .map_err(|error| error.to_string())?;
    executors
        .file_io
        .close_storage_session_durable(
            session
                .take()
                .ok_or_else(|| "storage hash session is unavailable".to_string())?,
        )
        .await
        .map_err(|error| error.to_string())?;
    Ok((snapshot.byte_size, content_hash))
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
    executors: &ExecutorHandles,
    original_path: &str,
    output_path: crate::io::file::NormalizedStoragePath,
    maximum_output_bytes: u64,
    process_config: &crate::config::MediaProcessConfig,
    claim_token: &str,
) -> Result<(), String> {
    let publication = crate::processor::artifact::prepare_artifact_publication(
        executors,
        crate::io::file::StorageRootId::Previews,
        output_path.clone(),
        maximum_output_bytes,
        "video_ai_frame",
        ArtifactPublicationOwner::MetadataClaim(claim_token),
    )
    .await?;
    let mut arguments = ffmpeg_single_thread_arguments();
    arguments.extend([
        OsString::from("-nostdin"),
        OsString::from("-y"),
        OsString::from("-ss"),
        OsString::from("0"),
        OsString::from("-i"),
        OsString::from("/proc/self/fd/10"),
        OsString::from("-threads"),
        OsString::from("1"),
        OsString::from("-map"),
        OsString::from("0:v:0"),
        OsString::from("-frames:v"),
        OsString::from("1"),
        OsString::from("-c:v"),
        OsString::from("png"),
        OsString::from("-f"),
        OsString::from("image2"),
        OsString::from("/proc/self/fd/11"),
    ]);
    let output = crate::executor::process::run_storage_media_tool(
        &executors.cpu,
        &executors.file_io,
        crate::executor::process::MediaTool::Ffmpeg {
            validated_media_duration: None,
        },
        arguments,
        0,
        process_config.maximum_stderr_bytes,
        vec![
            crate::executor::process::StorageChildDescriptor::Read {
                storage_root: crate::io::file::StorageRootId::Originals,
                path: crate::io::file::NormalizedStoragePath::parse(original_path)
                    .map_err(|error| error.to_string())?,
                child_fd: 10,
            },
            crate::executor::process::StorageChildDescriptor::Write {
                storage_root: publication.storage_root(),
                path: publication.temporary_path().clone(),
                child_fd: 11,
                rollback_length: 0,
                require_non_empty: true,
                maximum_bytes: maximum_output_bytes,
            },
        ],
    )
    .await;
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            publication.cancel(executors).await;
            return Err(format!(
                "failed to execute ffmpeg while extracting a representative frame from {}: {error}",
                original_path
            ));
        }
    };
    if !output.status.success() {
        let detail = format!(
            "ffmpeg could not extract a representative frame from {} to {}: {}",
            original_path,
            output_path.relative_path(),
            output.failure_detail("ffmpeg")
        );
        tracing::error!(
            input_path = original_path,
            output_path = output_path.relative_path(),
            status = %output.status,
            stderr = %output.stderr_text(),
            stderr_truncated = output.stderr_truncated,
            "FFmpeg representative-frame extraction failed"
        );
        publication.cancel(executors).await;
        return Err(bounded_error_detail(&detail));
    }
    publication.publish(executors).await
}
