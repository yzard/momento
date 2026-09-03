use std::str::FromStr;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use cron::Schedule;
use tracing::{info, warn};

use crate::config::{Config, ConfigManager};
use crate::error::{AppError, AppResult};
use crate::executor::{CpuExecutorHandle, SqliteExecutorHandle};
use crate::processor::ai::operation::AiFeature;
use crate::processor::ai::transport::TransportHandle;
use crate::processor::deduplicator::log_schedule_start;
use crate::runtime::{
    DurableSourceId, SchedulerAdmissionKind, SchedulerHandle, SystemTimezoneSnapshot,
};

const CATCH_UP_CONTINUATION_OCCURRENCES: usize = 256;

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
    const ALL: [Self; 7] = [
        Self::Ocr,
        Self::ImageTagging,
        Self::ImageAesthetics,
        Self::ScreenshotDetection,
        Self::DocumentDetection,
        Self::Deduplicate,
        Self::FaceDetection,
    ];

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
}

#[derive(Clone, Copy)]
struct CronTimerState {
    task: ScheduledTask,
    next: DateTime<Utc>,
}

pub fn next_scheduled_at(
    cron_expression: &str,
    job_name: &str,
    after: DateTime<Utc>,
    timezone: Tz,
) -> AppResult<DateTime<Utc>> {
    let schedule = Schedule::from_str(&format!("0 {cron_expression} *"))
        .map_err(|error| AppError::Validation(format!("invalid {job_name} cronjob: {error}")))?;
    schedule
        .after(&after.with_timezone(&timezone))
        .next()
        .map(|date| date.with_timezone(&Utc))
        .ok_or_else(|| AppError::Validation(format!("{job_name} cronjob has no next occurrence")))
}

pub async fn run_cronjobs(
    config_manager: ConfigManager,
    sqlite: SqliteExecutorHandle,
    cpu: CpuExecutorHandle,
    transport: TransportHandle,
    scheduler: SchedulerHandle,
    system_timezone: SystemTimezoneSnapshot,
) {
    let timezone = system_timezone.timezone();
    let mut config_updates = config_manager.subscribe();
    if config_updates.borrow().llm.enabled {
        let deduplicate_cron = AiFeature::Deduplicate
            .cron_expression(&config_updates.borrow().llm)
            .to_string();
        let startup = scheduler
            .acquire_durable(
                DurableSourceId::Maintenance,
                SchedulerAdmissionKind::RecoveryHandoff,
            )
            .await;
        let startup_result = match startup.map_err(AppError::Internal) {
            Ok(_admission) => {
                run_startup_or_catch_up(&sqlite, &cpu, &deduplicate_cron, timezone).await
            }
            Err(error) => Err(error),
        };
        match startup_result {
            Ok(queued) if queued > 0 => transport.wake_submissions(),
            Ok(_) => {}
            Err(error) => warn!("Deduplicate startup scheduling failed: {error}"),
        }
    }

    let mut timers = match build_timer_states(&config_updates.borrow(), Utc::now(), timezone) {
        Ok(timers) => timers,
        Err(error) => {
            warn!("AI schedule initialization failed: {error}");
            Vec::new()
        }
    };
    loop {
        if timers.is_empty() {
            if config_updates.changed().await.is_err() {
                return;
            }
            timers = match build_timer_states(&config_updates.borrow(), Utc::now(), timezone) {
                Ok(timers) => timers,
                Err(error) => {
                    warn!("AI schedule update failed: {error}");
                    continue;
                }
            };
            continue;
        }

        timers.sort_by_key(|timer| timer.next);
        let next_due = timers[0].next;
        let delay = (next_due - Utc::now())
            .to_std()
            .unwrap_or(std::time::Duration::ZERO);
        tokio::select! {
            config_changed = config_updates.changed() => {
                if config_changed.is_err() {
                    return;
                }
                timers = match build_timer_states(&config_updates.borrow(), Utc::now(), timezone) {
                    Ok(timers) => timers,
                    Err(error) => {
                        warn!("AI schedule update failed: {error}");
                        Vec::new()
                    }
                };
                continue;
            }
            () = tokio::time::sleep(delay) => {}
        }

        let now = Utc::now();
        let config = config_updates.borrow().clone();
        for timer in timers.iter_mut().filter(|timer| timer.next <= now) {
            let scheduled_for = timer.next.to_rfc3339();
            let admission = scheduler
                .acquire_durable(
                    DurableSourceId::Maintenance,
                    SchedulerAdmissionKind::NewClaim,
                )
                .await;
            match admission {
                Ok(_admission) => {
                    if timer.task == ScheduledTask::Deduplicate {
                        log_schedule_start(&scheduled_for);
                    }
                    match run_scheduled_occurrence(
                        config.as_ref(),
                        &sqlite,
                        timer.task,
                        &scheduled_for,
                    )
                    .await
                    {
                        Ok(queued) => {
                            if queued > 0 {
                                transport.wake_submissions();
                            }
                            info!(task = timer.task.name(), queued, "Scheduled AI task queued");
                        }
                        Err(error) => {
                            warn!(
                                task = timer.task.name(),
                                "Scheduled AI task failed: {error}"
                            )
                        }
                    }
                }
                Err(error) => {
                    warn!(task = timer.task.name(), error, "Scheduled AI task stopped");
                    return;
                }
            }
            let cron_expression = timer.task.feature().cron_expression(&config.llm);
            match next_scheduled_at(cron_expression, timer.task.name(), timer.next, timezone) {
                Ok(next) => timer.next = next,
                Err(error) => {
                    warn!(
                        task = timer.task.name(),
                        "Schedule evaluation failed: {error}"
                    );
                    timer.next = now + chrono::Duration::days(365 * 100);
                }
            }
        }
    }
}

