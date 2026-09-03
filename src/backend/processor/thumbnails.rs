use std::ffi::OsString;
use std::path::Path;

use tracing::error;

use crate::config::MediaProcessConfig;
use crate::executor::process::{
    bounded_error_detail, ffmpeg_single_thread_arguments, image_magick_resource_arguments,
    run_storage_media_tool, run_storage_media_tool_with_stdout, validate_storage_image_dimensions,
    MediaTool, StorageChildDescriptor,
};
use crate::io::file::{NormalizedStoragePath, StorageRootId};
pub use crate::processor::artifact::ArtifactPublicationOwner;
use crate::processor::artifact::{
    prepare_artifact_publication, retire_artifact, PreparedArtifactPublication,
};
use crate::runtime::ExecutorHandles;

const MAXIMUM_JPEG_BYTES_PER_PIXEL: u64 = 8;
const MAXIMUM_JPEG_CONTAINER_OVERHEAD_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct StorageMediaFile {
    pub storage_root: StorageRootId,
    pub path: NormalizedStoragePath,
}

#[derive(Clone, Copy)]
enum ImageVariant {
    CroppedThumbnail,
    AspectRatioPreview,
}

#[derive(Clone, Copy)]
struct ImageVariantSpec {
    maximum_size: u32,
    maximum_output_bytes: u64,
    quality: u8,
    variant: ImageVariant,
}

#[derive(Clone, Copy)]
enum OutputMode<'a> {
    Managed(ArtifactPublicationOwner<'a>),
    Prepared,
}

struct GeneratedMediaTool {
    tool: MediaTool,
    arguments: Vec<OsString>,
    maximum_output_bytes: u64,
}

impl<'a> OutputMode<'a> {
    fn embedded_owner(self) -> ArtifactPublicationOwner<'a> {
        match self {
            Self::Managed(owner) => owner,
            Self::Prepared => ArtifactPublicationOwner::JournalGroup,
        }
    }
}

pub async fn generate_image_thumbnail(
    executors: &ExecutorHandles,
    source: &StorageMediaFile,
    output: &StorageMediaFile,
    max_size: u32,
    quality: u8,
    process_config: &MediaProcessConfig,
    owner: ArtifactPublicationOwner<'_>,
) -> Result<(), String> {
    let maximum_output_bytes = maximum_jpeg_output_bytes(
        max_size,
        process_config.maximum_normalized_image_output_bytes as u64,
    )?;
    generate_image_variant_with_fallback(
        executors,
        source,
        output,
        ImageVariantSpec {
            maximum_size: max_size,
            maximum_output_bytes,
            quality,
            variant: ImageVariant::CroppedThumbnail,
        },
        process_config,
        OutputMode::Managed(owner),
    )
    .await
}

pub async fn generate_video_thumbnail(
    executors: &ExecutorHandles,
    source: &StorageMediaFile,
    output: &StorageMediaFile,
    max_size: u32,
    quality: u8,
    process_config: &MediaProcessConfig,
    owner: ArtifactPublicationOwner<'_>,
) -> Result<(), String> {
    let maximum_output_bytes = maximum_jpeg_output_bytes(
        max_size,
        process_config.maximum_normalized_image_output_bytes as u64,
    )?;
    generate_video_variant(
        executors,
        source,
        output,
        ImageVariantSpec {
            maximum_size: max_size,
            maximum_output_bytes,
            quality,
            variant: ImageVariant::CroppedThumbnail,
        },
        process_config,
        OutputMode::Managed(owner),
    )
    .await
}

pub async fn generate_image_preview(
    executors: &ExecutorHandles,
    source: &StorageMediaFile,
    output: &StorageMediaFile,
    max_size: u32,
    quality: u8,
    process_config: &MediaProcessConfig,
    owner: ArtifactPublicationOwner<'_>,
) -> Result<(), String> {
    let maximum_output_bytes = maximum_jpeg_output_bytes(
        max_size,
        process_config.maximum_normalized_image_output_bytes as u64,
    )?;
    generate_image_variant_with_fallback(
        executors,
        source,
        output,
        ImageVariantSpec {
            maximum_size: max_size,
            maximum_output_bytes,
            quality,
            variant: ImageVariant::AspectRatioPreview,
        },
        process_config,
        OutputMode::Managed(owner),
    )
    .await
}

