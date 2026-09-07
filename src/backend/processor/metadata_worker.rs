use std::sync::Arc;

use futures::{stream::FuturesUnordered, StreamExt};

use tracing::warn;

use crate::config::Config;
use crate::database::operations::FinishMetadataJob;
use crate::executor::ExecutorErrorKind;
use crate::processor::ai::input::AiInputStorage;
use crate::runtime::{DurableSourceId, ExecutorHandles, SchedulerAdmissionKind, SchedulerHandle};

#[cfg(test)]
#[path = "../../../tests/backend/processor/metadata_worker.rs"]
mod tests;

struct MetadataClaimGuard {
    sqlite: crate::executor::SqliteExecutorHandle,
    scheduler: SchedulerHandle,
    admission: Option<crate::runtime::DurableAdmission>,
    registration: Option<crate::runtime::ActiveDurableClaim>,
    claim: Option<crate::database::operations::MetadataJobClaim>,
    intended_error: Option<Option<String>>,
    intended_retryable: bool,
    retry_handoff: bool,
}

impl MetadataClaimGuard {
    fn new(
        sqlite: crate::executor::SqliteExecutorHandle,
        scheduler: SchedulerHandle,
        admission: crate::runtime::DurableAdmission,
        registration: crate::runtime::ActiveDurableClaim,
        claim: crate::database::operations::MetadataJobClaim,
    ) -> Self {
        Self {
            sqlite,
            scheduler,
            admission: Some(admission),
            registration: Some(registration),
            claim: Some(claim),
            intended_error: None,
            intended_retryable: true,
            retry_handoff: true,
        }
    }

    fn media_id(&self) -> i64 {
        self.claim.as_ref().expect("active metadata claim").media_id
    }

    fn claim_token(&self) -> &str {
        &self
            .claim
            .as_ref()
            .expect("active metadata claim")
            .claim_token
    }

    async fn resolve(mut self, error: Option<String>, retryable: bool) -> Result<(), String> {
        self.intended_error = Some(error.clone());
        self.intended_retryable = retryable;
        let claim = self.claim.as_ref().expect("active metadata claim");
        match self
            .sqlite
            .finish_metadata_job_durable(FinishMetadataJob {
                media_id: claim.media_id,
                claim_token: claim.claim_token.clone(),
                error,
                retryable,
            })
            .await
        {
            Ok(()) => {}
            Err(error) => {
                self.retry_handoff = metadata_finish_error_is_retryable(error.kind);
                return Err(error.to_string());
            }
        }
        self.claim.take();
        self.registration.take();
        self.admission.take();
        Ok(())
    }
}

impl Drop for MetadataClaimGuard {
    fn drop(&mut self) {
        let Some(claim) = self.claim.take() else {
            return;
        };
        let admission = self.admission.take();
        let registration = self.registration.take();
        let sqlite = self.sqlite.clone();
        let scheduler = self.scheduler.clone();
        let retry_handoff = self.retry_handoff;
        let error = self.intended_error.take().unwrap_or_else(|| {
            Some("metadata orchestration exited before resolving its claim".to_string())
        });
        let request = FinishMetadataJob {
            media_id: claim.media_id,
            claim_token: claim.claim_token,
            error,
            retryable: self.intended_retryable,
        };
        scheduler.clone().spawn_control(async move {
            retry_metadata_claim_handoff(
                sqlite,
                scheduler,
                request,
                admission,
                registration,
                retry_handoff,
            )
            .await;
        });
    }
}

