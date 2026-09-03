use std::collections::HashMap;

use crate::config::{Config, LlmConfig};
use crate::constants::{
    DOCUMENT_DETECTION_MODEL_TYPE, FACE_DETECTION_MODEL_TYPE, IMAGE_AESTHETICS_MODEL_TYPE,
    IMAGE_TAGGING_MODEL_TYPE, OCR_MODEL_TYPE, SCREENSHOT_DETECTION_MODEL_TYPE,
};
use crate::database::queries;
use crate::io::file::{NormalizedStoragePath, PathClaimMode, PathClaimScope, StorageRootId};
use crate::io::journal::{
    FileEntryAction, FileEntryPlan, FileOperationPlan, FilePathClaimPlan, PrepareJournalOutcome,
};
use crate::models::{
    AiActionResponse, AiFeatureActionResult, AiFeatureScheduleResponse, AiJobCounts,
    AiStatusResponse, AiTaskStatusResponse, DeduplicateStatusResponse,
};
use crate::processor::deduplicator;
use momento_common::llm::IMAGE_CLUSTERING_MODEL_VERSION;
use rusqlite::OptionalExtension;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AiFeature {
    Ocr,
    ImageTagging,
    ImageAesthetics,
    ScreenshotDetection,
    DocumentDetection,
    FaceDetection,
    Deduplicate,
}

#[derive(Debug)]
pub(crate) enum AiFeatureCleanOutcome {
    Cleaned {
        result: AiFeatureActionResult,
        cleanup_group_created: bool,
    },
    ActiveWork,
    PendingCancellation,
    PendingResultCleanup,
    PathConflict,
}

impl AiFeature {
    pub const ALL: [Self; 7] = [
        Self::Ocr,
        Self::ImageTagging,
        Self::ImageAesthetics,
        Self::ScreenshotDetection,
        Self::DocumentDetection,
        Self::FaceDetection,
        Self::Deduplicate,
    ];

    pub const INFERENCE: [Self; 6] = [
        Self::Ocr,
        Self::ImageTagging,
        Self::ImageAesthetics,
        Self::ScreenshotDetection,
        Self::DocumentDetection,
        Self::FaceDetection,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Ocr => OCR_MODEL_TYPE,
            Self::ImageTagging => IMAGE_TAGGING_MODEL_TYPE,
            Self::ImageAesthetics => IMAGE_AESTHETICS_MODEL_TYPE,
            Self::ScreenshotDetection => SCREENSHOT_DETECTION_MODEL_TYPE,
            Self::DocumentDetection => DOCUMENT_DETECTION_MODEL_TYPE,
            Self::FaceDetection => FACE_DETECTION_MODEL_TYPE,
            Self::Deduplicate => "deduplicate",
        }
    }

    pub fn from_control_name(name: &str) -> Option<Self> {
        match name {
            "ocr" => Some(Self::Ocr),
            "image_tagging" => Some(Self::ImageTagging),
            "image_aesthetics" => Some(Self::ImageAesthetics),
            "screenshot_detection" => Some(Self::ScreenshotDetection),
            "document_detection" => Some(Self::DocumentDetection),
            "face_detection" => Some(Self::FaceDetection),
            "deduplicate" => Some(Self::Deduplicate),
            _ => None,
        }
    }

    pub fn inference_task(self) -> &'static str {
        match self {
            Self::Deduplicate => "image_clustering",
            _ => self.name(),
        }
    }

    pub fn cron_config_field(self) -> &'static str {
        match self {
            Self::Ocr => "ocr_cron",
            Self::ImageTagging => "image_tagging_cron",
            Self::ImageAesthetics => "image_aesthetics_cron",
            Self::ScreenshotDetection => "screenshot_detection_cron",
            Self::DocumentDetection => "document_detection_cron",
            Self::FaceDetection => "face_detection_cron",
            Self::Deduplicate => "deduplicate_cron",
        }
    }

    pub fn cron_expression(self, config: &LlmConfig) -> &str {
        match self {
            Self::Ocr => &config.ocr_cron,
            Self::ImageTagging => &config.image_tagging_cron,
            Self::ImageAesthetics => &config.image_aesthetics_cron,
            Self::ScreenshotDetection => &config.screenshot_detection_cron,
            Self::DocumentDetection => &config.document_detection_cron,
            Self::FaceDetection => &config.face_detection_cron,
            Self::Deduplicate => &config.deduplicate_cron,
        }
    }
}

