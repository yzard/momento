use chrono::{DateTime, Utc};
use std::ffi::OsString;
use std::path::Path;
use tracing::{info, warn};

use crate::config::{Config, MediaProcessConfig};
use crate::constants::{image_mime_type, video_mime_type};
use crate::executor::process::{
    bounded_error_detail, ffprobe_single_thread_arguments, run_storage_media_tool,
    ExternalProcessOutput, MediaTool, StorageChildDescriptor,
};
use crate::io::file::{NormalizedStoragePath, StorageRootId};
use crate::runtime::ExecutorHandles;

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
    pub payload_json: String,
}

#[derive(Debug, Clone)]
pub struct ExtractedMediaMetadata {
    pub metadata: MediaMetadata,
    pub sources: Vec<MetadataSource>,
}

pub async fn generate_media_metadata(
    executors: &ExecutorHandles,
    media_id: i64,
    claim_token: &str,
    config: &Config,
) -> Result<(), String> {
    generation::generate_media_metadata(executors, media_id, claim_token, config).await
}

mod generation;
pub mod reverse_geocoding;

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

pub async fn load_supplemental_metadata_storage(
    executors: &ExecutorHandles,
    storage_root: StorageRootId,
    media_path: &NormalizedStoragePath,
) -> Result<Option<crate::executor::ParsedSupplementalMetadata>, String> {
    const MAXIMUM_SIDECAR_BYTES: u64 = 4 * 1024 * 1024;

    let logical_path = Path::new(media_path.relative_path());
    for candidate in supplemental_metadata_candidates(logical_path) {
        let candidate = NormalizedStoragePath::parse(&candidate.to_string_lossy())
            .map_err(|error| error.to_string())?;
        let (session, snapshot) = match executors
            .file_io
            .open_storage_read_session_durable(storage_root, candidate.clone())
            .await
        {
            Ok(opened) => opened,
            Err(error) if error.kind == crate::executor::ExecutorErrorKind::FileNotFound => {
                continue
            }
            Err(error) => return Err(error.to_string()),
        };
        if snapshot.byte_size == 0 || snapshot.byte_size > MAXIMUM_SIDECAR_BYTES {
            return Err(format!(
                "supplemental metadata {} contains {} bytes; expected 1..={MAXIMUM_SIDECAR_BYTES}",
                candidate.relative_path(),
                snapshot.byte_size
            ));
        }
        let capacity = usize::try_from(snapshot.byte_size)
            .map_err(|_| "supplemental metadata size exceeds this platform".to_string())?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(capacity)
            .map_err(|error| format!("failed to reserve supplemental metadata buffer: {error}"))?;
        let mut session = Some(session);
        loop {
            let (returned_session, chunk) = executors
                .file_io
                .read_storage_session_durable(
                    session.take().ok_or_else(|| {
                        "supplemental metadata session is unavailable".to_string()
                    })?,
                    crate::runtime::FILE_IO_CHUNK_BYTES as usize,
                )
                .await
                .map_err(|error| error.to_string())?;
            session = Some(returned_session);
            if chunk.is_empty() {
                break;
            }
            bytes.extend_from_slice(&chunk);
            if bytes.len() as u64 > snapshot.byte_size {
                return Err("supplemental metadata changed while reading".to_string());
            }
        }
        executors
            .file_io
            .close_storage_session_durable(
                session
                    .take()
                    .ok_or_else(|| "supplemental metadata session is unavailable".to_string())?,
            )
            .await
            .map_err(|error| error.to_string())?;
        if bytes.len() as u64 != snapshot.byte_size {
            return Err("supplemental metadata changed while reading".to_string());
        }
        let metadata = executors
            .cpu
            .parse_supplemental_metadata_durable(bytes)
            .await
            .map_err(|error| {
                format!(
                    "failed to parse supplemental metadata {}: {error}",
                    candidate.relative_path()
                )
            })?;
        return Ok(Some(metadata));
    }
    Ok(None)
}

