use std::path::Path;
use std::process::Command;

fn run_command(cmd: &[&str], _timeout_secs: u64) -> bool {
    match Command::new(cmd[0])
        .args(&cmd[1..])
        .output()
    {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

fn get_video_duration(source_path: &Path) -> f64 {
    let output = Command::new("ffprobe")
        .args([
            "-v", "error",
            "-show_entries", "format=duration",
            "-of", "default=noprint_wrappers=1:nokey=1",
            source_path.to_str().unwrap_or(""),
        ])
        .output();

    match output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout.trim().parse().unwrap_or(0.0)
        }
        Err(_) => 0.0,
    }
}

pub fn generate_image_thumbnail(
    source_path: &Path,
    output_path: &Path,
    max_size: u32,
    quality: u8,
) -> bool {
    if let Some(parent) = output_path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return false;
        }
    }

    generate_montage_thumbnail(source_path, output_path, max_size, quality)
}

pub fn generate_video_thumbnail(
    source_path: &Path,
    output_path: &Path,
    max_size: u32,
    quality: u8,
    video_frame_quality: u8,
) -> bool {
    if let Some(parent) = output_path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return false;
        }
    }

    let temp_frame = output_path.with_extension("temp.jpg");
    if !extract_video_frame(source_path, &temp_frame, video_frame_quality) {
        return false;
    }

    let success = generate_montage_thumbnail(&temp_frame, output_path, max_size, quality);

    let _ = std::fs::remove_file(&temp_frame);

    success
}

pub fn generate_image_preview(
    source_path: &Path,
    output_path: &Path,
    max_size: u32,
    quality: u8,
) -> bool {
    if let Some(parent) = output_path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return false;
        }
    }

    let cmd = [
        "convert",
        source_path.to_str().unwrap_or(""),
        "-auto-orient",
        "-resize",
        &format!("{}x{}>", max_size, max_size),
        "-quality",
        &quality.to_string(),
        output_path.to_str().unwrap_or(""),
    ];

    run_command(&cmd, 60) && output_path.exists()
}

fn generate_montage_thumbnail(
    source_path: &Path,
    output_path: &Path,
    max_size: u32,
    quality: u8,
) -> bool {
    let resized = format!("{}x{}", max_size, max_size);
    let cmd = [
        "convert",
        source_path.to_str().unwrap_or(""),
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

    run_command(&cmd, 60) && output_path.exists()
}

fn extract_video_frame(source_path: &Path, output_path: &Path, video_frame_quality: u8) -> bool {
    let duration = get_video_duration(source_path);
    let seek_time = if duration > 0.0 {
        (duration * 0.1).min(5.0)
    } else {
        0.0
    };

    let cmd = [
        "ffmpeg",
        "-y",
        "-ss",
        &seek_time.to_string(),
        "-i",
        source_path.to_str().unwrap_or(""),
        "-vframes",
        "1",
        "-q:v",
        &video_frame_quality.to_string(),
        output_path.to_str().unwrap_or(""),
    ];

    run_command(&cmd, 60) && output_path.exists()
}