pub(crate) fn start_feature_on_connection(
    connection: &mut rusqlite::Connection,
    feature: AiFeature,
    trigger: &str,
    scheduled_for: Option<&str>,
) -> rusqlite::Result<usize> {
    let transaction = connection.unchecked_transaction()?;
    if transaction.query_row(queries::metadata_jobs::IS_RESET_ACTIVE, [], |row| {
        row.get::<_, bool>(0)
    })? {
        transaction.rollback()?;
        return Ok(0);
    }
    let queued_jobs = match feature {
        AiFeature::Ocr
        | AiFeature::ImageTagging
        | AiFeature::ImageAesthetics
        | AiFeature::ScreenshotDetection
        | AiFeature::DocumentDetection => {
            let task = feature.name();
            let queued = if task == IMAGE_AESTHETICS_MODEL_TYPE {
                transaction.execute(queries::ai_jobs::INSERT_AESTHETICS_ELIGIBLE, [])?
            } else if task == SCREENSHOT_DETECTION_MODEL_TYPE {
                transaction.execute(queries::ai_jobs::INSERT_SCREENSHOT_ELIGIBLE, [])?
            } else if task == DOCUMENT_DETECTION_MODEL_TYPE {
                transaction.execute(queries::ai_jobs::INSERT_DOCUMENT_ELIGIBLE, [])?
            } else {
                transaction.execute(
                    queries::ai_jobs::INSERT_ELIGIBLE,
                    rusqlite::params![task, task, task, task],
                )?
            };
            transaction.execute(queries::ai_jobs::SNAPSHOT_QUEUED_INPUTS, [])?;
            queued
        }
        AiFeature::FaceDetection => {
            if transaction
                .query_row(queries::faces::SELECT_ACTIVE_RUN, [], |row| {
                    row.get::<_, i64>(0)
                })
                .optional()?
                .is_some()
            {
                transaction.rollback()?;
                return Ok(0);
            }
            transaction.execute(queries::faces::INSERT_GROUPING_RUN, [])?;
            let run_id = transaction.last_insert_rowid();
            let queued = transaction.execute(queries::ai_jobs::INSERT_FACE_ELIGIBLE, [run_id])?;
            transaction.execute(queries::ai_jobs::SNAPSHOT_QUEUED_INPUTS, [])?;
            queued
        }
        AiFeature::Deduplicate => {
            let inserted = transaction.execute(
                queries::deduplicate::INSERT_RUN,
                rusqlite::params![trigger, scheduled_for],
            );
            let run_id = match inserted {
                Ok(_) => transaction.last_insert_rowid(),
                Err(rusqlite::Error::SqliteFailure(database_error, _))
                    if database_error.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    transaction.rollback()?;
                    return Ok(0);
                }
                Err(error) => return Err(error),
            };
            let indexes_from_other_model = transaction.query_row(
                queries::deduplicate::COUNT_INDEXES_FROM_OTHER_MODEL,
                [IMAGE_CLUSTERING_MODEL_VERSION],
                |row| row.get::<_, i64>(0),
            )?;
            if indexes_from_other_model > 0 {
                transaction.execute(
                    queries::deduplicate::DELETE_HASH_BANDS_FROM_OTHER_MODEL,
                    [IMAGE_CLUSTERING_MODEL_VERSION],
                )?;
                transaction.execute(
                    queries::deduplicate::DELETE_INDEXES_FROM_OTHER_MODEL,
                    [IMAGE_CLUSTERING_MODEL_VERSION],
                )?;
                transaction.execute(queries::deduplicate::MARK_ALL_DIRTY, [])?;
            }
            let queued = transaction.execute(
                queries::deduplicate::CREATE_CLUSTERING_JOBS,
                rusqlite::params![run_id, run_id],
            )?;
            transaction.execute(queries::ai_jobs::SNAPSHOT_QUEUED_INPUTS, [])?;
            queued
        }
    };
    transaction.commit()?;
    Ok(queued_jobs)
}

