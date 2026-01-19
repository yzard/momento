use chrono::{DateTime, NaiveDateTime, Utc};
use image::ImageReader;
use serde::Deserialize;
use std::path::Path;
use std::process::Command;

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
    pub iso: Option<i32>,
    pub exposure_time: Option<String>,
    pub f_number: Option<f64>,
    pub focal_length: Option<f64>,
    pub keywords: Option<String>,
    pub duration_seconds: Option<f64>,
    pub mime_type: Option<String>,
    pub location_state: Option<String>,
    pub location_country: Option<String>,
}

fn fallback_to_mtime(file_path: &Path) -> Option<DateTime<Utc>> {
    file_path
        .metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .map(|t| DateTime::<Utc>::from(t))
}

pub fn extract_image_metadata(file_path: &Path) -> MediaMetadata {
    let mut metadata = MediaMetadata::default();

    // Try to read basic image info
    match ImageReader::open(file_path) {
        Ok(reader) => {
            if let Ok(img) = reader.decode() {
                metadata.width = Some(img.width() as i32);
                metadata.height = Some(img.height() as i32);
            }
        }
        Err(_) => {
            metadata.date_taken = fallback_to_mtime(file_path);
            return metadata;
        }
    }

    // Determine MIME type from extension
    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    metadata.mime_type = Some(match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" | "heif" => "image/heic",
        "tiff" => "image/tiff",
        "bmp" => "image/bmp",
        _ => "image/jpeg",
    }.to_string());

    // Try to read EXIF data using exiftool (more reliable than pure Rust)
    if let Ok(output) = Command::new("exiftool")
        .args(["-json", "-n", file_path.to_str().unwrap_or("")])
        .output()
    {
        if output.status.success() {
            if let Ok(json_str) = String::from_utf8(output.stdout) {
                if let Ok(exif_data) = serde_json::from_str::<Vec<ExifToolOutput>>(&json_str) {
                    if let Some(data) = exif_data.into_iter().next() {
                        apply_exif_data(&mut metadata, data);
                    }
                }
            }
        }
    }

    if metadata.date_taken.is_none() {
        metadata.date_taken = fallback_to_mtime(file_path);
    }

    metadata
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ExifToolOutput {
    #[serde(alias = "DateTimeOriginal")]
    date_time_original: Option<String>,
    #[serde(alias = "CreateDate")]
    create_date: Option<String>,
    #[serde(alias = "GPSLatitude")]
    gps_latitude: Option<f64>,
    #[serde(alias = "GPSLongitude")]
    gps_longitude: Option<f64>,
    #[serde(alias = "GPSAltitude")]
    gps_altitude: Option<f64>,
    make: Option<String>,
    model: Option<String>,
    #[serde(alias = "ISO")]
    iso: Option<i32>,
    #[serde(alias = "ExposureTime")]
    exposure_time: Option<f64>,
    #[serde(alias = "FNumber")]
    f_number: Option<f64>,
    #[serde(alias = "FocalLength")]
    focal_length: Option<f64>,
    #[serde(alias = "Keywords")]
    keywords: Option<serde_json::Value>,
    #[serde(alias = "ImageWidth")]
    image_width: Option<i32>,
    #[serde(alias = "ImageHeight")]
    image_height: Option<i32>,
}

fn apply_exif_data(metadata: &mut MediaMetadata, data: ExifToolOutput) {
    // Date
    let date_str = data.date_time_original.or(data.create_date);
    if let Some(date_str) = date_str {
        metadata.date_taken = parse_exif_datetime(&date_str);
    }

    // GPS
    metadata.gps_latitude = data.gps_latitude;
    metadata.gps_longitude = data.gps_longitude;
    metadata.gps_altitude = data.gps_altitude;

    // Camera info
    metadata.camera_make = data.make;
    metadata.camera_model = data.model;
    metadata.iso = data.iso;
    metadata.f_number = data.f_number;
    metadata.focal_length = data.focal_length;

    // Exposure time as string
    if let Some(exp) = data.exposure_time {
        if exp > 0.0 && exp < 1.0 {
            metadata.exposure_time = Some(format!("1/{}", (1.0 / exp).round() as i32));
        } else {
            metadata.exposure_time = Some(format!("{}", exp));
        }
    }

    // Keywords
    if let Some(kw) = data.keywords {
        metadata.keywords = match kw {
            serde_json::Value::String(s) => Some(s),
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

    // Dimensions from EXIF if not already set
    if metadata.width.is_none() {
        metadata.width = data.image_width;
    }
    if metadata.height.is_none() {
        metadata.height = data.image_height;
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

pub fn extract_video_metadata(file_path: &Path) -> MediaMetadata {
    let mut metadata = MediaMetadata::default();

    // Run ffprobe
    let output = Command::new("ffprobe")
        .args([
            "-v", "quiet",
            "-print_format", "json",
            "-show_format",
            "-show_streams",
            file_path.to_str().unwrap_or(""),
        ])
        .output();

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

    metadata.mime_type = Some(match ext.as_str() {
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "m4v" => "video/x-m4v",
        _ => "video/mp4",
    }.to_string());

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
    if let Some(pos) = lon_str[1..].find(|c: char| c == '+' || c == '-') {
        lon_str = lon_str[..pos + 1].to_string();
    }

    let lat: f64 = lat_str.parse().ok()?;
    let lon: f64 = lon_str.parse().ok()?;

    Some((lat, lon))
}
