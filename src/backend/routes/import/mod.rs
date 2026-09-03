use axum::{extract::State, response::Response, routing::post, Router};

use crate::auth::{AppState, RequireAdmin};
use crate::error::{AppError, AppResult};
use crate::models::{ImportStatusResponse, ImportTriggerResponse};
use crate::processor::import::{
    run_local_import, CreateImportJobOutcome, ImportSettings, ImportSource,
};
use crate::routes::render_json;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/import/local", post(trigger_local_import))
        .route("/import/status", post(get_import_job_status))
}

async fn trigger_local_import(
    State(state): State<AppState>,
    RequireAdmin(admin): RequireAdmin,
) -> AppResult<Response> {
    let user_id = admin.id;
    let settings = ImportSettings {
        user_id,
        executors: state.executors.clone(),
        scheduler: state.scheduler.clone(),
    };

    let job_id = match state
        .executors
        .sqlite
        .create_import_job_request(ImportSource::Local)
        .await?
    {
        CreateImportJobOutcome::Created(job_id) => job_id,
        CreateImportJobOutcome::AlreadyRunning => {
            return Err(AppError::Conflict("Import already in progress".to_string()));
        }
    };
    state.scheduler.spawn_control(async move {
        run_local_import(settings, job_id).await;
    });

    render_json(
        &state,
        ImportTriggerResponse {
            message: "Import started".to_string(),
            status: "running".to_string(),
        },
    )
    .await
}

async fn get_import_job_status(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
) -> AppResult<Response> {
    let snapshot = state
        .executors
        .sqlite
        .load_import_status_request(ImportSource::Local)
        .await?;
    let job = snapshot.job;

    render_json(
        &state,
        ImportStatusResponse {
            status: job.status,
            total_files: job.total_files,
            processed_files: job.processed_files,
            total_media: snapshot.total_media,
            successful_imports: job.successful_imports,
            failed_imports: job.failed_imports,
            started_at: job.started_at,
            completed_at: job.completed_at,
            errors: job.errors,
        },
    )
    .await
}