pub(crate) fn cancel_feature_on_connection(
    connection: &mut rusqlite::Connection,
    feature: AiFeature,
) -> rusqlite::Result<AiFeatureActionResult> {
    let transaction = connection.unchecked_transaction()?;
    let task = feature.inference_task();
    let affected_jobs =
        transaction.query_row(queries::ai_jobs::COUNT_ACTIVE_FOR_TASK, [task], |row| {
            row.get::<_, i64>(0)
        })?;
    let cancellation_requested = match feature {
        AiFeature::Deduplicate => {
            let run = transaction
                .query_row(
                    queries::deduplicate::SELECT_LATEST_RUN,
                    [],
                    deduplicator::map_run_status,
                )
                .optional()?;
            if let Some(run) = run.filter(|run| run.status == "running") {
                transaction.execute(queries::ai_jobs::QUEUE_CANCELLATION_SCOPE_FOR_TASK, [task])?;
                transaction.execute(queries::ai_jobs::QUEUE_CANCELLATIONS_FOR_TASK, [task])?;
                transaction.execute(queries::ai_jobs::CANCEL_FOR_TASK, [task])?;
                transaction.execute(queries::ai_jobs::CANCEL_RESULT_RECEIPTS_FOR_TASK, [task])?;
                transaction.execute(queries::deduplicate::REQUEST_CANCEL, [run.id])? > 0
            } else {
                false
            }
        }
        AiFeature::FaceDetection => {
            if affected_jobs > 0 {
                transaction.execute(queries::ai_jobs::QUEUE_CANCELLATION_SCOPE_FOR_TASK, [task])?;
                transaction.execute(queries::ai_jobs::QUEUE_CANCELLATIONS_FOR_TASK, [task])?;
                transaction.execute(queries::ai_jobs::CANCEL_FOR_TASK, [task])?;
                transaction.execute(queries::ai_jobs::CANCEL_RESULT_RECEIPTS_FOR_TASK, [task])?;
            }
            let cancelled_runs = transaction.execute(queries::faces::REQUEST_CANCEL_RUNS, [])? > 0;
            affected_jobs > 0 || cancelled_runs
        }
        _ => {
            if affected_jobs > 0 {
                transaction.execute(queries::ai_jobs::QUEUE_CANCELLATION_SCOPE_FOR_TASK, [task])?;
                transaction.execute(queries::ai_jobs::QUEUE_CANCELLATIONS_FOR_TASK, [task])?;
                transaction.execute(queries::ai_jobs::CANCEL_FOR_TASK, [task])?;
                transaction.execute(queries::ai_jobs::CANCEL_RESULT_RECEIPTS_FOR_TASK, [task])?;
                true
            } else {
                false
            }
        }
    };
    transaction.commit()?;
    Ok(AiFeatureActionResult {
        feature: feature.name().to_string(),
        outcome: if cancellation_requested {
            "cancellationRequested"
        } else {
            "noActiveWork"
        }
        .to_string(),
        affected_jobs,
        error: None,
    })
}

pub(crate) fn cancel_all_on_connection(
    connection: &mut rusqlite::Connection,
) -> rusqlite::Result<Vec<AiFeatureActionResult>> {
    let transaction = connection.unchecked_transaction()?;
    let mut counts = Vec::with_capacity(AiFeature::ALL.len());
    for feature in AiFeature::ALL {
        let count = transaction.query_row(
            queries::ai_jobs::COUNT_ACTIVE_FOR_TASK,
            [feature.inference_task()],
            |row| row.get::<_, i64>(0),
        )?;
        counts.push((feature, count));
    }
    let has_active_jobs = counts.iter().any(|(_, count)| *count > 0);
    if has_active_jobs {
        transaction.execute(queries::ai_jobs::QUEUE_ALL_CANCELLATION_SCOPE, [])?;
        transaction.execute(queries::ai_jobs::QUEUE_ALL_CANCELLATIONS, [])?;
        transaction.execute(queries::ai_jobs::CANCEL_ALL, [])?;
        transaction.execute(queries::ai_jobs::CANCEL_ALL_RESULT_RECEIPTS, [])?;
    }
    let face_run_cancelled = transaction.execute(queries::faces::REQUEST_CANCEL_RUNS, [])? > 0;
    let deduplicate_run = transaction
        .query_row(
            queries::deduplicate::SELECT_LATEST_RUN,
            [],
            deduplicator::map_run_status,
        )
        .optional()?
        .filter(|run| run.status == "running");
    let deduplicate_run_cancelled = if let Some(run) = deduplicate_run {
        if !has_active_jobs {
            transaction.execute(
                queries::ai_jobs::QUEUE_CANCELLATION_SCOPE_FOR_TASK,
                [AiFeature::Deduplicate.inference_task()],
            )?;
            transaction.execute(
                queries::ai_jobs::QUEUE_CANCELLATIONS_FOR_TASK,
                [AiFeature::Deduplicate.inference_task()],
            )?;
            transaction.execute(
                queries::ai_jobs::CANCEL_FOR_TASK,
                [AiFeature::Deduplicate.inference_task()],
            )?;
            transaction.execute(
                queries::ai_jobs::CANCEL_RESULT_RECEIPTS_FOR_TASK,
                [AiFeature::Deduplicate.inference_task()],
            )?;
        }
        transaction.execute(queries::deduplicate::REQUEST_CANCEL, [run.id])? > 0
    } else {
        false
    };
    transaction.commit()?;
    Ok(counts
        .into_iter()
        .map(|(feature, affected_jobs)| {
            let requested = affected_jobs > 0
                || (feature == AiFeature::FaceDetection && face_run_cancelled)
                || (feature == AiFeature::Deduplicate && deduplicate_run_cancelled);
            AiFeatureActionResult {
                feature: feature.name().to_string(),
                outcome: if requested {
                    "cancellationRequested"
                } else {
                    "noActiveWork"
                }
                .to_string(),
                affected_jobs,
                error: None,
            }
        })
        .collect())
}

