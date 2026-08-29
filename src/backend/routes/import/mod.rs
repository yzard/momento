use axum::{extract::State, routing::post, Json, Router};

use crate::auth::{AppState, RequireAdmin};
use crate::database::queries;
use crate::error::AppResult;
use crate::models::{ImportStatusResponse, ImportTriggerResponse};
use crate::processor::import::{
    create_import_job, get_import_status, run_local_import, ImportSettings, ImportSource,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/import/local", post(trigger_local_import))
        .route("/import/status", post(get_import_job_status))
}

async fn trigger_local_import(
    State(state): State<AppState>,
    RequireAdmin(admin): RequireAdmin,
) -> AppResult<Json<ImportTriggerResponse>> {
    let config = state.config.current();
    let pool = state.pool.clone();
    let user_id = admin.id;
    let settings = ImportSettings {
        user_id,
        pool,
        concurrency: config.regenerate.num_cpus,
    };

    let job_id = create_import_job(&state.pool, ImportSource::Local)?;
    tokio::spawn(async move {
        run_local_import(settings, job_id).await;
    });

    Ok(Json(ImportTriggerResponse {
        message: "Import started".to_string(),
        status: "running".to_string(),
    }))
}

async fn get_import_job_status(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
) -> AppResult<Json<ImportStatusResponse>> {
    let job = get_import_status(&state.pool, ImportSource::Local)?;
    let total_media =
        state
            .pool
            .get()?
            .query_row(queries::import::COUNT_IMPORTED_MEDIA, [], |row| row.get(0))?;

    Ok(Json(ImportStatusResponse {
        status: job.status,
        total_files: job.total_files,
        processed_files: job.processed_files,
        total_media,
        successful_imports: job.successful_imports,
        failed_imports: job.failed_imports,
        started_at: job.started_at,
        completed_at: job.completed_at,
        errors: job.errors,
    }))
}