fn build_timer_states(
    config: &Config,
    after: DateTime<Utc>,
    timezone: Tz,
) -> AppResult<Vec<CronTimerState>> {
    if !config.llm.enabled {
        return Ok(Vec::new());
    }
    ScheduledTask::ALL
        .into_iter()
        .map(|task| {
            next_scheduled_at(
                task.feature().cron_expression(&config.llm),
                task.name(),
                after,
                timezone,
            )
            .map(|next| CronTimerState { task, next })
        })
        .collect()
}

pub async fn run_scheduled_occurrence(
    config: &Config,
    sqlite: &SqliteExecutorHandle,
    task: ScheduledTask,
    scheduled_for: &str,
) -> AppResult<usize> {
    if !config.llm.enabled {
        return Ok(0);
    }
    Ok(sqlite
        .start_ai_feature_durable(
            task.feature(),
            "scheduled".to_string(),
            Some(scheduled_for.to_string()),
        )
        .await?)
}

async fn run_startup_or_catch_up(
    sqlite: &SqliteExecutorHandle,
    cpu: &CpuExecutorHandle,
    deduplicate_cron: &str,
    timezone: Tz,
) -> AppResult<usize> {
    let state = sqlite.load_deduplicate_schedule_state_durable().await?;
    if state
        .latest_run_status
        .is_some_and(|status| status == "interrupted" || status == "failed")
    {
        return Ok(sqlite
            .start_ai_feature_durable(AiFeature::Deduplicate, "startup".to_string(), None)
            .await?);
    }
    let last_scheduled = state
        .last_scheduled_for
        .map(|value| {
            crate::utils::datetime::parse_datetime(&value)
                .ok_or_else(|| AppError::Internal(format!("invalid stored schedule: {value}")))
        })
        .transpose()?;
    let now = Utc::now();
    let trigger = if let Some(last_scheduled) = last_scheduled {
        let mut cursor = last_scheduled;
        let mut latest_due = None;
        loop {
            let page = cpu
                .compute_cron_catch_up_page_durable(
                    deduplicate_cron.to_string(),
                    cursor,
                    now,
                    timezone,
                    CATCH_UP_CONTINUATION_OCCURRENCES as u16,
                )
                .await?;
            if let Some(page_latest) = page.latest_due {
                latest_due = Some(page_latest);
                cursor = page_latest;
            }
            if !page.continuation_required {
                break;
            }
        }
        let Some(latest_due) = latest_due else {
            return Ok(0);
        };
        ("scheduled", Some(latest_due.to_rfc3339()))
    } else {
        ("startup", None)
    };

    Ok(sqlite
        .start_ai_feature_durable(AiFeature::Deduplicate, trigger.0.to_string(), trigger.1)
        .await?)
}