pub(crate) fn clean_feature_on_connection(
    connection: &mut rusqlite::Connection,
    feature: AiFeature,
    cleanup_group_id: &str,
) -> rusqlite::Result<AiFeatureCleanOutcome> {
    let transaction = connection.unchecked_transaction()?;
    let task = feature.inference_task();
    let active_jobs =
        transaction.query_row(queries::ai_jobs::COUNT_ACTIVE_FOR_TASK, [task], |row| {
            row.get::<_, i64>(0)
        })?;
    if active_jobs > 0 {
        transaction.rollback()?;
        return Ok(AiFeatureCleanOutcome::ActiveWork);
    }
    let pending_cancellation = transaction.query_row(
        queries::ai_jobs::COUNT_PENDING_CANCELLATION_SCOPE_FOR_TASK,
        [task],
        |row| row.get::<_, i64>(0),
    )?;
    if pending_cancellation > 0 {
        transaction.rollback()?;
        return Ok(AiFeatureCleanOutcome::PendingCancellation);
    }
    let pending_result_cleanup = transaction.query_row(
        queries::ai_jobs::COUNT_PENDING_RESULT_CLEANUP_FOR_TASK,
        [task],
        |row| row.get::<_, i64>(0),
    )?;
    if pending_result_cleanup > 0 {
        transaction.rollback()?;
        return Ok(AiFeatureCleanOutcome::PendingResultCleanup);
    }
    let affected_jobs =
        transaction.query_row(queries::ai_jobs::COUNT_JOBS_FOR_TASK, [task], |row| {
            row.get::<_, i64>(0)
        })?;

    let cleanup_group_created = match feature {
        AiFeature::Ocr | AiFeature::ImageTagging => {
            transaction.execute(queries::ai_jobs::DELETE_TEXT_FOR_TASK, [task])?;
            transaction.execute(queries::ai_jobs::DELETE_TEXT_INPUTS_FOR_TASK, [task])?;
            transaction.execute(queries::ai_jobs::DELETE_JOBS_FOR_TASK, [task])?;
            false
        }
        AiFeature::ImageAesthetics => {
            transaction.execute(queries::ai_jobs::DELETE_AESTHETICS, [])?;
            transaction.execute(queries::ai_jobs::DELETE_AESTHETIC_INPUTS, [])?;
            transaction.execute(queries::ai_jobs::DELETE_JOBS_FOR_TASK, [task])?;
            false
        }
        AiFeature::ScreenshotDetection => {
            transaction.execute(queries::ai_jobs::DELETE_SCREENSHOT_CLASSIFICATIONS, [])?;
            transaction.execute(
                queries::ai_jobs::DELETE_SCREENSHOT_CLASSIFICATION_INPUTS,
                [],
            )?;
            transaction.execute(queries::ai_jobs::DELETE_JOBS_FOR_TASK, [task])?;
            false
        }
        AiFeature::DocumentDetection => {
            transaction.execute(queries::ai_jobs::DELETE_DOCUMENT_CLASSIFICATIONS, [])?;
            transaction.execute(queries::ai_jobs::DELETE_DOCUMENT_CLASSIFICATION_INPUTS, [])?;
            transaction.execute(queries::ai_jobs::DELETE_JOBS_FOR_TASK, [task])?;
            false
        }
        AiFeature::FaceDetection => {
            let active_runs =
                transaction.query_row(queries::faces::COUNT_ACTIVE_RUNS, [], |row| {
                    row.get::<_, i64>(0)
                })?;
            if active_runs > 0 {
                transaction.rollback()?;
                return Ok(AiFeatureCleanOutcome::ActiveWork);
            }
            let faces_path =
                NormalizedStoragePath::parse("faces").map_err(|_| rusqlite::Error::InvalidQuery)?;
            let plan = FileOperationPlan {
                group_id: cleanup_group_id.to_string(),
                kind: "face_detection_clean".to_string(),
                owner_kind: "ai_feature".to_string(),
                owner_id: feature.name().to_string(),
                claim_token: None,
                product_target: None,
                product_version: None,
                entries: vec![FileEntryPlan {
                    action: FileEntryAction::Cleanup,
                    storage_root: StorageRootId::Previews,
                    source_path: Some(faces_path.clone()),
                    temporary_path: None,
                    destination_path: None,
                    tombstone_path: None,
                    expected_size: None,
                    expected_sha256: None,
                    expected_version: None,
                }],
                claims: vec![FilePathClaimPlan {
                    storage_root: StorageRootId::Previews,
                    path: faces_path,
                    mode: PathClaimMode::Write,
                    scope: PathClaimScope::Subtree,
                    role: "face_crop_tree".to_string(),
                    expected_version: None,
                }],
                space_reservation: None,
            };
            if crate::io::journal::prepare_committed_cleanup(&transaction, plan)?
                == PrepareJournalOutcome::PathConflict
            {
                transaction.rollback()?;
                return Ok(AiFeatureCleanOutcome::PathConflict);
            }
            transaction.execute(queries::faces::CLEAN_JOBS, [])?;
            transaction.execute(queries::faces::CLEAN_GENERATION_STATE, [])?;
            transaction.execute(queries::faces::CLEAN_GROUPS, [])?;
            transaction.execute(queries::faces::CLEAN_GENERATIONS, [])?;
            transaction.execute(queries::faces::CLEAN_RUNS, [])?;
            transaction.execute(queries::faces::CLEAN_MANUAL_STATE, [])?;
            transaction.execute(queries::faces::CLEAN_FACES, [])?;
            transaction.execute(queries::faces::CLEAN_RESULTS, [])?;
            true
        }
        AiFeature::Deduplicate => {
            transaction.execute(queries::deduplicate::LOCK_RUNS, [])?;
            let active_runs =
                transaction.query_row(queries::deduplicate::COUNT_ACTIVE_RUNS, [], |row| {
                    row.get::<_, i64>(0)
                })?;
            if active_runs > 0 {
                transaction.rollback()?;
                return Ok(AiFeatureCleanOutcome::ActiveWork);
            }
            transaction.execute(queries::deduplicate::CLEAN_CLUSTERS, [])?;
            transaction.execute(queries::deduplicate::CLEAN_FINALIZATION_DIRTY, [])?;
            transaction.execute(queries::deduplicate::CLEAN_EDGES, [])?;
            transaction.execute(queries::deduplicate::CLEAN_LABELS, [])?;
            transaction.execute(queries::deduplicate::CLEAN_FINALIZATIONS, [])?;
            transaction.execute(queries::deduplicate::CLEAN_GENERATION_STATE, [])?;
            transaction.execute(queries::deduplicate::CLEAN_GENERATIONS, [])?;
            transaction.execute(queries::deduplicate::CLEAN_JOBS, [])?;
            transaction.execute(queries::deduplicate::CLEAN_BANDS, [])?;
            transaction.execute(queries::deduplicate::CLEAN_INDEX, [])?;
            transaction.execute(queries::deduplicate::CLEAN_DIRTY, [])?;
            transaction.execute(queries::deduplicate::CLEAN_RUNS, [])?;
            transaction.execute(queries::deduplicate::MARK_ALL_DIRTY, [])?;
            false
        }
    };
    transaction.commit()?;
    Ok(AiFeatureCleanOutcome::Cleaned {
        result: AiFeatureActionResult {
            feature: feature.name().to_string(),
            outcome: "cleaned".to_string(),
            affected_jobs,
            error: None,
        },
        cleanup_group_created,
    })
}

