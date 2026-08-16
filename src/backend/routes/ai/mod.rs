use axum::{extract::State, routing::post, Json, Router};

use crate::auth::{AppState, RequireAdmin};
use crate::database::queries;
use crate::error::AppResult;
use crate::models::{AiRequest, MetadataActionResponse, MetadataStatusResponse};
use crate::processor::ai;
use crate::processor::deduplicator::{
    clean, create_run, latest_run, queue_clustering_jobs, request_cancel,
};
use crate::processor::face_detection;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ai/trigger", post(trigger))
        .route("/ai/cancel", post(cancel))
        .route("/ai/clean", post(clean_all))
        .route("/ai/ocr/trigger", post(trigger_ocr))
        .route("/ai/ocr/cancel", post(cancel_ocr))
        .route("/ai/ocr/clean", post(clean_ocr))
        .route("/ai/ocr/status", post(ocr_status))
        .route("/ai/ocr/reset", post(reset_ocr))
        .route("/ai/image_tagging/trigger", post(trigger_image_tagging))
        .route("/ai/image_tagging/cancel", post(cancel_image_tagging))
        .route("/ai/image_tagging/clean", post(clean_image_tagging))
        .route("/ai/image_tagging/status", post(image_tagging_status))
        .route("/ai/image_tagging/reset", post(reset_image_tagging))
        .route(
            "/ai/image_clustering/trigger",
            post(trigger_image_clustering),
        )
        .route("/ai/image_clustering/cancel", post(cancel_image_clustering))
        .route("/ai/image_clustering/clean", post(clean_image_clustering))
        .route("/ai/faces/start", post(start_faces))
        .route("/ai/faces/cancel", post(cancel_faces))
        .route("/ai/faces/clean", post(clean_faces))
        .route("/ai/faces/status", post(faces_status))
}

async fn cancel_ocr(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(_request): Json<AiRequest>,
) -> AppResult<Json<MetadataActionResponse>> {
    cancel_task(&state, "ocr").await
}
async fn clean_ocr(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(_request): Json<AiRequest>,
) -> AppResult<Json<MetadataActionResponse>> {
    clean_task(&state.pool, "ocr")
}
async fn cancel_image_tagging(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(_request): Json<AiRequest>,
) -> AppResult<Json<MetadataActionResponse>> {
    cancel_task(&state, "image_tagging").await
}
async fn clean_image_tagging(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(_request): Json<AiRequest>,
) -> AppResult<Json<MetadataActionResponse>> {
    clean_task(&state.pool, "image_tagging")
}
async fn cancel_image_clustering(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(_request): Json<AiRequest>,
) -> AppResult<Json<MetadataActionResponse>> {
    let cancelled_jobs = ai::cancel_active_jobs(&state.pool, Some("image_clustering"))? as i64;
    request_cancel(&state.pool)?;
    deliver_cancellations(&state).await;
    Ok(Json(MetadataActionResponse {
        message: "Image clustering cancellation requested".to_string(),
        queued_jobs: cancelled_jobs,
    }))
}
async fn clean_image_clustering(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(_request): Json<AiRequest>,
) -> AppResult<Json<MetadataActionResponse>> {
    clean(&state.pool)?;
    Ok(Json(MetadataActionResponse {
        message: "Image clustering data cleaned".to_string(),
        queued_jobs: 0,
    }))
}

async fn start_faces(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(_request): Json<AiRequest>,
) -> AppResult<Json<MetadataActionResponse>> {
    let queued_jobs =
        face_detection::start(&state.pool, state.config.llm.face_detection_enabled)? as i64;
    Ok(Json(MetadataActionResponse {
        message: "Face detection processing queued".to_string(),
        queued_jobs,
    }))
}

async fn cancel_faces(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(_request): Json<AiRequest>,
) -> AppResult<Json<MetadataActionResponse>> {
    ai::cancel_active_jobs(&state.pool, Some("face_detection"))?;
    face_detection::cancel(&state.pool)?;
    deliver_cancellations(&state).await;
    Ok(Json(MetadataActionResponse {
        message: "Face detection cancelled".to_string(),
        queued_jobs: 0,
    }))
}

async fn clean_faces(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(_request): Json<AiRequest>,
) -> AppResult<Json<MetadataActionResponse>> {
    face_detection::clean(&state.pool)?;
    Ok(Json(MetadataActionResponse {
        message: "Face detection data cleaned".to_string(),
        queued_jobs: 0,
    }))
}

async fn faces_status(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(_request): Json<AiRequest>,
) -> AppResult<Json<MetadataStatusResponse>> {
    let Json(mut status) = task_status(&state.pool, "face_detection")?;
    status.face_groups = Some(state.pool.get()?.query_row(
        queries::faces::COUNT_GROUPS,
        [],
        |row| row.get(0),
    )?);
    Ok(Json(status))
}

