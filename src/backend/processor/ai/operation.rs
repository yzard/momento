use crate::config::Config;
use crate::constants::{
    DOCUMENT_DETECTION_MODEL_TYPE, IMAGE_AESTHETICS_MODEL_TYPE, IMAGE_TAGGING_MODEL_TYPE,
    OCR_MODEL_TYPE, SCREENSHOT_DETECTION_MODEL_TYPE,
};
use crate::database::DbPool;
use crate::error::{AppError, AppResult};
use crate::processor::{deduplicator, face_detection};
use tracing::warn;

use super::queue_task;

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

    pub fn name(self) -> &'static str {
        match self {
            Self::Ocr => OCR_MODEL_TYPE,
            Self::ImageTagging => IMAGE_TAGGING_MODEL_TYPE,
            Self::ImageAesthetics => IMAGE_AESTHETICS_MODEL_TYPE,
            Self::ScreenshotDetection => SCREENSHOT_DETECTION_MODEL_TYPE,
            Self::DocumentDetection => DOCUMENT_DETECTION_MODEL_TYPE,
            Self::FaceDetection => "face_detection",
            Self::Deduplicate => "deduplicate",
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