pub fn action_response(action: &str, results: Vec<AiFeatureActionResult>) -> AiActionResponse {
    AiActionResponse {
        action: action.to_string(),
        results,
    }
}

pub(crate) fn status_on_connection(
    config: &Config,
    connection: &rusqlite::Connection,
    schedules: Vec<AiFeatureScheduleResponse>,
) -> rusqlite::Result<AiStatusResponse> {
    let transaction = connection.unchecked_transaction()?;
    let mut counts_by_task = HashMap::<String, AiJobCounts>::new();
    for row in transaction
        .prepare(queries::ai_jobs::SELECT_LATEST_STATUS_COUNTS)?
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?
    {
        let (task, job_status, count) = row?;
        set_job_count(counts_by_task.entry(task).or_default(), &job_status, count);
    }
    let mut errors_by_task = HashMap::<String, Vec<String>>::new();
    for row in transaction
        .prepare(queries::ai_jobs::SELECT_LATEST_FAILURES)?
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
    {
        let (task, error) = row?;
        let task_errors = errors_by_task.entry(task).or_default();
        if task_errors.len() < 100 {
            task_errors.push(error);
        }
    }

    let tasks = AiFeature::INFERENCE
        .into_iter()
        .map(|feature| {
            let task = feature.inference_task();
            let jobs = counts_by_task.remove(task).unwrap_or_default();
            AiTaskStatusResponse {
                task: task.to_string(),
                enabled: config.llm.enabled,
                state: task_state(&jobs).to_string(),
                jobs,
                errors: errors_by_task.remove(task).unwrap_or_default(),
            }
        })
        .collect();
    let deduplicate_jobs = counts_by_task
        .remove(AiFeature::Deduplicate.inference_task())
        .unwrap_or_default();
    let run = transaction
        .query_row(
            queries::deduplicate::SELECT_LATEST_RUN,
            [],
            deduplicator::map_run_status,
        )
        .optional()?;
    let ensembled_media =
        transaction.query_row(queries::deduplicate::COUNT_ENSEMBLED_MEDIA, [], |row| {
            row.get(0)
        })?;
    let face_groups = transaction.query_row(queries::faces::COUNT_GROUPS, [], |row| row.get(0))?;
    transaction.commit()?;

    Ok(AiStatusResponse {
        tasks,
        deduplicate: deduplicate_status(run, ensembled_media, deduplicate_jobs),
        face_groups,
        schedules,
    })
}

