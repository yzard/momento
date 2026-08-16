use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Instant;

use momento_common::llm::{CancelJobsRequest, CancelJobsResponse};
use momento_common::rolling::{run_rolling_window, RollingWindowControl};
use reqwest::multipart::{Form, Part};
use rusqlite::OptionalExtension;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
use tokio_util::io::ReaderStream;

use crate::config::Config;
use crate::constants::{paths, IMAGE_TAGGING_MODEL_TYPE, OCR_MODEL_TYPE};
use crate::database::{queries, DbPool};

const CANCELLATION_CHUNK_SIZE: usize = 1000;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SubmissionManifest {
    job_id: String,
    media_id: i64,
    task: String,
    attempt: u32,
    callback_url: String,
    inputs: Vec<SubmissionInputDescriptor>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SubmissionInputDescriptor {
    sequence: i64,
    filename: String,
    mime_type: String,
    byte_size: u64,
    content_hash: String,
    input_kind: String,
    frame_timestamp_ms: Option<i64>,
}

pub async fn run(config: Arc<Config>, pool: DbPool) {
    let interval =
        std::time::Duration::from_secs(config.llm_submission_worker.poll_interval_seconds);
    loop {
        if config.llm.enabled {
            if let Err(error) = deliver_pending_cancellations(&config, &pool).await {
                tracing::warn!("LLM cancellation delivery failed: {error}");
            }
            if let Err(error) = submit_cycle(&config, &pool).await {
                tracing::warn!("LLM submission cycle failed: {error}");
            }
        }
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
    config: &Config,
    pool: &DbPool,
) -> Result<usize, String> {
    if !config.llm.enabled {
        return Ok(0);
    }
    let client = reqwest::Client::new();
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
        let response = client
            .post(format!("{}/api/v1/ai/cancel", config.llm.service_url))
            .header("x-api-key", &config.llm.api_key)
            .json(&CancelJobsRequest {
                all: scope == "all",
                tasks: if scope == "all" {
                    Vec::new()
                } else {
                    vec![task.clone()]
                },
                job_ids: job_ids.clone(),
            })
            .send()
            .await
            .map_err(|error| error.to_string())?;
        if !response.status().is_success() {
            let status = response.status();
            let detail = response.text().await.unwrap_or_default();
            return Err(format!(
                "llm service rejected cancellation: {status}: {detail}"
            ));
        }
        let response = response
            .json::<CancelJobsResponse>()
            .await
            .map_err(|error| format!("invalid llm cancellation response: {error}"))?;
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

pub fn queue_task(
    pool: &DbPool,
    task: &str,
    image_tagging_enabled: bool,
) -> Result<usize, rusqlite::Error> {
    if task == IMAGE_TAGGING_MODEL_TYPE && !image_tagging_enabled {
        return Ok(0);
    }
    let connection = pool
        .get()
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let transaction = connection.unchecked_transaction()?;
    let queued = transaction.execute(
        queries::ai_jobs::INSERT_ELIGIBLE,
        rusqlite::params![task, task, task, task],
    )?;
    transaction.execute(queries::ai_jobs::SNAPSHOT_QUEUED_INPUTS, [])?;
    transaction.commit()?;
    Ok(queued)
}

pub fn queue_all(pool: &DbPool, image_tagging_enabled: bool) -> Result<usize, rusqlite::Error> {
    Ok(queue_task(pool, OCR_MODEL_TYPE, true)?
        + queue_task(pool, IMAGE_TAGGING_MODEL_TYPE, image_tagging_enabled)?)
}

async fn submit_cycle(config: &Config, pool: &DbPool) -> Result<(), String> {
    reclaim_stale_claims(pool)?;
    pool.get()
        .map_err(|error| error.to_string())?
        .execute(queries::ai_jobs::SNAPSHOT_QUEUED_INPUTS, [])
        .map_err(|error| error.to_string())?;
    let client = reqwest::Client::new();
    let first_error = Arc::new(tokio::sync::Mutex::new(None));
    run_rolling_window(
        NonZeroUsize::new(config.llm_submission_worker.max_in_flight)
            .expect("validated LLM submission window"),
        |capacity| claim_queued_jobs(pool, capacity),
        |job| submit_claimed_job(config, pool, &client, job),
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
    config: &Config,
    pool: &DbPool,
    client: &reqwest::Client,
    (job_id, media_id, task, attempts): (String, i64, String, i64),
) -> Result<(), String> {
    let started = Instant::now();
    let task_name = task.clone();
    let inputs = load_inputs(pool, &job_id)?;
    if inputs.is_empty() {
        return mark_failed(pool, &job_id, "missing prepared AI inputs");
    }
    let mut descriptors = Vec::new();
    let mut parts = Vec::new();
    for input in inputs {
        let input_path = paths().previews.join(&input.file_path);
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
        let input_file = match tokio::fs::File::open(&input_path).await {
            Ok(input_file) => input_file,
            Err(error) => return mark_failed(pool, &job_id, &error.to_string()),
        };
        if input_size == 0 {
            return mark_failed(pool, &job_id, "prepared AI input must not be empty");
        }
        parts.push((
            format!("input-{}", input.sequence),
            Part::stream_with_length(
                reqwest::Body::wrap_stream(ReaderStream::new(input_file)),
                input_size,
            )
            .mime_str("application/octet-stream")
            .map_err(|error| error.to_string())?,
        ));
        descriptors.push(SubmissionInputDescriptor {
            sequence: input.sequence,
            filename: input.filename,
            mime_type: input.mime_type,
            byte_size: input.byte_size as u64,
            content_hash: input.content_hash,
            input_kind: input.input_kind,
            frame_timestamp_ms: input.frame_timestamp_ms,
        });
    }
    let verification_ms = started.elapsed().as_secs_f64() * 1000.0;
    let manifest = SubmissionManifest {
        job_id: job_id.clone(),
        media_id,
        task,
        attempt: (attempts + 1) as u32,
        callback_url: config.llm.callback_url.clone(),
        inputs: descriptors,
    };
    let mut form = Form::new().part(
        "manifest",
        Part::text(serde_json::to_string(&manifest).map_err(|error| error.to_string())?)
            .mime_str("application/json")
            .map_err(|error| error.to_string())?,
    );
    for (name, part) in parts {
        form = form.part(name, part);
    }
    let admission_started = Instant::now();
    let response = client
        .post(format!("{}/api/v1/jobs/submit", config.llm.service_url))
        .header("x-api-key", &config.llm.api_key)
        .multipart(form)
        .send()
        .await;
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
        Ok(response)
            if response.status() == reqwest::StatusCode::ACCEPTED
                || response.status().is_success() =>
        {
            connection
                .execute(queries::ai_jobs::MARK_SUBMITTED, [&job_id])
                .map_err(|error| error.to_string())?;
        }
        Ok(response) if response.status().is_server_error() => retry_job(
            &connection,
            &job_id,
            &format!("llm service error: {}", response.status()),
        )?,
        Ok(response) => {
            let status = response.status();
            let detail = response.text().await.unwrap_or_default();
            mark_failed(
                pool,
                &job_id,
                &format!("llm service rejected submission: {status}: {detail}"),
            )?
        }
        Err(error) => retry_job(&connection, &job_id, &error.to_string())?,
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
