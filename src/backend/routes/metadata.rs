use axum::{extract::State, routing::post, Json, Router};

use crate::auth::{AppState, RequireAdmin};
use crate::database::queries;
use crate::error::AppResult;
use crate::models::{MetadataActionResponse, MetadataRequest, MetadataStatusResponse};
use crate::processor::metadata_worker;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/metadata/generate", post(generate))
        .route("/metadata/status", post(status))
        .route("/metadata/reset", post(reset))
}

async fn generate(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(_request): Json<MetadataRequest>,
) -> AppResult<Json<MetadataActionResponse>> {
    let queued_jobs = metadata_worker::queue_incomplete(&state.pool)? as i64;
    Ok(Json(MetadataActionResponse {
        message: "Metadata generation queued".to_string(),
        queued_jobs,
    }))
}

async fn status(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(_request): Json<MetadataRequest>,
) -> AppResult<Json<MetadataStatusResponse>> {
    let counts = metadata_worker::status_counts(&state.pool)?;
    let count_for = |status: &str| {
        counts
            .iter()
            .find(|(job_status, _)| job_status == status)
            .map(|(_, count)| *count)
            .unwrap_or(0)
    };
    let connection = state.pool.get()?;
    let errors = connection
        .prepare(queries::metadata_jobs::SELECT_FAILURES)?
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    let queued_jobs = count_for("queued");
    let processing_jobs = count_for("processing");
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
    Ok(Json(MetadataStatusResponse {
        status: status.to_string(),
        queued_jobs,
        processing_jobs,
        completed_jobs: count_for("completed"),
        failed_jobs,
        errors,
    }))
}

async fn reset(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(_request): Json<MetadataRequest>,
) -> AppResult<Json<MetadataActionResponse>> {
    metadata_worker::reset_all(&state.pool)?;
    let connection = state.pool.get()?;
    let queued_jobs = connection.execute(queries::metadata_jobs::QUEUE_INCOMPLETE, [])? as i64;
    Ok(Json(MetadataActionResponse {
        message: "Metadata and AI data reset".to_string(),
        queued_jobs,
    }))
}
