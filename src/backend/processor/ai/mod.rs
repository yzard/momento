use std::sync::Arc;
use std::time::Instant;

use futures::stream::{FuturesUnordered, StreamExt};
use momento_common::llm::{CancelJobsRequest, JobInputDescriptor, JobManifest};
use thiserror::Error;

use crate::config::Config;
use crate::database::operations::{
    AcknowledgeLlmCancellation, FinishLlmSubmission, LlmSubmissionJob,
};
use crate::executor::{CpuExecutorHandle, FileIoExecutorHandle, SqliteExecutorHandle};
use crate::io::file::{NormalizedStoragePath, StorageRootId};
use crate::io::StorageFileSession;
use crate::runtime::{DurableSourceId, ExecutorHandles, SchedulerAdmissionKind, SchedulerHandle};

pub mod input;
pub mod operation;
pub mod result;
pub mod transport;

use transport::{LlmConnection, PreparedSubmissionInput, SubmissionOutcome, TransportHandle};

use self::input::AiInputStorage;

const CANCELLATION_CHUNK_SIZE: usize = 1000;
const TRANSPORT_RECONNECT_DELAY: std::time::Duration = std::time::Duration::from_secs(5);
const WORKER_ERROR_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(1);

pub async fn run(
    config: Arc<Config>,
    executors: ExecutorHandles,
    handle: TransportHandle,
    scheduler: SchedulerHandle,
) {
    let sqlite = executors.sqlite.clone();
    if !config.llm.enabled {
        return;
    }
    loop {
        let connection_result = match scheduler
            .acquire_durable(
                DurableSourceId::LlmSubmission,
                SchedulerAdmissionKind::NewClaim,
            )
            .await
        {
            Ok(_worker_permit) => {
                LlmConnection::connect(
                    &config.llm.server_address,
                    &config.llm.client_id,
                    &config.llm.api_key,
                    sqlite.clone(),
                    executors.file_io.clone(),
                    scheduler.clone(),
                )
                .await
            }
            Err(error) => Err(error),
        };
        let connection = match connection_result {
            Ok(connection) => connection,
            Err(error) => {
                tracing::warn!("LLM WebSocket connection failed: {error}");
                tokio::time::sleep(TRANSPORT_RECONNECT_DELAY).await;
                continue;
            }
        };
        tracing::info!(client_id = config.llm.client_id, "LLM WebSocket connected");
        let submission_executors = executors.clone();
        let submission_connection = connection.clone();
        let submission_handle = handle.clone();
        let submission_scheduler = scheduler.clone();
        let submission_loop = async move {
            let mut observed_version = submission_handle.submission_work_version();
            loop {
                let retry_delay = match submit_cycle(
                    &submission_executors,
                    &submission_connection,
                    &submission_scheduler,
                )
                .await
                {
                    Ok(()) => match submission_executors
                        .sqlite
                        .load_next_llm_submission_delay_durable()
                        .await
                    {
                        Ok(delay) => delay,
                        Err(error) => {
                            tracing::warn!(
                                "failed to load the next LLM submission retry deadline: {error}"
                            );
                            Some(WORKER_ERROR_RETRY_DELAY)
                        }
                    },
                    Err(error) => {
                        tracing::warn!("LLM submission cycle failed: {error}");
                        Some(WORKER_ERROR_RETRY_DELAY)
                    }
                };
                let current_version = submission_handle.submission_work_version();
                if current_version != observed_version {
                    observed_version = current_version;
                    continue;
                }
                match retry_delay {
                    Some(delay) => tokio::select! {
                        _ = submission_connection.closed() => return,
                        version = submission_handle.wait_for_submission_work(observed_version) => {
                            observed_version = version;
                        }
                        () = tokio::time::sleep(delay) => {}
                    },
                    None => tokio::select! {
                        _ = submission_connection.closed() => return,
                        version = submission_handle.wait_for_submission_work(observed_version) => {
                            observed_version = version;
                        }
                    },
                }
            }
        };
        let cancellation_sqlite = sqlite.clone();
        let cancellation_connection = connection.clone();
        let cancellation_handle = handle.clone();
        let cancellation_scheduler = scheduler.clone();
        let cancellation_loop = async move {
            let mut observed_version = cancellation_handle.cancellation_work_version();
            loop {
                let delivery = match cancellation_scheduler
                    .acquire_durable(
                        DurableSourceId::LlmCancellation,
                        SchedulerAdmissionKind::NewClaim,
                    )
                    .await
                {
                    Ok(_worker_permit) => {
                        deliver_pending_cancellations(
                            &cancellation_sqlite,
                            &cancellation_connection,
                        )
                        .await
                    }
                    Err(error) => Err(error),
                };
                let retry_after_failure = match delivery {
                    Ok(delivered) => {
                        if delivered > 0 {
                            cancellation_handle.wake_submissions();
                        }
                        false
                    }
                    Err(error) => {
                        tracing::warn!("LLM cancellation delivery failed: {error}");
                        true
                    }
                };
                let current_version = cancellation_handle.cancellation_work_version();
                if current_version != observed_version {
                    observed_version = current_version;
                    continue;
                }
                if retry_after_failure {
                    tokio::select! {
                        _ = cancellation_connection.closed() => return,
                        version = cancellation_handle.wait_for_cancellation_work(observed_version) => {
                            observed_version = version;
                        }
                        () = tokio::time::sleep(WORKER_ERROR_RETRY_DELAY) => {}
                    }
                } else {
                    tokio::select! {
                        _ = cancellation_connection.closed() => return,
                        version = cancellation_handle.wait_for_cancellation_work(observed_version) => {
                            observed_version = version;
                        }
                    }
                }
            }
        };
        tokio::join!(submission_loop, cancellation_loop);
        tracing::warn!(
            client_id = config.llm.client_id,
            "LLM WebSocket disconnected"
        );
        tokio::time::sleep(TRANSPORT_RECONNECT_DELAY).await;
    }
}