pub async fn generate_video_preview(
    executors: &ExecutorHandles,
    source: &StorageMediaFile,
    output: &StorageMediaFile,
    max_size: u32,
    quality: u8,
    process_config: &MediaProcessConfig,
    owner: ArtifactPublicationOwner<'_>,
) -> Result<(), String> {
    let maximum_output_bytes = maximum_jpeg_output_bytes(
        max_size,
        process_config.maximum_normalized_image_output_bytes as u64,
    )?;
    generate_video_variant(
        executors,
        source,
        output,
        ImageVariantSpec {
            maximum_size: max_size,
            maximum_output_bytes,
            quality,
            variant: ImageVariant::AspectRatioPreview,
        },
        process_config,
        OutputMode::Managed(owner),
    )
    .await
}

pub(crate) async fn generate_image_thumbnail_prepared(
    executors: &ExecutorHandles,
    source: &StorageMediaFile,
    output: &StorageMediaFile,
    max_size: u32,
    quality: u8,
    maximum_output_bytes: u64,
    process_config: &MediaProcessConfig,
) -> Result<(), String> {
    generate_image_variant_with_fallback(
        executors,
        source,
        output,
        ImageVariantSpec {
            maximum_size: max_size,
            maximum_output_bytes,
            quality,
            variant: ImageVariant::CroppedThumbnail,
        },
        process_config,
        OutputMode::Prepared,
    )
    .await
}

pub(crate) async fn generate_video_thumbnail_prepared(
    executors: &ExecutorHandles,
    source: &StorageMediaFile,
    output: &StorageMediaFile,
    max_size: u32,
    quality: u8,
    maximum_output_bytes: u64,
    process_config: &MediaProcessConfig,
) -> Result<(), String> {
    generate_video_variant(
        executors,
        source,
        output,
        ImageVariantSpec {
            maximum_size: max_size,
            maximum_output_bytes,
            quality,
            variant: ImageVariant::CroppedThumbnail,
        },
        process_config,
        OutputMode::Prepared,
    )
    .await
}

pub(crate) async fn generate_image_preview_prepared(
    executors: &ExecutorHandles,
    source: &StorageMediaFile,
    output: &StorageMediaFile,
    max_size: u32,
    quality: u8,
    maximum_output_bytes: u64,
    process_config: &MediaProcessConfig,
) -> Result<(), String> {
    generate_image_variant_with_fallback(
        executors,
        source,
        output,
        ImageVariantSpec {
            maximum_size: max_size,
            maximum_output_bytes,
            quality,
            variant: ImageVariant::AspectRatioPreview,
        },
        process_config,
        OutputMode::Prepared,
    )
    .await
}

pub(crate) async fn generate_video_preview_prepared(
    executors: &ExecutorHandles,
    source: &StorageMediaFile,
    output: &StorageMediaFile,
    max_size: u32,
    quality: u8,
    maximum_output_bytes: u64,
    process_config: &MediaProcessConfig,
) -> Result<(), String> {
    generate_video_variant(
        executors,
        source,
        output,
        ImageVariantSpec {
            maximum_size: max_size,
            maximum_output_bytes,
            quality,
            variant: ImageVariant::AspectRatioPreview,
        },
        process_config,
        OutputMode::Prepared,
    )
    .await
}

pub(crate) fn maximum_jpeg_output_bytes(
    maximum_size: u32,
    configured_maximum_bytes: u64,
) -> Result<u64, String> {
    if maximum_size == 0 || configured_maximum_bytes == 0 {
        return Err("JPEG artifact limits must be positive".to_string());
    }
    let maximum_pixels = u64::from(maximum_size)
        .checked_mul(u64::from(maximum_size))
        .ok_or_else(|| "JPEG artifact pixel bound overflowed".to_string())?;
    let dimension_bound = maximum_pixels
        .checked_mul(MAXIMUM_JPEG_BYTES_PER_PIXEL)
        .and_then(|bytes| bytes.checked_add(MAXIMUM_JPEG_CONTAINER_OVERHEAD_BYTES))
        .ok_or_else(|| "JPEG artifact byte bound overflowed".to_string())?;
    Ok(dimension_bound.min(configured_maximum_bytes))
}