pub fn apply_supplemental_metadata(
    metadata: &mut MediaMetadata,
    data: &crate::executor::ParsedSupplementalMetadata,
) {
    if metadata.gps_latitude == Some(0.0) && metadata.gps_longitude == Some(0.0) {
        metadata.gps_latitude = None;
        metadata.gps_longitude = None;
    }

    if data.date_taken.is_some() {
        metadata.date_taken = data.date_taken;
    }

    if let Some((latitude, longitude)) = data.gps_latitude.zip(data.gps_longitude) {
        metadata.gps_latitude = Some(latitude);
        metadata.gps_longitude = Some(longitude);
        metadata.location_city = None;
        metadata.location_state = None;
        metadata.location_country = None;
    }
    if data.gps_altitude.is_some() {
        metadata.gps_altitude = data.gps_altitude;
    }

    if data.description.is_some() {
        metadata.keywords = data.description.clone();
    }
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
    executors: &ExecutorHandles,
    file_path: &Path,
    storage_root: StorageRootId,
    storage_path: &NormalizedStoragePath,
    process_config: &MediaProcessConfig,
) -> Result<ExtractedMediaMetadata, String> {
    let mut extracted = extract_exif_metadata(
        executors,
        file_path,
        storage_root,
        storage_path,
        process_config,
    )
    .await?;

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
    executors: &ExecutorHandles,
    file_path: &Path,
    storage_root: StorageRootId,
    storage_path: &NormalizedStoragePath,
    process_config: &MediaProcessConfig,
) -> Result<ExtractedMediaMetadata, String> {
    let mut metadata = MediaMetadata::default();
    let mut sources = Vec::new();

    let output = run_storage_media_tool(
        &executors.cpu,
        &executors.file_io,
        MediaTool::ExifTool,
        exiftool_metadata_arguments(),
        process_config.maximum_metadata_output_bytes,
        process_config.maximum_stderr_bytes,
        vec![StorageChildDescriptor::Read {
            storage_root,
            path: storage_path.clone(),
            child_fd: 10,
        }],
    )
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
    let exif_payload = executors
        .cpu
        .parse_exif_metadata_durable(output.stdout)
        .await
        .map_err(|error| {
            format!(
                "exiftool returned invalid JSON for {}: {error}{command_failure_context}",
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
    apply_exif_data(&mut metadata, &exif_payload);
    normalize_gps_coordinates(&mut metadata);
    sources.push(MetadataSource {
        source_type: MetadataSourceType::ExifTool,
        payload_json: exif_payload.payload_json,
    });
    Ok(ExtractedMediaMetadata { metadata, sources })
}

fn apply_exif_data(metadata: &mut MediaMetadata, data: &crate::executor::ParsedExifMetadata) {
    metadata.date_taken = data.date_taken;
    metadata.gps_latitude = data.gps_latitude;
    metadata.gps_longitude = data.gps_longitude;
    metadata.gps_altitude = data.gps_altitude;
    metadata.camera_make = data.camera_make.clone();
    metadata.camera_model = data.camera_model.clone();
    metadata.lens_make = data.lens_make.clone();
    metadata.lens_model = data.lens_model.clone();
    metadata.iso = data.iso;
    metadata.exposure_time = data.exposure_time.clone();
    metadata.f_number = data.f_number;
    metadata.focal_length = data.focal_length;
    metadata.focal_length_35mm = data.focal_length_35mm;
    metadata.keywords = data.keywords.clone();
    metadata.width = data.width;
    metadata.height = data.height;
    metadata.mime_type = data.mime_type.clone();
}

fn exiftool_metadata_arguments() -> Vec<OsString> {
    let mut arguments = vec![OsString::from("-json"), OsString::from("-n")];
    arguments.extend(
        [
            "DateTimeOriginal",
            "CreateDate",
            "ModifyDate",
            "GPSLatitude",
            "GPSLongitude",
            "GPSAltitude",
            "Make",
            "Model",
            "HostComputer",
            "LensMake",
            "LensModel",
            "LensID",
            "ISO",
            "FNumber",
            "Aperture",
            "FocalLength",
            "FocalLengthIn35mmFormat",
            "FocalLength35efl",
            "ExposureTime",
            "ShutterSpeed",
            "Keywords",
            "ImageWidth",
            "ExifImageWidth",
            "SourceImageWidth",
            "ImageHeight",
            "ExifImageHeight",
            "SourceImageHeight",
            "MIMEType",
        ]
        .into_iter()
        .map(|field| OsString::from(format!("-{field}"))),
    );
    arguments.push(OsString::from("/proc/self/fd/10"));
    arguments
}

pub async fn extract_video_metadata(
    executors: &ExecutorHandles,
    file_path: &Path,
    storage_root: StorageRootId,
    storage_path: &NormalizedStoragePath,
    process_config: &MediaProcessConfig,
) -> Result<ExtractedMediaMetadata, String> {
    let mut extracted = extract_exif_metadata(
        executors,
        file_path,
        storage_root,
        storage_path,
        process_config,
    )
    .await?;

    // Run ffprobe
    let mut arguments = ffprobe_single_thread_arguments();
    arguments.extend([
        OsString::from("-v"),
        OsString::from("error"),
        OsString::from("-print_format"),
        OsString::from("json"),
        OsString::from("-show_format"),
        OsString::from("-show_streams"),
        OsString::from("/proc/self/fd/10"),
    ]);
    let output = run_storage_media_tool(
        &executors.cpu,
        &executors.file_io,
        MediaTool::Ffprobe,
        arguments,
        process_config.maximum_metadata_output_bytes,
        process_config.maximum_stderr_bytes,
        vec![StorageChildDescriptor::Read {
            storage_root,
            path: storage_path.clone(),
            child_fd: 10,
        }],
    )
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
    let ffprobe_payload = executors
        .cpu
        .parse_ffprobe_metadata_durable(output.stdout)
        .await
        .map_err(|error| {
            format!(
                "ffprobe returned invalid JSON for {}: {error}",
                file_path.display()
            )
        })?;
    extracted.sources.push(MetadataSource {
        source_type: MetadataSourceType::Ffprobe,
        payload_json: ffprobe_payload.payload_json.clone(),
    });
    let metadata = &mut extracted.metadata;
    metadata.width = ffprobe_payload.width.or(metadata.width);
    metadata.height = ffprobe_payload.height.or(metadata.height);
    metadata.video_codec = ffprobe_payload.video_codec;
    metadata.duration_seconds = ffprobe_payload.duration_seconds;
    if ffprobe_payload.date_taken.is_some() {
        metadata.date_taken = ffprobe_payload.date_taken;
    }
    if metadata.gps_latitude.is_none() {
        metadata.gps_latitude = ffprobe_payload.gps_latitude;
    }
    if metadata.gps_longitude.is_none() {
        metadata.gps_longitude = ffprobe_payload.gps_longitude;
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