pub async fn deliver_pending_cancellations(
    sqlite: &SqliteExecutorHandle,
    connection: &LlmConnection,
) -> Result<usize, String> {
    let mut delivered = 0;
    loop {
        let Some(cancellation) = sqlite
            .load_llm_cancellation_batch_durable(CANCELLATION_CHUNK_SIZE as u16)
            .await
            .map_err(|error| error.to_string())?
        else {
            return Ok(delivered);
        };
        let scope = cancellation.scope;
        let task = cancellation.task;
        let job_ids = cancellation.job_ids;
        let response = connection
            .cancel(CancelJobsRequest {
                all: scope == "all",
                tasks: if scope == "all" {
                    Vec::new()
                } else {
                    vec![task.clone()]
                },
                job_ids: job_ids.clone(),
            })
            .await
            .map_err(|error| format!("llm service rejected cancellation: {error}"))?;
        if response.requested_jobs != job_ids.len() {
            return Err("llm cancellation response count does not match request".to_string());
        }
        sqlite
            .acknowledge_llm_cancellation_durable(AcknowledgeLlmCancellation {
                scope,
                task,
                job_ids: job_ids.clone(),
            })
            .await
            .map_err(|error| error.to_string())?;
        delivered += job_ids.len();
    }
}

async fn submit_cycle(
    executors: &ExecutorHandles,
    connection: &LlmConnection,
    scheduler: &SchedulerHandle,
) -> Result<(), String> {
    {
        let _worker_permit = scheduler
            .acquire_durable(
                DurableSourceId::LlmSubmission,
                SchedulerAdmissionKind::NewClaim,
            )
            .await?;
        executors
            .sqlite
            .prepare_llm_submission_cycle_durable()
            .await
            .map_err(|error| error.to_string())?;
    }
    let maximum_in_flight = scheduler.outbound_stream_capacity();
    let mut in_flight = FuturesUnordered::new();
    let mut first_error = None;
    loop {
        if first_error.is_none() && in_flight.len() < maximum_in_flight {
            let capacity = maximum_in_flight - in_flight.len();
            let jobs = {
                let _worker_permit = scheduler
                    .acquire_durable(
                        DurableSourceId::LlmSubmission,
                        SchedulerAdmissionKind::NewClaim,
                    )
                    .await?;
                executors
                    .sqlite
                    .claim_llm_submission_jobs_durable(capacity as u16)
                    .await
                    .map_err(|error| error.to_string())?
            };
            for job in jobs {
                in_flight.push(submit_claimed_job(executors, connection, scheduler, job));
            }
        }
        let Some(result) = in_flight.next().await else {
            return first_error.map_or(Ok(()), Err);
        };
        if let Err(error) = result {
            first_error.get_or_insert(error);
        }
    }
}

