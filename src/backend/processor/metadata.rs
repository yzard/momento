use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Deserialize;
use std::path::Path;
use tokio::process::Command;
use tracing::{info, warn};

#[derive(Debug, Default, Clone)]
pub struct MediaMetadata {
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub date_taken: Option<DateTime<Utc>>,
    pub gps_latitude: Option<f64>,
    pub gps_longitude: Option<f64>,
    pub gps_altitude: Option<f64>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens_make: Option<String>,
    pub lens_model: Option<String>,
    pub iso: Option<i32>,
    pub exposure_time: Option<String>,
    pub f_number: Option<f64>,
    pub focal_length: Option<f64>,
    pub keywords: Option<String>,
    pub duration_seconds: Option<f64>,
    pub mime_type: Option<String>,
    pub location_state: Option<String>,
    pub location_country: Option<String>,
    pub location_city: Option<String>,
    pub video_codec: Option<String>,
    pub focal_length_35mm: Option<f64>,
}

fn fallback_to_mtime(file_path: &Path) -> Option<DateTime<Utc>> {
    file_path
        .metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .map(DateTime::<Utc>::from)
}

pub async fn extract_image_metadata(file_path: &Path) -> MediaMetadata {
    let mut metadata = MediaMetadata::default();

    let output = Command::new("exiftool")
        .args(["-json", "-n", file_path.to_str().unwrap_or("")])
        .output()
        .await;

    match output {
        Ok(output) if output.status.success() => match String::from_utf8(output.stdout) {
            Ok(json_str) => match serde_json::from_str::<Vec<serde_json::Value>>(&json_str) {
                Ok(exif_data) => {
                    if let Some(data) = exif_data.first() {
                        apply_exif_data(&mut metadata, data);
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to parse exiftool JSON for {:?}: {}",
                        file_path.file_name().unwrap_or_default(),
                        e
                    );
                }
            },
            Err(e) => {
                warn!(
                    "Failed to read exiftool output for {:?}: {}",
                    file_path.file_name().unwrap_or_default(),
                    e
                );
            }
        },
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(
                "exiftool failed for {:?}: {}",
                file_path.file_name().unwrap_or_default(),
                stderr
            );
        }
        Err(e) => {
            warn!(
                "Failed to run exiftool for {:?}: {}",
                file_path.file_name().unwrap_or_default(),
                e
            );
        }
    }

    if metadata.date_taken.is_none() {
        metadata.date_taken = fallback_to_mtime(file_path);
    }

    if metadata.mime_type.is_none() {
        let ext = file_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        metadata.mime_type = Some(
            match ext.as_str() {
                "jpg" | "jpeg" => "image/jpeg",
                "png" => "image/png",
                "gif" => "image/gif",
                "webp" => "image/webp",
                "heic" | "heif" => "image/heic",
                "tiff" | "tif" => "image/tiff",
                "bmp" => "image/bmp",
                "avif" => "image/avif",
                "svg" => "image/svg+xml",
                _ => "application/octet-stream",
            }
            .to_string(),
        );
    }

    log_extracted_metadata(file_path, &metadata);
    metadata
}

fn apply_exif_data(metadata: &mut MediaMetadata, data: &serde_json::Value) {
    fn get_str(data: &serde_json::Value, keys: &[&str]) -> Option<String> {
        for key in keys {
            if let Some(v) = data.get(key) {
                if let Some(s) = v.as_str() {
                    return Some(s.to_string());
                }
            }
        }
        None
    }

    fn get_i32(data: &serde_json::Value, keys: &[&str]) -> Option<i32> {
        for key in keys {
            if let Some(v) = data.get(key) {
                if let Some(n) = v.as_i64() {
                    return Some(n as i32);
                }
                if let Some(n) = v.as_f64() {
                    return Some(n as i32);
                }
            }
        }
        None
    }

    fn get_f64(data: &serde_json::Value, keys: &[&str]) -> Option<f64> {
        for key in keys {
            if let Some(v) = data.get(key) {
                if let Some(n) = v.as_f64() {
                    return Some(n);
                }
                if let Some(n) = v.as_i64() {
                    return Some(n as f64);
                }
            }
        }
        None
    }

    if let Some(date_str) = get_str(data, &["DateTimeOriginal", "CreateDate", "ModifyDate"]) {
        metadata.date_taken = parse_exif_datetime(&date_str);
    }

    metadata.gps_latitude = get_f64(data, &["GPSLatitude"]);
    metadata.gps_longitude = get_f64(data, &["GPSLongitude"]);
    metadata.gps_altitude = get_f64(data, &["GPSAltitude"]);

    metadata.camera_make = get_str(data, &["Make"]);
    metadata.camera_model = get_str(data, &["Model", "HostComputer"]);
    metadata.lens_make = get_str(data, &["LensMake"]);
    metadata.lens_model = get_str(data, &["LensModel", "LensID"]);

    metadata.iso = get_i32(data, &["ISO"]);
    metadata.f_number = get_f64(data, &["FNumber", "Aperture"]);
    metadata.focal_length = get_f64(data, &["FocalLength"]);
    metadata.focal_length_35mm = get_f64(data, &["FocalLengthIn35mmFormat", "FocalLength35efl"]);

    if let Some(exp) = get_f64(data, &["ExposureTime", "ShutterSpeed"]) {
        if exp > 0.0 && exp < 1.0 {
            metadata.exposure_time = Some(format!("1/{}", (1.0 / exp).round() as i32));
        } else {
            metadata.exposure_time = Some(format!("{}", exp));
        }
    }

    if let Some(kw) = data.get("Keywords") {
        metadata.keywords = match kw {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Array(arr) => {
                let strs: Vec<String> = arr
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
                if strs.is_empty() {
                    None
                } else {
                    Some(strs.join(","))
                }
            }
            _ => None,
        };
    }

    metadata.width = get_i32(data, &["ImageWidth", "ExifImageWidth", "SourceImageWidth"]);
    metadata.height = get_i32(
        data,
        &["ImageHeight", "ExifImageHeight", "SourceImageHeight"],
    );

    if let Some(mime) = get_str(data, &["MIMEType"]) {
        metadata.mime_type = Some(mime);
    }
}