async fn retry_metadata_claim_handoff(
    sqlite: crate::executor::SqliteExecutorHandle,
    scheduler: SchedulerHandle,
    request: FinishMetadataJob,
    admission: Option<crate::runtime::DurableAdmission>,
    registration: Option<crate::runtime::ActiveDurableClaim>,
    mut retry_handoff: bool,
) {
    drop(admission);
    if !retry_handoff {
        warn!(
            media_id = request.media_id,
            "metadata claim handoff failed permanently; startup recovery will requeue it"
        );
        drop(registration);
        return;
    }
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let admission = match scheduler
            .acquire_durable(
                DurableSourceId::Metadata,
                SchedulerAdmissionKind::ExistingClaimCompletion,
            )
            .await
        {
            Ok(admission) => admission,
            Err(acquire_error) => {
                warn!(
                    media_id = request.media_id,
                    error = %acquire_error,
                    "metadata claim handoff stopped; startup recovery will requeue it"
                );
                drop(registration);
                return;
            }
        };
        let outcome = sqlite.finish_metadata_job_durable(request.clone()).await;
        drop(admission);
        match outcome {
            Ok(()) => {
                drop(registration);
                return;
            }
            Err(finish_error) => {
                retry_handoff = metadata_finish_error_is_retryable(finish_error.kind);
                warn!(
                    media_id = request.media_id,
                    error = %finish_error,
                    retrying = retry_handoff,
                    "metadata claim handoff failed after releasing its worker"
                );
                if !retry_handoff {
                    drop(registration);
                    return;
                }
            }
        }
    }
}

fn metadata_finish_error_is_retryable(kind: ExecutorErrorKind) -> bool {
    matches!(
        kind,
        ExecutorErrorKind::Overloaded
            | ExecutorErrorKind::DatabaseBusy
            | ExecutorErrorKind::DatabaseTimeout
            | ExecutorErrorKind::Database
            | ExecutorErrorKind::FileTransient
    )
}

pub async fn run(config: Arc<Config>, executors: ExecutorHandles, scheduler: SchedulerHandle) {
    let mut observed_version = scheduler.metadata_work_version();
    loop {
        let retry_delay = match process_cycle(&config, &executors, &scheduler).await {
            Ok(()) => match executors
                .sqlite
                .load_next_metadata_job_delay_durable()
                .await
            {
                Ok(delay) => delay,
                Err(error) => {
                    warn!("failed to load the next metadata retry deadline: {error}");
                    Some(std::time::Duration::from_secs(1))
                }
            },
            Err(error) => {
                warn!("metadata worker cycle failed: {error}");
                Some(std::time::Duration::from_secs(1))
            }
        };
        let current_version = scheduler.metadata_work_version();
        if current_version != observed_version {
            observed_version = current_version;
            continue;
        }
        match retry_delay {
            Some(delay) => {
                tokio::select! {
                    version = scheduler.wait_for_metadata_work(observed_version) => {
                        observed_version = version;
                    }
                    () = tokio::time::sleep(delay) => {}
                }
            }
            None => {
                observed_version = scheduler.wait_for_metadata_work(observed_version).await;
            }
        }
    }
}

async fn process_cycle(
    config: &Config,
    executors: &ExecutorHandles,
    scheduler: &SchedulerHandle,
) -> Result<(), String> {
    let sqlite = &executors.sqlite;
    let concurrency = scheduler.durable_capacity();
    {
        let _worker_permit = scheduler
            .acquire_durable(DurableSourceId::Metadata, SchedulerAdmissionKind::NewClaim)
            .await
            .map_err(|error| error.to_string())?;
        let mut clean_progressed = false;
        while sqlite
            .continue_metadata_clean_durable()
            .await
            .map_err(|error| error.to_string())?
        {
            clean_progressed = true;
        }
        if clean_progressed {
            scheduler.wake_journal_recovery();
        }
    }
    drain_metadata_window(
        concurrency,
        scheduler.metadata_work_version(),
        || process_metadata_job(config, executors, scheduler),
        |version| scheduler.wait_for_metadata_work(version),
    )
    .await
}

async fn drain_metadata_window<Job, Wake>(
    concurrency: usize,
    mut observed_version: u64,
    make_job: impl Fn() -> Job,
    wait_for_work: impl Fn(u64) -> Wake,
) -> Result<(), String>
where
    Job: std::future::Future<Output = Result<bool, String>>,
    Wake: std::future::Future<Output = u64>,
{
    let mut lanes = FuturesUnordered::new();
    for _ in 0..concurrency {
        lanes.push(make_job());
    }
    let mut failure = None;
    while !lanes.is_empty() {
        tokio::select! {
            outcome = lanes.next() => {
                match outcome.expect("nonempty metadata window") {
                    Ok(true) if failure.is_none() => lanes.push(make_job()),
                    Err(error) => { failure.get_or_insert(error); },
                    _ => {},
                }
            }
            version = wait_for_work(observed_version), if failure.is_none() => {
                observed_version = version;
                while lanes.len() < concurrency {
                    lanes.push(make_job());
                }
            }
        }
    }
    failure.map_or(Ok(()), Err)
}

