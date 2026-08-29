use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Deserialize;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use tracing::{info, warn};

use crate::config::{Config, MediaProcessConfig};
use crate::constants::{image_mime_type, video_mime_type};
use crate::database::DbPool;
use crate::utils::process::{bounded_error_detail, ExternalProcess, ExternalProcessOutput};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataSourceType {
    ExifTool,
    Ffprobe,
    SupplementalSidecar,
}

impl MetadataSourceType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExifTool => "exiftool",
            Self::Ffprobe => "ffprobe",
            Self::SupplementalSidecar => "supplemental_sidecar",
        }
    }
}

#[derive(Debug, Clone)]
pub struct MetadataSource {
    pub source_type: MetadataSourceType,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct ExtractedMediaMetadata {
    pub metadata: MediaMetadata,
    pub sources: Vec<MetadataSource>,
}

pub async fn generate_media_metadata(
    pool: &DbPool,
    media_id: i64,
    config: &Config,
) -> Result<(), String> {
    generation::generate_media_metadata(pool, media_id, config).await
}

mod generation;
pub mod reverse_geocoding;

pub fn supplemental_metadata_path(file_path: &Path) -> Option<std::path::PathBuf> {
    supplemental_metadata_candidates(file_path)
        .into_iter()
        .find(|path| path.is_file())
}

pub(crate) fn supplemental_metadata_candidates(file_path: &Path) -> Vec<std::path::PathBuf> {
    const SUPPLEMENTAL_METADATA_SUFFIX: &str = ".supplemental-metadata.json";

    let Some(file_name) = file_path.file_name().and_then(|name| name.to_str()) else {
        return Vec::new();
    };
    let Some(directory) = file_path.parent() else {
        return Vec::new();
    };

    let exact_name = format!("{file_name}{SUPPLEMENTAL_METADATA_SUFFIX}");
    let mut candidate_names = vec![exact_name.clone()];
    push_unique_candidate_name(
        &mut candidate_names,
        takeout_truncated_sidecar_name(&exact_name),
    );

    if let Some((unnumbered_file_name, duplicate_index)) =
        split_takeout_duplicate_filename(file_name)
    {
        // Takeout moves a collision index from the media stem to the end of its sidecar name:
        // photo(2).jpg -> photo.jpg.supplemental-metadata(2).json.
        let unnumbered_sidecar_name =
            format!("{unnumbered_file_name}{SUPPLEMENTAL_METADATA_SUFFIX}");
        push_unique_candidate_name(
            &mut candidate_names,
            format!(
                "{}({duplicate_index}).json",
                unnumbered_sidecar_name.trim_end_matches(".json")
            ),
        );
        let truncated_unnumbered_sidecar_name =
            takeout_truncated_sidecar_name(&unnumbered_sidecar_name);
        push_unique_candidate_name(
            &mut candidate_names,
            format!(
                "{}({duplicate_index}).json",
                truncated_unnumbered_sidecar_name.trim_end_matches(".json")
            ),
        );
    }

    candidate_names
        .into_iter()
        .map(|candidate_name| directory.join(candidate_name))
        .collect()
}

fn push_unique_candidate_name(candidate_names: &mut Vec<String>, candidate_name: String) {
    if !candidate_names.contains(&candidate_name) {
        candidate_names.push(candidate_name);
    }
}

fn takeout_truncated_sidecar_name(sidecar_name: &str) -> String {
    // Takeout preserves the .json suffix while truncating long supplemental filenames.
    const TAKEOUT_SIDECAR_MAX_CHARACTERS: usize = 51;
    const JSON_SUFFIX: &str = ".json";
    const TAKEOUT_SIDECAR_PREFIX_CHARACTERS: usize =
        TAKEOUT_SIDECAR_MAX_CHARACTERS - JSON_SUFFIX.len();

    if sidecar_name.chars().count() <= TAKEOUT_SIDECAR_MAX_CHARACTERS {
        return sidecar_name.to_string();
    }

    let Some(name_without_json) = sidecar_name.strip_suffix(JSON_SUFFIX) else {
        return sidecar_name.to_string();
    };
    let truncated_name = name_without_json
        .chars()
        .take(TAKEOUT_SIDECAR_PREFIX_CHARACTERS)
        .collect::<String>();
    format!("{truncated_name}{JSON_SUFFIX}")
}

fn split_takeout_duplicate_filename(file_name: &str) -> Option<(String, &str)> {
    let extension_index = file_name.rfind('.')?;
    let stem = &file_name[..extension_index];
    let stem_without_closing_parenthesis = stem.strip_suffix(')')?;
    let opening_parenthesis_index = stem_without_closing_parenthesis.rfind('(')?;
    let duplicate_index = &stem_without_closing_parenthesis[opening_parenthesis_index + 1..];
    if duplicate_index.is_empty()
        || !duplicate_index
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        return None;
    }