async fn submit_claimed_job(
    executors: &ExecutorHandles,
    connection: &LlmConnection,
    scheduler: &SchedulerHandle,
    job: LlmSubmissionJob,
) -> Result<(), String> {
    let outbound_admission = scheduler.acquire_outbound_stream().await?;
    let worker_permit = scheduler
        .acquire_durable(
            DurableSourceId::LlmSubmission,
            SchedulerAdmissionKind::ExistingClaimCompletion,
        )
        .await?;
    let _claim_registration = scheduler
        .register_durable_claim(&worker_permit, job.job_id.clone())
        .map_err(|error| format!("could not register LLM submission claim: {error}"))?;
    let started = Instant::now();
    let job_id = job.job_id;
    let media_id = job.media_id;
    let task = job.task;
    let attempts = job.attempts;
    let task_name = task.clone();
    let inputs = executors
        .sqlite
        .load_llm_prepared_inputs_durable(job_id.clone())
        .await
        .map_err(|error| error.to_string())?;
    if inputs.is_empty() {
        return finish_submission(
            &executors.sqlite,
            FinishLlmSubmission::Failed {
                job_id,
                error: "missing prepared AI inputs".to_string(),
            },
        )
        .await;
    }
    let mut descriptors = Vec::new();
    let mut prepared_inputs = Vec::new();
    for input in inputs {
        let storage = match AiInputStorage::parse(&input.storage_root) {
            Ok(storage) => storage,
            Err(error) => {
                return fail_submission(&executors.sqlite, &job_id, error.to_string()).await
            }
        };
        let input_path = match storage.normalized_path(&input.file_path) {
            Ok(input_path) => input_path,
            Err(error) => {
                return fail_submission(&executors.sqlite, &job_id, error.to_string()).await
            }
        };
        let input_size = match u64::try_from(input.byte_size) {
            Ok(input_size) => input_size,
            Err(_) => {
                return fail_submission(
                    &executors.sqlite,
                    &job_id,
                    "prepared AI input has an invalid byte size".to_string(),
                )
                .await
            }
        };
        if input_size == 0 {
            return fail_submission(
                &executors.sqlite,
                &job_id,
                "prepared AI input must not be empty".to_string(),
            )
            .await;
        }
        let session = match open_verified_input(
            &executors.file_io,
            &executors.cpu,
            storage.storage_root_id(),
            input_path,
            input_size,
            &input.content_hash,
        )
        .await
        {
            Ok(session) => session,
            Err(error) => {
                return fail_submission(&executors.sqlite, &job_id, error.to_string()).await
            }
        };
        let sequence = match u32::try_from(input.sequence) {
            Ok(sequence) => sequence,
            Err(_) => {
                return fail_submission(
                    &executors.sqlite,
                    &job_id,
                    "prepared AI input sequence is invalid".to_string(),
                )
                .await
            }
        };
        prepared_inputs.push(PreparedSubmissionInput {
            sequence,
            file_io: executors.file_io.clone(),
            session,
        });
        descriptors.push(JobInputDescriptor {
            sequence,
            filename: input.filename,
            mime_type: input.mime_type,
            byte_size: input.byte_size as u64,
            content_hash: input.content_hash,
            input_kind: input.input_kind,
            frame_timestamp_ms: input.frame_timestamp_ms,
        });
    }
    let verification_ms = started.elapsed().as_secs_f64() * 1000.0;
    let manifest = JobManifest {
        job_id: job_id.clone(),
        media_id,
        task,
        attempt: (attempts + 1) as u32,
        inputs: descriptors,
    };
    let admission_started = Instant::now();
    drop(worker_permit);
    let response = connection.submit(manifest, prepared_inputs).await;
    tracing::debug!(
        job_id,
        task = task_name,
        verification_ms,
        admission_ms = admission_started.elapsed().as_secs_f64() * 1000.0,
        total_ms = started.elapsed().as_secs_f64() * 1000.0,
        "LLM job submission timing"
    );
    drop(outbound_admission);
    let completion = match response {
        Ok(SubmissionOutcome::Acknowledged { status }) if status == "queued" => {
            FinishLlmSubmission::Submitted {
                job_id,
                attempt: attempts + 1,
            }
        }
        Ok(SubmissionOutcome::Acknowledged { status }) => FinishLlmSubmission::Failed {
            job_id,
            error: format!("llm service acknowledged submission with status {status}"),
        },
        Ok(SubmissionOutcome::Deferred {
            retry_after,
            required_bytes,
            available_bytes,
        }) => {
            tracing::info!(
                job_id,
                required_bytes,
                available_bytes,
                retry_after_ms = retry_after.as_millis(),
                "LLM submission deferred by remote queue capacity"
            );
            let retry_after_seconds = i64::try_from(retry_after.as_secs().max(1))
                .map_err(|_| "LLM submission defer delay is too large".to_string())?;
            FinishLlmSubmission::Deferred {
                job_id,
                retry_after_seconds,
            }
        }
        Ok(SubmissionOutcome::Rejected {
            retryable: true,
            error,
        }) => FinishLlmSubmission::Retry { job_id, error },
        Ok(SubmissionOutcome::Rejected {
            retryable: false,
            error,
        }) => FinishLlmSubmission::Failed { job_id, error },
        Err(error) => {
            tracing::warn!(job_id, error, "LLM submission outcome was not acknowledged");
            FinishLlmSubmission::RequeueAmbiguous { job_id }
        }
    };
    let _worker_permit = scheduler
        .acquire_durable(
            DurableSourceId::LlmSubmission,
            SchedulerAdmissionKind::ExistingClaimCompletion,
        )
        .await?;
    finish_submission(&executors.sqlite, completion).await
}

