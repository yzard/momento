use axum::{extract::State, response::Response, routing::post, Router};

use crate::auth::{AppState, RequireAdmin};
use crate::database::operations::CleanMetadataOutcome;
use crate::error::{AppError, AppResult};
use crate::models::{MetadataActionResponse, MetadataRequest, MetadataStatusResponse};
use crate::routes::{render_json, CpuJson};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/metadata/generate", post(generate))
        .route("/metadata/cancel", post(cancel))
        .route("/metadata/clean", post(clean))
        .route("/metadata/status", post(status))
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
            affected_jobs: queued_jobs,
        },
    )
    .await
}

async fn cancel(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    CpuJson(_request): CpuJson<MetadataRequest>,
) -> AppResult<Response> {
    let affected_jobs = state
        .executors
        .sqlite
        .cancel_active_metadata_jobs_request()
        .await? as i64;
    state.scheduler.wake_metadata();
    render_json(
        &state,
        MetadataActionResponse {
            message: "Metadata generation cancelled".to_string(),
            affected_jobs,
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
        .load_metadata_job_status_durable()
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
    let cancelling_jobs = count_for("cancelling");
    let failed_jobs = count_for("failed");
    let status = if count_for("cleaning") > 0 {
        "cleaning"
    } else if cancelling_jobs > 0 {
        "cancelling"
    } else if processing_jobs > 0 {
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
            waiting_for_rollback_jobs: count_for("waiting_for_rollback"),
            processing_jobs,
            cancelling_jobs,
            completed_jobs: count_for("completed"),
            failed_jobs,
            errors: job_status.errors,
            face_groups: None,
        },
    )
    .await
}

async fn clean(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    CpuJson(_request): CpuJson<MetadataRequest>,
) -> AppResult<Response> {
    let cleanup_group_id = format!("metadata-clean-{}", uuid::Uuid::new_v4().simple());
    let media_count = match state
        .executors
        .sqlite
        .clean_metadata_request(cleanup_group_id)
        .await?
    {
        CleanMetadataOutcome::Cleaned { media_count } => {
            state.scheduler.wake_journal_recovery();
            media_count
        }
        CleanMetadataOutcome::PathConflict => {
            return Err(AppError::Conflict(
                "metadata cleanup conflicts with active file work; cancel metadata generation and wait for cancellation to finish"
                    .to_string(),
            ));
        }
    };
    render_json(
        &state,
        MetadataActionResponse {
            message: "Metadata and related AI data cleaned".to_string(),
            affected_jobs: media_count,
        },
    )
    .await
}