    let unnumbered_stem = &stem[..opening_parenthesis_index];
    if unnumbered_stem.is_empty() {
        return None;
    }
    Some((
        format!("{unnumbered_stem}{}", &file_name[extension_index..]),
        duplicate_index,
    ))
}

pub fn load_supplemental_metadata(file_path: &Path) -> Option<serde_json::Value> {
    let metadata_path = supplemental_metadata_path(file_path)?;
    let content = fs::read_to_string(&metadata_path).ok()?;
    match serde_json::from_str(&content) {
        Ok(metadata) => Some(metadata),
        Err(error) => {
            warn!(
                "Failed to parse supplemental metadata {:?}: {}",
                metadata_path.file_name().unwrap_or_default(),
                error
            );
            None
        }
    }
}

pub fn apply_supplemental_metadata(metadata: &mut MediaMetadata, data: &serde_json::Value) {
    if metadata.gps_latitude == Some(0.0) && metadata.gps_longitude == Some(0.0) {
        metadata.gps_latitude = None;
        metadata.gps_longitude = None;
    }

    let supplemental_date = data
        .get("photoTakenTime")
        .and_then(|value| value.get("timestamp"))
        .and_then(parse_unix_timestamp)
        .or_else(|| {
            data.get("creationTime")
                .and_then(|value| value.get("timestamp"))
                .and_then(parse_unix_timestamp)
        });
    if supplemental_date.is_some() {
        metadata.date_taken = supplemental_date;
    }

    let geo_data_exif = data.get("geoDataExif");
    let geo_data = data.get("geoData");
    let coordinates = geo_data_exif
        .and_then(gps_pair_from_json)
        .or_else(|| geo_data.and_then(gps_pair_from_json));
    if let Some((latitude, longitude)) = coordinates {
        metadata.gps_latitude = Some(latitude);
        metadata.gps_longitude = Some(longitude);
        metadata.location_city = None;
        metadata.location_state = None;
        metadata.location_country = None;
    }
    let supplemental_altitude = geo_data_exif
        .and_then(|data| json_f64(data.get("altitude")))
        .or_else(|| geo_data.and_then(|data| json_f64(data.get("altitude"))));
    if supplemental_altitude.is_some() {
        metadata.gps_altitude = supplemental_altitude;
    }

    let supplemental_keywords = data
        .get("description")
        .and_then(|value| value.as_str())
        .filter(|description| !description.is_empty())
        .map(str::to_string);
    if supplemental_keywords.is_some() {
        metadata.keywords = supplemental_keywords;
    }
}

fn parse_unix_timestamp(value: &serde_json::Value) -> Option<DateTime<Utc>> {
    let timestamp = value
        .as_i64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))?;
    DateTime::from_timestamp(timestamp, 0)
}

fn json_f64(value: Option<&serde_json::Value>) -> Option<f64> {
    value
        .and_then(|value| value.as_f64())
        .or_else(|| value.and_then(|value| value.as_str()?.parse().ok()))
}

fn gps_pair_from_json(data: &serde_json::Value) -> Option<(f64, f64)> {
    let latitude = json_f64(data.get("latitude"))?;
    let longitude = json_f64(data.get("longitude"))?;
    if !is_valid_gps_pair(latitude, longitude) {
        return None;
    }

    Some((latitude, longitude))
}