async fn generate_image_variant_with_fallback(
    executors: &ExecutorHandles,
    source: &StorageMediaFile,
    output: &StorageMediaFile,
    spec: ImageVariantSpec,
    process_config: &MediaProcessConfig,
    mode: OutputMode<'_>,
) -> Result<(), String> {
    let direct_error =
        match generate_image_variant(executors, source, output, spec, process_config, mode).await {
            Ok(()) => return Ok(()),
            Err(error) => error,
        };
    let embedded = extract_embedded_image_preview(
        executors,
        source,
        output,
        process_config,
        mode.embedded_owner(),
    )
    .await
        .map_err(|embedded_error| {
            bounded_error_detail(&format!(
                "image conversion failed for {}: {direct_error}; embedded-preview fallback failed: {embedded_error}",
                source.path.relative_path()
            ))
        })?;
    let conversion =
        generate_image_variant(executors, &embedded, output, spec, process_config, mode).await;
    let retirement = retire_artifact(executors, embedded.storage_root, embedded.path).await;
    match (conversion, retirement) {
        (Ok(()), Ok(())) => Ok(()),
        (Ok(()), Err(error)) => Err(format!("failed to retire embedded preview: {error}")),
        (Err(embedded_error), _) => Err(bounded_error_detail(&format!(
            "image conversion failed for {}: {direct_error}; converting the extracted embedded preview also failed: {embedded_error}",
            source.path.relative_path()
        ))),
    }
}

async fn generate_image_variant(
    executors: &ExecutorHandles,
    source: &StorageMediaFile,
    output: &StorageMediaFile,
    spec: ImageVariantSpec,
    process_config: &MediaProcessConfig,
    mode: OutputMode<'_>,
) -> Result<(), String> {
    validate_storage_image_dimensions(
        &executors.cpu,
        &executors.file_io,
        source.storage_root,
        source.path.clone(),
        process_config,
    )
    .await
    .map_err(|error| {
        error!(
            source_path = source.path.relative_path(),
            error = %error,
            "Image input failed dimension validation"
        );
        format!(
            "identify could not validate {}: {error}",
            source.path.relative_path()
        )
    })?;
    let size = format!("{}x{}", spec.maximum_size, spec.maximum_size);
    let mut arguments = image_magick_resource_arguments(process_config);
    arguments.extend([
        OsString::from("/proc/self/fd/10[0]"),
        OsString::from("-auto-orient"),
    ]);
    match spec.variant {
        ImageVariant::CroppedThumbnail => arguments.extend([
            OsString::from("-thumbnail"),
            OsString::from(format!("{size}^")),
            OsString::from("-gravity"),
            OsString::from("center"),
            OsString::from("-extent"),
            OsString::from(size),
        ]),
        ImageVariant::AspectRatioPreview => arguments.extend([
            OsString::from("-resize"),
            OsString::from(format!("{size}>")),
        ]),
    }
    arguments.extend([
        OsString::from("-strip"),
        OsString::from("-quality"),
        OsString::from(spec.quality.to_string()),
        OsString::from("jpg:/proc/self/fd/11"),
    ]);
    run_generated_tool_for_mode(
        executors,
        source,
        output,
        mode,
        GeneratedMediaTool {
            tool: MediaTool::ImageMagick,
            arguments,
            maximum_output_bytes: spec.maximum_output_bytes,
        },
        process_config,
    )
    .await
}

