use std::ffi::OsString;
use std::path::Path;
use tracing::error;

use crate::config::MediaProcessConfig;
use crate::utils::process::{
    bounded_error_detail, image_magick_resource_arguments, validate_image_dimensions,
    ExternalProcess,
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
) -> Result<(), String> {
    match ExternalProcess::new(
        executable,
        arguments,
        maximum_stdout_bytes,
        process_config.maximum_stderr_bytes,
    )
    .run()
    .await
    {
        Ok(output) => {
            if !output.status.success() {
                let detail = output.failure_detail(executable);
                error!(
                    executable,
                    status = %output.status,
                    stderr = %output.stderr_text(),
                    stderr_truncated = output.stderr_truncated,
                    "Media command failed"
                );
                return Err(detail);
            }
            Ok(())
        }
        Err(error) => {
            error!(executable, error = %error, "Failed to execute media command");
            Err(format!("failed to execute {executable}: {error}"))
        }
    }
}

pub async fn generate_image_thumbnail(
    source_path: &Path,
    output_path: &Path,
    max_size: u32,
    quality: u8,
    process_config: &MediaProcessConfig,
) -> Result<(), String> {
    if let Some(parent) = output_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|error| {
            format!(
                "failed to create image thumbnail directory {}: {error}",
                parent.display()
            )
        })?;
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
) -> Result<(), String> {
    if let Some(parent) = output_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|error| {
            format!(
                "failed to create video thumbnail directory {}: {error}",
                parent.display()
            )
        })?;
    }

    let temp_frame = output_path.with_extension("temp.jpg");
    extract_video_frame(
        source_path,
        &temp_frame,
        video_frame_quality,
        process_config,
    )
    .await?;

    let result =
        generate_montage_thumbnail(&temp_frame, output_path, max_size, quality, process_config)
            .await;
    let _ = tokio::fs::remove_file(&temp_frame).await;
    result
}

pub async fn generate_image_preview(
    source_path: &Path,
    output_path: &Path,
    max_size: u32,
    quality: u8,
    process_config: &MediaProcessConfig,
) -> Result<(), String> {
    if let Some(parent) = output_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|error| {
            format!(
                "failed to create image preview directory {}: {error}",
                parent.display()
            )
        })?;
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
) -> Result<(), String> {
    if let Some(parent) = output_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|error| {
            format!(
                "failed to create video preview directory {}: {error}",
                parent.display()
            )
        })?;
    }

    let temporary_frame = output_path.with_extension("temp.jpg");
    extract_video_frame(
        source_path,
        &temporary_frame,
        video_frame_quality,
        process_config,
    )
    .await?;
    let result = generate_image_preview(
        &temporary_frame,
        output_path,
        max_size,
        quality,
        process_config,
    )
    .await;
    let _ = tokio::fs::remove_file(&temporary_frame).await;
    result
}

async fn generate_montage_thumbnail(
    source_path: &Path,
    output_path: &Path,
    max_size: u32,
    quality: u8,
    process_config: &MediaProcessConfig,
) -> Result<(), String> {
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
) -> Result<(), String> {
    let direct_error = match generate_image_variant(
        source_path,
        output_path,
        maximum_size,
        quality,
        variant,
        process_config,
    )
    .await
    {
        Ok(()) => return Ok(()),
        Err(error) => error,
    };
    let _ = tokio::fs::remove_file(output_path).await;

    let embedded_preview_path = output_path.with_extension("embedded-preview.jpg");
    if let Err(embedded_error) =
        extract_embedded_image_preview(source_path, &embedded_preview_path, process_config).await
    {
        return Err(bounded_error_detail(&format!(
            "image conversion failed for {}: {direct_error}; embedded-preview fallback failed: {embedded_error}",
            source_path.display()
        )));
    }
    let result = generate_image_variant(
        &embedded_preview_path,
        output_path,
        maximum_size,
        quality,
        variant,
        process_config,
    )
    .await;
    let _ = tokio::fs::remove_file(embedded_preview_path).await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(output_path).await;
    }
    result.map_err(|embedded_error| {
        bounded_error_detail(&format!(
            "image conversion failed for {}: {direct_error}; converting the extracted embedded preview also failed: {embedded_error}",
            source_path.display()
        ))
    })
}

