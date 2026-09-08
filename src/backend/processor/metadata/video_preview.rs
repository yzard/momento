use std::{ffi::OsString, time::Duration};

use crate::executor::process::{
    ffmpeg_single_thread_arguments, run_storage_media_tool, MediaTool, StorageChildDescriptor,
};
use crate::processor::thumbnails::StorageMediaFile;
use crate::runtime::ExecutorHandles;

#[cfg(test)]
#[path = "../../../../tests/backend/processor/metadata/video_preview.rs"]
mod tests;

pub(super) fn output_limit(duration_seconds: Option<f64>) -> Result<u64, String> {
    let duration = duration_seconds
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| "MOV preview requires a valid positive video duration".to_string())?;
    // 20 Mbit/s video + 192 kbit/s audio, with muxing/VBV headroom.
    // This is a disk reservation, not an in-memory video buffer.
    (duration.ceil() as u64)
        .checked_mul(3_000_000)
        .and_then(|bytes| bytes.checked_add(16 * 1024 * 1024))
        .map(|bytes| bytes.min(32 * 1024 * 1024 * 1024))
        .ok_or_else(|| "MOV preview output reservation overflowed".to_string())
}

pub(super) async fn generate(
    executors: &ExecutorHandles,
    original: &StorageMediaFile,
    output: &StorageMediaFile,
    maximum_bytes: u64,
    duration_seconds: Option<f64>,
    maximum_stderr_bytes: usize,
) -> Result<(), String> {
    let mut arguments = ffmpeg_single_thread_arguments();
    arguments.extend(
        [
            "-nostdin",
            "-y",
            "-i",
            "/proc/self/fd/10",
            "-map",
            "0:v:0",
            "-map",
            "0:a:0?",
            "-c:v",
            "libx264",
            "-threads",
            "1",
            "-preset",
            "fast",
            "-crf",
            "23",
            "-maxrate",
            "20M",
            "-bufsize",
            "40M",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            // Fragmented MP4 is streamable and needs no faststart second pass
            // reopening a write-only output descriptor. No scale/pad/crop filter.
            "-movflags",
            "+frag_keyframe+empty_moov+default_base_moof",
            "-f",
            "mp4",
            "/proc/self/fd/11",
        ]
        .map(OsString::from),
    );
    let result = run_storage_media_tool(
        &executors.cpu,
        &executors.file_io,
        MediaTool::Ffmpeg {
            validated_media_duration: duration_seconds
                .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok()),
        },
        arguments,
        0,
        maximum_stderr_bytes,
        vec![
            StorageChildDescriptor::Read {
                storage_root: original.storage_root,
                path: original.path.clone(),
                child_fd: 10,
            },
            StorageChildDescriptor::Write {
                storage_root: output.storage_root,
                path: output.path.clone(),
                child_fd: 11,
                rollback_length: 0,
                require_non_empty: true,
                maximum_bytes,
            },
        ],
    )
    .await
    .map_err(|error| format!("MOV preview conversion failed: {error}"))?;
    if !result.status.success() {
        return Err(format!(
            "MOV preview conversion failed: {}",
            result.failure_detail("ffmpeg")
        ));
    }
    Ok(())
}
