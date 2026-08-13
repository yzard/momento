use axum::{extract::State, http::header::HeaderMap, routing::post, Json, Router};

use crate::auth::AppState;
use crate::database::queries;
use crate::error::{AppError, AppResult};
use crate::models::LlmCallbackRequest;

pub fn router() -> Router<AppState> {
    Router::new().route("/internal/llm/callback", post(callback))
}

async fn callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<LlmCallbackRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let callback_key = headers
        .get("x-momento-callback-key")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if state.config.llm.callback_key.is_empty() || callback_key != state.config.llm.callback_key {
        return Err(AppError::Authentication(
            "invalid LLM callback key".to_string(),
        ));
    }
    let connection = state.pool.get()?;
    let transaction = connection.unchecked_transaction()?;
    let job: (i64, String, i64, String) = transaction
        .query_row(
            queries::llm_callback::SELECT_JOB,
            [&request.job_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|_| AppError::NotFound("LLM job not found".to_string()))?;
    if job.0 != request.media_id || job.1 != request.task || job.2 != request.attempt {
        return Err(AppError::Conflict(
            "LLM callback does not match submitted job".to_string(),
        ));
    }
    if matches!(job.3.as_str(), "completed" | "failed" | "cancelled") {
        return Ok(Json(serde_json::json!({"status":"acknowledged"})));
    }
    if job.3 != "submitted" {
        return Err(AppError::Conflict(
            "LLM callback job is not awaiting a result".to_string(),
        ));
    }
    if !matches!(request.status.as_str(), "completed" | "failed") {
        return Err(AppError::BadRequest(
            "LLM callback status must be completed or failed".to_string(),
        ));
    }
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
        } else if matches!(request.task.as_str(), "ocr" | "image_tagging") {
            persist_text_results(
                &transaction,
                request.media_id,
                &model_type,
                &model_version,
                &result,
                request.input_results.as_deref(),
            )?;
        } else {
            return Err(AppError::BadRequest(
                "completed callback task is not supported".to_string(),
            ));
        }
        if transaction.execute(
            queries::llm_callback::MARK_COMPLETED,
            rusqlite::params![request.job_id, request.attempt],
        )? != 1
        {
            return Err(AppError::Conflict(
                "LLM callback job changed during persistence".to_string(),
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
            "LLM callback job changed during persistence".to_string(),
        ));
    }
    transaction.commit()?;
    Ok(Json(serde_json::json!({"status":"acknowledged"})))
}

fn persist_text_results(
    transaction: &rusqlite::Transaction<'_>,
    media_id: i64,
    model_type: &str,
    model_version: &str,
    result: &serde_json::Value,
    input_results: Option<&[crate::models::LlmInputResult]>,
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
