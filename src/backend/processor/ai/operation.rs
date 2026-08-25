use std::collections::HashMap;

use rusqlite::OptionalExtension;
use tracing::warn;

use crate::config::Config;
use crate::constants::{
    DOCUMENT_DETECTION_MODEL_TYPE, FACE_DETECTION_MODEL_TYPE, IMAGE_AESTHETICS_MODEL_TYPE,
    IMAGE_TAGGING_MODEL_TYPE, OCR_MODEL_TYPE, SCREENSHOT_DETECTION_MODEL_TYPE,
};
use crate::database::{queries, DbPool};
use crate::error::{AppError, AppResult};
use crate::models::{
    AiActionResponse, AiFeatureActionResult, AiJobCounts, AiStatusResponse, AiTaskStatusResponse,
    DeduplicateStatusResponse,
};
use crate::processor::{deduplicator, face_detection};

use super::{cancel_active_jobs, queue_task};

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

    pub fn is_enabled(self, config: &Config) -> bool {
        if !config.llm.enabled {
            return false;
        }
        match self {
            Self::Ocr => true,
            Self::ImageTagging => config.llm.image_tagging_enabled,
            Self::ImageAesthetics => config.llm.image_aesthetics_enabled,
            Self::ScreenshotDetection => config.llm.screenshot_detection_enabled,
            Self::DocumentDetection => config.llm.document_detection_enabled,
            Self::FaceDetection => config.llm.face_detection_enabled,
            Self::Deduplicate => config.llm.deduplicate_enabled,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AiStartSource<'a> {
    Manual,
    Scheduled { scheduled_for: &'a str },
    StartupRecovery,
}

pub fn start_feature(
    config: &Config,
    pool: &DbPool,
    feature: AiFeature,
    source: AiStartSource<'_>,
) -> AppResult<usize> {
    if !feature.is_enabled(config) {
        return Err(AppError::Validation(format!(
            "{} is disabled in LLM configuration",
            feature.name()
        )));
    }
    match feature {
        AiFeature::Ocr
        | AiFeature::ImageTagging
        | AiFeature::ImageAesthetics
        | AiFeature::ScreenshotDetection
        | AiFeature::DocumentDetection => {
            queue_task(pool, feature.name(), true).map_err(AppError::Database)
        }
        AiFeature::FaceDetection => face_detection::start(pool, true),
        AiFeature::Deduplicate => start_deduplicate(pool, source),
    }
}

pub fn start_all_features(
    config: &Config,
    pool: &DbPool,
    source: AiStartSource<'_>,
) -> AppResult<usize> {
    let mut queued_jobs = 0;
    let mut successful_features = 0;
    let mut first_error = None;
    for feature in AiFeature::ALL {
        if !feature.is_enabled(config) {
            continue;
        }
        match start_feature(config, pool, feature, source) {
            Ok(feature_jobs) => {
                successful_features += 1;
                queued_jobs += feature_jobs;
            }
            Err(error) => {
                warn!(
                    feature = feature.name(),
                    "Could not start AI feature: {error}"
                );
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }
    if successful_features == 0 {
        if let Some(error) = first_error {
            return Err(error);
        }
    }
    Ok(queued_jobs)
}

pub fn action_response(action: &str, results: Vec<AiFeatureActionResult>) -> AiActionResponse {
    AiActionResponse {
        action: action.to_string(),
        results,
    }
}

pub fn start_feature_action(
    config: &Config,
    pool: &DbPool,
    feature: AiFeature,
) -> AppResult<AiFeatureActionResult> {
    let queued_jobs = start_feature(config, pool, feature, AiStartSource::Manual)? as i64;
    Ok(AiFeatureActionResult {
        feature: feature.name().to_string(),
        outcome: if queued_jobs > 0 { "queued" } else { "noWork" }.to_string(),
        affected_jobs: queued_jobs,
        error: None,
    })
}

pub fn start_all_actions(config: &Config, pool: &DbPool) -> Vec<AiFeatureActionResult> {
    AiFeature::ALL
        .into_iter()
        .map(|feature| {
            if !feature.is_enabled(config) {
                return AiFeatureActionResult {
                    feature: feature.name().to_string(),
                    outcome: "disabled".to_string(),
                    affected_jobs: 0,
                    error: None,
                };
            }
            match start_feature_action(config, pool, feature) {
                Ok(result) => result,
                Err(error) => AiFeatureActionResult {
                    feature: feature.name().to_string(),
                    outcome: "failed".to_string(),
                    affected_jobs: 0,
                    error: Some(error.to_string()),
                },
            }
        })
        .collect()
}

pub fn cancel_feature_action(
    pool: &DbPool,
    feature: AiFeature,
) -> AppResult<AiFeatureActionResult> {
    let affected_jobs = count_jobs(pool, feature.inference_task(), true)?;
    let cancellation_requested = match feature {
        AiFeature::Deduplicate => deduplicator::request_cancel(pool)?,
        AiFeature::FaceDetection => {
            if affected_jobs > 0 {
                cancel_active_jobs(pool, Some(feature.inference_task()))?;
            }
            face_detection::cancel(pool)? || affected_jobs > 0
        }
        _ => {
            if affected_jobs > 0 {
                cancel_active_jobs(pool, Some(feature.inference_task()))?;
            }
            affected_jobs > 0
        }
    };
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

pub fn cancel_all_actions(pool: &DbPool) -> Vec<AiFeatureActionResult> {
    let counts = AiFeature::ALL
        .into_iter()
        .map(|feature| {
            count_jobs(pool, feature.inference_task(), true).map(|count| (feature, count))
        })
        .collect::<AppResult<Vec<_>>>();
    let counts = match counts {
        Ok(counts) => counts,
        Err(error) => return failed_actions("cancel", error),
    };
    if counts.iter().any(|(_, count)| *count > 0) {
        if let Err(error) = cancel_active_jobs(pool, None) {
            return failed_actions("cancel", AppError::Database(error));
        }
    }
    let face_result = face_detection::cancel(pool);
    let deduplicate_result = deduplicator::request_cancel(pool);

    counts
        .into_iter()
        .map(|(feature, affected_jobs)| {
            if feature == AiFeature::FaceDetection {
                return match &face_result {
                    Ok(requested) => AiFeatureActionResult {
                        feature: feature.name().to_string(),
                        outcome: if *requested || affected_jobs > 0 {
                            "cancellationRequested"
                        } else {
                            "noActiveWork"
                        }
                        .to_string(),
                        affected_jobs,
                        error: None,
                    },
                    Err(error) => action_failure(feature, error.to_string()),
                };
            }
            let cancellation_requested = if feature == AiFeature::Deduplicate {
                match &deduplicate_result {
                    Ok(requested) => *requested,
                    Err(error) => return action_failure(feature, error.to_string()),
                }
            } else {
                affected_jobs > 0
            };
            AiFeatureActionResult {
                feature: feature.name().to_string(),
                outcome: if cancellation_requested {
                    "cancellationRequested"
                } else {
                    "noActiveWork"
                }
                .to_string(),
                affected_jobs,
                error: None,
            }
        })
        .collect()
}

pub fn clean_feature_action(pool: &DbPool, feature: AiFeature) -> AppResult<AiFeatureActionResult> {
    ensure_feature_is_cleanable(pool, feature)?;
    let affected_jobs = count_jobs(pool, feature.inference_task(), false)?;
    match feature {
        AiFeature::Deduplicate => deduplicator::clean(pool)?,
        AiFeature::FaceDetection => face_detection::clean(pool)?,
        _ => clean_inference_feature(pool, feature)?,
    }
    Ok(AiFeatureActionResult {
        feature: feature.name().to_string(),
        outcome: "cleaned".to_string(),
        affected_jobs,
        error: None,
    })
}

pub fn clean_all_actions(pool: &DbPool) -> Vec<AiFeatureActionResult> {
    AiFeature::ALL
        .into_iter()
        .map(|feature| match clean_feature_action(pool, feature) {
            Ok(result) => result,
            Err(error) => AiFeatureActionResult {
                feature: feature.name().to_string(),
                outcome: "failed".to_string(),
                affected_jobs: 0,
                error: Some(error.to_string()),
            },
        })
        .collect()
}

pub fn status(config: &Config, pool: &DbPool) -> AppResult<AiStatusResponse> {
    let connection = pool.get()?;
    let transaction = connection.unchecked_transaction()?;
    let mut counts_by_task = HashMap::<String, AiJobCounts>::new();
    for row in transaction
        .prepare(queries::ai_jobs::SELECT_ALL_STATUS_COUNTS)?
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
        .prepare(queries::ai_jobs::SELECT_ALL_FAILURES)?
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
                enabled: feature.is_enabled(config),
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
        .query_row(queries::deduplicate::SELECT_LATEST_RUN, [], |row| {
            Ok(deduplicator::DeduplicateRunStatus {
                id: row.get(0)?,
                trigger: row.get(1)?,
                status: row.get(2)?,
                scheduled_for: row.get(3)?,
                started_at: row.get(4)?,
                completed_at: row.get(5)?,
                indexed_media: row.get(6)?,
                processed_media: row.get(7)?,
                candidate_comparisons: row.get(8)?,
                clusters_created: row.get(9)?,
                error: row.get(10)?,
            })
        })
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
    })
}

fn start_deduplicate(pool: &DbPool, source: AiStartSource<'_>) -> AppResult<usize> {
    let (trigger, scheduled_for) = match source {
        AiStartSource::Manual => ("manual", None),
        AiStartSource::Scheduled { scheduled_for } => ("scheduled", Some(scheduled_for)),
        AiStartSource::StartupRecovery => ("startup", None),
    };
    let run_id = match deduplicator::create_run(pool, trigger, scheduled_for) {
        Ok(run_id) => run_id,
        Err(AppError::Conflict(_)) => return Ok(0),
        Err(error) => return Err(error),
    };
    deduplicator::queue_clustering_jobs(pool, run_id)
}

fn count_jobs(pool: &DbPool, task: &str, active_only: bool) -> AppResult<i64> {
    let query = if active_only {
        queries::ai_jobs::COUNT_ACTIVE_FOR_TASK
    } else {
        queries::ai_jobs::COUNT_JOBS_FOR_TASK
    };
    Ok(pool.get()?.query_row(query, [task], |row| row.get(0))?)
}

fn ensure_feature_is_cleanable(pool: &DbPool, feature: AiFeature) -> AppResult<()> {
    let task = feature.inference_task();
    if count_jobs(pool, task, true)? > 0 {
        return Err(AppError::Conflict(format!(
            "{} has active jobs and cannot be cleaned",
            feature.name()
        )));
    }
    let pending_cancellation: i64 = pool.get()?.query_row(
        queries::ai_jobs::COUNT_PENDING_CANCELLATION_SCOPE_FOR_TASK,
        [task],
        |row| row.get(0),
    )?;
    if pending_cancellation > 0 {
        return Err(AppError::Conflict(format!(
            "{} cancellation has not been acknowledged",
            feature.name()
        )));
    }
    Ok(())
}

fn clean_inference_feature(pool: &DbPool, feature: AiFeature) -> AppResult<()> {
    let connection = pool.get()?;
    let transaction = connection.unchecked_transaction()?;
    match feature {
        AiFeature::Ocr | AiFeature::ImageTagging => {
            transaction.execute(
                queries::ai_jobs::DELETE_TEXT_FOR_TASK,
                [feature.inference_task()],
            )?;
            transaction.execute(
                queries::ai_jobs::DELETE_TEXT_INPUTS_FOR_TASK,
                [feature.inference_task()],
            )?;
        }
        AiFeature::ImageAesthetics => {
            transaction.execute(queries::ai_jobs::DELETE_AESTHETICS, [])?;
            transaction.execute(queries::ai_jobs::DELETE_AESTHETIC_INPUTS, [])?;
        }
        AiFeature::ScreenshotDetection => {
            transaction.execute(queries::ai_jobs::DELETE_SCREENSHOT_CLASSIFICATIONS, [])?;
            transaction.execute(
                queries::ai_jobs::DELETE_SCREENSHOT_CLASSIFICATION_INPUTS,
                [],
            )?;
        }
        AiFeature::DocumentDetection => {
            transaction.execute(queries::ai_jobs::DELETE_DOCUMENT_CLASSIFICATIONS, [])?;
            transaction.execute(queries::ai_jobs::DELETE_DOCUMENT_CLASSIFICATION_INPUTS, [])?;
        }
        AiFeature::FaceDetection | AiFeature::Deduplicate => unreachable!(),
    }
    transaction.execute(
        queries::ai_jobs::DELETE_JOBS_FOR_TASK,
        [feature.inference_task()],
    )?;
    transaction.commit()?;
    Ok(())
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

fn failed_actions(action: &str, error: AppError) -> Vec<AiFeatureActionResult> {
    let error = error.to_string();
    AiFeature::ALL
        .into_iter()
        .map(|feature| action_failure(feature, format!("{action}: {error}")))
        .collect()
}

fn action_failure(feature: AiFeature, error: String) -> AiFeatureActionResult {
    AiFeatureActionResult {
        feature: feature.name().to_string(),
        outcome: "failed".to_string(),
        affected_jobs: 0,
        error: Some(error),
    }
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