fn set_job_count(counts: &mut AiJobCounts, status: &str, count: i64) {
    match status {
        "queued" => counts.queued = count,
        "submitting" => counts.submitting = count,
        "submitted" => counts.submitted = count,
        "completed" => counts.completed = count,
        "failed" => counts.failed = count,
        "cancelled" => counts.cancelled = count,
        _ => {}
    }
}

fn task_state(counts: &AiJobCounts) -> &'static str {
    if counts.submitting > 0 {
        return "submitting";
    }
    if counts.submitted > 0 {
        return "submitted";
    }
    if counts.queued > 0 {
        return "queued";
    }
    if counts.failed > 0 {
        return "failed";
    }
    "idle"
}

fn deduplicate_status(
    run: Option<deduplicator::DeduplicateRunStatus>,
    ensembled_media: i64,
    jobs: AiJobCounts,
) -> DeduplicateStatusResponse {
    let Some(run) = run else {
        return DeduplicateStatusResponse {
            status: "idle".to_string(),
            run_id: None,
            trigger: None,
            scheduled_for: None,
            started_at: None,
            completed_at: None,
            ensembled_media,
            processed_media: 0,
            candidate_comparisons: 0,
            clusters_created: 0,
            error: None,
            jobs,
        };
    };
    DeduplicateStatusResponse {
        status: run.status,
        run_id: Some(run.id),
        trigger: Some(run.trigger),
        scheduled_for: run.scheduled_for,
        started_at: Some(run.started_at),
        completed_at: run.completed_at,
        ensembled_media,
        processed_media: run.processed_media,
        candidate_comparisons: run.candidate_comparisons,
        clusters_created: run.clusters_created,
        error: run.error,
        jobs,
    }
}