fn parse_exif_datetime(dt_str: &str) -> Option<DateTime<Utc>> {
    // Try common formats
    let formats = [
        "%Y:%m:%d %H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
        "%Y:%m:%d",
        "%Y-%m-%d",
    ];

    let clean_str = dt_str.trim();
    for fmt in &formats {
        if let Ok(naive) = NaiveDateTime::parse_from_str(clean_str, fmt) {
            return Some(DateTime::from_naive_utc_and_offset(naive, Utc));
        }
    }

    None
}

pub async fn extract_video_metadata(file_path: &Path) -> MediaMetadata {
    let mut metadata = MediaMetadata::default();

    let exif_output = Command::new("exiftool")
        .args(["-json", "-n", file_path.to_str().unwrap_or("")])
        .output()
        .await;

    match exif_output {
        Ok(output) if output.status.success() => match String::from_utf8(output.stdout) {
            Ok(json_str) => match serde_json::from_str::<Vec<serde_json::Value>>(&json_str) {
                Ok(exif_data) => {
                    if let Some(data) = exif_data.first() {
                        apply_exif_data(&mut metadata, data);
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to parse exiftool JSON for {:?}: {}",
                        file_path.file_name().unwrap_or_default(),
                        e
                    );
                }
            },
            Err(e) => {
                warn!(
                    "Failed to read exiftool output for {:?}: {}",
                    file_path.file_name().unwrap_or_default(),
                    e
                );
            }
        },
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(
                "exiftool failed for {:?}: {}",
                file_path.file_name().unwrap_or_default(),
                stderr
            );
        }
        Err(e) => {
            warn!(
                "Failed to run exiftool for {:?}: {}",
                file_path.file_name().unwrap_or_default(),
                e
            );
        }
    }

    // Run ffprobe
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            file_path.to_str().unwrap_or(""),
        ])
        .output()
        .await;

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => {
            metadata.date_taken = fallback_to_mtime(file_path);
            metadata.duration_seconds = Some(0.0);
            return metadata;
        }
    };

    let json_str = match String::from_utf8(output.stdout) {
        Ok(s) => s,
        Err(_) => {
            metadata.date_taken = fallback_to_mtime(file_path);
            return metadata;
        }
    };

    let ffprobe_data: FfprobeOutput = match serde_json::from_str(&json_str) {
        Ok(d) => d,
        Err(_) => {
            metadata.date_taken = fallback_to_mtime(file_path);
            return metadata;
        }
    };

    // Extract video stream info
    if let Some(streams) = ffprobe_data.streams {
        for stream in streams {
            if stream.codec_type.as_deref() == Some("video") {
                metadata.width = stream.width;
                metadata.height = stream.height;
                metadata.video_codec = stream.codec_name;
                break;
            }
        }
    }

    // Extract format info
    if let Some(format) = ffprobe_data.format {
        // Duration
        if let Some(duration) = format.duration {
            metadata.duration_seconds = duration.parse().ok();
        }

        // Tags
        if let Some(tags) = format.tags {
            // Creation time
            let creation_time = tags.creation_time.or(tags.com_apple_quicktime_creationdate);
            if let Some(ct) = creation_time {
                let clean_ct = ct.replace("Z", "+00:00");
                if let Ok(dt) = DateTime::parse_from_rfc3339(&clean_ct) {
                    metadata.date_taken = Some(dt.with_timezone(&Utc));
                }
            }

            // Location
            let location = tags.location.or(tags.com_apple_quicktime_location_iso6709);
            if let Some(loc) = location {
                if let Some((lat, lon)) = parse_iso6709_location(&loc) {
                    metadata.gps_latitude = Some(lat);
                    metadata.gps_longitude = Some(lon);
                }
            }
        }
    }

    // Fallback date
    if metadata.date_taken.is_none() {
        metadata.date_taken = fallback_to_mtime(file_path);
    }

    // MIME type from extension
    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    metadata.mime_type = Some(
        match ext.as_str() {
            "mp4" => "video/mp4",
            "mov" => "video/quicktime",
            "avi" => "video/x-msvideo",
            "mkv" => "video/x-matroska",
            "webm" => "video/webm",
            "m4v" => "video/x-m4v",
            _ => "video/mp4",
        }
        .to_string(),
    );

    log_extracted_metadata(file_path, &metadata);
    metadata
}