pub async fn verify_prepared_input(
    file_io: &FileIoExecutorHandle,
    cpu: &CpuExecutorHandle,
    storage_root: StorageRootId,
    path: NormalizedStoragePath,
    expected_size: u64,
    expected_hash: &str,
) -> Result<(), PreparedInputError> {
    let session = open_verified_input(
        file_io,
        cpu,
        storage_root,
        path,
        expected_size,
        expected_hash,
    )
    .await?;
    file_io
        .close_storage_session_durable(session)
        .await
        .map_err(|error| PreparedInputError::Executor(error.to_string()))
}

#[derive(Debug, Error)]
pub enum PreparedInputError {
    #[error("prepared AI input no longer matches its durable descriptor")]
    Changed,
    #[error("prepared AI input executor failed: {0}")]
    Executor(String),
    #[error("prepared AI input state is invalid: {0}")]
    InvalidState(&'static str),
}

pub async fn open_verified_input(
    file_io: &FileIoExecutorHandle,
    cpu: &CpuExecutorHandle,
    storage_root: StorageRootId,
    path: NormalizedStoragePath,
    expected_size: u64,
    expected_hash: &str,
) -> Result<StorageFileSession, PreparedInputError> {
    let (opened_session, snapshot) = file_io
        .open_storage_read_session_durable(storage_root, path)
        .await
        .map_err(|error| PreparedInputError::Executor(error.to_string()))?;
    if snapshot.byte_size != expected_size {
        drop(opened_session);
        return Err(PreparedInputError::Changed);
    }
    let mut file = Some(opened_session);
    let mut hasher = Some(
        cpu.start_sha256_session_durable()
            .await
            .map_err(|error| PreparedInputError::Executor(error.to_string()))?,
    );
    let mut byte_count = 0_u64;
    loop {
        let (returned_file, bytes) = file_io
            .read_storage_session_durable(
                file.take().ok_or(PreparedInputError::InvalidState(
                    "file session is unavailable",
                ))?,
                crate::runtime::FILE_IO_CHUNK_BYTES as usize,
            )
            .await
            .map_err(|error| PreparedInputError::Executor(error.to_string()))?;
        file = Some(returned_file);
        if bytes.is_empty() {
            break;
        }
        byte_count = byte_count
            .checked_add(bytes.len() as u64)
            .ok_or(PreparedInputError::InvalidState("byte count overflowed"))?;
        if byte_count > expected_size {
            return Err(PreparedInputError::Changed);
        }
        let (returned_hasher, _) = cpu
            .update_sha256_session_durable(
                hasher.take().ok_or(PreparedInputError::InvalidState(
                    "hash session is unavailable",
                ))?,
                bytes,
            )
            .await
            .map_err(|error| PreparedInputError::Executor(error.to_string()))?;
        hasher = Some(returned_hasher);
    }
    let actual_hash = cpu
        .finish_sha256_session_durable(hasher.take().ok_or(PreparedInputError::InvalidState(
            "hash session is unavailable",
        ))?)
        .await
        .map_err(|error| PreparedInputError::Executor(error.to_string()))?;
    if byte_count != expected_size || actual_hash != expected_hash {
        return Err(PreparedInputError::Changed);
    }
    file_io
        .seek_storage_read_session_durable(
            file.take().ok_or(PreparedInputError::InvalidState(
                "file session is unavailable",
            ))?,
            0,
        )
        .await
        .map_err(|error| PreparedInputError::Executor(error.to_string()))
}

async fn fail_submission(
    sqlite: &SqliteExecutorHandle,
    job_id: &str,
    error: String,
) -> Result<(), String> {
    finish_submission(
        sqlite,
        FinishLlmSubmission::Failed {
            job_id: job_id.to_string(),
            error,
        },
    )
    .await
}

async fn finish_submission(
    sqlite: &SqliteExecutorHandle,
    completion: FinishLlmSubmission,
) -> Result<(), String> {
    sqlite
        .finish_llm_submission_durable(completion)
        .await
        .map_err(|error| error.to_string())
}
