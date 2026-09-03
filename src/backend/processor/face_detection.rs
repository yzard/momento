use std::ffi::OsString;

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};

use crate::config::{FaceGroupConfig, MediaProcessConfig};
use crate::constants::FACE_DETECTION_MODEL_TYPE;
use crate::database::queries;
use crate::error::{AppError, AppResult};
use crate::executor::process::{
    bounded_error_detail, image_magick_resource_arguments,
    inspect_storage_oriented_image_dimensions, run_storage_media_tool, MediaTool,
    StorageChildDescriptor,
};
use crate::processor::ai::input::AiInputStorage;
use crate::runtime::ExecutorHandles;
use crate::utils::embedding::{blob_to_embedding, cosine_similarity};
use momento_common::llm::result_payload::FacePayload;
use momento_common::llm::result_stream::{ValidatedResultInput, ValidatedResultValue};

const EMBEDDING_DIMENSIONS: usize = 512;
const BOUNDING_BOX_EPSILON: f64 = 1e-6;
const MAXIMUM_FACE_CROP_BYTES: u64 = 1024 * 1024;
const FINALIZATION_PAGE_SIZE: usize = 64;

#[derive(Clone)]
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
    face_size_score: f64,
    frontality_score: f64,
    visibility_score: f64,
    feature_clarity_score: f64,
    embedding: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct FaceRepresentativeCandidate {
    pub id: i64,
    pub crop_path: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub confidence: f64,
    pub face_size_score: f64,
    pub frontality_score: f64,
    pub visibility_score: f64,
    pub feature_clarity_score: f64,
}

#[derive(Debug)]
pub struct MergedFaceGroup {
    pub face_group_id: i64,
    pub face_count: i64,
    pub media_count: i64,
}

#[derive(Debug)]
pub enum MergeFaceGroupsOutcome {
    NotFound,
    Merged(MergedFaceGroup),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceComparisonKind {
    Manual,
    Automatic,
}

impl FaceComparisonKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Automatic => "automatic",
        }
    }

    fn parse(value: &str) -> AppResult<Self> {
        match value {
            "manual" => Ok(Self::Manual),
            "automatic" => Ok(Self::Automatic),
            _ => Err(AppError::Internal(format!(
                "unknown face comparison kind {value}"
            ))),
        }
    }
}

#[derive(Debug)]
pub struct FaceComparisonCandidate {
    pub group_id: i64,
    pub cursor: i64,
    pub embedding: Vec<f32>,
}

#[derive(Debug)]
pub struct FaceComparisonPage {
    pub run_id: i64,
    pub generation_id: i64,
    pub face_id: i64,
    pub kind: FaceComparisonKind,
    pub similarity_threshold: f32,
    pub source_embedding: Vec<f32>,
    pub candidates: Vec<FaceComparisonCandidate>,
    pub exhausted: bool,
}

#[derive(Debug)]
pub struct FaceRepresentativeReductionPage {
    pub run_id: i64,
    pub generation_id: i64,
    pub group_id: i64,
    pub config: FaceGroupConfig,
    pub candidates: Vec<FaceRepresentativeCandidate>,
    pub exhausted: bool,
}

#[derive(Debug)]
pub enum FaceGroupFinalizationWork {
    Idle,
    Progressed,
    Compare(FaceComparisonPage),
    ReduceRepresentative(FaceRepresentativeReductionPage),
}

#[derive(Debug)]
pub enum FaceGroupCpuResult {
    Comparison {
        run_id: i64,
        generation_id: i64,
        face_id: i64,
        kind: FaceComparisonKind,
        candidate_cursor: i64,
        exhausted: bool,
        best_group_id: Option<i64>,
        best_similarity: Option<f32>,
    },
    Representative {
        run_id: i64,
        generation_id: i64,
        group_id: i64,
        candidate_cursor: i64,
        exhausted: bool,
        best_face_id: Option<i64>,
        best_score: Option<f64>,
    },
}

#[derive(Debug)]
struct FaceFinalizationState {
    generation_id: i64,
    phase: String,
    manual_revision: i64,
    face_snapshot_cursor: i64,
    manual_snapshot_cursor: i64,
    face_cursor: i64,
    current_face_id: Option<i64>,
    candidate_kind: FaceComparisonKind,
    candidate_cursor: i64,
    best_group_id: Option<i64>,
    best_similarity: Option<f32>,
    group_cursor: i64,
    current_group_id: Option<i64>,
    representative_cursor: i64,
    best_representative_face_id: Option<i64>,
    best_representative_score: Option<f64>,
    completion_error: Option<String>,
}

struct FaceComparisonCommit {
    run_id: i64,
    generation_id: i64,
    face_id: i64,
    kind: FaceComparisonKind,
    candidate_cursor: i64,
    exhausted: bool,
    page_best: Option<(i64, f32)>,
}

struct FaceRepresentativeCommit {
    run_id: i64,
    generation_id: i64,
    group_id: i64,
    candidate_cursor: i64,
    exhausted: bool,
    page_best: Option<(i64, f64)>,
}

pub struct PreparedFaceDetectionResult {
    media_id: i64,
    model_version: String,
    faces: Vec<(FaceResult, String)>,
    old_crop_paths: Vec<crate::io::file::NormalizedStoragePath>,
    artifact_groups: Vec<crate::processor::artifact::CommittedResultArtifactGroup>,
}

#[derive(Debug)]
pub(crate) struct FaceInputDescriptor {
    sequence: i64,
    frame_timestamp_ms: Option<i64>,
    storage_root: String,
    file_path: String,
    byte_size: i64,
    content_hash: String,
}

#[derive(Debug)]
pub struct FacePreparationContext {
    inputs: Vec<FaceInputDescriptor>,
    old_crop_paths: Vec<String>,
}

pub struct TypedFaceResultPreparationRequest<'a> {
    pub context: FacePreparationContext,
    pub job_id: &'a str,
    pub media_id: i64,
    pub model_type: &'a str,
    pub model_version: &'a str,
    pub input_results: &'a [ValidatedResultInput],
    pub claim_token: &'a str,
    pub product_version: i64,
    pub process_config: &'a MediaProcessConfig,
}

struct TypedFaceArtifactRequest<'a> {
    context: FacePreparationContext,
    job_id: &'a str,
    media_id: i64,
    model_version: &'a str,
    claim_token: &'a str,
    product_version: i64,
    process_config: &'a MediaProcessConfig,
    faces: Vec<FaceResult>,
}

