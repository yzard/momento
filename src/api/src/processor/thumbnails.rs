use std::path::Path;
use tokio::process::Command;
use tracing::error;

async fn run_command(cmd: &[&str], _timeout_secs: u64) -> bool {
    match Command::new(cmd[0]).args(&cmd[1..]).output().await {
        Ok(output) => {
            if !output.status.success() {
                error!(
                    "Command failed: {:?}\nStderr: {}",
                    cmd,
                    String::from_utf8_lossy(&output.stderr)
                );
                return false;
            }
            true
        }
        Err(e) => {
            error!("Failed to execute command {:?}: {}", cmd, e);
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

    generate_montage_thumbnail(source_path, output_path, max_size, quality).await
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

    let source_input = format!("{}[0]", source_path.to_str().unwrap_or(""));
    let cmd = [
        "convert",
        source_input.as_str(),
        "-auto-orient",
        "-resize",
        &format!("{}x{}>", max_size, max_size),
        "-quality",
        &quality.to_string(),
        output_path.to_str().unwrap_or(""),
    ];

    run_command(&cmd, 60).await && output_path.exists()
}

async fn generate_montage_thumbnail(
    source_path: &Path,
    output_path: &Path,
    max_size: u32,
    quality: u8,
) -> bool {
    let resized = format!("{}x{}", max_size, max_size);
    let source_input = format!("{}[0]", source_path.to_str().unwrap_or(""));
    let cmd = [
        "convert",
        source_input.as_str(),
        "-auto-orient",
        "-thumbnail",
        &format!("{}^", resized),
        "-gravity",
        "center",
        "-extent",
        &resized,
        "-quality",
        &quality.to_string(),
        output_path.to_str().unwrap_or(""),
    ];

    run_command(&cmd, 60).await && output_path.exists()
}

async fn extract_video_frame(
    source_path: &Path,
    output_path: &Path,
    video_frame_quality: u8,
) -> bool {
    let seek_time = "00:00:00";

    let cmd = [
        "ffmpeg",
        "-y",
        "-ss",
        seek_time,
        "-i",
        source_path.to_str().unwrap_or(""),
        "-vframes",
        "1",
        "-q:v",
        &video_frame_quality.to_string(),
        output_path.to_str().unwrap_or(""),
    ];

    run_command(&cmd, 60).await && output_path.exists()
}
