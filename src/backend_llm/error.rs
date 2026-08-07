use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("authentication failed")]
    Authentication,
    #[error("this inference task is not configured")]
    NotImplemented(String),
    #[error("configuration error: {0}")]
    Configuration(String),
    #[error("upstream inference request failed: {0}")]
    Upstream(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        let (status, detail) = match self {
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            Self::Authentication => (StatusCode::UNAUTHORIZED, "invalid API key".to_string()),
            Self::NotImplemented(message) => (StatusCode::NOT_IMPLEMENTED, message),
            Self::Configuration(message) => (StatusCode::INTERNAL_SERVER_ERROR, message),
            Self::Upstream(message) => (StatusCode::BAD_GATEWAY, message),
            Self::Internal(message) => (StatusCode::INTERNAL_SERVER_ERROR, message),
        };

        (status, Json(json!({ "detail": detail }))).into_response()
    }
}
