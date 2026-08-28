use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

const INTERNAL_SERVER_ERROR_MESSAGE: &str = "Internal server error";

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Authentication failed: {0}")]
    Authentication(String),

    #[error("Authorization failed: {0}")]
    Authorization(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Password change required")]
    PasswordChangeRequired,

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

    #[error("Too many authentication attempts; retry after {retry_after_seconds} seconds")]
    RateLimited { retry_after_seconds: u64 },

    #[error("Pool error: {0}")]
    Pool(#[from] r2d2::Error),

    #[error("JWT error: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
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
        if let AppError::RateLimited {
            retry_after_seconds,
        } = &self
        {
            tracing::warn!(
                retry_after_seconds,
                "Password authentication request was rate limited"
            );
            let retry_after = retry_after_seconds.to_string();
            let mut response = (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({ "detail": "Too many authentication attempts; retry later" })),
            )
                .into_response();
            if let Ok(header_value) = axum::http::HeaderValue::from_str(&retry_after) {
                response
                    .headers_mut()
                    .insert(axum::http::header::RETRY_AFTER, header_value);
            }
            return response;
        }
        let (status, message, error_code) = match &self {
            AppError::Authentication(message) => (StatusCode::UNAUTHORIZED, message.clone(), None),
            AppError::Authorization(message) => (StatusCode::FORBIDDEN, message.clone(), None),
            AppError::Forbidden(message) => (StatusCode::FORBIDDEN, message.clone(), None),
            AppError::PasswordChangeRequired => (
                StatusCode::FORBIDDEN,
                "Password change required".to_string(),
                Some("password_change_required"),
            ),
            AppError::NotFound(message) => (StatusCode::NOT_FOUND, message.clone(), None),
            AppError::Validation(message) => (StatusCode::BAD_REQUEST, message.clone(), None),
            AppError::Conflict(message) => (StatusCode::CONFLICT, message.clone(), None),
            AppError::BadRequest(message) => (StatusCode::BAD_REQUEST, message.clone(), None),
            AppError::Internal(message) => internal_server_error("Internal", message),
            AppError::Database(error) => internal_server_error("Database", error),
            AppError::DatabaseBusy => unreachable!(),
            AppError::RateLimited { .. } => unreachable!(),
            AppError::Pool(error) => internal_server_error("Connection pool", error),
            AppError::Jwt(error) => {
                tracing::error!("JWT error: {}", error);
                (StatusCode::UNAUTHORIZED, "Invalid token".to_string(), None)
            }
            AppError::Io(error) => internal_server_error("IO", error),
            AppError::Json(error) => {
                tracing::error!("JSON error: {}", error);
                (
                    StatusCode::BAD_REQUEST,
                    "JSON parsing error".to_string(),
                    None,
                )
            }
        };

        let response_body = match error_code {
            Some(error_code) => Json(json!({ "detail": message, "code": error_code })),
            None => Json(json!({ "detail": message })),
        };
        (status, response_body).into_response()
    }
}

fn internal_server_error(
    category: &str,
    error: &dyn std::fmt::Display,
) -> (StatusCode, String, Option<&'static str>) {
    tracing::error!(
        "{} error: {}\nBacktrace: {:?}",
        category,
        error,
        std::backtrace::Backtrace::capture()
    );
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        INTERNAL_SERVER_ERROR_MESSAGE.to_string(),
        None,
    )
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