pub fn load_preparation_context_on_connection(
    connection: &Connection,
    job_id: &str,
    media_id: i64,
) -> rusqlite::Result<FacePreparationContext> {
    let inputs = connection
        .prepare(queries::faces::SELECT_PREPARATION_INPUTS)?
        .query_map([job_id], |row| {
            Ok(FaceInputDescriptor {
                sequence: row.get(0)?,
                frame_timestamp_ms: row.get(1)?,
                storage_root: row.get(2)?,
                file_path: row.get(3)?,
                byte_size: row.get(4)?,
                content_hash: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let old_crop_paths = connection
        .prepare(queries::faces::SELECT_MEDIA_CROPS)?
        .query_map([media_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(FacePreparationContext {
        inputs,
        old_crop_paths,
    })
}

pub async fn prepare_typed_result(
    executors: &ExecutorHandles,
    request: TypedFaceResultPreparationRequest<'_>,
) -> AppResult<PreparedFaceDetectionResult> {
    let TypedFaceResultPreparationRequest {
        context,
        job_id,
        media_id,
        model_type,
        model_version,
        input_results,
        claim_token,
        product_version,
        process_config,
    } = request;
    if model_type != FACE_DETECTION_MODEL_TYPE {
        return Err(AppError::BadRequest(
            "face detection result modelType must be face_detection".to_string(),
        ));
    }
    let expected_inputs = context
        .inputs
        .iter()
        .map(|input| (input.sequence, input.frame_timestamp_ms))
        .collect::<Vec<_>>();
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
        let ValidatedResultValue::Faces(input_faces) = &input_result.value else {
            return Err(AppError::BadRequest(
                "face_detection result contains a different task payload".to_string(),
            ));
        };
        for (expected_index, face) in input_faces.iter().enumerate() {
            if face.index != expected_index as u32 {
                return Err(AppError::BadRequest(
                    "face_detection face indices must be contiguous and ordered".to_string(),
                ));
            }
            faces.push(parse_typed_face(i64::from(input_result.sequence), face)?);
        }
    }
    prepare_typed_faces(
        executors,
        TypedFaceArtifactRequest {
            context,
            job_id,
            media_id,
            model_version,
            claim_token,
            product_version,
            process_config,
            faces,
        },
    )
    .await
}

async fn prepare_typed_faces(
    executors: &ExecutorHandles,
    request: TypedFaceArtifactRequest<'_>,
) -> AppResult<PreparedFaceDetectionResult> {
    let TypedFaceArtifactRequest {
        context,
        job_id,
        media_id,
        model_version,
        claim_token,
        product_version,
        process_config,
        faces,
    } = request;
    let old_crop_paths = context
        .old_crop_paths
        .iter()
        .map(|path| {
            crate::io::file::NormalizedStoragePath::parse(path)
                .map_err(|error| AppError::Internal(error.to_string()))
        })
        .collect::<AppResult<Vec<_>>>()?;
    let mut persisted_faces = Vec::with_capacity(faces.len());
    let mut artifact_groups = Vec::new();
    for face_chunk in faces.chunks(crate::io::journal::MAX_FILE_OPERATION_ENTRIES_PER_GROUP) {
        let destinations = face_chunk
            .iter()
            .map(|face| {
                crate::io::file::NormalizedStoragePath::parse(&format!(
                    "faces/{media_id}/{job_id}/v{product_version}/{}-{}.jpg",
                    face.sequence, face.index
                ))
                .map_err(|error| AppError::Internal(error.to_string()))
            })
            .collect::<AppResult<Vec<_>>>()?;
        let batch = match crate::processor::artifact::prepare_result_artifact_batch(
            executors,
            crate::io::file::StorageRootId::Previews,
            destinations.clone(),
            MAXIMUM_FACE_CROP_BYTES,
            job_id,
            claim_token,
            product_version,
        )
        .await
        {
            Ok(batch) => batch,
            Err(error) => {
                discard_committed_result_groups(executors, &artifact_groups).await;
                return Err(AppError::Internal(error));
            }
        };
        for (index, (face, destination_path)) in
            face_chunk.iter().zip(destinations.iter()).enumerate()
        {
            let Some(input) = context
                .inputs
                .iter()
                .find(|input| input.sequence == face.sequence)
            else {
                batch.cancel(executors).await;
                discard_committed_result_groups(executors, &artifact_groups).await;
                return Err(AppError::BadRequest(
                    "face_detection result references an unknown input".to_string(),
                ));
            };
            let Some(temporary_path) = batch.temporary_path(index).cloned() else {
                batch.cancel(executors).await;
                discard_committed_result_groups(executors, &artifact_groups).await;
                return Err(AppError::Internal(
                    "face crop batch index changed".to_string(),
                ));
            };
            if let Err(error) = write_crop(
                executors,
                input,
                job_id,
                media_id,
                face,
                process_config,
                CropOutput::Prepared {
                    destination_path: destination_path.clone(),
                    storage_root: batch.storage_root(),
                    temporary_path,
                },
            )
            .await
            {
                batch.cancel(executors).await;
                discard_committed_result_groups(executors, &artifact_groups).await;
                return Err(error);
            }
            persisted_faces.push((face.clone(), destination_path.relative_path().to_string()));
        }
        match batch.publish_result(executors, product_version).await {
            Ok(group) => artifact_groups.push(group),
            Err(error) => {
                batch.cancel(executors).await;
                discard_committed_result_groups(executors, &artifact_groups).await;
                return Err(AppError::Internal(error));
            }
        }
    }
    Ok(PreparedFaceDetectionResult {
        media_id,
        model_version: model_version.to_string(),
        faces: persisted_faces,
        old_crop_paths,
        artifact_groups,
    })
}

async fn discard_committed_result_groups(
    executors: &ExecutorHandles,
    groups: &[crate::processor::artifact::CommittedResultArtifactGroup],
) {
    for group in groups {
        group.discard(executors).await;
    }
    if !groups.is_empty() {
        executors.scheduler.wake_journal_recovery();
    }
}

enum CropOutput {
    Prepared {
        destination_path: crate::io::file::NormalizedStoragePath,
        storage_root: crate::io::file::StorageRootId,
        temporary_path: crate::io::file::NormalizedStoragePath,
    },
}

pub fn persist_prepared_result(
    connection: &Connection,
    prepared: PreparedFaceDetectionResult,
) -> AppResult<Vec<crate::io::file::NormalizedStoragePath>> {
    let PreparedFaceDetectionResult {
        media_id,
        model_version,
        faces,
        old_crop_paths,
        artifact_groups,
    } = prepared;
    for group in artifact_groups {
        if connection.execute(
            queries::llm_callback::FINALIZE_RESULT_ARTIFACT_GROUP,
            rusqlite::params![group.group_id, group.version, group.product_version],
        )? != 1
        {
            return Err(AppError::Conflict(
                "face result artifact group changed before finalization".to_string(),
            ));
        }
        connection.execute(
            queries::file_operations::RELEASE_GROUP_CLAIMS,
            [&group.group_id],
        )?;
        connection.execute(
            queries::file_operations::RELEASE_GROUP_RESERVATION,
            [&group.group_id],
        )?;
    }
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
                face.face_size_score,
                face.frontality_score,
                face.visibility_score,
                face.feature_clarity_score,
                face.embedding,
                crop_path
            ],
        )?;
    }
    connection.execute(
        queries::faces::UPSERT_RESULT,
        rusqlite::params![media_id, model_version],
    )?;
    Ok(old_crop_paths)
}

fn parse_typed_face(sequence: i64, face: &FacePayload) -> AppResult<FaceResult> {
    let values = [
        face.x,
        face.y,
        face.width,
        face.height,
        face.eye_center_x,
        face.eye_center_y,
        face.confidence,
        face.face_size_score,
        face.frontality_score,
        face.visibility_score,
        face.feature_clarity_score,
    ];
    if values.iter().any(|value| !value.is_finite())
        || face.x < 0.0
        || face.y < 0.0
        || face.x >= 1.0
        || face.y >= 1.0
        || face.width <= 0.0
        || face.height <= 0.0
        || face.x + face.width > 1.0 + BOUNDING_BOX_EPSILON as f32
        || face.y + face.height > 1.0 + BOUNDING_BOX_EPSILON as f32
        || !(0.0..=1.0).contains(&face.eye_center_x)
        || !(0.0..=1.0).contains(&face.eye_center_y)
        || values[6..].iter().any(|value| !(0.0..=1.0).contains(value))
    {
        return Err(AppError::BadRequest(
            "face_detection typed face geometry or score is invalid".to_string(),
        ));
    }
    if face.embedding.len() != EMBEDDING_DIMENSIONS {
        return Err(AppError::BadRequest(
            "face embedding must contain 512 dimensions".to_string(),
        ));
    }
    let squared_norm = face
        .embedding
        .iter()
        .map(|component| f64::from(*component) * f64::from(*component))
        .sum::<f64>();
    if face
        .embedding
        .iter()
        .any(|component| !component.is_finite())
        || (squared_norm.sqrt() - 1.0).abs() > 0.01
    {
        return Err(AppError::BadRequest(
            "face embedding must be normalized and finite".to_string(),
        ));
    }
    let mut embedding = Vec::with_capacity(EMBEDDING_DIMENSIONS * 4);
    for component in &face.embedding {
        embedding.extend_from_slice(&component.to_le_bytes());
    }
    Ok(FaceResult {
        sequence,
        index: i64::from(face.index),
        x: f64::from(face.x),
        y: f64::from(face.y),
        width: f64::from((face.x + face.width).min(1.0) - face.x),
        height: f64::from((face.y + face.height).min(1.0) - face.y),
        eye_center_x: f64::from(face.eye_center_x),
        eye_center_y: f64::from(face.eye_center_y),
        confidence: f64::from(face.confidence),
        face_size_score: f64::from(face.face_size_score),
        frontality_score: f64::from(face.frontality_score),
        visibility_score: f64::from(face.visibility_score),
        feature_clarity_score: f64::from(face.feature_clarity_score),
        embedding,
    })
}

async fn write_crop(
    executors: &ExecutorHandles,
    descriptor: &FaceInputDescriptor,
    job_id: &str,
    media_id: i64,
    face: &FaceResult,
    process_config: &MediaProcessConfig,
    output: CropOutput,
) -> AppResult<crate::io::file::NormalizedStoragePath> {
    let storage = AiInputStorage::parse(&descriptor.storage_root).map_err(AppError::Internal)?;
    let storage_root = storage.storage_root_id();
    let input_path = storage
        .normalized_path(&descriptor.file_path)
        .map_err(AppError::Internal)?;
    let expected_size = u64::try_from(descriptor.byte_size).map_err(|_| {
        AppError::BadRequest("face input byte size must be non-negative".to_string())
    })?;
    crate::processor::ai::verify_prepared_input(
        &executors.file_io,
        &executors.cpu,
        storage_root,
        input_path.clone(),
        expected_size,
        &descriptor.content_hash,
    )
    .await
    .map_err(|error| match error {
        crate::processor::ai::PreparedInputError::Changed => {
            AppError::Conflict("prepared face input changed after the job was queued".to_string())
        }
        other => AppError::Internal(other.to_string()),
    })?;
    let (width, height) = inspect_storage_oriented_image_dimensions(
        &executors.cpu,
        &executors.file_io,
        storage_root,
        input_path.clone(),
        process_config,
    )
    .await
    .map_err(|error| {
        let detail = format!(
            "ImageMagick could not validate face input {}: {error}",
            descriptor.file_path
        );
        tracing::warn!(
            job_id,
            media_id,
            input_path = descriptor.file_path,
            error = %error,
            "Face input failed dimension validation"
        );
        AppError::BadRequest(bounded_error_detail(&detail))
    })?;
    let (crop_x, crop_y, crop_width, crop_height) = portrait_crop_box(
        width,
        height,
        face.eye_center_x,
        face.eye_center_y,
        face.width,
        face.height,
    );
    let CropOutput::Prepared {
        destination_path: relative,
        storage_root: output_storage_root,
        temporary_path: output_path,
    } = output;
    let mut source = OsString::from("/proc/self/fd/10");
    source.push("[0]");
    let mut arguments = image_magick_resource_arguments(process_config);
    arguments.extend([
        source,
        OsString::from("-auto-orient"),
        OsString::from("-crop"),
        OsString::from(format!("{crop_width}x{crop_height}+{crop_x}+{crop_y}")),
        OsString::from("+repage"),
        OsString::from("-filter"),
        OsString::from("Lanczos"),
        OsString::from("-resize"),
        OsString::from("256x256!"),
        OsString::from("-strip"),
        OsString::from("-quality"),
        OsString::from("90"),
        OsString::from("jpeg:/proc/self/fd/11"),
    ]);
    let output = run_storage_media_tool(
        &executors.cpu,
        &executors.file_io,
        MediaTool::ImageMagick,
        arguments,
        0,
        process_config.maximum_stderr_bytes,
        vec![
            StorageChildDescriptor::Read {
                storage_root,
                path: input_path,
                child_fd: 10,
            },
            StorageChildDescriptor::Write {
                storage_root: output_storage_root,
                path: output_path,
                child_fd: 11,
                rollback_length: 0,
                require_non_empty: true,
                maximum_bytes: process_config.maximum_normalized_image_output_bytes as u64,
            },
        ],
    )
    .await;
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            let detail = format!(
                "failed to execute ImageMagick while cropping face input {}: {error}",
                descriptor.file_path
            );
            tracing::error!(
                job_id,
                media_id,
                input_path = descriptor.file_path,
                error = %error,
                "ImageMagick failed to crop a face input"
            );
            return Err(AppError::BadRequest(bounded_error_detail(&detail)));
        }
    };
    if !output.status.success() {
        let stderr = output.stderr_text();
        let detail = format!(
            "ImageMagick could not crop face input {}: {}",
            descriptor.file_path,
            output.failure_detail("convert")
        );
        tracing::error!(
            job_id,
            media_id,
            input_path = descriptor.file_path,
            status = %output.status,
            stderr = %stderr,
            stderr_truncated = output.stderr_truncated,
            "ImageMagick failed to crop a face input"
        );
        return Err(AppError::BadRequest(bounded_error_detail(&detail)));
    }
    Ok(relative)
}

