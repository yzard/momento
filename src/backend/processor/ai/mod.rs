use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Instant;

use momento_common::llm::{CancelJobsRequest, JobInputDescriptor, JobManifest};
use momento_common::rolling::{run_rolling_window, RollingWindowControl};
use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

use crate::config::Config;
use crate::constants::{
    paths, DOCUMENT_DETECTION_MODEL_TYPE, IMAGE_AESTHETICS_MODEL_TYPE, IMAGE_TAGGING_MODEL_TYPE,
    OCR_MODEL_TYPE, SCREENSHOT_DETECTION_MODEL_TYPE,
};
use crate::database::{queries, DbPool};

pub mod result;
pub mod transport;

use transport::{LlmConnection, PreparedSubmissionInput, SubmissionOutcome, TransportHandle};

const CANCELLATION_CHUNK_SIZE: usize = 1000;

pub async fn run(config: Arc<Config>, pool: DbPool, handle: TransportHandle) {
    let interval =
        std::time::Duration::from_secs(config.llm_submission_worker.poll_interval_seconds);
    loop {
        if !config.llm.enabled {
            tokio::time::sleep(interval).await;
            continue;
        }
        let connection = match LlmConnection::connect(
            &config.llm.server_address,
            &config.llm.client_id,
            &config.llm.api_key,
            pool.clone(),
        )
        .await
        {
            Ok(connection) => connection,
            Err(error) => {
                tracing::warn!("LLM WebSocket connection failed: {error}");
                tokio::time::sleep(interval).await;
                continue;
            }
        };
        tracing::info!(client_id = config.llm.client_id, "LLM WebSocket connected");
        let submission_config = Arc::clone(&config);
        let submission_pool = pool.clone();
        let submission_connection = connection.clone();
        let submission_task = tokio::spawn(async move {
            let mut poll = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    _ = submission_connection.closed() => return,
                    _ = poll.tick() => {}
                }
                if let Err(error) =
                    submit_cycle(&submission_config, &submission_pool, &submission_connection).await
                {
                    tracing::warn!("LLM submission cycle failed: {error}");
                }
            }
        });
        let cancellation_pool = pool.clone();
        let cancellation_connection = connection.clone();
        let cancellation_handle = handle.clone();
        let cancellation_task = tokio::spawn(async move {
            let mut poll = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    _ = cancellation_connection.closed() => return,
                    _ = poll.tick() => {}
                    _ = cancellation_handle.notified() => {}
                }
                if let Err(error) =
                    deliver_pending_cancellations(&cancellation_pool, &cancellation_connection)
                        .await
                {
                    tracing::warn!("LLM cancellation delivery failed: {error}");
                }
            }
        });
        connection.closed().await;
        submission_task.abort();
        cancellation_task.abort();
        tracing::warn!(
            client_id = config.llm.client_id,
            "LLM WebSocket disconnected"
        );
        tokio::time::sleep(interval).await;
    }
}

pub fn cancel_active_jobs(pool: &DbPool, task: Option<&str>) -> Result<usize, rusqlite::Error> {
    let connection = pool
        .get()
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let transaction = connection.unchecked_transaction()?;
    let cancelled = if let Some(task) = task {
        transaction.execute(queries::ai_jobs::QUEUE_CANCELLATION_SCOPE_FOR_TASK, [task])?;
        transaction.execute(queries::ai_jobs::QUEUE_CANCELLATIONS_FOR_TASK, [task])?;
        transaction.execute(queries::ai_jobs::CANCEL_FOR_TASK, [task])?
    } else {
        transaction.execute(queries::ai_jobs::QUEUE_ALL_CANCELLATION_SCOPE, [])?;
        transaction.execute(queries::ai_jobs::QUEUE_ALL_CANCELLATIONS, [])?;
        transaction.execute(queries::ai_jobs::CANCEL_ALL, [])?
    };
    transaction.commit()?;
    Ok(cancelled)
}

