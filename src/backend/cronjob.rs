use std::str::FromStr;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use cron::Schedule;
use tracing::{info, warn};

use crate::config::{Config, CronjobConfig};
use crate::database::{fetch_one, queries, DbPool};
use crate::error::{AppError, AppResult};
use crate::processor::ai::operation::{start_feature, AiFeature, AiStartSource};
use crate::processor::ai::transport::TransportHandle;
use crate::processor::deduplicator::{latest_run, log_schedule_start};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScheduledTask {
    Ocr,
    ImageTagging,
    ImageAesthetics,
    ScreenshotDetection,
    DocumentDetection,
    Deduplicate,
    FaceDetection,
}

impl ScheduledTask {
    fn feature(self) -> AiFeature {
        match self {
            Self::Ocr => AiFeature::Ocr,
            Self::ImageTagging => AiFeature::ImageTagging,
            Self::ImageAesthetics => AiFeature::ImageAesthetics,
            Self::ScreenshotDetection => AiFeature::ScreenshotDetection,
            Self::DocumentDetection => AiFeature::DocumentDetection,
            Self::Deduplicate => AiFeature::Deduplicate,
            Self::FaceDetection => AiFeature::FaceDetection,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Ocr => "ocr",
            Self::ImageTagging => "image_tagging",
            Self::ImageAesthetics => "image_aesthetics",
            Self::ScreenshotDetection => "screenshot_detection",
            Self::DocumentDetection => "document_detection",
            Self::Deduplicate => "deduplicate",
            Self::FaceDetection => "face_detection",
        }
    }

    fn cron_expression(self, config: &CronjobConfig) -> &str {
        match self {
            Self::Ocr => &config.ocr_cron,
            Self::ImageTagging => &config.image_tagging_cron,
            Self::ImageAesthetics => &config.image_aesthetics_cron,
            Self::ScreenshotDetection => &config.screenshot_detection_cron,
            Self::DocumentDetection => &config.document_detection_cron,
            Self::Deduplicate => &config.deduplicate_cron,
            Self::FaceDetection => &config.face_detection_cron,
        }
    }

    fn is_enabled(self, config: &Config) -> bool {
        self.feature().is_enabled(config)
    }
}

pub fn next_scheduled_at(
    config: &CronjobConfig,
    cron_expression: &str,
    job_name: &str,
    after: DateTime<Utc>,
) -> AppResult<DateTime<Utc>> {
    let timezone = config
        .timezone
        .parse::<chrono_tz::Tz>()
        .map_err(|error| AppError::Validation(format!("invalid cronjob timezone: {error}")))?;
    let schedule = Schedule::from_str(&format!("0 {cron_expression} *"))
        .map_err(|error| AppError::Validation(format!("invalid {job_name} cronjob: {error}")))?;
    schedule
        .after(&after.with_timezone(&timezone))
        .next()
        .map(|date| date.with_timezone(&Utc))
        .ok_or_else(|| AppError::Validation(format!("{job_name} cronjob has no next occurrence")))
}

pub async fn run_cronjobs(config: Arc<Config>, pool: DbPool, transport: TransportHandle) {
    let mut cronjobs = tokio::task::JoinSet::new();
    for task in [
        ScheduledTask::Ocr,
        ScheduledTask::ImageTagging,
        ScheduledTask::ImageAesthetics,
        ScheduledTask::ScreenshotDetection,
        ScheduledTask::DocumentDetection,
        ScheduledTask::Deduplicate,
        ScheduledTask::FaceDetection,
    ] {
        if !task.is_enabled(&config) {
            info!(task = task.name(), "Scheduled AI task is disabled");
            continue;
        }
        let task_config = Arc::clone(&config);
        let task_pool = pool.clone();
        let task_transport = transport.clone();
        cronjobs.spawn(async move {
            if task == ScheduledTask::Deduplicate {
                run_deduplicate_cronjob(task_config, task_pool, task_transport).await;
            } else {
                run_task_cronjob(task_config, task_pool, task_transport, task).await;
            }
        });
    }

    while let Some(cronjob) = cronjobs.join_next().await {
        if let Err(error) = cronjob {
            warn!("Cronjob task stopped unexpectedly: {error}");
        }
    }
}