fn is_valid_gps_pair(latitude: f64, longitude: f64) -> bool {
    latitude.is_finite()
        && longitude.is_finite()
        && (-90.0..=90.0).contains(&latitude)
        && (-180.0..=180.0).contains(&longitude)
        && latitude != 0.0
        && longitude != 0.0
}

pub fn normalize_gps_coordinates(metadata: &mut MediaMetadata) {
    if metadata.gps_latitude.is_some() != metadata.gps_longitude.is_some() {
        metadata.gps_latitude = None;
        metadata.gps_longitude = None;
        return;
    }
    let Some((latitude, longitude)) = metadata.gps_latitude.zip(metadata.gps_longitude) else {
        return;
    };
    if is_valid_gps_pair(latitude, longitude) {
        return;
    }

    metadata.gps_latitude = None;
    metadata.gps_longitude = None;
}

pub async fn extract_image_metadata(
    file_path: &Path,
    process_config: &MediaProcessConfig,
) -> Result<ExtractedMediaMetadata, String> {
    let mut extracted = extract_exif_metadata(file_path, process_config).await?;

    if extracted.metadata.mime_type.is_none() {
        extracted.metadata.mime_type = Some(
            image_mime_type(file_path)
                .unwrap_or("application/octet-stream")
                .to_string(),
        );
    }

    log_extracted_metadata(file_path, &extracted.metadata);
    Ok(extracted)
}

