use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Authentication failed: {0}")]
    Authentication(String),

    #[error("Authorization failed: {0}")]
    Authorization(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Database error: {0}")]
    Database(rusqlite::Error),

    #[error("Database is busy")]
    DatabaseBusy,

    #[error("Pool error: {0}")]
    Pool(#[from] r2d2::Error),

    #[error("JWT error: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Request error: {0}")]
    Request(#[from] reqwest::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        if matches!(&self, AppError::DatabaseBusy) {
            tracing::warn!("Database is busy; request should be retried");
            let mut response = (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "detail": "Database is busy; retry shortly" })),
            )
                .into_response();
            response.headers_mut().insert(
                axum::http::header::RETRY_AFTER,
                axum::http::HeaderValue::from_static("1"),
            );
            return response;
        }
        let (status, message) = match &self {
            AppError::Authentication(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            AppError::Authorization(msg) => (StatusCode::FORBIDDEN, msg.clone()),
            AppError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg.clone()),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            AppError::Validation(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::Conflict(msg) => (StatusCode::CONFLICT, msg.clone()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::Internal(msg) => {
                tracing::error!(
                    "Internal error: {}\nBacktrace: {:?}",
                    msg,
                    std::backtrace::Backtrace::capture()
                );
                (StatusCode::INTERNAL_SERVER_ERROR, msg.clone())
            }
            AppError::Database(e) => {
                tracing::error!(
                    "Database error: {}\nBacktrace: {:?}",
                    e,
                    std::backtrace::Backtrace::capture()
                );
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Database error".to_string(),
                )
            }
            AppError::DatabaseBusy => unreachable!(),
            AppError::Pool(e) => {
                tracing::error!(
                    "Pool error: {}\nBacktrace: {:?}",
                    e,
                    std::backtrace::Backtrace::capture()
                );
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Connection pool error".to_string(),
                )
            }
            AppError::Jwt(e) => {
                tracing::error!("JWT error: {}", e);
                (StatusCode::UNAUTHORIZED, "Invalid token".to_string())
            }
            AppError::Io(e) => {
                tracing::error!(
                    "IO error: {}\nBacktrace: {:?}",
                    e,
                    std::backtrace::Backtrace::capture()
                );
                (StatusCode::INTERNAL_SERVER_ERROR, "IO error".to_string())
            }
            AppError::Json(e) => {
                tracing::error!("JSON error: {}", e);
                (StatusCode::BAD_REQUEST, "JSON parsing error".to_string())
            }
            AppError::Request(e) => {
                tracing::error!(
                    "Request error: {}\nBacktrace: {:?}",
                    e,
                    std::backtrace::Backtrace::capture()
                );
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "External request failed".to_string(),
                )
            }
        };

        let body = Json(json!({ "detail": message }));
        (status, body).into_response()
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(error: rusqlite::Error) -> Self {
        match error.sqlite_error_code() {
            Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {
                Self::DatabaseBusy
            }
            _ => Self::Database(error),
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;