async fn generate_video_variant(
    executors: &ExecutorHandles,
    source: &StorageMediaFile,
    output: &StorageMediaFile,
    spec: ImageVariantSpec,
    process_config: &MediaProcessConfig,
    mode: OutputMode<'_>,
) -> Result<(), String> {
    let filter = match spec.variant {
        ImageVariant::CroppedThumbnail => format!(
            "scale={0}:{0}:force_original_aspect_ratio=increase,crop={0}:{0}",
            spec.maximum_size
        ),
        ImageVariant::AspectRatioPreview => format!(
            "scale={0}:{0}:force_original_aspect_ratio=decrease",
            spec.maximum_size
        ),
    };
    let quantizer = 2_u16 + (u16::from(100_u8.saturating_sub(spec.quality)) * 29 / 100);
    let mut arguments = ffmpeg_single_thread_arguments();
    arguments.extend([
        OsString::from("-nostdin"),
        OsString::from("-y"),
        OsString::from("-ss"),
        OsString::from("0"),
        OsString::from("-i"),
        OsString::from("/proc/self/fd/10"),
        OsString::from("-vf"),
        OsString::from(filter),
        OsString::from("-frames:v"),
        OsString::from("1"),
        OsString::from("-q:v"),
        OsString::from(quantizer.to_string()),
        OsString::from("-f"),
        OsString::from("image2"),
        OsString::from("/proc/self/fd/11"),
    ]);
    run_generated_tool_for_mode(
        executors,
        source,
        output,
        mode,
        GeneratedMediaTool {
            tool: MediaTool::Ffmpeg {
                validated_media_duration: None,
            },
            arguments,
            maximum_output_bytes: spec.maximum_output_bytes,
        },
        process_config,
    )
    .await
}

async fn run_generated_tool_for_mode(
    executors: &ExecutorHandles,
    source: &StorageMediaFile,
    output: &StorageMediaFile,
    mode: OutputMode<'_>,
    generated: GeneratedMediaTool,
    process_config: &MediaProcessConfig,
) -> Result<(), String> {
    match mode {
        OutputMode::Managed(owner) => {
            let publication = prepare_output_publication(
                executors,
                output,
                generated.maximum_output_bytes,
                owner,
            )
            .await?;
            run_generated_tool(
                executors,
                source,
                publication,
                generated.tool,
                generated.arguments,
                generated.maximum_output_bytes,
                process_config,
            )
            .await
        }
        OutputMode::Prepared => {
            run_generated_tool_to_prepared_output(
                executors,
                source,
                output,
                generated.tool,
                generated.arguments,
                generated.maximum_output_bytes,
                process_config,
            )
            .await
        }
    }
}

async fn run_generated_tool_to_prepared_output(
    executors: &ExecutorHandles,
    source: &StorageMediaFile,
    output: &StorageMediaFile,
    tool: MediaTool,
    arguments: Vec<OsString>,
    maximum_output_bytes: u64,
    process_config: &MediaProcessConfig,
) -> Result<(), String> {
    let executable = tool.executable().to_string_lossy().into_owned();
    let completion = run_storage_media_tool(
        &executors.cpu,
        &executors.file_io,
        tool,
        arguments,
        0,
        process_config.maximum_stderr_bytes,
        vec![
            StorageChildDescriptor::Read {
                storage_root: source.storage_root,
                path: source.path.clone(),
                child_fd: 10,
            },
            StorageChildDescriptor::Write {
                storage_root: output.storage_root,
                path: output.path.clone(),
                child_fd: 11,
                rollback_length: 0,
                require_non_empty: true,
                maximum_bytes: maximum_output_bytes,
            },
        ],
    )
    .await
    .map_err(|error| format!("failed to execute {executable}: {error}"))?;
    if completion.status.success() {
        return Ok(());
    }
    let detail = completion.failure_detail(&executable);
    error!(
        executable,
        status = %completion.status,
        stderr = %completion.stderr_text(),
        stderr_truncated = completion.stderr_truncated,
        "Media command failed"
    );
    Err(detail)
}

async fn prepare_output_publication(
    executors: &ExecutorHandles,
    output: &StorageMediaFile,
    maximum_output_bytes: u64,
    owner: ArtifactPublicationOwner<'_>,
) -> Result<PreparedArtifactPublication, String> {
    prepare_artifact_publication(
        executors,
        output.storage_root,
        output.path.clone(),
        maximum_output_bytes,
        "media_preview",
        owner,
    )
    .await
}