async fn extract_exif_metadata(
    file_path: &Path,
    process_config: &MediaProcessConfig,
) -> Result<ExtractedMediaMetadata, String> {
    let mut metadata = MediaMetadata::default();
    let mut sources = Vec::new();

    let output = ExternalProcess::new(
        "exiftool",
        vec![
            OsString::from("-json"),
            OsString::from("-n"),
            file_path.as_os_str().to_os_string(),
        ],
        process_config.maximum_metadata_output_bytes,
        process_config.maximum_stderr_bytes,
    )
    .run()
    .await
    .map_err(|error| {
        let detail = format!(
            "failed to execute exiftool for {}: {error}",
            file_path.display()
        );
        tracing::error!(
            executable = "exiftool",
            input_path = %file_path.display(),
            error = %error,
            "Metadata command could not be executed"
        );
        detail
    })?;
    if output.stdout_truncated {
        return Err(format!(
            "exiftool metadata output for {} exceeded {} bytes",
            file_path.display(),
            process_config.maximum_metadata_output_bytes
        ));
    }
    let command_failure_detail = if output.status.success() {
        None
    } else {
        log_metadata_command_failure("exiftool", file_path, &output);
        Some(output.failure_detail("exiftool"))
    };
    let command_failure_context = command_failure_detail
        .as_deref()
        .map(|detail| format!("; {detail}"))
        .unwrap_or_default();
    let json_str = String::from_utf8(output.stdout).map_err(|error| {
        format!(
            "exiftool returned non-UTF-8 JSON for {}: {error}{command_failure_context}",
            file_path.display(),
        )
    })?;
    let exif_data = serde_json::from_str::<Vec<serde_json::Value>>(&json_str).map_err(|error| {
        format!(
            "exiftool returned invalid JSON for {}: {error}{command_failure_context}",
            file_path.display(),
        )
    })?;
    let data = exif_data.first().ok_or_else(|| {
        format!(
            "exiftool returned no metadata record for {}{command_failure_context}",
            file_path.display(),
        )
    })?;
    if let Some(detail) = command_failure_detail {
        warn!(
            input_path = %file_path.display(),
            detail,
            "ExifTool returned usable metadata with a non-success exit status"
        );
    }
    apply_exif_data(&mut metadata, data);
    normalize_gps_coordinates(&mut metadata);
    sources.push(MetadataSource {
        source_type: MetadataSourceType::ExifTool,
        payload: data.clone(),
    });
    Ok(ExtractedMediaMetadata { metadata, sources })
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

pub async fn extract_video_metadata(
    file_path: &Path,
    process_config: &MediaProcessConfig,
) -> Result<ExtractedMediaMetadata, String> {
    let mut extracted = extract_exif_metadata(file_path, process_config).await?;

    // Run ffprobe
    let output = ExternalProcess::new(
        "ffprobe",
        vec![
            OsString::from("-v"),
            OsString::from("error"),
            OsString::from("-print_format"),
            OsString::from("json"),
            OsString::from("-show_format"),
            OsString::from("-show_streams"),
            file_path.as_os_str().to_os_string(),
        ],
        process_config.maximum_metadata_output_bytes,
        process_config.maximum_stderr_bytes,
    )
    .run()
    .await
    .map_err(|error| {
        let detail = format!(
            "failed to execute ffprobe for {}: {error}",
            file_path.display()
        );
        tracing::error!(
            executable = "ffprobe",
            input_path = %file_path.display(),
            error = %error,
            "Metadata command could not be executed"
        );
        detail
    })?;
    if !output.status.success() {
        log_metadata_command_failure("ffprobe", file_path, &output);
        return Err(bounded_error_detail(&format!(
            "ffprobe could not read metadata from {}: {}",
            file_path.display(),
            output.failure_detail("ffprobe")
        )));
    }
    if output.stdout_truncated {
        return Err(format!(
            "ffprobe metadata output for {} exceeded {} bytes",
            file_path.display(),
            process_config.maximum_metadata_output_bytes
        ));
    }
    let json_str = String::from_utf8(output.stdout).map_err(|error| {
        format!(
            "ffprobe returned non-UTF-8 JSON for {}: {error}",
            file_path.display()
        )
    })?;

    let ffprobe_payload: serde_json::Value = serde_json::from_str(&json_str).map_err(|error| {
        format!(
            "ffprobe returned invalid JSON for {}: {error}",
            file_path.display()
        )
    })?;
    extracted.sources.push(MetadataSource {
        source_type: MetadataSourceType::Ffprobe,
        payload: ffprobe_payload.clone(),
    });
    let ffprobe_data: FfprobeOutput = serde_json::from_value(ffprobe_payload).map_err(|error| {
        format!(
            "ffprobe returned an unsupported metadata structure for {}: {error}",
            file_path.display()
        )
    })?;
    let metadata = &mut extracted.metadata;

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

        if let Some(container_metadata) = format.container_metadata {
            // Creation time
            let creation_time = container_metadata
                .creation_time
                .or(container_metadata.com_apple_quicktime_creationdate);
            if let Some(ct) = creation_time {
                let clean_ct = ct.replace("Z", "+00:00");
                if let Ok(dt) = DateTime::parse_from_rfc3339(&clean_ct) {
                    metadata.date_taken = Some(dt.with_timezone(&Utc));
                }
            }

            // Location
            let location = container_metadata
                .location
                .or(container_metadata.com_apple_quicktime_location_iso6709);
            if let Some(loc) = location {
                if let Some((lat, lon)) = parse_iso6709_location(&loc) {
                    if metadata.gps_latitude.is_none() {
                        metadata.gps_latitude = Some(lat);
                    }
                    if metadata.gps_longitude.is_none() {
                        metadata.gps_longitude = Some(lon);
                    }
                }
            }
        }
    }

    metadata.mime_type = Some(
        video_mime_type(file_path)
            .unwrap_or("application/octet-stream")
            .to_string(),
    );

    log_extracted_metadata(file_path, metadata);
    Ok(extracted)
}

fn log_metadata_command_failure(
    executable: &str,
    input_path: &Path,
    output: &ExternalProcessOutput,
) {
    tracing::error!(
        executable,
        input_path = %input_path.display(),
        status = %output.status,
        detail = %output.failure_detail(executable),
        stderr = %output.stderr_text(),
        stderr_truncated = output.stderr_truncated,
        "Metadata command failed"
    );
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
    #[serde(rename = "tags")]
    container_metadata: Option<FfprobeContainerMetadata>,
}

#[derive(Debug, Deserialize)]
struct FfprobeContainerMetadata {
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