async fn process_metadata_job(
    config: &Config,
    executors: &ExecutorHandles,
    scheduler: &SchedulerHandle,
) -> Result<bool, String> {
    let sqlite = &executors.sqlite;
    let admission = scheduler
        .acquire_durable(DurableSourceId::Metadata, SchedulerAdmissionKind::NewClaim)
        .await
        .map_err(|error| error.to_string())?;
    let claim = sqlite
        .claim_next_metadata_job_durable()
        .await
        .map_err(|error| error.to_string())?;
    let Some(claim) = claim else {
        drop(admission);
        return Ok(false);
    };
    let registration = match scheduler.register_durable_claim(&admission, claim.claim_token.clone())
    {
        Ok(registration) => registration,
        Err(error) => {
            sqlite
                .finish_metadata_job_durable(FinishMetadataJob {
                    media_id: claim.media_id,
                    claim_token: claim.claim_token,
                    error: Some(format!("metadata claim registration failed: {error}")),
                    retryable: true,
                })
                .await
                .map_err(|finish_error| {
                    format!("{error}; metadata claim recovery also failed: {finish_error}")
                })?;
            return Err(error);
        }
    };
    let claim_guard = MetadataClaimGuard::new(
        sqlite.clone(),
        scheduler.clone(),
        admission,
        registration,
        claim,
    );
    let media_id = claim_guard.media_id();
    let outcome = match crate::processor::metadata::generate_media_metadata(
        executors,
        media_id,
        claim_guard.claim_token(),
        config,
    )
    .await
    {
        Ok(()) => verify_ai_inputs(executors, media_id, config)
            .await
            .map_err(Into::into),
        Err(error) => Err(error),
    };
    let retryable = outcome
        .as_ref()
        .err()
        .is_some_and(|error| error.retryable());
    if let Err(error) = &outcome {
        match error {
            crate::processor::metadata::MetadataGenerationError::MagickMemoryQuotaExceeded {
                source_path,
                requested_bytes,
                quota_bytes,
            } => tracing::error!(
                media_id, source_path, requested_bytes, quota_bytes,
                error_code = "magick_memory_quota_exceeded", retryable = false,
                error = %error,
                "Metadata request exceeds total ImageMagick quota; marking failed without automatic retry"
            ),
            _ => {
                warn!(media_id, error = %error, retryable, "metadata processing failed; queued for retry")
            }
        }
    }
    if let Err(error) = claim_guard
        .resolve(outcome.err().map(|error| error.to_string()), retryable)
        .await
    {
        warn!("failed to persist metadata job {media_id} outcome: {error}");
    }
    Ok(true)
}

async fn verify_ai_inputs(
    executors: &ExecutorHandles,
    media_id: i64,
    config: &Config,
) -> Result<(), String> {
    if !config.llm.enabled {
        return Ok(());
    }
    let verification = executors
        .sqlite
        .load_metadata_ai_input_verification_durable(media_id)
        .await
        .map_err(|error| error.to_string())?;
    for input in verification.inputs {
        let storage = AiInputStorage::parse(&input.storage_root)?;
        let path = storage.normalized_path(&input.file_path)?;
        let (session, _) = executors
            .file_io
            .open_storage_read_session_durable(storage.storage_root_id(), path)
            .await
            .map_err(|error| {
                if error.kind == ExecutorErrorKind::FileNotFound {
                    format!("prepared {} AI input file is missing", input.task)
                } else {
                    error.to_string()
                }
            })?;
        executors
            .file_io
            .close_storage_session_durable(session)
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}