async fn generate_image_variant(
    source_path: &Path,
    output_path: &Path,
    maximum_size: u32,
    quality: u8,
    variant: ImageVariant,
    process_config: &MediaProcessConfig,
) -> Result<(), String> {
    if let Err(error) = validate_image_dimensions(source_path, process_config).await {
        error!(
            source_path = %source_path.display(),
            error = %error,
            "Image input failed dimension validation"
        );
        return Err(format!(
            "identify could not validate {}: {error}",
            source_path.display()
        ));
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

    run_command("magick", arguments, 0, process_config)
        .await
        .map_err(|error| {
            format!(
                "magick could not convert {} to {}: {error}",
                source_path.display(),
                output_path.display()
            )
        })?;
    if !output_path.is_file() {
        return Err(format!(
            "magick reported success but did not create {} from {}",
            output_path.display(),
            source_path.display()
        ));
    }
    Ok(())
}

async fn extract_embedded_image_preview(
    source_path: &Path,
    output_path: &Path,
    process_config: &MediaProcessConfig,
) -> Result<(), String> {
    let mut failures = Vec::new();
    for preview_tag in ["PreviewImage", "JpgFromRaw", "ThumbnailImage"] {
        let command_output = ExternalProcess::new(
            "exiftool",
            vec![
                OsString::from("-b"),
                OsString::from(format!("-{preview_tag}")),
                source_path.as_os_str().to_os_string(),
            ],
            process_config.maximum_normalized_image_output_bytes,
            process_config.maximum_stderr_bytes,
        )
        .run()
        .await;
        let extracted = match command_output {
            Ok(output)
                if output.status.success()
                    && !output.stdout.is_empty()
                    && !output.stdout_truncated =>
            {
                tokio::fs::write(output_path, output.stdout)
                    .await
                    .map_err(|error| {
                        format!(
                            "failed to write exiftool {preview_tag} output to {}: {error}",
                            output_path.display()
                        )
                    })?;
                true
            }
            Ok(output) if !output.status.success() => {
                failures.push(format!(
                    "{preview_tag}: {}",
                    output.failure_detail("exiftool")
                ));
                false
            }
            Ok(output) if output.stdout_truncated => {
                failures.push(format!(
                    "{preview_tag}: exiftool output exceeded {} bytes",
                    process_config.maximum_normalized_image_output_bytes
                ));
                false
            }
            Ok(_) => {
                failures.push(format!("{preview_tag}: no embedded image present"));
                false
            }
            Err(error) => {
                failures.push(format!(
                    "{preview_tag}: failed to execute exiftool: {error}"
                ));
                false
            }
        };
        if extracted {
            return Ok(());
        }
        let _ = tokio::fs::remove_file(output_path).await;
    }

    let detail = bounded_error_detail(&format!(
        "exiftool could not extract an embedded preview from {}: {}",
        source_path.display(),
        failures.join("; ")
    ));
    error!(
        executable = "exiftool",
        input_path = %source_path.display(),
        error = %detail,
        "Embedded image extraction failed"
    );
    Err(detail)
}

async fn extract_video_frame(
    source_path: &Path,
    output_path: &Path,
    video_frame_quality: u8,
    process_config: &MediaProcessConfig,
) -> Result<(), String> {
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

    run_command("ffmpeg", arguments, 0, process_config)
        .await
        .map_err(|error| {
            format!(
                "ffmpeg could not extract a frame from {} to {}: {error}",
                source_path.display(),
                output_path.display()
            )
        })?;
    if !output_path.is_file() {
        return Err(format!(
            "ffmpeg reported success but did not create {} from {}",
            output_path.display(),
            source_path.display()
        ));
    }
    Ok(())
}
