use axum::{extract::State, routing::post, Json, Router};
use std::sync::Arc;

use crate::auth::{AppState, RequireAdmin};
use crate::error::{AppError, AppResult};
use crate::models::{
    ImportStatusResponse, ImportTriggerResponse, RegenerateRequest, RegenerateResponse,
    RegenerationStatusResponse,
};
use crate::processor::importer::{get_import_status, is_import_running, run_local_import};
use crate::processor::regenerator::{
    cancel_regeneration, clear_all_metadata_and_thumbnails, generate_missing_metadata,
    get_regeneration_status, is_regeneration_running,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/import/local", post(trigger_local_import))
        .route("/import/status", post(get_import_job_status))
        .route("/import/regenerate", post(trigger_regeneration))
        .route(
            "/import/regenerate/status",
            post(get_regeneration_job_status),
        )
        .route("/import/regenerate/cancel", post(cancel_regeneration_job))
        .route("/import/reset", post(trigger_reset))
}

async fn trigger_local_import(
    State(state): State<AppState>,
    RequireAdmin(admin): RequireAdmin,
) -> AppResult<Json<ImportTriggerResponse>> {
    if is_import_running() {
        return Err(AppError::Conflict("Import already in progress".to_string()));
    }

    let config = Arc::clone(&state.config);
    let pool = state.pool.clone();
    let user_id = admin.id;
    let concurrency = config.regenerate.num_cpus;

    tokio::spawn(async move {
        run_local_import(
            user_id,
            config.thumbnails.max_size,
            config.thumbnails.tiny_size,
            config.thumbnails.quality,
            config.thumbnails.video_frame_quality,
            true,
            Some(&config.reverse_geocoding),
            &pool,
            concurrency,
        )
        .await;
    });

    Ok(Json(ImportTriggerResponse {
        message: "Import started".to_string(),
        status: "running".to_string(),
    }))
}

async fn get_import_job_status(
    RequireAdmin(_): RequireAdmin,
) -> AppResult<Json<ImportStatusResponse>> {
    let job = get_import_status();

    Ok(Json(ImportStatusResponse {
        status: job.status.to_string(),
        total_files: job.total_files,
        processed_files: job.processed_files,
        successful_imports: job.successful_imports,
        failed_imports: job.failed_imports,
        started_at: job.started_at.map(|dt| dt.to_rfc3339()),
        completed_at: job.completed_at.map(|dt| dt.to_rfc3339()),
        errors: job.errors,
    }))
}

async fn trigger_regeneration(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(_request): Json<RegenerateRequest>,
) -> AppResult<Json<RegenerateResponse>> {
    if is_regeneration_running() {
        return Err(AppError::Conflict(
            "Regeneration already in progress".to_string(),
        ));
    }

    let config = Arc::clone(&state.config);
    let pool = state.pool.clone();

    tokio::spawn(async move {
        generate_missing_metadata(&config, &pool).await;
    });

    Ok(Json(RegenerateResponse {
        message: "Metadata generation started".to_string(),
        status: "running".to_string(),
    }))
}

async fn get_regeneration_job_status(
    RequireAdmin(_): RequireAdmin,
) -> AppResult<Json<RegenerationStatusResponse>> {
    let job = get_regeneration_status();

    Ok(Json(RegenerationStatusResponse {
        status: job.status.to_string(),
        total_media: job.total_media,
        processed_media: job.processed_media,
        updated_metadata: job.updated_metadata,
        generated_thumbnails: job.generated_thumbnails,
        updated_tags: job.updated_tags,
        started_at: job.started_at.map(|dt| dt.to_rfc3339()),
        completed_at: job.completed_at.map(|dt| dt.to_rfc3339()),
        errors: job.errors,
    }))
}

async fn cancel_regeneration_job(
    RequireAdmin(_): RequireAdmin,
) -> AppResult<Json<RegenerateResponse>> {
    if cancel_regeneration() {
        Ok(Json(RegenerateResponse {
            message: "Cancellation requested".to_string(),
            status: "cancelling".to_string(),
        }))
    } else {
        Ok(Json(RegenerateResponse {
            message: "No regeneration job to cancel".to_string(),
            status: "idle".to_string(),
        }))
    }
}

async fn trigger_reset(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
) -> AppResult<Json<RegenerateResponse>> {
    if is_regeneration_running() {
        return Err(AppError::Conflict(
            "Regeneration already in progress".to_string(),
        ));
    }

    if is_import_running() {
        return Err(AppError::Conflict("Import already in progress".to_string()));
    }

    let config = Arc::clone(&state.config);
    let pool = state.pool.clone();

    tokio::spawn(async move {
        let pool_clone = pool.clone();
        tokio::task::spawn_blocking(move || {
            clear_all_metadata_and_thumbnails(&pool_clone);
        })
        .await
        .unwrap();

        generate_missing_metadata(&config, &pool).await;
    });

    Ok(Json(RegenerateResponse {
        message: "Cleaning and regeneration started".to_string(),
        status: "running".to_string(),
    }))
}
