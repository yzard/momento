use std::collections::{BTreeMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use base64::Engine;
use image::imageops::FilterType;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use sha2::{Digest, Sha256};

use crate::constants::{paths, FACE_DETECTION_MODEL_TYPE};
use crate::database::{queries, DbPool};
use crate::error::{AppError, AppResult};
use crate::processor::ai::input::AiInputStorage;
use crate::utils::embedding::{blob_to_embedding, cosine_similarity};
use crate::utils::path::resolve_storage_path;
use momento_common::llm::JobInputResult;

const EMBEDDING_DIMENSIONS: usize = 512;
const BOUNDING_BOX_EPSILON: f64 = 1e-6;

struct FaceResult {
    sequence: i64,
    index: i64,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    eye_center_x: f64,
    eye_center_y: f64,
    confidence: f64,
    quality: f64,
    frontality: f64,
    embedding: Vec<u8>,
}

type ManualGroupAnchors = BTreeMap<i64, Vec<Vec<f32>>>;

pub struct FaceFileChanges {
    new_paths: Vec<PathBuf>,
    old_paths: Vec<PathBuf>,
    committed: bool,
}

pub struct PreparedFaceDetectionResult {
    media_id: i64,
    model_version: String,
    faces: Vec<(FaceResult, String)>,
    file_changes: FaceFileChanges,
}

impl FaceFileChanges {
    pub fn commit(mut self) {
        self.committed = true;
        for path in &self.old_paths {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl Drop for FaceFileChanges {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for path in &self.new_paths {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub fn prepare_result(
    connection: &Connection,
    job_id: &str,
    media_id: i64,
    model_type: &str,
    model_version: &str,
    input_results: Option<&[JobInputResult]>,
) -> AppResult<PreparedFaceDetectionResult> {
    if model_type != FACE_DETECTION_MODEL_TYPE {
        return Err(AppError::BadRequest(
            "face detection result modelType must be face_detection".to_string(),
        ));
    }
    let input_results = input_results.ok_or_else(|| {
        AppError::BadRequest("face_detection inputResults is required".to_string())
    })?;
    let expected_inputs = connection
        .prepare(queries::faces::SELECT_INPUT_CORRELATION)?
        .query_map([job_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let result_inputs = input_results
        .iter()
        .map(|result| (i64::from(result.sequence), result.frame_timestamp_ms))
        .collect::<Vec<_>>();
    if expected_inputs != result_inputs {
        return Err(AppError::BadRequest(
            "face_detection input correlation does not match prepared inputs".to_string(),
        ));
    }
    let mut faces = Vec::new();
    for input_result in input_results {
        validate_input_result(&input_result.result, model_type, model_version)?;
        let face_values = input_result
            .result
            .get("faces")
            .and_then(|value| value.as_array())
            .ok_or_else(|| AppError::BadRequest("face_detection faces is required".to_string()))?;
        let mut indices = HashSet::new();
        for (expected_index, face_value) in face_values.iter().enumerate() {
            let parsed = parse_face(i64::from(input_result.sequence), face_value)?;
            if !indices.insert(parsed.index) {
                return Err(AppError::BadRequest(
                    "face_detection face indices must be unique".to_string(),
                ));
            }
            if parsed.index != expected_index as i64 {
                return Err(AppError::BadRequest(
                    "face_detection face indices must be contiguous and ordered".to_string(),
                ));
            }
            faces.push(parsed);
        }
    }
    let old_paths = connection
        .prepare(queries::faces::SELECT_MEDIA_CROPS)?
        .query_map([media_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter_map(|path| resolve_storage_path(&paths().previews, &path).ok())
        .collect();
    let mut changes = FaceFileChanges {
        new_paths: Vec::new(),
        old_paths,
        committed: false,
    };
    let mut persisted_faces = Vec::with_capacity(faces.len());
    for face in faces {
        let (crop_path, absolute_path) = write_crop(connection, job_id, media_id, &face)?;
        changes.new_paths.push(absolute_path);
        persisted_faces.push((face, crop_path));
    }
    Ok(PreparedFaceDetectionResult {
        media_id,
        model_version: model_version.to_string(),
        faces: persisted_faces,
        file_changes: changes,
    })
}

pub fn persist_prepared_result(
    connection: &Connection,
    prepared: PreparedFaceDetectionResult,
) -> AppResult<FaceFileChanges> {
    let PreparedFaceDetectionResult {
        media_id,
        model_version,
        faces,
        file_changes,
    } = prepared;
    connection.execute(queries::faces::DELETE_MEDIA_FACES, [media_id])?;
    for (face, crop_path) in faces {
        connection.execute(
            queries::faces::INSERT_FACE,
            rusqlite::params![
                media_id,
                face.sequence,
                face.index,
                face.x,
                face.y,
                face.width,
                face.height,
                face.confidence,
                face.quality,
                face.frontality,
                face.embedding,
                crop_path
            ],
        )?;
    }
    connection.execute(
        queries::faces::UPSERT_RESULT,
        rusqlite::params![media_id, model_version],
    )?;
    Ok(file_changes)
}

fn validate_input_result(
    result: &serde_json::Value,
    model_type: &str,
    model_version: &str,
) -> AppResult<()> {
    if result.get("task").and_then(|value| value.as_str()) != Some(FACE_DETECTION_MODEL_TYPE)
        || result.get("modelType").and_then(|value| value.as_str()) != Some(model_type)
        || result.get("modelVersion").and_then(|value| value.as_str()) != Some(model_version)
    {
        return Err(AppError::BadRequest(
            "face_detection input result model correlation is invalid".to_string(),
        ));
    }
    Ok(())
}

fn parse_face(sequence: i64, value: &serde_json::Value) -> AppResult<FaceResult> {
    let index = value
        .get("index")
        .and_then(|value| value.as_i64())
        .ok_or_else(|| AppError::BadRequest("face index is required".to_string()))?;
    if index < 0 {
        return Err(AppError::BadRequest(
            "faceIndex must be non-negative".to_string(),
        ));
    }
    let box_value = value
        .get("boundingBox")
        .ok_or_else(|| AppError::BadRequest("face box is required".to_string()))?;
    let number = |name: &str| {
        box_value
            .get(name)
            .and_then(|value| value.as_f64())
            .filter(|value| value.is_finite())
            .ok_or_else(|| AppError::BadRequest(format!("face box {name} is invalid")))
    };
    let x = number("x")?;
    let y = number("y")?;
    let reported_width = number("width")?;
    let reported_height = number("height")?;
    if x < 0.0
        || y < 0.0
        || x >= 1.0
        || y >= 1.0
        || reported_width <= 0.0
        || reported_height <= 0.0
        || x + reported_width > 1.0 + BOUNDING_BOX_EPSILON
        || y + reported_height > 1.0 + BOUNDING_BOX_EPSILON
    {
        return Err(AppError::BadRequest(
            "face box must be normalized within the input".to_string(),
        ));
    }
    let width = (x + reported_width).min(1.0) - x;
    let height = (y + reported_height).min(1.0) - y;
    let eye_center = value
        .get("eyeCenter")
        .ok_or_else(|| AppError::BadRequest("face eyeCenter is required".to_string()))?;
    let eye_coordinate = |name: &str| {
        eye_center
            .get(name)
            .and_then(|coordinate| coordinate.as_f64())
            .filter(|coordinate| coordinate.is_finite() && (0.0..=1.0).contains(coordinate))
            .ok_or_else(|| AppError::BadRequest(format!("face eyeCenter {name} is invalid")))
    };
    let scalar = |name: &str| {
        value
            .get(name)
            .and_then(|value| value.as_f64())
            .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
            .ok_or_else(|| AppError::BadRequest(format!("face {name} is invalid")))
    };
    let embedding = value
        .get("embedding")
        .and_then(|value| value.as_str())
        .ok_or_else(|| AppError::BadRequest("face embedding is required".to_string()))?;
    if value
        .get("embeddingEncoding")
        .and_then(|value| value.as_str())
        != Some("float32_le")
    {
        return Err(AppError::BadRequest(
            "face embedding must use float32_le".to_string(),
        ));
    }
    if value
        .get("embeddingDimensions")
        .and_then(|value| value.as_u64())
        != Some(EMBEDDING_DIMENSIONS as u64)
    {
        return Err(AppError::BadRequest(
            "face embeddingDimensions must be 512".to_string(),
        ));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(embedding)
        .map_err(|error| AppError::BadRequest(format!("invalid face embedding: {error}")))?;
    if bytes.len() != EMBEDDING_DIMENSIONS * 4 {
        return Err(AppError::BadRequest(
            "face embedding must contain 512 dimensions".to_string(),
        ));
    }
    let components = blob_to_embedding(&bytes);
    let squared_norm = components
        .iter()
        .map(|component| f64::from(*component) * f64::from(*component))
        .sum::<f64>();
    if components.iter().any(|component| !component.is_finite())
        || (squared_norm.sqrt() - 1.0).abs() > 0.01
    {
        return Err(AppError::BadRequest(
            "face embedding must be normalized and finite".to_string(),
        ));
    }
    Ok(FaceResult {
        sequence,
        index,
        x,
        y,
        width,
        height,
        eye_center_x: eye_coordinate("x")?,
        eye_center_y: eye_coordinate("y")?,
        confidence: scalar("confidence")?,
        quality: scalar("qualityScore")?,
        frontality: scalar("frontalityScore")?,
        embedding: bytes,
    })
}

fn write_crop(
    connection: &Connection,
    job_id: &str,
    media_id: i64,
    face: &FaceResult,
) -> AppResult<(String, PathBuf)> {
    let (storage_root, input_path, expected_size, expected_hash): (String, String, i64, String) =
        connection.query_row(
            queries::faces::SELECT_INPUT_PATH,
            rusqlite::params![job_id, face.sequence],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
    let storage = AiInputStorage::parse(&storage_root).map_err(AppError::Internal)?;
    let input_path = storage
        .resolve_existing_sync(&input_path)
        .map_err(AppError::Internal)?;
    let input_bytes = std::fs::read(&input_path)?;
    if input_bytes.len() as i64 != expected_size
        || format!("{:x}", Sha256::digest(&input_bytes)) != expected_hash
    {
        return Err(AppError::Conflict(
            "prepared face input changed after the job was queued".to_string(),
        ));
    }
    drop(input_bytes);
    let input = normalize_face_input(&input_path, job_id, media_id)?;
    let width = input.width();
    let height = input.height();
    let (crop_x, crop_y, crop_width, crop_height) = portrait_crop_box(
        width,
        height,
        face.eye_center_x,
        face.eye_center_y,
        face.width,
        face.height,
    );
    let crop = input
        .crop_imm(crop_x, crop_y, crop_width, crop_height)
        .resize_exact(256, 256, FilterType::Lanczos3);
    let relative = PathBuf::from("faces")
        .join(media_id.to_string())
        .join(format!("{job_id}-{}-{}.jpg", face.sequence, face.index));
    let output = resolve_storage_path(&paths().previews, &relative.to_string_lossy())?;
    let parent = output
        .parent()
        .ok_or_else(|| AppError::Internal("face crop path has no parent".to_string()))?;
    std::fs::create_dir_all(parent)?;
    crop.save_with_format(&output, image::ImageFormat::Jpeg)
        .map_err(|error| AppError::Internal(error.to_string()))?;
    Ok((relative.to_string_lossy().into_owned(), output))
}

fn normalize_face_input(
    input_path: &Path,
    job_id: &str,
    media_id: i64,
) -> AppResult<image::DynamicImage> {
    let mut source = OsString::from(input_path.as_os_str());
    source.push("[0]");
    let output = Command::new("magick")
        .arg(source)
        .args(["-auto-orient", "png:-"])
        .output();
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            tracing::error!(
                job_id,
                media_id,
                input_path = %input_path.display(),
                error = %error,
                "ImageMagick failed to start while normalizing a face input"
            );
            return Err(AppError::BadRequest(
                "prepared face input could not be normalized".to_string(),
            ));
        }
    };
    if !output.status.success() || output.stdout.is_empty() {
        let detail = String::from_utf8_lossy(&output.stderr)
            .chars()
            .take(4096)
            .collect::<String>();
        tracing::error!(
            job_id,
            media_id,
            input_path = %input_path.display(),
            status = %output.status,
            error = %detail,
            "ImageMagick failed to normalize a face input"
        );
        return Err(AppError::BadRequest(
            "prepared face input could not be normalized".to_string(),
        ));
    }
    image::load_from_memory_with_format(&output.stdout, image::ImageFormat::Png).map_err(|error| {
        tracing::error!(
            job_id,
            media_id,
            input_path = %input_path.display(),
            error = %error,
            "Rust failed to decode the normalized face input"
        );
        AppError::BadRequest("normalized face input could not be decoded".to_string())
    })
}

pub fn portrait_crop_box(
    image_width: u32,
    image_height: u32,
    eye_center_x: f64,
    eye_center_y: f64,
    face_width: f64,
    face_height: f64,
) -> (u32, u32, u32, u32) {
    let image_width_pixels = image_width;
    let image_height_pixels = image_height;
    let image_width = f64::from(image_width_pixels);
    let image_height = f64::from(image_height_pixels);
    let face_width = face_width * image_width;
    let face_height = face_height * image_height;
    let center_x = eye_center_x * image_width;
    let center_y = eye_center_y * image_height;
    let crop_width = (face_width * 2.2)
        .max(face_height * 2.0)
        .min(image_width)
        .max(1.0);
    let crop_height = crop_width.min(image_height).max(1.0);
    let crop_x = (center_x - (crop_width / 2.0))
        .max(0.0)
        .min(image_width - crop_width);
    let crop_y = (center_y - (crop_height / 2.0))
        .max(0.0)
        .min(image_height - crop_height);
    let crop_x = crop_x.floor() as u32;
    let crop_y = crop_y.floor() as u32;
    let crop_width = (crop_width.ceil() as u32).min(image_width_pixels - crop_x);
    let crop_height = (crop_height.ceil() as u32).min(image_height_pixels - crop_y);
    (crop_x, crop_y, crop_width, crop_height)
}

pub fn start(pool: &DbPool, enabled: bool) -> AppResult<usize> {
    if !enabled {
        return Err(AppError::BadRequest(
            "Face detection is disabled".to_string(),
        ));
    }
    let connection = pool.get().map_err(AppError::Pool)?;
    if connection
        .query_row(queries::faces::SELECT_ACTIVE_RUN, [], |row| {
            row.get::<_, i64>(0)
        })
        .optional()?
        .is_some()
    {
        return Ok(0);
    }
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(queries::faces::INSERT_GROUPING_RUN, [])?;
    let run_id = transaction.last_insert_rowid();
    let queued_jobs = transaction.execute(queries::ai_jobs::INSERT_FACE_ELIGIBLE, [run_id])?;
    transaction.execute(queries::ai_jobs::SNAPSHOT_QUEUED_INPUTS, [])?;
    transaction.commit()?;
    Ok(queued_jobs)
}

pub fn finalize_ready_runs(pool: &DbPool, group_similarity_threshold: f32) -> AppResult<()> {
    let connection = pool.get().map_err(AppError::Pool)?;
    let run = connection
        .query_row(queries::faces::SELECT_ACTIVE_RUN, [], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .optional()?;
    let Some((run_id, status)) = run else {
        return Ok(());
    };
    if status == "cancelling" {
        connection.execute(
            queries::faces::MARK_RUN,
            rusqlite::params!["cancelled", Option::<String>::None, run_id],
        )?;
        return Ok(());
    }
    let pending: i64 =
        connection.query_row(queries::faces::COUNT_PENDING_JOBS, [run_id], |row| {
            row.get(0)
        })?;
    if pending != 0 {
        return Ok(());
    }
    let failed_jobs: i64 =
        connection.query_row(queries::faces::COUNT_FAILED_JOBS, [run_id], |row| {
            row.get(0)
        })?;
    let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)?;
    transaction.execute(queries::faces::DELETE_AUTOMATIC_GROUPS, [])?;
    transaction.execute(queries::faces::DELETE_AUTOMATIC_MANUAL_GROUP_MEMBERS, [])?;
    let manual_group_anchors = load_manual_group_anchors(&transaction)?;
    let faces = transaction
        .prepare(queries::faces::SELECT_FACES_FOR_GROUPING)?
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                blob_to_embedding(&row.get::<_, Vec<u8>>(1)?),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut automatic_groups: Vec<(i64, Vec<f32>)> = Vec::new();
    for (face_id, embedding) in faces {
        if let Some(group_id) = matching_manual_group(
            &manual_group_anchors,
            &embedding,
            group_similarity_threshold,
        ) {
            transaction.execute(queries::faces::INSERT_AUTOMATIC_MEMBER, [group_id, face_id])?;
            continue;
        }
        let matching = automatic_groups.iter().position(|(_, representative)| {
            cosine_similarity(&embedding, representative)
                .is_some_and(|score| score >= group_similarity_threshold)
        });
        let group_id = if let Some(index) = matching {
            automatic_groups[index].0
        } else {
            transaction.execute(queries::faces::INSERT_GROUP, [])?;
            let group_id = transaction.last_insert_rowid();
            automatic_groups.push((group_id, embedding));
            group_id
        };
        transaction.execute(queries::faces::INSERT_AUTOMATIC_MEMBER, [group_id, face_id])?;
    }
    for group_id in manual_group_anchors
        .keys()
        .copied()
        .chain(automatic_groups.iter().map(|(group_id, _)| *group_id))
    {
        transaction.execute(queries::faces::UPDATE_GROUP_REPRESENTATIVE, [group_id])?;
    }
    let completion_error = (failed_jobs > 0).then(|| {
        format!(
            "{failed_jobs} face detection jobs failed; groups generated from successful results"
        )
    });
    transaction.execute(
        queries::faces::MARK_RUN,
        rusqlite::params!["completed", completion_error, run_id],
    )?;
    transaction.commit()?;
    Ok(())
}

fn load_manual_group_anchors(transaction: &Transaction<'_>) -> AppResult<ManualGroupAnchors> {
    let anchor_rows = transaction
        .prepare(queries::faces::SELECT_MANUAL_GROUP_ANCHORS)?
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                blob_to_embedding(&row.get::<_, Vec<u8>>(1)?),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut manual_group_anchors = ManualGroupAnchors::new();
    for (group_id, embedding) in anchor_rows {
        manual_group_anchors
            .entry(group_id)
            .or_default()
            .push(embedding);
    }
    Ok(manual_group_anchors)
}

fn matching_manual_group(
    manual_group_anchors: &ManualGroupAnchors,
    embedding: &[f32],
    group_similarity_threshold: f32,
) -> Option<i64> {
    let mut best_match: Option<(i64, f32)> = None;
    for (group_id, anchors) in manual_group_anchors {
        let best_anchor_similarity = anchors
            .iter()
            .filter_map(|anchor| cosine_similarity(embedding, anchor))
            .max_by(f32::total_cmp);
        let Some(similarity) = best_anchor_similarity else {
            continue;
        };
        if similarity < group_similarity_threshold {
            continue;
        }
        let should_replace = best_match.is_none_or(|(best_group_id, best_similarity)| {
            similarity > best_similarity
                || (similarity == best_similarity && *group_id < best_group_id)
        });
        if should_replace {
            best_match = Some((*group_id, similarity));
        }
    }
    best_match.map(|(group_id, _)| group_id)
}

pub fn cancel(pool: &DbPool) -> AppResult<bool> {
    let connection = pool.get().map_err(AppError::Pool)?;
    let transaction = connection.unchecked_transaction()?;
    let cancelled_jobs = transaction.execute(queries::faces::CANCEL_ACTIVE, [])?;
    let cancelled_runs = transaction.execute(queries::faces::REQUEST_CANCEL_RUNS, [])?;
    transaction.commit()?;
    Ok(cancelled_jobs > 0 || cancelled_runs > 0)
}

pub fn clean(pool: &DbPool) -> AppResult<()> {
    let connection = pool.get().map_err(AppError::Pool)?;
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(queries::faces::CLEAN_RUNS, [])?;
    transaction.execute(queries::faces::CLEAN_GROUPS, [])?;
    transaction.execute(queries::faces::CLEAN_FACES, [])?;
    transaction.execute(queries::faces::CLEAN_RESULTS, [])?;
    transaction.execute(queries::faces::CLEAN_JOBS, [])?;
    transaction.commit()?;
    let _ = std::fs::remove_dir_all(paths().previews.join("faces"));
    Ok(())
}

pub fn recover_interrupted_runs(pool: &DbPool) -> AppResult<()> {
    let connection = pool.get().map_err(AppError::Pool)?;
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(queries::faces::QUEUE_RECOVERED_CANCELLATION_SCOPE, [])?;
    transaction.execute(queries::faces::QUEUE_RECOVERED_CANCELLATIONS, [])?;
    transaction.execute(queries::faces::CANCEL_RECOVERED_CANCELLING_JOBS, [])?;
    transaction.execute(queries::faces::FINALIZE_RECOVERED_CANCELLING_RUNS, [])?;
    transaction.commit()?;
    Ok(())
}