pub async fn deliver_pending_cancellations(
    pool: &DbPool,
    connection: &LlmConnection,
) -> Result<usize, String> {
    let mut delivered = 0;
    loop {
        let cancellation = {
            let connection = pool.get().map_err(|error| error.to_string())?;
            let scope = connection
                .query_row(queries::ai_jobs::SELECT_CANCELLATION_SCOPE, [], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .optional()
                .map_err(|error| error.to_string())?;
            let Some((scope, task)) = scope else {
                return Ok(delivered);
            };
            let job_ids = if scope == "all" {
                connection
                    .prepare(queries::ai_jobs::SELECT_ALL_CANCELLATIONS)
                    .map_err(|error| error.to_string())?
                    .query_map([CANCELLATION_CHUNK_SIZE as i64], |row| row.get(0))
                    .map_err(|error| error.to_string())?
                    .collect::<Result<Vec<String>, _>>()
                    .map_err(|error| error.to_string())?
            } else {
                connection
                    .prepare(queries::ai_jobs::SELECT_CANCELLATIONS_FOR_TASK)
                    .map_err(|error| error.to_string())?
                    .query_map(
                        rusqlite::params![task, CANCELLATION_CHUNK_SIZE as i64],
                        |row| row.get(0),
                    )
                    .map_err(|error| error.to_string())?
                    .collect::<Result<Vec<String>, _>>()
                    .map_err(|error| error.to_string())?
            };
            (scope, task, job_ids)
        };
        let (scope, task, job_ids) = cancellation;
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
        let connection = pool.get().map_err(|error| error.to_string())?;
        let transaction = connection
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        for job_id in &job_ids {
            transaction
                .execute(queries::ai_jobs::DELETE_CANCELLATION, [job_id])
                .map_err(|error| error.to_string())?;
        }
        let remaining: i64 = if scope == "all" {
            transaction
                .query_row(queries::ai_jobs::COUNT_ALL_CANCELLATIONS, [], |row| {
                    row.get(0)
                })
                .map_err(|error| error.to_string())?
        } else {
            transaction
                .query_row(
                    queries::ai_jobs::COUNT_CANCELLATIONS_FOR_TASK,
                    [&task],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?
        };
        if remaining == 0 {
            if scope == "all" {
                transaction
                    .execute(queries::ai_jobs::DELETE_ALL_CANCELLATION_SCOPES, [])
                    .map_err(|error| error.to_string())?;
            } else {
                transaction
                    .execute(
                        queries::ai_jobs::DELETE_CANCELLATION_SCOPE_FOR_TASK,
                        [&task],
                    )
                    .map_err(|error| error.to_string())?;
            }
        }
        transaction.commit().map_err(|error| error.to_string())?;
        delivered += job_ids.len();
    }
}

pub fn queue_task(pool: &DbPool, task: &str, task_enabled: bool) -> Result<usize, rusqlite::Error> {
    if !task_enabled {
        return Ok(0);
    }
    let connection = pool
        .get()
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let transaction = connection.unchecked_transaction()?;
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
    transaction.commit()?;
    Ok(queued)
}

pub fn queue_all(
    pool: &DbPool,
    image_tagging_enabled: bool,
    image_aesthetics_enabled: bool,
    screenshot_detection_enabled: bool,
    document_detection_enabled: bool,
) -> Result<usize, rusqlite::Error> {
    Ok(queue_task(pool, OCR_MODEL_TYPE, true)?
        + queue_task(pool, IMAGE_TAGGING_MODEL_TYPE, image_tagging_enabled)?
        + queue_task(pool, IMAGE_AESTHETICS_MODEL_TYPE, image_aesthetics_enabled)?
        + queue_task(
            pool,
            SCREENSHOT_DETECTION_MODEL_TYPE,
            screenshot_detection_enabled,
        )?
        + queue_task(
            pool,
            DOCUMENT_DETECTION_MODEL_TYPE,
            document_detection_enabled,
        )?)
}

async fn submit_cycle(
    config: &Config,
    pool: &DbPool,
    connection: &LlmConnection,
) -> Result<(), String> {
    reclaim_stale_claims(pool)?;
    pool.get()
        .map_err(|error| error.to_string())?
        .execute(queries::ai_jobs::SNAPSHOT_QUEUED_INPUTS, [])
        .map_err(|error| error.to_string())?;
    let first_error = Arc::new(tokio::sync::Mutex::new(None));
    run_rolling_window(
        NonZeroUsize::new(config.llm_submission_worker.max_in_flight)
            .expect("validated LLM submission window"),
        |capacity| claim_queued_jobs(pool, capacity),
        |job| submit_claimed_job(pool, connection, job),
        {
            let first_error = Arc::clone(&first_error);
            move |result| {
                let first_error = Arc::clone(&first_error);
                async move {
                    let Err(error) = result else {
                        return RollingWindowControl::Continue;
                    };
                    *first_error.lock().await = Some(error);
                    RollingWindowControl::Stop
                }
            }
        },
    )
    .await?;
    if let Some(error) = first_error.lock().await.take() {
        return Err(error);
    }
    Ok(())
}

fn claim_queued_jobs(
    pool: &DbPool,
    capacity: usize,
) -> Result<Vec<(String, i64, String, i64)>, String> {
    let connection = pool.get().map_err(|error| error.to_string())?;
    let jobs = connection
        .prepare(queries::ai_jobs::SELECT_QUEUED)
        .map_err(|error| error.to_string())?
        .query_map([capacity as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let mut claimed = Vec::with_capacity(jobs.len());
    for job in jobs {
        if connection
            .execute(queries::ai_jobs::CLAIM, [&job.0])
            .map_err(|error| error.to_string())?
            == 1
        {
            claimed.push(job);
        }
    }
    Ok(claimed)
}

async fn submit_claimed_job(
    pool: &DbPool,
    connection: &LlmConnection,
    (job_id, media_id, task, attempts): (String, i64, String, i64),
) -> Result<(), String> {
    let started = Instant::now();
    let task_name = task.clone();
    let inputs = load_inputs(pool, &job_id)?;
    if inputs.is_empty() {
        return mark_failed(pool, &job_id, "missing prepared AI inputs");
    }
    let mut descriptors = Vec::new();
    let mut prepared_inputs = Vec::new();
    for input in inputs {
        let input_path = match crate::utils::path::resolve_existing_storage_path(
            &paths().previews,
            &input.file_path,
        )
        .await
        {
            Ok(input_path) => input_path,
            Err(error) => return mark_failed(pool, &job_id, &error.to_string()),
        };
        let input_size = match u64::try_from(input.byte_size) {
            Ok(input_size) => input_size,
            Err(_) => {
                return mark_failed(pool, &job_id, "prepared AI input has an invalid byte size")
            }
        };
        if let Err(error) =
            verify_prepared_input(&input_path, input_size, &input.content_hash).await
        {
            return mark_failed(pool, &job_id, &error);
        }
        if input_size == 0 {
            return mark_failed(pool, &job_id, "prepared AI input must not be empty");
        }
        let sequence = match u32::try_from(input.sequence) {
            Ok(sequence) => sequence,
            Err(_) => return mark_failed(pool, &job_id, "prepared AI input sequence is invalid"),
        };
        prepared_inputs.push(PreparedSubmissionInput {
            sequence,
            path: input_path,
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
    let response = connection.submit(manifest, prepared_inputs).await;
    tracing::debug!(
        job_id,
        task = task_name,
        verification_ms,
        admission_ms = admission_started.elapsed().as_secs_f64() * 1000.0,
        total_ms = started.elapsed().as_secs_f64() * 1000.0,
        "LLM job submission timing"
    );
    let connection = pool.get().map_err(|error| error.to_string())?;
    match response {
        Ok(SubmissionOutcome::Acknowledged { status }) if status == "queued" => {
            // The reader persists this transition before forwarding the acknowledgement so a
            // result arriving in the next WebSocket frame cannot observe `submitting`.
        }
        Ok(SubmissionOutcome::Acknowledged { status }) => mark_failed(
            pool,
            &job_id,
            &format!("llm service acknowledged submission with status {status}"),
        )?,
        Ok(SubmissionOutcome::Rejected {
            retryable: true,
            error,
        }) => retry_job(&connection, &job_id, &error)?,
        Ok(SubmissionOutcome::Rejected {
            retryable: false,
            error,
        }) => mark_failed(pool, &job_id, &error)?,
        Err(error) => {
            connection
                .execute(queries::ai_jobs::REQUEUE_AMBIGUOUS, [&job_id])
                .map_err(|database_error| database_error.to_string())?;
            tracing::warn!(job_id, error, "LLM submission outcome was not acknowledged");
        }
    }
    Ok(())
}

pub async fn verify_prepared_input(
    path: &std::path::Path,
    expected_size: u64,
    expected_hash: &str,
) -> Result<(), String> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut byte_count = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let bytes_read = file
            .read(&mut buffer)
            .await
            .map_err(|error| error.to_string())?;
        if bytes_read == 0 {
            break;
        }
        byte_count = byte_count
            .checked_add(bytes_read as u64)
            .ok_or_else(|| "prepared AI input is too large".to_string())?;
        if byte_count > expected_size {
            return Err("prepared AI input no longer matches its durable descriptor".to_string());
        }
        hasher.update(&buffer[..bytes_read]);
    }
    if byte_count != expected_size || format!("{:x}", hasher.finalize()) != expected_hash {
        return Err("prepared AI input no longer matches its durable descriptor".to_string());
    }
    Ok(())
}

struct PreparedInput {
    sequence: i64,
    file_path: String,
    filename: String,
    mime_type: String,
    byte_size: i64,
    content_hash: String,
    input_kind: String,
    frame_timestamp_ms: Option<i64>,
}

fn load_inputs(pool: &DbPool, job_id: &str) -> Result<Vec<PreparedInput>, String> {
    let connection = pool.get().map_err(|error| error.to_string())?;
    let inputs = connection
        .prepare(queries::ai_jobs::SELECT_INPUTS)
        .map_err(|error| error.to_string())?
        .query_map([job_id], |row| {
            Ok(PreparedInput {
                sequence: row.get(0)?,
                file_path: row.get(1)?,
                filename: row.get(2)?,
                mime_type: row.get(3)?,
                byte_size: row.get(4)?,
                content_hash: row.get(5)?,
                input_kind: row.get(6)?,
                frame_timestamp_ms: row.get(7)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(inputs)
}

fn reclaim_stale_claims(pool: &DbPool) -> Result<(), String> {
    pool.get()
        .map_err(|error| error.to_string())?
        .execute(queries::ai_jobs::RECLAIM_STALE, [])
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn retry_job(connection: &rusqlite::Connection, job_id: &str, error: &str) -> Result<(), String> {
    connection
        .execute(
            queries::ai_jobs::RETRY_OR_FAIL,
            rusqlite::params![error, job_id],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn mark_failed(pool: &DbPool, job_id: &str, error: &str) -> Result<(), String> {
    pool.get()
        .map_err(|error| error.to_string())?
        .execute(
            queries::ai_jobs::MARK_FAILED,
            rusqlite::params![error, job_id],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}
