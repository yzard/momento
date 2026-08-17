use axum::{
    extract::{DefaultBodyLimit, Multipart, State},
    http::header::HeaderMap,
    routing::{get, post},
    Json, Router,
};
use momento_common::llm::{CancelJobsRequest, CancelJobsResponse};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::config::Config;
use crate::error::ServiceError;
use crate::provider::ServiceManager;
use crate::scheduler::{QueueAdmission, QueueManifest, Scheduler};

const MAX_MANIFEST_BYTES: usize = 1024 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub manager: Arc<Mutex<ServiceManager>>,
    pub scheduler: Arc<Scheduler>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/jobs/submit", post(submit))
        .route("/api/v1/ai/cancel", post(cancel))
        .route("/health", get(health))
        .route("/ready", get(ready))
        .layer(DefaultBodyLimit::disable())
        .with_state(state)
}

async fn cancel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CancelJobsRequest>,
) -> Result<Json<CancelJobsResponse>, ServiceError> {
    validate_api_key(&headers, &state.config.server.api_key)?;
    Ok(Json(state.scheduler.cancel_jobs(&request)?))
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    provider: &'static str,
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy",
        provider: state.manager.lock().await.active_name(),
    })
}
async fn ready(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ready",
        provider: state.manager.lock().await.active_name(),
    })
}

async fn submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, ServiceError> {
    validate_api_key(&headers, &state.config.server.api_key)?;
    let mut manifest = None;
    let mut admission = None;
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|error| ServiceError::BadRequest(error.to_string()))?
    {
        match field.name() {
            Some("manifest") => {
                let mut bytes = Vec::new();
                while let Some(chunk) = field
                    .chunk()
                    .await
                    .map_err(|error| ServiceError::BadRequest(error.to_string()))?
                {
                    if bytes.len() + chunk.len() > MAX_MANIFEST_BYTES {
                        return Err(ServiceError::BadRequest(
                            "manifest exceeds 1 MiB".to_string(),
                        ));
                    }
                    bytes.extend_from_slice(&chunk);
                }
                if manifest.is_some() {
                    return Err(ServiceError::BadRequest(
                        "manifest may only be supplied once".to_string(),
                    ));
                }
                let parsed_manifest = serde_json::from_slice::<QueueManifest>(&bytes)
                    .map_err(|error| ServiceError::BadRequest(error.to_string()))?;
                admission = Some(state.scheduler.begin_admission(parsed_manifest.clone())?);
                manifest = Some(parsed_manifest);
            }
            Some(name) if name.starts_with("input-") => {
                let sequence = name
                    .strip_prefix("input-")
                    .and_then(|value| value.parse::<u32>().ok())
                    .ok_or_else(|| {
                        ServiceError::BadRequest("input field names must use input-N".to_string())
                    })?;
                let manifest = manifest.as_ref().ok_or_else(|| {
                    ServiceError::BadRequest("manifest must be supplied before inputs".to_string())
                })?;
                let descriptor = manifest
                    .inputs
                    .iter()
                    .find(|descriptor| descriptor.sequence == sequence)
                    .cloned()
                    .ok_or_else(|| {
                        ServiceError::BadRequest(
                            "multipart input has no manifest descriptor".to_string(),
                        )
                    })?;
                let Some(QueueAdmission::Staging(staging)) = admission.as_mut() else {
                    while field
                        .chunk()
                        .await
                        .map_err(|error| ServiceError::BadRequest(error.to_string()))?
                        .is_some()
                    {}
                    continue;
                };
                let input_path = staging.input_path(&descriptor)?;
                let mut input_file = tokio::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(input_path)
                    .await
                    .map_err(|error| ServiceError::Internal(error.to_string()))?;
                let mut byte_count = 0_u64;
                let mut hasher = Sha256::new();
                while let Some(chunk) = field
                    .chunk()
                    .await
                    .map_err(|error| ServiceError::BadRequest(error.to_string()))?
                {
                    let chunk_size = u64::try_from(chunk.len()).map_err(|_| {
                        ServiceError::BadRequest("multipart input chunk is too large".to_string())
                    })?;
                    byte_count = byte_count.checked_add(chunk_size).ok_or_else(|| {
                        ServiceError::BadRequest(
                            "multipart input exceeds descriptor size".to_string(),
                        )
                    })?;
                    if byte_count > descriptor.byte_size {
                        return Err(ServiceError::BadRequest(
                            "multipart input exceeds descriptor size".to_string(),
                        ));
                    }
                    tokio::io::AsyncWriteExt::write_all(&mut input_file, &chunk)
                        .await
                        .map_err(|error| ServiceError::Internal(error.to_string()))?;
                    hasher.update(&chunk);
                }
                input_file
                    .sync_all()
                    .await
                    .map_err(|error| ServiceError::Internal(error.to_string()))?;
                drop(input_file);
                staging.verify_input(&descriptor, byte_count, hasher.finalize())?;
            }
            _ => {
                return Err(ServiceError::BadRequest(
                    "only manifest and input-N multipart fields are allowed".to_string(),
                ))
            }
        }
    }
    let manifest = manifest.ok_or_else(|| {
        ServiceError::BadRequest("manifest multipart field is required".to_string())
    })?;
    let admission =
        admission.ok_or_else(|| ServiceError::Internal("missing queue admission".to_string()))?;
    let status = match admission {
        QueueAdmission::Cancelled => "cancelled",
        QueueAdmission::Duplicate => "queued",
        QueueAdmission::Staging(staging) => {
            if staging.commit()? {
                "queued"
            } else {
                "cancelled"
            }
        }
    };
    Ok(Json(
        serde_json::json!({ "jobId": manifest.job_id, "status": status }),
    ))
}

fn validate_api_key(headers: &HeaderMap, configured_key: &str) -> Result<(), ServiceError> {
    if configured_key.is_empty() {
        return Ok(());
    }
    let provided_key = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if provided_key == configured_key {
        Ok(())
    } else {
        Err(ServiceError::Authentication)
    }
}

pub async fn serve(
    listener: tokio::net::TcpListener,
    state: AppState,
) -> Result<(), std::io::Error> {
    let manager = Arc::clone(&state.manager);
    let server_result = axum::serve(listener, router(state)).await;
    if let Err(error) = manager.lock().await.shutdown().await {
        tracing::error!("Failed to stop active LLM runtime: {error}");
    }
    server_result
}
