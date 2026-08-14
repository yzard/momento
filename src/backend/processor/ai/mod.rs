use std::sync::Arc;

use reqwest::multipart::{Form, Part};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::config::Config;
use crate::constants::{paths, IMAGE_TAGGING_MODEL_TYPE, OCR_MODEL_TYPE};
use crate::database::{queries, DbPool};

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
            if let Err(error) = submit_cycle(&config, &pool).await {
                tracing::warn!("LLM submission cycle failed: {error}");
            }
        }
        tokio::time::sleep(interval).await;
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
    connection.execute(
        queries::ai_jobs::INSERT_ELIGIBLE,
        rusqlite::params![task, task, task, task],
    )
}

pub fn queue_all(pool: &DbPool, image_tagging_enabled: bool) -> Result<usize, rusqlite::Error> {
    Ok(queue_task(pool, OCR_MODEL_TYPE, true)?
        + queue_task(pool, IMAGE_TAGGING_MODEL_TYPE, image_tagging_enabled)?)
}

async fn submit_cycle(config: &Config, pool: &DbPool) -> Result<(), String> {
    reclaim_stale_claims(pool)?;
    let jobs = {
        let connection = pool.get().map_err(|error| error.to_string())?;
        let jobs = connection
            .prepare(queries::ai_jobs::SELECT_QUEUED)
            .map_err(|error| error.to_string())?
            .query_map([config.llm_submission_worker.batch_size as i64], |row| {
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
        jobs
    };
    let client = reqwest::Client::new();
    for (job_id, media_id, task, attempts) in jobs {
        let connection = pool.get().map_err(|error| error.to_string())?;
        let claimed = connection
            .execute(queries::ai_jobs::CLAIM, [&job_id])
            .map_err(|error| error.to_string())?;
        if claimed != 1 {
            continue;
        }
        drop(connection);

        let inputs = load_inputs(pool, media_id, &task)?;
        if inputs.is_empty() {
            mark_failed(pool, &job_id, "missing prepared AI inputs")?;
            continue;
        }
        let mut descriptors = Vec::new();
        let mut parts = Vec::new();
        let mut input_error = None;
        for input in inputs {
            let bytes = match tokio::fs::read(paths().previews.join(&input.file_path)).await {
                Ok(bytes) => bytes,
                Err(error) => {
                    input_error = Some(error.to_string());
                    break;
                }
            };
            if bytes.len() as i64 != input.byte_size
                || format!("{:x}", Sha256::digest(&bytes)) != input.content_hash
            {
                input_error =
                    Some("prepared AI input no longer matches its durable descriptor".to_string());
                break;
            }
            parts.push((
                format!("input-{}", input.sequence),
                Part::bytes(bytes)
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
        if let Some(error) = input_error {
            mark_failed(pool, &job_id, &error)?;
            continue;
        }
        let manifest = SubmissionManifest {
            job_id: job_id.clone(),
            media_id,
            task: task.clone(),
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
        let response = client
            .post(format!("{}/api/v1/jobs/submit", config.llm.service_url))
            .header("x-api-key", &config.llm.api_key)
            .multipart(form)
            .send()
            .await;
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
            Ok(response) if response.status().is_server_error() => {
                retry_job(
                    &connection,
                    &job_id,
                    &format!("llm service error: {}", response.status()),
                )?;
            }
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

fn load_inputs(pool: &DbPool, media_id: i64, task: &str) -> Result<Vec<PreparedInput>, String> {
    let connection = pool.get().map_err(|error| error.to_string())?;
    let inputs = connection
        .prepare(queries::ai_jobs::SELECT_INPUTS)
        .map_err(|error| error.to_string())?
        .query_map(rusqlite::params![media_id, task], |row| {
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