async fn run_task_cronjob(
    config: Arc<Config>,
    pool: DbPool,
    transport: TransportHandle,
    task: ScheduledTask,
) {
    loop {
        let now = Utc::now();
        let next = match next_scheduled_at(
            &config.cronjob,
            task.cron_expression(&config.cronjob),
            task.name(),
            now,
        ) {
            Ok(next) => next,
            Err(error) => {
                warn!(task = task.name(), "Schedule evaluation failed: {error}");
                return;
            }
        };
        let delay = (next - now)
            .to_std()
            .unwrap_or_else(|_| std::time::Duration::from_secs(1));
        tokio::time::sleep(delay).await;
        match run_scheduled_occurrence(&config, &pool, task, &next.to_rfc3339()) {
            Ok(queued) => {
                if queued > 0 {
                    transport.wake_submissions();
                }
                info!(task = task.name(), queued, "Scheduled AI task queued");
            }
            Err(error) => warn!(task = task.name(), "Scheduled AI task failed: {error}"),
        }
    }
}

pub fn run_scheduled_occurrence(
    config: &Config,
    pool: &DbPool,
    task: ScheduledTask,
    scheduled_for: &str,
) -> AppResult<usize> {
    if !task.is_enabled(config) {
        return Ok(0);
    }
    start_feature(
        config,
        pool,
        task.feature(),
        AiStartSource::Scheduled { scheduled_for },
    )
}

async fn run_deduplicate_cronjob(config: Arc<Config>, pool: DbPool, transport: TransportHandle) {
    match run_startup_or_catch_up(&config, &pool).await {
        Ok(queued) if queued > 0 => transport.wake_submissions(),
        Ok(_) => {}
        Err(error) => warn!("Deduplicate startup scheduling failed: {}", error),
    }

    loop {
        let now = Utc::now();
        let next = match next_scheduled_at(
            &config.cronjob,
            &config.cronjob.deduplicate_cron,
            "deduplicate",
            now,
        ) {
            Ok(next) => next,
            Err(error) => {
                warn!("Deduplicate schedule evaluation failed: {}", error);
                return;
            }
        };
        let delay = (next - now)
            .to_std()
            .unwrap_or_else(|_| std::time::Duration::from_secs(1));
        tokio::time::sleep(delay).await;
        let scheduled_for = next.to_rfc3339();
        log_schedule_start(&scheduled_for);
        match run_scheduled_occurrence(&config, &pool, ScheduledTask::Deduplicate, &scheduled_for) {
            Ok(queued) => {
                if queued > 0 {
                    transport.wake_submissions();
                }
                info!(queued, "Scheduled deduplicate jobs queued");
            }
            Err(error) => warn!("Could not queue scheduled deduplicate jobs: {error}"),
        }
    }
}

async fn run_startup_or_catch_up(config: &Config, pool: &DbPool) -> AppResult<usize> {
    if latest_run(pool)?.is_some_and(|run| run.status == "interrupted" || run.status == "failed") {
        return start_feature(
            config,
            pool,
            AiFeature::Deduplicate,
            AiStartSource::StartupRecovery,
        );
    }
    let last_scheduled = last_scheduled_for(pool)?;
    let now = Utc::now();
    let trigger = if let Some(last_scheduled) = last_scheduled {
        let mut occurrence = next_scheduled_at(
            &config.cronjob,
            &config.cronjob.deduplicate_cron,
            "deduplicate",
            last_scheduled,
        )?;
        let mut latest_due = None;
        while occurrence <= now {
            latest_due = Some(occurrence);
            occurrence = next_scheduled_at(
                &config.cronjob,
                &config.cronjob.deduplicate_cron,
                "deduplicate",
                occurrence,
            )?;
        }
        let Some(latest_due) = latest_due else {
            return Ok(0);
        };
        ("scheduled", Some(latest_due.to_rfc3339()))
    } else {
        ("startup", None)
    };

    let source = match trigger {
        ("scheduled", Some(ref scheduled_for)) => AiStartSource::Scheduled { scheduled_for },
        _ => AiStartSource::StartupRecovery,
    };
    start_feature(config, pool, AiFeature::Deduplicate, source)
}

fn last_scheduled_for(pool: &DbPool) -> AppResult<Option<DateTime<Utc>>> {
    let connection = pool.get().map_err(AppError::Pool)?;
    let scheduled_for = fetch_one(
        &connection,
        queries::deduplicate::SELECT_LAST_SCHEDULED_FOR,
        &[],
        |row| row.get::<_, String>(0),
    )?;
    scheduled_for
        .map(|value| {
            crate::utils::datetime::parse_datetime(&value)
                .ok_or_else(|| AppError::Internal(format!("invalid stored schedule: {value}")))
        })
        .transpose()
}