pub async fn retire_replaced_crops(
    executors: &ExecutorHandles,
    paths: Vec<crate::io::file::NormalizedStoragePath>,
) {
    for path in paths {
        if let Err(error) = crate::processor::artifact::retire_artifact(
            executors,
            crate::io::file::StorageRootId::Previews,
            path.clone(),
        )
        .await
        {
            tracing::error!(
                crop_path = path.relative_path(),
                error,
                "Could not journal cleanup for a replaced face crop"
            );
        }
    }
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
    let crop_size = (face_width * 2.2)
        .max(face_height * 2.0)
        .min(image_width)
        .min(image_height)
        .max(1.0);
    let crop_x = (center_x - (crop_size / 2.0))
        .max(0.0)
        .min(image_width - crop_size);
    let crop_y = (center_y - (crop_size / 2.0))
        .max(0.0)
        .min(image_height - crop_size);
    let crop_x = crop_x.floor() as u32;
    let crop_y = crop_y.floor() as u32;
    let crop_width = (crop_size.ceil() as u32).min(image_width_pixels - crop_x);
    let crop_height = (crop_size.ceil() as u32).min(image_height_pixels - crop_y);
    (crop_x, crop_y, crop_width, crop_height)
}

pub fn start(connection: &Connection, enabled: bool) -> AppResult<usize> {
    if !enabled {
        return Err(AppError::BadRequest(
            "Face detection is disabled".to_string(),
        ));
    }
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

pub async fn finalize_ready_runs(
    executors: &ExecutorHandles,
    config: &FaceGroupConfig,
) -> Result<(), crate::executor::ExecutorError> {
    loop {
        let work = executors
            .sqlite
            .load_face_group_finalization_work(config.clone())
            .await?;
        match work {
            FaceGroupFinalizationWork::Idle => return Ok(()),
            FaceGroupFinalizationWork::Progressed => {}
            FaceGroupFinalizationWork::Compare(page) => {
                let result = executors.cpu.compare_face_group_page(page).await?;
                executors
                    .sqlite
                    .commit_face_group_cpu_result(result)
                    .await?;
            }
            FaceGroupFinalizationWork::ReduceRepresentative(page) => {
                let result = executors.cpu.reduce_face_representative_page(page).await?;
                executors
                    .sqlite
                    .commit_face_group_cpu_result(result)
                    .await?;
            }
        }
    }
}

pub async fn recompute_face_representatives(
    executors: &ExecutorHandles,
    config: &FaceGroupConfig,
) -> Result<(), crate::executor::ExecutorError> {
    let mut after_group_id = 0;
    loop {
        let page = executors
            .sqlite
            .load_face_representative_group_page_durable(
                crate::database::operations::FaceRepresentativeGroupPageQuery {
                    after_group_id,
                    limit: FINALIZATION_PAGE_SIZE as u16,
                },
            )
            .await?;
        if page.group_ids.is_empty() {
            return Ok(());
        }

        for group_id in &page.group_ids {
            let mut after_face_id = 0;
            let mut best = None;
            loop {
                let candidate_page = executors
                    .sqlite
                    .load_face_representative_candidate_page_durable(
                        crate::database::operations::FaceRepresentativeCandidatePageQuery {
                            group_id: *group_id,
                            after_face_id,
                            limit: FINALIZATION_PAGE_SIZE as u16,
                        },
                    )
                    .await?;
                if candidate_page.candidates.is_empty() {
                    break;
                }
                let exhausted = candidate_page.exhausted;
                let result = executors
                    .cpu
                    .reduce_face_representative_page(FaceRepresentativeReductionPage {
                        run_id: 0,
                        generation_id: 0,
                        group_id: *group_id,
                        config: config.clone(),
                        candidates: candidate_page.candidates,
                        exhausted,
                    })
                    .await?;
                let FaceGroupCpuResult::Representative {
                    group_id: result_group_id,
                    candidate_cursor,
                    best_face_id,
                    best_score,
                    ..
                } = result
                else {
                    return Err(crate::executor::ExecutorError::new(
                        crate::executor::ExecutorErrorKind::Internal,
                        "recompute_face_representatives",
                        "CPU executor returned a non-representative result",
                    ));
                };
                if result_group_id != *group_id {
                    return Err(crate::executor::ExecutorError::new(
                        crate::executor::ExecutorErrorKind::Internal,
                        "recompute_face_representatives",
                        "CPU executor changed the face group correlation",
                    ));
                }
                best = merge_representative(best, best_face_id.zip(best_score));
                after_face_id = candidate_cursor;
                if exhausted {
                    break;
                }
            }
            executors
                .sqlite
                .update_face_representative_durable(
                    crate::database::operations::UpdateFaceRepresentative {
                        group_id: *group_id,
                        representative_face_id: best.map(|value| value.0),
                    },
                )
                .await?;
        }
        after_group_id = *page.group_ids.last().expect("non-empty group page");
        if page.group_ids.len() < FINALIZATION_PAGE_SIZE {
            return Ok(());
        }
    }
}

pub fn load_finalization_work(
    connection: &Connection,
    config: &FaceGroupConfig,
) -> AppResult<FaceGroupFinalizationWork> {
    if cleanup_finalization_page(connection)? || cleanup_retired_generation_page(connection)? {
        return Ok(FaceGroupFinalizationWork::Progressed);
    }
    let run = connection
        .query_row(queries::faces::SELECT_ACTIVE_RUN, [], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .optional()?;
    let Some((run_id, status)) = run else {
        return Ok(FaceGroupFinalizationWork::Idle);
    };
    if status == "cancelling" {
        cancel_finalization(connection, run_id)?;
        return Ok(FaceGroupFinalizationWork::Progressed);
    }
    let pending: i64 =
        connection.query_row(queries::faces::COUNT_PENDING_JOBS, [run_id], |row| {
            row.get(0)
        })?;
    if pending != 0 {
        return Ok(FaceGroupFinalizationWork::Idle);
    }
    let Some(state) = load_finalization_state(connection, run_id)? else {
        initialize_finalization(connection, run_id)?;
        return Ok(FaceGroupFinalizationWork::Progressed);
    };
    let manual_revision = current_manual_revision(connection)?;
    if manual_revision != state.manual_revision
        && !matches!(state.phase.as_str(), "cleanup" | "restart_cleanup")
    {
        restart_after_manual_change(connection, run_id, state.generation_id)?;
        return Ok(FaceGroupFinalizationWork::Progressed);
    }
    match state.phase.as_str() {
        "face_snapshot" => snapshot_face_page(connection, run_id, &state),
        "manual_snapshot" => snapshot_manual_page(connection, run_id, &state),
        "grouping" => load_comparison_work(connection, run_id, &state, config),
        "representatives" => load_representative_work(connection, run_id, &state, config),
        "publishing" => publish_generation(connection, run_id, &state),
        "cleanup" | "restart_cleanup" => Ok(FaceGroupFinalizationWork::Progressed),
        phase => Err(AppError::Internal(format!(
            "unknown face-group finalization phase {phase}"
        ))),
    }
}

pub fn compare_group_page(page: FaceComparisonPage) -> Result<FaceGroupCpuResult, String> {
    validate_grouping_embedding(&page.source_embedding)?;
    let candidate_cursor = page
        .candidates
        .last()
        .map_or(page.face_id, |candidate| candidate.cursor);
    let mut best_match = None;
    for candidate in page.candidates {
        if validate_grouping_embedding(&candidate.embedding).is_err() {
            continue;
        }
        let Some(similarity) = cosine_similarity(&page.source_embedding, &candidate.embedding)
        else {
            continue;
        };
        if similarity < page.similarity_threshold {
            continue;
        }
        if is_better_group_match(best_match, candidate.group_id, similarity) {
            best_match = Some((candidate.group_id, similarity));
        }
    }
    Ok(FaceGroupCpuResult::Comparison {
        run_id: page.run_id,
        generation_id: page.generation_id,
        face_id: page.face_id,
        kind: page.kind,
        candidate_cursor,
        exhausted: page.exhausted,
        best_group_id: best_match.map(|value| value.0),
        best_similarity: best_match.map(|value| value.1),
    })
}

pub fn reduce_representative_page(
    page: FaceRepresentativeReductionPage,
) -> Result<FaceGroupCpuResult, String> {
    let candidate_cursor = page
        .candidates
        .last()
        .map_or(page.group_id, |candidate| candidate.id);
    let best = select_representative(&page.candidates, &page.config);
    Ok(FaceGroupCpuResult::Representative {
        run_id: page.run_id,
        generation_id: page.generation_id,
        group_id: page.group_id,
        candidate_cursor,
        exhausted: page.exhausted,
        best_face_id: best.map(|candidate| candidate.id),
        best_score: best.map(|candidate| representative_score(candidate, &page.config)),
    })
}

pub fn commit_cpu_result(connection: &Connection, result: FaceGroupCpuResult) -> AppResult<()> {
    match result {
        FaceGroupCpuResult::Comparison {
            run_id,
            generation_id,
            face_id,
            kind,
            candidate_cursor,
            exhausted,
            best_group_id,
            best_similarity,
        } => commit_comparison_page(
            connection,
            FaceComparisonCommit {
                run_id,
                generation_id,
                face_id,
                kind,
                candidate_cursor,
                exhausted,
                page_best: best_group_id.zip(best_similarity),
            },
        ),
        FaceGroupCpuResult::Representative {
            run_id,
            generation_id,
            group_id,
            candidate_cursor,
            exhausted,
            best_face_id,
            best_score,
        } => commit_representative_page(
            connection,
            FaceRepresentativeCommit {
                run_id,
                generation_id,
                group_id,
                candidate_cursor,
                exhausted,
                page_best: best_face_id.zip(best_score),
            },
        ),
    }
}

fn load_finalization_state(
    connection: &Connection,
    run_id: i64,
) -> AppResult<Option<FaceFinalizationState>> {
    connection
        .query_row(queries::faces::SELECT_FINALIZATION_STATE, [run_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, Option<i64>>(9)?,
                row.get::<_, Option<f32>>(10)?,
                row.get::<_, i64>(11)?,
                row.get::<_, Option<i64>>(12)?,
                row.get::<_, i64>(13)?,
                row.get::<_, Option<i64>>(14)?,
                row.get::<_, Option<f64>>(15)?,
                row.get::<_, Option<String>>(16)?,
            ))
        })
        .optional()?
        .map(|row| {
            Ok(FaceFinalizationState {
                generation_id: row.0,
                phase: row.1,
                manual_revision: row.2,
                face_snapshot_cursor: row.3,
                manual_snapshot_cursor: row.4,
                face_cursor: row.5,
                current_face_id: row.6,
                candidate_kind: FaceComparisonKind::parse(&row.7)?,
                candidate_cursor: row.8,
                best_group_id: row.9,
                best_similarity: row.10,
                group_cursor: row.11,
                current_group_id: row.12,
                representative_cursor: row.13,
                best_representative_face_id: row.14,
                best_representative_score: row.15,
                completion_error: row.16,
            })
        })
        .transpose()
}

