use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;
use tracing::error;

#[derive(Clone, Copy)]
enum ImageVariant {
    CroppedThumbnail,
    AspectRatioPreview,
}

async fn run_command(command: &[String], _timeout_seconds: u64) -> bool {
    match Command::new(&command[0]).args(&command[1..]).output().await {
        Ok(output) => {
            if !output.status.success() {
                error!(
                    "Command failed: {:?}\nStderr: {}",
                    command,
                    String::from_utf8_lossy(&output.stderr)
                );
                return false;
            }
            true
        }
        Err(e) => {
            error!("Failed to execute command {:?}: {}", command, e);
            false
        }
    }
}

pub async fn generate_image_thumbnail(
    source_path: &Path,
    output_path: &Path,
    max_size: u32,
    quality: u8,
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
    )
    .await
}

pub async fn generate_video_thumbnail(
    source_path: &Path,
    output_path: &Path,
    max_size: u32,
    quality: u8,
    video_frame_quality: u8,
) -> bool {
    if let Some(parent) = output_path.parent() {
        if tokio::fs::create_dir_all(parent).await.is_err() {
            return false;
        }
    }

    let temp_frame = output_path.with_extension("temp.jpg");
    if !extract_video_frame(source_path, &temp_frame, video_frame_quality).await {
        error!(
            "Failed to extract video frame for thumbnail: {:?}",
            source_path
        );
        return false;
    }

    let success = generate_montage_thumbnail(&temp_frame, output_path, max_size, quality).await;
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
    )
    .await
}

pub async fn generate_video_preview(
    source_path: &Path,
    output_path: &Path,
    max_size: u32,
    quality: u8,
    video_frame_quality: u8,
) -> bool {
    if let Some(parent) = output_path.parent() {
        if tokio::fs::create_dir_all(parent).await.is_err() {
            return false;
        }
    }

    let temporary_frame = output_path.with_extension("temp.jpg");
    if !extract_video_frame(source_path, &temporary_frame, video_frame_quality).await {
        return false;
    }
    let generated = generate_image_preview(&temporary_frame, output_path, max_size, quality).await;
    let _ = tokio::fs::remove_file(&temporary_frame).await;
    generated
}

async fn generate_montage_thumbnail(
    source_path: &Path,
    output_path: &Path,
    max_size: u32,
    quality: u8,
) -> bool {
    generate_image_variant(
        source_path,
        output_path,
        max_size,
        quality,
        ImageVariant::CroppedThumbnail,
    )
    .await
}

async fn generate_image_variant_with_fallback(
    source_path: &Path,
    output_path: &Path,
    maximum_size: u32,
    quality: u8,
    variant: ImageVariant,
) -> bool {
    if generate_image_variant(source_path, output_path, maximum_size, quality, variant).await {
        return true;
    }
    let _ = tokio::fs::remove_file(output_path).await;

    let embedded_preview_path = output_path.with_extension("embedded-preview.jpg");
    if !extract_embedded_image_preview(source_path, &embedded_preview_path).await {
        return false;
    }
    let generated = generate_image_variant(
        &embedded_preview_path,
        output_path,
        maximum_size,
        quality,
        variant,
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
) -> bool {
    let source_input = format!("{}[0]", source_path.to_str().unwrap_or(""));
    let size = format!("{maximum_size}x{maximum_size}");
    let mut command = vec![
        "convert".to_string(),
        source_input,
        "-auto-orient".to_string(),
    ];
    match variant {
        ImageVariant::CroppedThumbnail => command.extend([
            "-thumbnail".to_string(),
            format!("{size}^"),
            "-gravity".to_string(),
            "center".to_string(),
            "-extent".to_string(),
            size,
        ]),
        ImageVariant::AspectRatioPreview => {
            command.extend(["-resize".to_string(), format!("{size}>")]);
        }
    }
    command.extend([
        "-quality".to_string(),
        quality.to_string(),
        output_path.to_string_lossy().into_owned(),
    ]);

    run_command(&command, 60).await && output_path.exists()
}

async fn extract_embedded_image_preview(source_path: &Path, output_path: &Path) -> bool {
    for preview_tag in ["PreviewImage", "JpgFromRaw", "ThumbnailImage"] {
        let Ok(output_file) = std::fs::File::create(output_path) else {
            return false;
        };
        let command_output = Command::new("exiftool")
            .args(["-b", &format!("-{preview_tag}")])
            .arg(source_path)
            .stdout(Stdio::from(output_file))
            .output()
            .await;
        let extracted = command_output
            .as_ref()
            .is_ok_and(|output| output.status.success())
            && std::fs::metadata(output_path).is_ok_and(|metadata| metadata.len() > 0);
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
) -> bool {
    let seek_time = "00:00:00";

    let command = vec![
        "ffmpeg".to_string(),
        "-y".to_string(),
        "-ss".to_string(),
        seek_time.to_string(),
        "-i".to_string(),
        source_path.to_string_lossy().into_owned(),
        "-vframes".to_string(),
        "1".to_string(),
        "-q:v".to_string(),
        video_frame_quality.to_string(),
        output_path.to_string_lossy().into_owned(),
    ];

    run_command(&command, 60).await && output_path.exists()
}