async fn run_generated_tool(
    executors: &ExecutorHandles,
    source: &StorageMediaFile,
    publication: PreparedArtifactPublication,
    tool: MediaTool,
    arguments: Vec<OsString>,
    maximum_output_bytes: u64,
    process_config: &MediaProcessConfig,
) -> Result<(), String> {
    let executable = tool.executable().to_string_lossy().into_owned();
    let result = run_storage_media_tool(
        &executors.cpu,
        &executors.file_io,
        tool,
        arguments,
        0,
        process_config.maximum_stderr_bytes,
        vec![
            StorageChildDescriptor::Read {
                storage_root: source.storage_root,
                path: source.path.clone(),
                child_fd: 10,
            },
            StorageChildDescriptor::Write {
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
    let output = match result {
        Ok(output) => output,
        Err(error) => {
            publication.cancel(executors).await;
            return Err(format!("failed to execute {executable}: {error}"));
        }
    };
    if !output.status.success() {
        let detail = output.failure_detail(&executable);
        error!(
            executable,
            status = %output.status,
            stderr = %output.stderr_text(),
            stderr_truncated = output.stderr_truncated,
            "Media command failed"
        );
        publication.cancel(executors).await;
        return Err(detail);
    }
    publication.publish(executors).await
}

async fn extract_embedded_image_preview(
    executors: &ExecutorHandles,
    source: &StorageMediaFile,
    output: &StorageMediaFile,
    process_config: &MediaProcessConfig,
    owner: ArtifactPublicationOwner<'_>,
) -> Result<StorageMediaFile, String> {
    let (source_session, source_snapshot) = executors
        .file_io
        .open_storage_read_session_durable(source.storage_root, source.path.clone())
        .await
        .map_err(|error| error.to_string())?;
    executors
        .file_io
        .close_storage_session_durable(source_session)
        .await
        .map_err(|error| error.to_string())?;
    let maximum_embedded_bytes = source_snapshot
        .byte_size
        .min(process_config.maximum_normalized_image_output_bytes as u64);
    if maximum_embedded_bytes == 0 {
        return Err("cannot extract an embedded preview from an empty image".to_string());
    }
    let output_parent = Path::new(output.path.relative_path())
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let mut failures = Vec::new();
    for preview_tag in ["PreviewImage", "JpgFromRaw", "ThumbnailImage"] {
        let embedded_path = NormalizedStoragePath::parse(
            &output_parent
                .join(format!(".momento-embedded-{}.jpg", uuid::Uuid::new_v4()))
                .to_string_lossy(),
        )
        .map_err(|error| error.to_string())?;
        let publication = prepare_artifact_publication(
            executors,
            output.storage_root,
            embedded_path.clone(),
            maximum_embedded_bytes,
            "embedded_image_preview",
            owner,
        )
        .await?;
        let command = run_storage_media_tool_with_stdout(
            &executors.cpu,
            &executors.file_io,
            MediaTool::ExifTool,
            vec![
                OsString::from("-b"),
                OsString::from(format!("-{preview_tag}")),
                OsString::from("/proc/self/fd/10"),
            ],
            process_config.maximum_stderr_bytes,
            vec![
                StorageChildDescriptor::Read {
                    storage_root: source.storage_root,
                    path: source.path.clone(),
                    child_fd: 10,
                },
                StorageChildDescriptor::Write {
                    storage_root: publication.storage_root(),
                    path: publication.temporary_path().clone(),
                    child_fd: 11,
                    rollback_length: 0,
                    require_non_empty: true,
                    maximum_bytes: maximum_embedded_bytes,
                },
            ],
            11,
        )
        .await;
        match command {
            Ok(command) if command.status.success() => {
                publication.publish(executors).await?;
                return Ok(StorageMediaFile {
                    storage_root: output.storage_root,
                    path: embedded_path,
                });
            }
            Ok(command) => {
                failures.push(format!(
                    "{preview_tag}: {}",
                    command.failure_detail("exiftool")
                ));
                publication.cancel(executors).await;
            }
            Err(error) => {
                failures.push(format!(
                    "{preview_tag}: failed to execute exiftool: {error}"
                ));
                publication.cancel(executors).await;
            }
        }
    }
    let detail = bounded_error_detail(&format!(
        "exiftool could not extract an embedded preview from {}: {}",
        source.path.relative_path(),
        failures.join("; ")
    ));
    error!(
        executable = "exiftool",
        input_path = source.path.relative_path(),
        error = %detail,
        "Embedded image extraction failed"
    );
    Err(detail)
}
