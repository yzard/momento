use axum::{extract::State, routing::post, Json, Router};

use crate::auth::{AppState, RequireAdmin};
use crate::database::queries;
use crate::error::AppResult;
use crate::models::{AiRequest, MetadataActionResponse, MetadataStatusResponse};
use crate::processor::ai;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ai/trigger", post(trigger))
        .route("/ai/ocr/trigger", post(trigger_ocr))
        .route("/ai/ocr/status", post(ocr_status))
        .route("/ai/ocr/reset", post(reset_ocr))
        .route("/ai/image_tagging/trigger", post(trigger_image_tagging))
        .route("/ai/image_tagging/status", post(image_tagging_status))
        .route("/ai/image_tagging/reset", post(reset_image_tagging))
}

async fn ocr_status(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(_request): Json<AiRequest>,
) -> AppResult<Json<MetadataStatusResponse>> {
    task_status(&state.pool, "ocr")
}

async fn image_tagging_status(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(_request): Json<AiRequest>,
) -> AppResult<Json<MetadataStatusResponse>> {
    task_status(&state.pool, "image_tagging")
}

async fn reset_ocr(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(_request): Json<AiRequest>,
) -> AppResult<Json<MetadataActionResponse>> {
    reset_task(&state.pool, "ocr")
}

async fn reset_image_tagging(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(_request): Json<AiRequest>,
) -> AppResult<Json<MetadataActionResponse>> {
    reset_task(&state.pool, "image_tagging")
}

fn task_status(
    pool: &crate::database::DbPool,
    task: &str,
) -> AppResult<Json<MetadataStatusResponse>> {
    let connection = pool.get()?;
    let counts = connection
        .prepare(queries::ai_jobs::SELECT_STATUS_COUNTS)?
        .query_map([task], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let count_for = |status: &str| {
        counts
            .iter()
            .find(|(job_status, _)| job_status == status)
            .map(|(_, count)| *count)
            .unwrap_or(0)
    };
    let queued_jobs = count_for("queued");
    let processing_jobs = count_for("submitting") + count_for("submitted");
    let failed_jobs = count_for("failed");
    let status = if processing_jobs > 0 {
        "processing"
    } else if queued_jobs > 0 {
        "queued"
    } else if failed_jobs > 0 {
        "failed"
    } else {
        "idle"
    };
    let errors = connection
        .prepare(queries::ai_jobs::SELECT_FAILURES)?
        .query_map([task], |row| row.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(Json(MetadataStatusResponse {
        status: status.to_string(),
        queued_jobs,
        processing_jobs,
        completed_jobs: count_for("completed"),
        failed_jobs,
        errors,
    }))
}

fn reset_task(
    pool: &crate::database::DbPool,
    task: &str,
) -> AppResult<Json<MetadataActionResponse>> {
    let connection = pool.get()?;
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(queries::ai_jobs::DELETE_TEXT_FOR_TASK, [task])?;
    transaction.execute(queries::ai_jobs::CANCEL_ACTIVE_FOR_TASK, [task])?;
    transaction.commit()?;
    let queued_jobs = ai::queue_task(pool, task, true)? as i64;
    Ok(Json(MetadataActionResponse {
        message: format!("{task} processing reset"),
        queued_jobs,
    }))
}

async fn trigger(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(_request): Json<AiRequest>,
) -> AppResult<Json<MetadataActionResponse>> {
    let queued_jobs = ai::queue_all(&state.pool, state.config.llm.image_tagging_enabled)? as i64;
    Ok(Json(MetadataActionResponse {
        message: "AI processing queued".to_string(),
        queued_jobs,
    }))
}

async fn trigger_ocr(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(_request): Json<AiRequest>,
) -> AppResult<Json<MetadataActionResponse>> {
    let queued_jobs = ai::queue_task(&state.pool, "ocr", true)? as i64;
    Ok(Json(MetadataActionResponse {
        message: "OCR processing queued".to_string(),
        queued_jobs,
    }))
}

async fn trigger_image_tagging(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(_request): Json<AiRequest>,
) -> AppResult<Json<MetadataActionResponse>> {
    let queued_jobs = ai::queue_task(
        &state.pool,
        "image_tagging",
        state.config.llm.image_tagging_enabled,
    )? as i64;
    Ok(Json(MetadataActionResponse {
        message: "Image tagging processing queued".to_string(),
        queued_jobs,
    }))
}
