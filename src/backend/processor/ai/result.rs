use momento_common::llm::{JobInputResult, JobResult};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior};
use std::collections::HashSet;
use std::time::Duration;

use crate::constants::{DOCUMENT_DETECTION_MODEL_TYPE, SCREENSHOT_DETECTION_MODEL_TYPE};
use crate::database::{queries, DbPool};
use crate::error::{AppError, AppResult};

const RESULT_PROCESSING_MAX_ATTEMPTS: i64 = 5;

pub fn receive_result(pool: &DbPool, request: JobResult) -> AppResult<()> {
    let payload = serde_json::to_string(&request)?;
    let connection = pool.get()?;
    let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)?;
    let Some((media_id, task, attempts, status)) = transaction
        .query_row(
            queries::llm_callback::SELECT_JOB,
            [&request.job_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?
    else {
        tracing::warn!(
            job_id = request.job_id,
            "discarding result for an unknown Momento job"
        );
        transaction.commit()?;
        return Ok(());
    };
    if matches!(status.as_str(), "completed" | "failed" | "cancelled") {
        transaction.commit()?;
        return Ok(());
    }
    let expected_attempt = if status == "submitted" {
        attempts
    } else {
        attempts + 1
    };
    if media_id != request.media_id
        || task != request.task
        || expected_attempt != i64::from(request.attempt)
    {
        let error = "received LLM result does not match the Momento job";
        transaction.execute(
            queries::llm_callback::MARK_RESULT_CORRELATION_FAILED,
            rusqlite::params![error, request.job_id],
        )?;
        transaction.commit()?;
        tracing::error!(
            job_id = request.job_id,
            media_id = request.media_id,
            task = request.task,
            attempt = request.attempt,
            "{error}"
        );
        return Ok(());
    }
    if matches!(status.as_str(), "queued" | "submitting")
        && transaction.execute(
            queries::llm_callback::MARK_UNACKNOWLEDGED_RESULT_SUBMITTED,
            rusqlite::params![request.attempt, request.job_id, request.attempt],
        )? != 1
    {
        return Err(AppError::Conflict(
            "Momento job changed while receiving the LLM result".to_string(),
        ));
    }
    transaction.execute(
        queries::llm_callback::INSERT_RECEIVED_RESULT,
        rusqlite::params![request.job_id, payload],
    )?;
    transaction.commit()?;
    Ok(())
}

pub async fn run(pool: DbPool, interval: Duration, batch_size: usize) {
    loop {
        let claim_pool = pool.clone();
        let claimed =
            tokio::task::spawn_blocking(move || claim_received_results(&claim_pool, batch_size))
                .await;
        let claimed = match claimed {
            Ok(Ok(claimed)) => claimed,
            Ok(Err(error)) => {
                tracing::warn!("Momento LLM result claim failed: {error}");
                tokio::time::sleep(interval).await;
                continue;
            }
            Err(error) => {
                tracing::warn!("Momento LLM result claim worker stopped unexpectedly: {error}");
                tokio::time::sleep(interval).await;
                continue;
            }
        };
        let claimed_count = claimed.len();
        let mut processing = tokio::task::JoinSet::new();
        for (job_id, payload, attempts) in claimed {
            let result_pool = pool.clone();
            processing.spawn_blocking(move || {
                process_claimed_result(&result_pool, &job_id, &payload, attempts)
                    .map_err(|error| (job_id, error))
            });
        }
        while let Some(processed) = processing.join_next().await {
            match processed {
                Ok(Ok(())) => {}
                Ok(Err((job_id, error))) => {
                    tracing::warn!(job_id, error = %error, "Momento LLM result processing failed")
                }
                Err(error) => {
                    tracing::warn!("Momento LLM result worker stopped unexpectedly: {error}")
                }
            }
        }
        if claimed_count < batch_size {
            tokio::time::sleep(interval).await;
        }
    }
}

pub fn process_received_results(pool: &DbPool, batch_size: usize) -> AppResult<usize> {
    let claimed = claim_received_results(pool, batch_size)?;
    let processed = claimed.len();
    for (job_id, payload, attempts) in claimed {
        process_claimed_result(pool, &job_id, &payload, attempts)?;
    }
    Ok(processed)
}

fn claim_received_results(
    pool: &DbPool,
    batch_size: usize,
) -> AppResult<Vec<(String, String, i64)>> {
    if batch_size == 0 {
        return Err(AppError::Validation(
            "LLM result worker batch size must be positive".to_string(),
        ));
    }
    pool.get()?
        .execute(queries::llm_callback::RECLAIM_RESULTS, [])?;
    let mut claimed = Vec::with_capacity(batch_size);
    while claimed.len() < batch_size {
        let Some((job_id, payload, attempts)) = claim_received_result(pool)? else {
            break;
        };
        claimed.push((job_id, payload, attempts));
    }
    Ok(claimed)
}

fn claim_received_result(pool: &DbPool) -> AppResult<Option<(String, String, i64)>> {
    pool.get()?
        .query_row(queries::llm_callback::CLAIM_RESULT, [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .optional()
        .map_err(AppError::from)
}

fn process_claimed_result(
    pool: &DbPool,
    job_id: &str,
    payload: &str,
    attempts: i64,
) -> AppResult<()> {
    let request = match serde_json::from_str::<JobResult>(payload) {
        Ok(request) => request,
        Err(error) => return fail_received_result(pool, job_id, &error.to_string()),
    };
    match process_result(pool, request) {
        Ok(()) => delete_received_result(pool, job_id),
        Err(error)
            if result_error_is_retryable(&error) && attempts < RESULT_PROCESSING_MAX_ATTEMPTS =>
        {
            pool.get()?.execute(
                queries::llm_callback::RETRY_RESULT,
                rusqlite::params![error.to_string(), job_id],
            )?;
            tracing::warn!(job_id, error = %error, attempts, "retrying Momento LLM result processing");
            Ok(())
        }
        Err(error) => fail_received_result(pool, job_id, &error.to_string()),
    }
}

fn result_error_is_retryable(error: &AppError) -> bool {
    matches!(
        error,
        AppError::DatabaseBusy
            | AppError::Database(_)
            | AppError::Pool(_)
            | AppError::Io(_)
            | AppError::Internal(_)
    )
}

fn delete_received_result(pool: &DbPool, job_id: &str) -> AppResult<()> {
    pool.get()?
        .execute(queries::llm_callback::DELETE_RESULT, [job_id])?;
    Ok(())
}

fn fail_received_result(pool: &DbPool, job_id: &str, error: &str) -> AppResult<()> {
    let connection = pool.get()?;
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(
        queries::llm_callback::MARK_RECEIVED_RESULT_FAILED,
        rusqlite::params![error, job_id],
    )?;
    transaction.execute(queries::llm_callback::DELETE_RESULT, [job_id])?;
    transaction.commit()?;
    tracing::error!(
        job_id,
        error,
        "Momento LLM result processing failed permanently"
    );
    Ok(())
}

pub fn process_result(pool: &DbPool, request: JobResult) -> AppResult<()> {
    let connection = pool.get()?;
    let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)?;
    let job: (i64, String, i64, String) = transaction
        .query_row(
            queries::llm_callback::SELECT_JOB,
            [&request.job_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|_| AppError::NotFound("LLM job not found".to_string()))?;
    if job.0 != request.media_id || job.1 != request.task || job.2 != i64::from(request.attempt) {
        return Err(AppError::Conflict(
            "LLM result does not match submitted job".to_string(),
        ));
    }
    if matches!(job.3.as_str(), "completed" | "failed" | "cancelled") {
        return Ok(());
    }
    if job.3 != "submitted" {
        return Err(AppError::Conflict(
            "LLM job is not awaiting a result".to_string(),
        ));
    }
    if !matches!(request.status.as_str(), "completed" | "failed") {
        return Err(AppError::BadRequest(
            "LLM result status must be completed or failed".to_string(),
        ));
    }
    let mut face_file_changes = None;
    if request.status == "completed" {
        let model_type = request
            .model_type
            .ok_or_else(|| AppError::BadRequest("modelType is required".to_string()))?;
        let model_version = request
            .model_version
            .ok_or_else(|| AppError::BadRequest("modelVersion is required".to_string()))?;
        let result = request
            .result
            .ok_or_else(|| AppError::BadRequest("result is required".to_string()))?;
        if request.task == "image_clustering" {
            persist_clustering_result(&transaction, request.media_id, &model_version, &result)?;
        } else if request.task == "image_aesthetics" {
            if model_type != "image_aesthetics" {
                return Err(AppError::BadRequest(
                    "aesthetics result modelType must be image_aesthetics".to_string(),
                ));
            }
            persist_aesthetics_results(
                &transaction,
                request.media_id,
                &request.job_id,
                &model_version,
                &result,
                request.input_results.as_deref(),
            )?;
        } else if matches!(request.task.as_str(), "ocr" | "image_tagging") {
            persist_text_results(
                &transaction,
                request.media_id,
                &model_type,
                &model_version,
                &result,
                request.input_results.as_deref(),
            )?;
        } else if matches!(
            request.task.as_str(),
            SCREENSHOT_DETECTION_MODEL_TYPE | DOCUMENT_DETECTION_MODEL_TYPE
        ) {
            if model_type != request.task {
                return Err(AppError::BadRequest(format!(
                    "{} result modelType must match the task",
                    request.task
                )));
            }
            persist_classification_results(
                &transaction,
                request.media_id,
                &request.job_id,
                &request.task,
                &model_version,
                &result,
                request.input_results.as_deref(),
            )?;
        } else if request.task == "face_detection" {
            face_file_changes = Some(crate::processor::face_detection::persist_result(
                &transaction,
                &request.job_id,
                request.media_id,
                &model_type,
                &model_version,
                request.input_results.as_deref(),
            )?);
        } else {
            return Err(AppError::BadRequest(
                "completed result task is not supported".to_string(),
            ));
        }
        if transaction.execute(
            queries::llm_callback::MARK_COMPLETED,
            rusqlite::params![request.job_id, request.attempt],
        )? != 1
        {
            return Err(AppError::Conflict(
                "LLM job changed during result persistence".to_string(),
            ));
        }
    } else if transaction.execute(
        queries::llm_callback::MARK_FAILED,
        rusqlite::params![
            request
                .error
                .unwrap_or_else(|| "LLM inference failed".to_string()),
            request.job_id,
            request.attempt
        ],
    )? != 1
    {
        return Err(AppError::Conflict(
            "LLM job changed during result persistence".to_string(),
        ));
    }
    transaction.commit()?;
    if let Some(changes) = face_file_changes {
        changes.commit();
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq)]
struct ClassificationResult {
    detected: bool,
    confidence: f64,
}

fn persist_classification_results(
    transaction: &rusqlite::Transaction<'_>,
    media_id: i64,
    job_id: &str,
    task: &str,
    model_version: &str,
    result: &serde_json::Value,
    input_results: Option<&[JobInputResult]>,
) -> AppResult<()> {
    let aggregate = parse_classification_result(result, task)?;
    let input_results = input_results
        .filter(|results| !results.is_empty())
        .ok_or_else(|| AppError::BadRequest(format!("{task} inputResults are required")))?;
    let submitted_inputs = transaction
        .prepare(queries::llm_callback::SELECT_JOB_INPUT_CORRELATION)?
        .query_map([job_id], |row| {
            Ok((row.get::<_, u32>(0)?, row.get::<_, Option<i64>>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if submitted_inputs.len() != input_results.len() {
        return Err(AppError::BadRequest(format!(
            "{task} inputResults do not match submitted inputs"
        )));
    }

    let mut sequences = HashSet::with_capacity(input_results.len());
    let mut first_input_result = None;
    for (input_result, submitted_input) in input_results.iter().zip(&submitted_inputs) {
        if !sequences.insert(input_result.sequence) {
            return Err(AppError::BadRequest(format!(
                "{task} inputResults contain duplicate sequences"
            )));
        }
        if (input_result.sequence, input_result.frame_timestamp_ms) != *submitted_input {
            return Err(AppError::BadRequest(format!(
                "{task} inputResults do not match submitted inputs"
            )));
        }
        let classification = parse_classification_result(&input_result.result, task)?;
        first_input_result.get_or_insert(classification);
        let query = match task {
            SCREENSHOT_DETECTION_MODEL_TYPE => {
                queries::llm_callback::UPSERT_SCREENSHOT_CLASSIFICATION_INPUT
            }
            DOCUMENT_DETECTION_MODEL_TYPE => {
                queries::llm_callback::UPSERT_DOCUMENT_CLASSIFICATION_INPUT
            }
            _ => {
                return Err(AppError::BadRequest(
                    "classification task is not supported".to_string(),
                ));
            }
        };
        transaction.execute(
            query,
            rusqlite::params![
                media_id,
                input_result.sequence,
                input_result.frame_timestamp_ms,
                model_version,
                classification.detected,
                classification.confidence
            ],
        )?;
    }
    if first_input_result != Some(aggregate) {
        return Err(AppError::BadRequest(format!(
            "{task} aggregate must match the first input result"
        )));
    }
    let query = match task {
        SCREENSHOT_DETECTION_MODEL_TYPE => queries::llm_callback::UPSERT_SCREENSHOT_CLASSIFICATION,
        DOCUMENT_DETECTION_MODEL_TYPE => queries::llm_callback::UPSERT_DOCUMENT_CLASSIFICATION,
        _ => {
            return Err(AppError::BadRequest(
                "classification task is not supported".to_string(),
            ));
        }
    };
    transaction.execute(
        query,
        rusqlite::params![
            media_id,
            model_version,
            aggregate.detected,
            aggregate.confidence
        ],
    )?;
    Ok(())
}

fn parse_classification_result(
    result: &serde_json::Value,
    task: &str,
) -> AppResult<ClassificationResult> {
    let detected = result
        .get("detected")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| AppError::BadRequest(format!("{task} detected is required")))?;
    let confidence = result
        .get("confidence")
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| AppError::BadRequest(format!("{task} confidence is required")))?;
    if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
        return Err(AppError::BadRequest(format!(
            "{task} confidence must be within [0, 1]"
        )));
    }
    Ok(ClassificationResult {
        detected,
        confidence,
    })
}

#[derive(Clone, Copy, PartialEq)]
struct AestheticScores {
    aesthetic: f64,
    scenic: f64,
    simplicity: f64,
    landscape: f64,
    technical_quality: f64,
}

fn persist_aesthetics_results(
    transaction: &rusqlite::Transaction<'_>,
    media_id: i64,
    job_id: &str,
    model_version: &str,
    result: &serde_json::Value,
    input_results: Option<&[JobInputResult]>,
) -> AppResult<()> {
    let aggregate = parse_aesthetic_scores(result)?;
    let input_results = input_results
        .filter(|results| !results.is_empty())
        .ok_or_else(|| AppError::BadRequest("aesthetics inputResults are required".to_string()))?;
    let submitted_inputs = transaction
        .prepare(queries::llm_callback::SELECT_JOB_INPUT_CORRELATION)?
        .query_map([job_id], |row| {
            Ok((row.get::<_, u32>(0)?, row.get::<_, Option<i64>>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if submitted_inputs.len() != input_results.len() {
        return Err(AppError::BadRequest(
            "aesthetics inputResults do not match submitted inputs".to_string(),
        ));
    }
    let mut sequences = HashSet::with_capacity(input_results.len());
    let mut first_input_scores = None;
    for (input_result, submitted_input) in input_results.iter().zip(&submitted_inputs) {
        if !sequences.insert(input_result.sequence) {
            return Err(AppError::BadRequest(
                "aesthetics inputResults contain duplicate sequences".to_string(),
            ));
        }
        if (input_result.sequence, input_result.frame_timestamp_ms) != *submitted_input {
            return Err(AppError::BadRequest(
                "aesthetics inputResults do not match submitted inputs".to_string(),
            ));
        }
        let scores = parse_aesthetic_scores(&input_result.result)?;
        first_input_scores.get_or_insert(scores);
        transaction.execute(
            queries::llm_callback::UPSERT_AESTHETIC_INPUT,
            rusqlite::params![
                media_id,
                input_result.sequence,
                input_result.frame_timestamp_ms,
                model_version,
                scores.aesthetic,
                scores.scenic,
                scores.simplicity,
                scores.landscape,
                scores.technical_quality
            ],
        )?;
    }
    if first_input_scores != Some(aggregate) {
        return Err(AppError::BadRequest(
            "aesthetics aggregate must match the first input result".to_string(),
        ));
    }
    transaction.execute(
        queries::llm_callback::UPSERT_AESTHETICS,
        rusqlite::params![
            media_id,
            model_version,
            aggregate.aesthetic,
            aggregate.scenic,
            aggregate.simplicity,
            aggregate.landscape,
            aggregate.technical_quality
        ],
    )?;
    Ok(())
}

fn parse_aesthetic_scores(result: &serde_json::Value) -> AppResult<AestheticScores> {
    Ok(AestheticScores {
        aesthetic: parse_bounded_score(result, "aestheticScore")?,
        scenic: parse_bounded_score(result, "scenicScore")?,
        simplicity: parse_bounded_score(result, "simplicityScore")?,
        landscape: parse_bounded_score(result, "landscapeScore")?,
        technical_quality: parse_bounded_score(result, "technicalQualityScore")?,
    })
}

fn parse_bounded_score(result: &serde_json::Value, field: &str) -> AppResult<f64> {
    let score = result
        .get(field)
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| AppError::BadRequest(format!("aesthetics {field} is required")))?;
    if !score.is_finite() || !(0.0..=1.0).contains(&score) {
        return Err(AppError::BadRequest(format!(
            "aesthetics {field} must be within [0, 1]"
        )));
    }
    Ok(score)
}

fn persist_text_results(
    transaction: &rusqlite::Transaction<'_>,
    media_id: i64,
    model_type: &str,
    model_version: &str,
    result: &serde_json::Value,
    input_results: Option<&[JobInputResult]>,
) -> AppResult<()> {
    let text = input_results
        .filter(|results| !results.is_empty())
        .map(|results| {
            results
                .iter()
                .filter_map(|input_result| {
                    input_result
                        .result
                        .get("text")
                        .and_then(|value| value.as_str())
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_else(|| {
            result
                .get("text")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string()
        });
    transaction.execute(
        queries::llm_callback::UPSERT_TEXT,
        rusqlite::params![media_id, model_type, model_version, text],
    )?;
    let Some(input_results) = input_results else {
        return Ok(());
    };
    for input_result in input_results {
        let input_text = input_result
            .result
            .get("text")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        transaction.execute(
            queries::llm_callback::UPSERT_INPUT_TEXT,
            rusqlite::params![
                media_id,
                model_type,
                input_result.sequence,
                input_result.frame_timestamp_ms,
                model_version,
                input_text
            ],
        )?;
    }
    Ok(())
}

fn persist_clustering_result(
    transaction: &rusqlite::Transaction<'_>,
    media_id: i64,
    model_version: &str,
    result: &serde_json::Value,
) -> AppResult<()> {
    let embedding = result
        .get("embedding")
        .and_then(|value| value.as_str())
        .ok_or_else(|| AppError::BadRequest("clustering embedding is required".to_string()))?;
    let encoding = result
        .get("embeddingEncoding")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            AppError::BadRequest("clustering embeddingEncoding is required".to_string())
        })?;
    if encoding != "float32_le" {
        return Err(AppError::BadRequest(
            "clustering embedding must use float32_le encoding".to_string(),
        ));
    }
    let embedding_bytes =
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, embedding).map_err(
            |error| AppError::BadRequest(format!("invalid clustering embedding: {error}")),
        )?;
    let dimensions = result
        .get("embeddingDimensions")
        .and_then(|value| value.as_u64())
        .ok_or_else(|| {
            AppError::BadRequest("clustering embeddingDimensions is required".to_string())
        })? as usize;
    if dimensions != 384 || embedding_bytes.len() != dimensions * std::mem::size_of::<f32>() {
        return Err(AppError::BadRequest(
            "clustering embedding has invalid dimensions".to_string(),
        ));
    }
    let perceptual_hash = result
        .get("perceptualHash")
        .and_then(|value| value.as_str())
        .ok_or_else(|| AppError::BadRequest("clustering perceptualHash is required".to_string()))?;
    let perceptual_hash = u64::from_str_radix(perceptual_hash, 16).map_err(|error| {
        AppError::BadRequest(format!("invalid clustering perceptualHash: {error}"))
    })?;
    let (content_hash, capture_time_seconds): (String, Option<i64>) = transaction.query_row(
        queries::llm_callback::SELECT_CLUSTER_MEDIA,
        [media_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    transaction.execute(
        queries::llm_callback::UPSERT_SIMILARITY_INDEX,
        rusqlite::params![
            media_id,
            content_hash,
            model_version,
            embedding_bytes,
            perceptual_hash as i64,
            capture_time_seconds
        ],
    )?;
    transaction.execute(queries::llm_callback::DELETE_HASH_BANDS, [media_id])?;
    for band_index in 0..4_i64 {
        transaction.execute(
            queries::llm_callback::INSERT_HASH_BAND,
            rusqlite::params![
                media_id,
                band_index,
                ((perceptual_hash >> (band_index * 16)) & 0xffff) as i64
            ],
        )?;
    }
    transaction.execute(queries::llm_callback::UPSERT_DIRTY, [media_id])?;
    Ok(())
}