fn current_manual_revision(connection: &Connection) -> AppResult<i64> {
    Ok(connection.query_row(queries::faces::SELECT_MANUAL_REVISION, [], |row| row.get(0))?)
}

fn initialize_finalization(connection: &Connection, run_id: i64) -> AppResult<()> {
    let failed_jobs: i64 =
        connection.query_row(queries::faces::COUNT_FAILED_JOBS, [run_id], |row| {
            row.get(0)
        })?;
    let completion_error = (failed_jobs > 0).then(|| {
        format!(
            "{failed_jobs} face detection jobs failed; groups generated from successful results"
        )
    });
    let manual_revision = current_manual_revision(connection)?;
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    transaction.execute(queries::faces::INSERT_GENERATION, [run_id])?;
    let generation_id = transaction.last_insert_rowid();
    transaction.execute(
        queries::faces::INSERT_FINALIZATION,
        rusqlite::params![run_id, generation_id, manual_revision, completion_error],
    )?;
    transaction.commit()?;
    Ok(())
}

fn snapshot_face_page(
    connection: &Connection,
    run_id: i64,
    state: &FaceFinalizationState,
) -> AppResult<FaceGroupFinalizationWork> {
    let rows = connection
        .prepare(queries::faces::SELECT_FACE_SNAPSHOT_PAGE)?
        .query_map(
            rusqlite::params![state.face_snapshot_cursor, FINALIZATION_PAGE_SIZE as i64],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    for (face_id, embedding) in &rows {
        transaction.execute(
            queries::faces::INSERT_FINALIZATION_FACE,
            rusqlite::params![state.generation_id, face_id, embedding],
        )?;
    }
    if let Some((face_id, _)) = rows.last() {
        transaction.execute(
            queries::faces::ADVANCE_FACE_SNAPSHOT,
            rusqlite::params![face_id, run_id],
        )?;
    } else {
        transaction.execute(queries::faces::FINISH_FACE_SNAPSHOT, [run_id])?;
    }
    transaction.commit()?;
    Ok(FaceGroupFinalizationWork::Progressed)
}

fn snapshot_manual_page(
    connection: &Connection,
    run_id: i64,
    state: &FaceFinalizationState,
) -> AppResult<FaceGroupFinalizationWork> {
    let rows = connection
        .prepare(queries::faces::SELECT_MANUAL_SNAPSHOT_PAGE)?
        .query_map(
            rusqlite::params![state.manual_snapshot_cursor, FINALIZATION_PAGE_SIZE as i64],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    for (face_id, group_id, embedding) in &rows {
        transaction.execute(
            queries::faces::INSERT_FINALIZATION_MANUAL_ANCHOR,
            rusqlite::params![state.generation_id, face_id, group_id, embedding],
        )?;
    }
    if let Some((face_id, _, _)) = rows.last() {
        transaction.execute(
            queries::faces::ADVANCE_MANUAL_SNAPSHOT,
            rusqlite::params![face_id, run_id],
        )?;
    } else {
        transaction.execute(queries::faces::FINISH_MANUAL_SNAPSHOT, [run_id])?;
    }
    transaction.commit()?;
    Ok(FaceGroupFinalizationWork::Progressed)
}

fn load_comparison_work(
    connection: &Connection,
    run_id: i64,
    state: &FaceFinalizationState,
    config: &FaceGroupConfig,
) -> AppResult<FaceGroupFinalizationWork> {
    let Some(face_id) = state.current_face_id else {
        let next_face_id = connection
            .query_row(
                queries::faces::SELECT_NEXT_FINALIZATION_FACE,
                rusqlite::params![state.generation_id, state.face_cursor],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        if let Some(next_face_id) = next_face_id {
            connection.execute(
                queries::faces::START_FINALIZATION_FACE,
                rusqlite::params![next_face_id, run_id],
            )?;
        } else {
            connection.execute(queries::faces::FINISH_FACE_GROUPING, [run_id])?;
        }
        return Ok(FaceGroupFinalizationWork::Progressed);
    };
    let source_blob: Vec<u8> = connection.query_row(
        queries::faces::SELECT_FINALIZATION_FACE,
        rusqlite::params![state.generation_id, face_id],
        |row| row.get(0),
    )?;
    let candidates = match state.candidate_kind {
        FaceComparisonKind::Manual => connection
            .prepare(queries::faces::SELECT_MANUAL_CANDIDATE_PAGE)?
            .query_map(
                rusqlite::params![
                    state.generation_id,
                    state.candidate_cursor,
                    FINALIZATION_PAGE_SIZE as i64
                ],
                |row| {
                    Ok(FaceComparisonCandidate {
                        group_id: row.get(0)?,
                        cursor: row.get(1)?,
                        embedding: blob_to_embedding(&row.get::<_, Vec<u8>>(2)?),
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?,
        FaceComparisonKind::Automatic => connection
            .prepare(queries::faces::SELECT_AUTOMATIC_CANDIDATE_PAGE)?
            .query_map(
                rusqlite::params![
                    state.generation_id,
                    state.candidate_cursor,
                    FINALIZATION_PAGE_SIZE as i64
                ],
                |row| {
                    Ok(FaceComparisonCandidate {
                        group_id: row.get(0)?,
                        cursor: row.get(0)?,
                        embedding: blob_to_embedding(&row.get::<_, Vec<u8>>(2)?),
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?,
    };
    let exhausted = candidates.len() < FINALIZATION_PAGE_SIZE;
    Ok(FaceGroupFinalizationWork::Compare(FaceComparisonPage {
        run_id,
        generation_id: state.generation_id,
        face_id,
        kind: state.candidate_kind,
        similarity_threshold: config.similarity_threshold,
        source_embedding: blob_to_embedding(&source_blob),
        candidates,
        exhausted,
    }))
}

fn commit_comparison_page(connection: &Connection, request: FaceComparisonCommit) -> AppResult<()> {
    let state = load_finalization_state(connection, request.run_id)?
        .ok_or_else(|| AppError::Conflict("face-group finalization changed".to_string()))?;
    if state.generation_id != request.generation_id
        || state.phase != "grouping"
        || state.current_face_id != Some(request.face_id)
        || state.candidate_kind != request.kind
    {
        return Err(AppError::Conflict(
            "face-group comparison generation changed".to_string(),
        ));
    }
    let best = merge_group_match(
        state.best_group_id.zip(state.best_similarity),
        request.page_best,
    );
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    if !request.exhausted {
        transaction.execute(
            queries::faces::ADVANCE_COMPARISON_PAGE,
            rusqlite::params![
                request.candidate_cursor,
                best.map(|value| value.0),
                best.map(|value| value.1),
                request.run_id,
                request.generation_id,
                request.face_id,
                request.kind.as_str(),
            ],
        )?;
        transaction.commit()?;
        return Ok(());
    }
    if request.kind == FaceComparisonKind::Manual && best.is_none() {
        transaction.execute(
            queries::faces::SWITCH_TO_AUTOMATIC_CANDIDATES,
            rusqlite::params![request.run_id, request.generation_id, request.face_id],
        )?;
        transaction.commit()?;
        return Ok(());
    }
    let group_id = if let Some((group_id, _)) = best {
        group_id
    } else {
        transaction.execute(
            queries::faces::INSERT_GENERATION_GROUP,
            rusqlite::params![request.face_id, request.generation_id],
        )?;
        transaction.last_insert_rowid()
    };
    transaction.execute(
        queries::faces::INSERT_GENERATION_MEMBER,
        rusqlite::params![group_id, request.face_id, request.generation_id],
    )?;
    transaction.execute(
        queries::faces::TRACK_FINALIZATION_GROUP,
        rusqlite::params![request.generation_id, group_id],
    )?;
    transaction.execute(
        queries::faces::FINISH_FINALIZATION_FACE,
        rusqlite::params![
            request.face_id,
            request.run_id,
            request.generation_id,
            request.face_id
        ],
    )?;
    transaction.commit()?;
    Ok(())
}

fn load_representative_work(
    connection: &Connection,
    run_id: i64,
    state: &FaceFinalizationState,
    config: &FaceGroupConfig,
) -> AppResult<FaceGroupFinalizationWork> {
    let Some(group_id) = state.current_group_id else {
        let next_group_id = connection
            .query_row(
                queries::faces::SELECT_NEXT_FINALIZATION_GROUP,
                rusqlite::params![state.generation_id, state.group_cursor],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        if let Some(next_group_id) = next_group_id {
            connection.execute(
                queries::faces::START_REPRESENTATIVE_GROUP,
                rusqlite::params![next_group_id, run_id],
            )?;
        } else {
            connection.execute(queries::faces::FINISH_REPRESENTATIVES, [run_id])?;
        }
        return Ok(FaceGroupFinalizationWork::Progressed);
    };
    let candidates = connection
        .prepare(queries::faces::SELECT_REPRESENTATIVE_CANDIDATE_PAGE)?
        .query_map(
            rusqlite::params![
                state.representative_cursor,
                state.generation_id,
                group_id,
                state.generation_id,
                group_id,
                FINALIZATION_PAGE_SIZE as i64,
            ],
            map_representative_candidate,
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let exhausted = candidates.len() < FINALIZATION_PAGE_SIZE;
    Ok(FaceGroupFinalizationWork::ReduceRepresentative(
        FaceRepresentativeReductionPage {
            run_id,
            generation_id: state.generation_id,
            group_id,
            config: config.clone(),
            candidates,
            exhausted,
        },
    ))
}

fn commit_representative_page(
    connection: &Connection,
    request: FaceRepresentativeCommit,
) -> AppResult<()> {
    let state = load_finalization_state(connection, request.run_id)?
        .ok_or_else(|| AppError::Conflict("face-group finalization changed".to_string()))?;
    if state.generation_id != request.generation_id
        || state.phase != "representatives"
        || state.current_group_id != Some(request.group_id)
    {
        return Err(AppError::Conflict(
            "face representative generation changed".to_string(),
        ));
    }
    let best = merge_representative(
        state
            .best_representative_face_id
            .zip(state.best_representative_score),
        request.page_best,
    );
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    if !request.exhausted {
        transaction.execute(
            queries::faces::ADVANCE_REPRESENTATIVE_PAGE,
            rusqlite::params![
                request.candidate_cursor,
                best.map(|value| value.0),
                best.map(|value| value.1),
                request.run_id,
                request.generation_id,
                request.group_id,
            ],
        )?;
        transaction.commit()?;
        return Ok(());
    }
    let (representative_face_id, representative_score) = best.ok_or_else(|| {
        AppError::Internal(format!(
            "face group {} has no representative candidate",
            request.group_id
        ))
    })?;
    transaction.execute(
        queries::faces::UPDATE_BUILDING_AUTOMATIC_REPRESENTATIVE,
        rusqlite::params![
            representative_face_id,
            request.group_id,
            request.generation_id
        ],
    )?;
    transaction.execute(
        queries::faces::UPSERT_GENERATION_REPRESENTATIVE,
        rusqlite::params![
            request.generation_id,
            request.group_id,
            representative_face_id
        ],
    )?;
    transaction.execute(
        queries::faces::COMPLETE_FINALIZATION_GROUP,
        rusqlite::params![
            representative_face_id,
            representative_score,
            request.generation_id,
            request.group_id,
        ],
    )?;
    transaction.execute(
        queries::faces::FINISH_REPRESENTATIVE_GROUP,
        rusqlite::params![
            request.group_id,
            request.run_id,
            request.generation_id,
            request.group_id
        ],
    )?;
    transaction.commit()?;
    Ok(())
}

fn publish_generation(
    connection: &Connection,
    run_id: i64,
    state: &FaceFinalizationState,
) -> AppResult<FaceGroupFinalizationWork> {
    if current_manual_revision(connection)? != state.manual_revision {
        restart_after_manual_change(connection, run_id, state.generation_id)?;
        return Ok(FaceGroupFinalizationWork::Progressed);
    }
    let incomplete: i64 = connection.query_row(
        queries::faces::COUNT_INCOMPLETE_FINALIZATION_GROUPS,
        [state.generation_id],
        |row| row.get(0),
    )?;
    if incomplete != 0 {
        return Err(AppError::Internal(
            "face-group generation reached publish with unfinished representatives".to_string(),
        ));
    }
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    transaction.execute(
        queries::faces::RETIRE_ACTIVE_GENERATION,
        [state.generation_id],
    )?;
    if transaction.execute(queries::faces::ACTIVATE_GENERATION, [state.generation_id])? == 0 {
        return Err(AppError::Conflict(
            "face-group generation is no longer publishable".to_string(),
        ));
    }
    transaction.execute(
        queries::faces::SWITCH_ACTIVE_GENERATION,
        [state.generation_id],
    )?;
    transaction.execute(
        queries::faces::MARK_RUN,
        rusqlite::params!["completed", state.completion_error, run_id],
    )?;
    transaction.execute(queries::faces::ENTER_FINALIZATION_CLEANUP, [run_id])?;
    transaction.commit()?;
    Ok(FaceGroupFinalizationWork::Progressed)
}

fn restart_after_manual_change(
    connection: &Connection,
    run_id: i64,
    generation_id: i64,
) -> AppResult<()> {
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    transaction.execute(queries::faces::CANCEL_BUILDING_GENERATION, [run_id])?;
    transaction.execute(queries::faces::ENTER_RESTART_CLEANUP, [run_id])?;
    let status: String = transaction.query_row(
        queries::faces::SELECT_GENERATION_STATUS,
        [generation_id],
        |row| row.get(0),
    )?;
    if status != "cancelled" {
        return Err(AppError::Conflict(
            "face-group generation changed during manual-edit restart".to_string(),
        ));
    }
    transaction.commit()?;
    Ok(())
}

fn cancel_finalization(connection: &Connection, run_id: i64) -> AppResult<()> {
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    transaction.execute(queries::faces::CANCEL_BUILDING_GENERATION, [run_id])?;
    transaction.execute(queries::faces::ENTER_RESTART_CLEANUP, [run_id])?;
    transaction.execute(
        queries::faces::MARK_RUN,
        rusqlite::params!["cancelled", Option::<String>::None, run_id],
    )?;
    transaction.commit()?;
    Ok(())
}

fn cleanup_finalization_page(connection: &Connection) -> AppResult<bool> {
    let cleanup = connection
        .query_row(queries::faces::SELECT_FINALIZATION_CLEANUP, [], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .optional()?;
    let Some((run_id, generation_id, _phase)) = cleanup else {
        return Ok(false);
    };
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    let deleted_faces = transaction.execute(
        queries::faces::DELETE_FINALIZATION_FACE_PAGE,
        rusqlite::params![generation_id, FINALIZATION_PAGE_SIZE as i64],
    )?;
    if deleted_faces == 0 {
        let deleted_anchors = transaction.execute(
            queries::faces::DELETE_FINALIZATION_MANUAL_PAGE,
            rusqlite::params![generation_id, FINALIZATION_PAGE_SIZE as i64],
        )?;
        if deleted_anchors == 0 {
            let deleted_groups = transaction.execute(
                queries::faces::DELETE_FINALIZATION_GROUP_PAGE,
                rusqlite::params![generation_id, FINALIZATION_PAGE_SIZE as i64],
            )?;
            if deleted_groups == 0 {
                transaction.execute(queries::faces::DELETE_FINALIZATION, [run_id])?;
            }
        }
    }
    transaction.commit()?;
    Ok(true)
}

fn cleanup_retired_generation_page(connection: &Connection) -> AppResult<bool> {
    let generation_id = connection
        .query_row(queries::faces::SELECT_RETIRED_GENERATION, [], |row| {
            row.get::<_, i64>(0)
        })
        .optional()?;
    let Some(generation_id) = generation_id else {
        return Ok(false);
    };
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    let deleted_members = transaction.execute(
        queries::faces::DELETE_RETIRED_MEMBER_PAGE,
        rusqlite::params![generation_id, FINALIZATION_PAGE_SIZE as i64],
    )?;
    if deleted_members == 0 {
        let deleted_representatives = transaction.execute(
            queries::faces::DELETE_RETIRED_REPRESENTATIVE_PAGE,
            rusqlite::params![generation_id, FINALIZATION_PAGE_SIZE as i64],
        )?;
        if deleted_representatives == 0 {
            let deleted_groups = transaction.execute(
                queries::faces::DELETE_RETIRED_GROUP_PAGE,
                rusqlite::params![generation_id, FINALIZATION_PAGE_SIZE as i64],
            )?;
            if deleted_groups == 0 {
                transaction.execute(queries::faces::DELETE_RETIRED_GENERATION, [generation_id])?;
            }
        }
    }
    transaction.commit()?;
    Ok(true)
}

fn validate_grouping_embedding(embedding: &[f32]) -> Result<(), String> {
    if embedding.len() != EMBEDDING_DIMENSIONS || embedding.iter().any(|value| !value.is_finite()) {
        return Err("face-group embedding must contain 512 finite dimensions".to_string());
    }
    Ok(())
}

fn is_better_group_match(current: Option<(i64, f32)>, group_id: i64, score: f32) -> bool {
    current.is_none_or(|(current_group_id, current_score)| {
        score > current_score || (score == current_score && group_id < current_group_id)
    })
}

fn merge_group_match(
    current: Option<(i64, f32)>,
    candidate: Option<(i64, f32)>,
) -> Option<(i64, f32)> {
    match candidate {
        Some((group_id, score)) if is_better_group_match(current, group_id, score) => {
            Some((group_id, score))
        }
        _ => current,
    }
}

fn merge_representative(
    current: Option<(i64, f64)>,
    candidate: Option<(i64, f64)>,
) -> Option<(i64, f64)> {
    match (current, candidate) {
        (None, candidate) => candidate,
        (current, None) => current,
        (Some(current), Some(candidate)) => {
            if candidate.1 > current.1 || (candidate.1 == current.1 && candidate.0 < current.0) {
                Some(candidate)
            } else {
                Some(current)
            }
        }
    }
}

pub(crate) fn map_representative_candidate(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<FaceRepresentativeCandidate> {
    Ok(FaceRepresentativeCandidate {
        id: row.get(0)?,
        crop_path: row.get(1)?,
        x: row.get(2)?,
        y: row.get(3)?,
        width: row.get(4)?,
        height: row.get(5)?,
        confidence: row.get(6)?,
        face_size_score: row.get(7)?,
        frontality_score: row.get(8)?,
        visibility_score: row.get(9)?,
        feature_clarity_score: row.get(10)?,
    })
}

fn center_proximity(candidate: &FaceRepresentativeCandidate) -> f64 {
    let center_x_offset = candidate.x + candidate.width / 2.0 - 0.5;
    let center_y_offset = candidate.y + candidate.height / 2.0 - 0.5;
    (1.0 - 2.0 * (center_x_offset.powi(2) + center_y_offset.powi(2))).clamp(0.0, 1.0)
}

fn representative_score(candidate: &FaceRepresentativeCandidate, config: &FaceGroupConfig) -> f64 {
    config.confidence_weight * candidate.confidence
        + config.face_size_weight * candidate.face_size_score
        + config.center_proximity_weight * center_proximity(candidate)
        + config.frontality_weight * candidate.frontality_score
        + config.visibility_weight * candidate.visibility_score
        + config.feature_clarity_weight * candidate.feature_clarity_score
}

fn select_representative<'a>(
    candidates: &'a [FaceRepresentativeCandidate],
    config: &FaceGroupConfig,
) -> Option<&'a FaceRepresentativeCandidate> {
    candidates.iter().max_by(|left, right| {
        representative_score(left, config)
            .total_cmp(&representative_score(right, config))
            .then_with(|| right.id.cmp(&left.id))
    })
}

pub fn update_group_representative(
    connection: &Connection,
    group_id: i64,
    config: &FaceGroupConfig,
) -> AppResult<()> {
    let candidates = connection
        .prepare(queries::faces::SELECT_GROUP_REPRESENTATIVE_CANDIDATES)?
        .query_map([group_id], map_representative_candidate)?
        .collect::<Result<Vec<_>, _>>()?;
    let representative_id = select_representative(&candidates, config).map(|face| face.id);
    connection.execute(
        queries::faces::UPDATE_GROUP_REPRESENTATIVE_ID,
        rusqlite::params![representative_id, group_id],
    )?;
    Ok(())
}

pub fn visible_representative_crop(
    connection: &Connection,
    group_id: i64,
    user_id: i64,
    config: &FaceGroupConfig,
) -> rusqlite::Result<Option<String>> {
    let stored_crop = connection
        .query_row(
            queries::faces::SELECT_VISIBLE_STORED_REPRESENTATIVE_CROP,
            rusqlite::params![group_id, user_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if stored_crop.is_some() {
        return Ok(stored_crop);
    }
    let candidates = connection
        .prepare(queries::faces::SELECT_VISIBLE_GROUP_REPRESENTATIVE_CANDIDATES)?
        .query_map(
            rusqlite::params![group_id, user_id],
            map_representative_candidate,
        )?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(select_representative(&candidates, config).map(|face| face.crop_path.clone()))
}

pub fn merge_groups(
    connection: &Connection,
    ordered_group_ids: Vec<i64>,
    config: &FaceGroupConfig,
) -> rusqlite::Result<MergeFaceGroupsOutcome> {
    let parameters = ordered_group_ids
        .iter()
        .map(|id| id as &dyn rusqlite::ToSql)
        .collect::<Vec<_>>();
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    let found = transaction
        .prepare(&queries::faces::build_existing_groups_query(
            ordered_group_ids.len(),
        ))?
        .query_map(parameters.as_slice(), |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if found.len() != ordered_group_ids.len() {
        transaction.rollback()?;
        return Ok(MergeFaceGroupsOutcome::NotFound);
    }
    let target_id = ordered_group_ids[0];
    let members = transaction
        .prepare(&queries::faces::build_merge_members_query(
            ordered_group_ids.len(),
        ))?
        .query_map(parameters.as_slice(), |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for face_id in members {
        transaction.execute(
            queries::faces::DELETE_AUTOMATIC_MEMBERSHIP_FOR_FACE,
            [face_id],
        )?;
        transaction.execute(queries::faces::INSERT_MANUAL_MEMBER, [target_id, face_id])?;
    }
    transaction.execute(queries::faces::UPDATE_MANUAL_GROUP, [target_id])?;
    for source_id in ordered_group_ids.into_iter().skip(1) {
        transaction.execute(queries::faces::DELETE_GROUP, [source_id])?;
    }
    transaction.execute(
        queries::faces::DELETE_GENERATION_REPRESENTATIVES_FOR_GROUP,
        [target_id],
    )?;
    transaction.execute(queries::faces::INCREMENT_MANUAL_REVISION, [])?;
    update_group_representative(&transaction, target_id, config)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let face_count =
        transaction.query_row(queries::faces::COUNT_GROUP_MEMBERS, [target_id], |row| {
            row.get(0)
        })?;
    let media_count =
        transaction.query_row(queries::faces::COUNT_GROUP_MEDIA, [target_id], |row| {
            row.get(0)
        })?;
    transaction.commit()?;
    Ok(MergeFaceGroupsOutcome::Merged(MergedFaceGroup {
        face_group_id: target_id,
        face_count,
        media_count,
    }))
}

pub fn cancel(connection: &Connection) -> AppResult<bool> {
    let transaction = connection.unchecked_transaction()?;
    let cancelled_jobs = transaction.execute(queries::faces::CANCEL_ACTIVE, [])?;
    let cancelled_runs = transaction.execute(queries::faces::REQUEST_CANCEL_RUNS, [])?;
    transaction.commit()?;
    Ok(cancelled_jobs > 0 || cancelled_runs > 0)
}
