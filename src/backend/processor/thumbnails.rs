use std::ffi::OsString;
use std::path::Path;
use tracing::error;

use crate::config::MediaProcessConfig;
use crate::utils::process::{
    image_magick_resource_arguments, process_limits, validate_image_dimensions, ExternalProcess,
};

#[derive(Clone, Copy)]
enum ImageVariant {
    CroppedThumbnail,
    AspectRatioPreview,
}

async fn run_command(
    executable: &str,
    arguments: Vec<OsString>,
    maximum_stdout_bytes: usize,
    process_config: &MediaProcessConfig,
) -> bool {
    let (timeout, termination_grace, maximum_stderr_bytes) = process_limits(process_config);
    match ExternalProcess::new(
        executable,
        arguments,
        timeout,
        termination_grace,
        maximum_stdout_bytes,
        maximum_stderr_bytes,
    )
    .run()
    .await
    {
        Ok(output) => {
            if !output.status.success() {
                error!(
                    executable,
                    stderr = %output.stderr_text(),
                    stderr_truncated = output.stderr_truncated,
                    "Media command failed"
                );
                return false;
            }
            true
        }
        Err(error) => {
            error!(executable, error = %error, "Failed to execute media command");
            false
        }
    }
}

pub async fn generate_image_thumbnail(
    source_path: &Path,
    output_path: &Path,
    max_size: u32,
    quality: u8,
    process_config: &MediaProcessConfig,
) -> bool {
    if let Some(parent) = output_path.parent() {
        if tokio::fs::create_dir_all(parent).await.is_err() {
            return false;
        }
    }

    generate_image_variant_with_fallback(
        source_path,
        output_path,
        max_size,
        quality,
        ImageVariant::CroppedThumbnail,
        process_config,
    )
    .await
}

pub async fn generate_video_thumbnail(
    source_path: &Path,
    output_path: &Path,
    max_size: u32,
    quality: u8,
    video_frame_quality: u8,
    process_config: &MediaProcessConfig,
) -> bool {
    if let Some(parent) = output_path.parent() {
        if tokio::fs::create_dir_all(parent).await.is_err() {
            return false;
        }
    }

    let temp_frame = output_path.with_extension("temp.jpg");
    if !extract_video_frame(
        source_path,
        &temp_frame,
        video_frame_quality,
        process_config,
    )
    .await
    {
        error!(
            "Failed to extract video frame for thumbnail: {:?}",
            source_path
        );
        return false;
    }

    let success =
        generate_montage_thumbnail(&temp_frame, output_path, max_size, quality, process_config)
            .await;
    if !success {
        error!("Failed to generate montage thumbnail: {:?}", output_path);
    }

    let _ = tokio::fs::remove_file(&temp_frame).await;

    success
}

pub async fn generate_image_preview(
    source_path: &Path,
    output_path: &Path,
    max_size: u32,
    quality: u8,
    process_config: &MediaProcessConfig,
) -> bool {
    if let Some(parent) = output_path.parent() {
        if tokio::fs::create_dir_all(parent).await.is_err() {
            return false;
        }
    }

    generate_image_variant_with_fallback(
        source_path,
        output_path,
        max_size,
        quality,
        ImageVariant::AspectRatioPreview,
        process_config,
    )
    .await
}

pub async fn generate_video_preview(
    source_path: &Path,
    output_path: &Path,
    max_size: u32,
    quality: u8,
    video_frame_quality: u8,
    process_config: &MediaProcessConfig,
) -> bool {
    if let Some(parent) = output_path.parent() {
        if tokio::fs::create_dir_all(parent).await.is_err() {
            return false;
        }
    }

    let temporary_frame = output_path.with_extension("temp.jpg");
    if !extract_video_frame(
        source_path,
        &temporary_frame,
        video_frame_quality,
        process_config,
    )
    .await
    {
        return false;
    }
    let generated = generate_image_preview(
        &temporary_frame,
        output_path,
        max_size,
        quality,
        process_config,
    )
    .await;
    let _ = tokio::fs::remove_file(&temporary_frame).await;
    generated
}

async fn generate_montage_thumbnail(
    source_path: &Path,
    output_path: &Path,
    max_size: u32,
    quality: u8,
    process_config: &MediaProcessConfig,
) -> bool {
    generate_image_variant(
        source_path,
        output_path,
        max_size,
        quality,
        ImageVariant::CroppedThumbnail,
        process_config,
    )
    .await
}

