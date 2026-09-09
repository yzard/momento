use std::{ffi::OsString, time::Duration};

use crate::executor::process::{
    ffmpeg_single_thread_arguments, run_storage_media_tool, MediaTool, StorageChildDescriptor,
};
use crate::processor::thumbnails::StorageMediaFile;
use crate::runtime::ExecutorHandles;

#[cfg(test)]
#[path = "../../../../tests/backend/processor/metadata/video_preview.rs"]
mod tests;

pub(super) struct PreviewPlan {
    pub required: bool,
    pub transcode_video: bool,
    pub transcode_audio: bool,
    pub copy_hevc: bool,
    pub audio_stream_ordinal: Option<usize>,
    pub duration_seconds: Option<f64>,
}

impl PreviewPlan {
    pub fn from_probe(probe: &crate::executor::ParsedFfprobeMetadata) -> Result<Self, String> {
        if probe.video_codec.is_none() {
            return Err("video preview probe has no identifiable video stream".into());
        }
        let copy_hevc = probe.video_codec.as_deref() == Some("hevc")
            && matches!(
                (
                    probe.video_profile.as_deref(),
                    probe.pixel_format.as_deref()
                ),
                (Some("Main"), Some("yuv420p" | "yuvj420p"))
                    | (
                        Some("Main 10"),
                        Some("yuv420p" | "yuvj420p" | "yuv420p10le")
                    )
            );
        let transcode_video = !(copy_hevc
            || (probe.video_codec.as_deref() == Some("h264")
                // yuvj420p is FFmpeg's full-range 8-bit 4:2:0 representation,
                // not a different codec/profile requiring re-encoding.
                && matches!(probe.pixel_format.as_deref(), Some("yuv420p" | "yuvj420p"))
                && matches!(
                    probe.video_profile.as_deref(),
                    Some("Constrained Baseline" | "Baseline" | "Main" | "High")
                )));
        let transcode_audio = probe.audio_present && !probe.audio_can_copy();
        if probe.audio_present && probe.audio_stream_ordinal.is_none() {
            return Err("video preview probe is missing the selected audio stream".into());
        }
        if transcode_video
            && !matches!((probe.width, probe.height), (Some(w), Some(h)) if w > 0 && h > 0 && w % 2 == 0 && h % 2 == 0)
        {
            return Err("H.264/yuv420p preview requires even positive pixel dimensions; refusing to resize the original dimensions".into());
        }
        let native_mp4 = matches!(
            probe.major_brand.as_deref().map(str::trim),
            Some("isom" | "iso2" | "mp41" | "mp42" | "avc1")
        );
        Ok(Self {
            required: !native_mp4
                || transcode_video
                || transcode_audio
                || probe.audio_stream_count > 1,
            transcode_video,
            transcode_audio,
            copy_hevc,
            audio_stream_ordinal: probe.audio_stream_ordinal,
            duration_seconds: probe.duration_seconds,
        })
    }

    pub fn output_limit(&self, source_bytes: u64) -> Result<u64, String> {
        let copied = output_limit(source_bytes)?;
        if !self.transcode_video && !self.transcode_audio {
            return Ok(copied);
        }
        let duration = self
            .duration_seconds
            .filter(|d| d.is_finite() && *d > 0.0)
            .ok_or_else(|| {
                "video preview transcoding requires a valid positive duration".to_string()
            })?;
        let rate = if self.transcode_video {
            3_000_000
        } else {
            32_000
        };
        (duration.ceil() as u64)
            .checked_mul(rate)
            .and_then(|bytes| bytes.checked_add(copied))
            .map(|bytes| bytes.min(32 * 1024 * 1024 * 1024))
            .ok_or_else(|| "video preview transcoding reservation overflowed".into())
    }
}

pub(super) fn output_limit(source_bytes: u64) -> Result<u64, String> {
    if source_bytes == 0 {
        return Err("MOV preview requires a nonempty source".to_string());
    }
    // Stream copying preserves the encoded payload. Reserve the source size plus
    // 10% and 16 MiB for fragmented MP4 overhead, not a transcoding bitrate.
    source_bytes
        .checked_add(source_bytes / 10)
        .and_then(|bytes| bytes.checked_add(16 * 1024 * 1024))
        .map(|bytes| bytes.min(32 * 1024 * 1024 * 1024))
        .ok_or_else(|| "MOV preview output reservation overflowed".to_string())
}

pub(super) async fn generate(
    executors: &ExecutorHandles,
    original: &StorageMediaFile,
    output: &StorageMediaFile,
    maximum_bytes: u64,
    plan: &PreviewPlan,
    maximum_stderr_bytes: usize,
) -> Result<(), String> {
    let mut arguments = ffmpeg_single_thread_arguments();
    arguments
        .extend(["-nostdin", "-y", "-i", "/proc/self/fd/10", "-map", "0:v:0"].map(OsString::from));
    if let Some(ordinal) = plan.audio_stream_ordinal {
        arguments.extend([
            OsString::from("-map"),
            OsString::from(format!("0:a:{ordinal}")),
        ]);
    }
    arguments.extend(
        if plan.transcode_video {
            vec![
                "-c:v",
                "libx264",
                "-threads",
                "1",
                "-preset",
                "fast",
                "-crf",
                "23",
                "-pix_fmt",
                "yuv420p",
                "-profile:v",
                "high",
                "-maxrate",
                "20M",
                "-bufsize",
                "40M",
            ]
        } else {
            vec!["-c:v", "copy"]
        }
        .into_iter()
        .map(OsString::from),
    );
    arguments.extend(
        if plan.transcode_audio {
            vec![
                "-c:a",
                "aac",
                "-profile:a",
                "aac_low",
                "-b:a",
                "192k",
                "-ac",
                "2",
                "-ar",
                "48000",
            ]
        } else {
            vec!["-c:a", "copy"]
        }
        .into_iter()
        .map(OsString::from),
    );
    if plan.copy_hevc {
        // Advertise copied HEVC with the broadly recognized MP4 sample entry.
        arguments.extend(["-tag:v", "hvc1"].map(OsString::from));
    }
    tracing::info!(source = %original.path.relative_path(), video_mode = if plan.transcode_video { "h264" } else { "copy" },
        audio_mode = if plan.transcode_audio { "aac" } else { "copy" }, audio_stream_ordinal = ?plan.audio_stream_ordinal, "Generating browser MP4 preview from verified stream encoding");
    arguments.extend(
        [
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
            validated_media_duration: plan
                .duration_seconds
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
    .map_err(|error| format!("video preview generation failed: {error}"))?;
    if !result.status.success() {
        return Err(format!(
            "video preview generation failed: {}",
            result.failure_detail("ffmpeg")
        ));
    }
    Ok(())
}
