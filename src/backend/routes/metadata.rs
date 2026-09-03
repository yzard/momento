use axum::{extract::State, response::Response, routing::post, Router};

use crate::auth::{AppState, RequireAdmin};
use crate::database::operations::ResetMetadataOutcome;
use crate::error::{AppError, AppResult};
use crate::models::{MetadataActionResponse, MetadataRequest, MetadataStatusResponse};
use crate::routes::{render_json, CpuJson};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/metadata/generate", post(generate))
        .route("/metadata/status", post(status))
        .route("/metadata/reset", post(reset))
}

async fn generate(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    CpuJson(_request): CpuJson<MetadataRequest>,
) -> AppResult<Response> {
    let queued_jobs = state
        .executors
        .sqlite
        .queue_incomplete_metadata_request()
        .await? as i64;
    state.scheduler.wake_metadata();
    render_json(
        &state,
        MetadataActionResponse {
            message: "Metadata generation queued".to_string(),
            queued_jobs,
        },
    )
    .await
}

async fn status(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    CpuJson(_request): CpuJson<MetadataRequest>,
) -> AppResult<Response> {
    let job_status = state
        .executors
        .sqlite
        .load_metadata_job_status_request()
        .await?;
    let counts = job_status.counts;
    let count_for = |status: &str| {
        counts
            .iter()
            .find(|(job_status, _)| job_status == status)
            .map(|(_, count)| *count)
            .unwrap_or(0)
    };
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
    render_json(
        &state,
        MetadataStatusResponse {
            status: status.to_string(),
            queued_jobs,
            processing_jobs,
            completed_jobs: count_for("completed"),
            failed_jobs,
            errors: job_status.errors,
            face_groups: None,
        },
    )
    .await
}

async fn reset(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    CpuJson(_request): CpuJson<MetadataRequest>,
) -> AppResult<Response> {
    let cleanup_group_id = format!("metadata-reset-{}", uuid::Uuid::new_v4().simple());
    match state
        .executors
        .sqlite
        .reset_metadata_request(cleanup_group_id)
        .await?
    {
        ResetMetadataOutcome::Reset { .. } => state.scheduler.wake_journal_recovery(),
        ResetMetadataOutcome::PathConflict => {
            return Err(AppError::Conflict(
                "metadata reset conflicts with active file work".to_string(),
            ));
        }
    }
    state.scheduler.wake_metadata();
    let queued_jobs = state
        .executors
        .sqlite
        .queue_incomplete_metadata_request()
        .await? as i64;
    render_json(
        &state,
        MetadataActionResponse {
            message: "Metadata and AI data reset".to_string(),
            queued_jobs,
        },
    )
    .await
}