async fn generate_image_variant_with_fallback(
    source_path: &Path,
    output_path: &Path,
    maximum_size: u32,
    quality: u8,
    variant: ImageVariant,
    process_config: &MediaProcessConfig,
) -> bool {
    if generate_image_variant(
        source_path,
        output_path,
        maximum_size,
        quality,
        variant,
        process_config,
    )
    .await
    {
        return true;
    }
    let _ = tokio::fs::remove_file(output_path).await;

    let embedded_preview_path = output_path.with_extension("embedded-preview.jpg");
    if !extract_embedded_image_preview(source_path, &embedded_preview_path, process_config).await {
        return false;
    }
    let generated = generate_image_variant(
        &embedded_preview_path,
        output_path,
        maximum_size,
        quality,
        variant,
        process_config,
    )
    .await;
    let _ = tokio::fs::remove_file(embedded_preview_path).await;
    if !generated {
        let _ = tokio::fs::remove_file(output_path).await;
    }
    generated
}

async fn generate_image_variant(
    source_path: &Path,
    output_path: &Path,
    maximum_size: u32,
    quality: u8,
    variant: ImageVariant,
    process_config: &MediaProcessConfig,
) -> bool {
    if let Err(error) = validate_image_dimensions(source_path, process_config).await {
        error!(
            source_path = %source_path.display(),
            error = %error,
            "Image input failed dimension validation"
        );
        return false;
    }
    let mut source_input = source_path.as_os_str().to_os_string();
    source_input.push("[0]");
    let size = format!("{maximum_size}x{maximum_size}");
    let mut arguments = image_magick_resource_arguments(process_config);
    arguments.extend([source_input, OsString::from("-auto-orient")]);
    match variant {
        ImageVariant::CroppedThumbnail => arguments.extend([
            OsString::from("-thumbnail"),
            OsString::from(format!("{size}^")),
            OsString::from("-gravity"),
            OsString::from("center"),
            OsString::from("-extent"),
            OsString::from(size),
        ]),
        ImageVariant::AspectRatioPreview => {
            arguments.extend([
                OsString::from("-resize"),
                OsString::from(format!("{size}>")),
            ]);
        }
    }
    arguments.extend([
        OsString::from("-quality"),
        OsString::from(quality.to_string()),
        output_path.as_os_str().to_os_string(),
    ]);

    run_command("magick", arguments, 0, process_config).await && output_path.exists()
}

async fn extract_embedded_image_preview(
    source_path: &Path,
    output_path: &Path,
    process_config: &MediaProcessConfig,
) -> bool {
    for preview_tag in ["PreviewImage", "JpgFromRaw", "ThumbnailImage"] {
        let (timeout, termination_grace, maximum_stderr_bytes) = process_limits(process_config);
        let command_output = ExternalProcess::new(
            "exiftool",
            vec![
                OsString::from("-b"),
                OsString::from(format!("-{preview_tag}")),
                source_path.as_os_str().to_os_string(),
            ],
            timeout,
            termination_grace,
            process_config.maximum_normalized_image_output_bytes,
            maximum_stderr_bytes,
        )
        .run()
        .await;
        let extracted = match command_output {
            Ok(output)
                if output.status.success()
                    && !output.stdout.is_empty()
                    && !output.stdout_truncated =>
            {
                tokio::fs::write(output_path, output.stdout).await.is_ok()
            }
            _ => false,
        };
        if extracted {
            return true;
        }
        let _ = tokio::fs::remove_file(output_path).await;
    }

    false
}

async fn extract_video_frame(
    source_path: &Path,
    output_path: &Path,
    video_frame_quality: u8,
    process_config: &MediaProcessConfig,
) -> bool {
    let seek_time = "00:00:00";

    let arguments = vec![
        OsString::from("-nostdin"),
        OsString::from("-y"),
        OsString::from("-ss"),
        OsString::from(seek_time),
        OsString::from("-i"),
        source_path.as_os_str().to_os_string(),
        OsString::from("-vframes"),
        OsString::from("1"),
        OsString::from("-q:v"),
        OsString::from(video_frame_quality.to_string()),
        output_path.as_os_str().to_os_string(),
    ];

    run_command("ffmpeg", arguments, 0, process_config).await && output_path.exists()
}