#[derive(Debug, Deserialize)]
struct FfprobeOutput {
    streams: Option<Vec<FfprobeStream>>,
    format: Option<FfprobeFormat>,
}

#[derive(Debug, Deserialize)]
struct FfprobeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<i32>,
    height: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct FfprobeFormat {
    duration: Option<String>,
    tags: Option<FfprobeTags>,
}

#[derive(Debug, Deserialize)]
struct FfprobeTags {
    creation_time: Option<String>,
    #[serde(rename = "com.apple.quicktime.creationdate")]
    com_apple_quicktime_creationdate: Option<String>,
    location: Option<String>,
    #[serde(rename = "com.apple.quicktime.location.ISO6709")]
    com_apple_quicktime_location_iso6709: Option<String>,
}

fn log_extracted_metadata(file_path: &Path, metadata: &MediaMetadata) {
    let mut fields = Vec::new();

    if let Some(w) = metadata.width {
        fields.push(format!("width={}", w));
    }
    if let Some(h) = metadata.height {
        fields.push(format!("height={}", h));
    }
    if let Some(ref dt) = metadata.date_taken {
        fields.push(format!("date_taken={}", dt.to_rfc3339()));
    }
    if let Some(lat) = metadata.gps_latitude {
        fields.push(format!("gps_latitude={:.6}", lat));
    }
    if let Some(lon) = metadata.gps_longitude {
        fields.push(format!("gps_longitude={:.6}", lon));
    }
    if let Some(alt) = metadata.gps_altitude {
        fields.push(format!("gps_altitude={:.2}", alt));
    }
    if let Some(ref make) = metadata.camera_make {
        fields.push(format!("camera_make={}", make));
    }
    if let Some(ref model) = metadata.camera_model {
        fields.push(format!("camera_model={}", model));
    }
    if let Some(ref make) = metadata.lens_make {
        fields.push(format!("lens_make={}", make));
    }
    if let Some(ref model) = metadata.lens_model {
        fields.push(format!("lens_model={}", model));
    }
    if let Some(iso) = metadata.iso {
        fields.push(format!("iso={}", iso));
    }
    if let Some(ref exp) = metadata.exposure_time {
        fields.push(format!("exposure_time={}", exp));
    }
    if let Some(f) = metadata.f_number {
        fields.push(format!("f_number={:.1}", f));
    }
    if let Some(fl) = metadata.focal_length {
        fields.push(format!("focal_length={:.1}mm", fl));
    }
    if let Some(fl35) = metadata.focal_length_35mm {
        fields.push(format!("focal_length_35mm={:.1}mm", fl35));
    }
    if let Some(dur) = metadata.duration_seconds {
        fields.push(format!("duration={:.2}s", dur));
    }
    if let Some(ref mime) = metadata.mime_type {
        fields.push(format!("mime_type={}", mime));
    }
    if let Some(ref codec) = metadata.video_codec {
        fields.push(format!("video_codec={}", codec));
    }
    if let Some(ref city) = metadata.location_city {
        fields.push(format!("location_city={}", city));
    }
    if let Some(ref state) = metadata.location_state {
        fields.push(format!("location_state={}", state));
    }
    if let Some(ref country) = metadata.location_country {
        fields.push(format!("location_country={}", country));
    }
    if let Some(ref kw) = metadata.keywords {
        fields.push(format!("keywords={}", kw));
    }

    info!(
        "Extracted metadata from {:?}: [{}]",
        file_path.file_name().unwrap_or_default(),
        fields.join(", ")
    );
}

fn parse_iso6709_location(location: &str) -> Option<(f64, f64)> {
    let location = location.trim_end_matches('/');
    if location.len() < 2 {
        return None;
    }

    // Find second +/- after position 1
    let chars: Vec<char> = location.chars().collect();
    let mut split_idx = 0;

    for (i, &c) in chars.iter().enumerate().skip(1) {
        if c == '+' || c == '-' {
            split_idx = i;
            break;
        }
    }

    if split_idx == 0 {
        return None;
    }

    let lat_str: String = chars[..split_idx].iter().collect();
    let mut lon_str: String = chars[split_idx..].iter().collect();

    // Handle altitude suffix
    if let Some(pos) = lon_str[1..].find(['+', '-']) {
        lon_str = lon_str[..pos + 1].to_string();
    }

    let lat: f64 = lat_str.parse().ok()?;
    let lon: f64 = lon_str.parse().ok()?;

    Some((lat, lon))
}