async fn cancel_task(state: &AppState, task: &str) -> AppResult<Json<MetadataActionResponse>> {
    let cancelled_jobs = ai::cancel_active_jobs(&state.pool, Some(task))? as i64;
    deliver_cancellations(state).await;
    Ok(Json(MetadataActionResponse {
        message: format!("{task} jobs cancelled"),
        queued_jobs: cancelled_jobs,
    }))
}

fn clean_task(
    pool: &crate::database::DbPool,
    task: &str,
) -> AppResult<Json<MetadataActionResponse>> {
    let connection = pool.get()?;
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(queries::ai_jobs::DELETE_TEXT_FOR_TASK, [task])?;
    transaction.execute(queries::ai_jobs::DELETE_TEXT_INPUTS_FOR_TASK, [task])?;
    transaction.execute(queries::ai_jobs::DELETE_JOBS_FOR_TASK, [task])?;
    transaction.commit()?;
    Ok(Json(MetadataActionResponse {
        message: format!("{task} data cleaned"),
        queued_jobs: 0,
    }))
}

async fn clean_all(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(_request): Json<AiRequest>,
) -> AppResult<Json<MetadataActionResponse>> {
    let _ = clean_task(&state.pool, "ocr")?;
    let _ = clean_task(&state.pool, "image_tagging")?;
    face_detection::clean(&state.pool)?;
    clean(&state.pool)?;
    Ok(Json(MetadataActionResponse {
        message: "All AI data cleaned".to_string(),
        queued_jobs: 0,
    }))
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
    reset_task(&state, "ocr").await
}

async fn reset_image_tagging(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(_request): Json<AiRequest>,
) -> AppResult<Json<MetadataActionResponse>> {
    reset_task(&state, "image_tagging").await
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
        face_groups: None,
    }))
}

async fn reset_task(state: &AppState, task: &str) -> AppResult<Json<MetadataActionResponse>> {
    ai::cancel_active_jobs(&state.pool, Some(task))?;
    deliver_cancellations(state).await;
    let connection = state.pool.get()?;
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(queries::ai_jobs::DELETE_TEXT_FOR_TASK, [task])?;
    transaction.commit()?;
    let queued_jobs = ai::queue_task(&state.pool, task, true)? as i64;
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
    let face_jobs = if state.config.llm.face_detection_enabled {
        face_detection::start(&state.pool, true)? as i64
    } else {
        0
    };
    let clustering_jobs =
        trigger_clustering_jobs(&state.pool, state.config.llm.deduplicate_enabled)? as i64;
    Ok(Json(MetadataActionResponse {
        message: "AI processing queued".to_string(),
        queued_jobs: queued_jobs + clustering_jobs + face_jobs,
    }))
}

async fn cancel(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(_request): Json<AiRequest>,
) -> AppResult<Json<MetadataActionResponse>> {
    let cancelled_jobs = ai::cancel_active_jobs(&state.pool, None)? as i64;
    let clustering_cancelled = request_cancel(&state.pool)?;
    face_detection::cancel(&state.pool)?;
    deliver_cancellations(&state).await;
    Ok(Json(MetadataActionResponse {
        message: if clustering_cancelled {
            "AI jobs cancelled and image clustering cancellation requested".to_string()
        } else {
            "AI jobs cancelled".to_string()
        },
        queued_jobs: cancelled_jobs,
    }))
}

async fn deliver_cancellations(state: &AppState) {
    if let Err(error) = ai::deliver_pending_cancellations(&state.config, &state.pool).await {
        tracing::warn!("immediate LLM cancellation delivery failed: {error}");
    }
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

async fn trigger_image_clustering(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(_request): Json<AiRequest>,
) -> AppResult<Json<MetadataActionResponse>> {
    let queued_jobs =
        trigger_clustering_jobs(&state.pool, state.config.llm.deduplicate_enabled)? as i64;
    Ok(Json(MetadataActionResponse {
        message: "Image clustering processing queued".to_string(),
        queued_jobs,
    }))
}

fn trigger_clustering_jobs(
    pool: &crate::database::DbPool,
    deduplicate_enabled: bool,
) -> AppResult<usize> {
    if !deduplicate_enabled {
        return Ok(0);
    }
    if latest_run(pool)?.is_some_and(|run| matches!(run.status.as_str(), "running" | "cancelling"))
    {
        return Ok(0);
    }
    let run_id = create_run(pool, "manual", None)?;
    queue_clustering_jobs(pool, run_id)
}
