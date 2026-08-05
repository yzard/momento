use axum::extract::{DefaultBodyLimit, Multipart, State};
use axum::http::header::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;
use std::sync::Arc;

use crate::config::Config;
use crate::error::ServiceError;
use crate::provider::{InferenceResponse, Provider, RamProvider};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub provider: Arc<Provider>,
    pub image_tagging: Option<Arc<RamProvider>>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/infer", post(infer))
        .route("/health", get(health))
        .route("/ready", get(ready))
        .layer(DefaultBodyLimit::max(
            state.config.general.max_request_bytes,
        ))
        .with_state(state)
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    provider: &'static str,
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy",
        provider: state.provider.name(),
    })
}

async fn ready(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ready",
        provider: state.provider.name(),
    })
}

async fn infer(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<InferenceResponse>, ServiceError> {
    validate_api_key(&headers, &state.config.general.api_key)?;

    let mut task = None;
    let mut filename = None;
    let mut content_type = None;
    let mut image = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| ServiceError::BadRequest(format!("invalid multipart body: {error}")))?
    {
        match field.name() {
            Some("task") => {
                task = Some(field.text().await.map_err(|error| {
                    ServiceError::BadRequest(format!("failed to read task: {error}"))
                })?);
            }
            Some("file") => {
                filename = field.file_name().map(ToString::to_string);
                content_type = field.content_type().map(ToString::to_string);
                image = Some(field.bytes().await.map_err(|error| {
                    ServiceError::BadRequest(format!("failed to read image: {error}"))
                })?);
            }
            _ => {}
        }
    }

    let filename = filename.unwrap_or_else(|| "image.jpg".to_string());
    let image = image.ok_or_else(|| ServiceError::BadRequest("missing file field".to_string()))?;
    if image.is_empty() {
        return Err(ServiceError::BadRequest(
            "image must not be empty".to_string(),
        ));
    }
    if !is_supported_image(content_type.as_deref(), &filename) {
        return Err(ServiceError::BadRequest(
            "only image files are supported".to_string(),
        ));
    }

    let task = task.ok_or_else(|| ServiceError::BadRequest("missing task field".to_string()))?;
    if task == "image_tagging" {
        let tagger = state.image_tagging.as_ref().ok_or_else(|| {
            ServiceError::NotImplemented(
                "image tagging has no configured model provider".to_string(),
            )
        })?;
        return Ok(Json(tagger.infer(&image).await?));
    }
    if task != "ocr" {
        return Err(ServiceError::NotImplemented(format!(
            "inference task `{task}` has no configured model provider"
        )));
    }

    let result = state.provider.infer(&image, &filename).await?;
    Ok(Json(result))
}

fn validate_api_key(headers: &HeaderMap, configured_key: &str) -> Result<(), ServiceError> {
    if configured_key.is_empty() {
        return Ok(());
    }
    let provided_key = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if provided_key != configured_key {
        return Err(ServiceError::Authentication);
    }
    Ok(())
}

fn is_supported_image(content_type: Option<&str>, filename: &str) -> bool {
    if content_type
        .map(|value| value.starts_with("image/"))
        .unwrap_or(false)
    {
        return true;
    }

    mime_guess::from_path(filename)
        .first_raw()
        .map(|value| value.starts_with("image/"))
        .unwrap_or(false)
}

pub async fn serve(
    listener: tokio::net::TcpListener,
    state: AppState,
) -> Result<(), std::io::Error> {
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let ctrl_c = tokio::signal::ctrl_c();
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("failed to install SIGTERM handler");

        tokio::select! {
            _ = ctrl_c => {}
            _ = terminate.recv() => {}
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl-C handler");
    }
}

#[cfg(test)]
mod tests {
    use super::is_supported_image;

    #[test]
    fn recognizes_image_content_types_and_extensions() {
        assert!(is_supported_image(Some("image/jpeg"), "unknown.bin"));
        assert!(is_supported_image(None, "photo.png"));
        assert!(!is_supported_image(Some("text/plain"), "notes.txt"));
    }
}
