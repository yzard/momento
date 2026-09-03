use axum::{
    body::Body,
    http::header,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use thiserror::Error;

use crate::executor::{CpuExecutorHandle, ErrorResponse};

const INTERNAL_SERVER_ERROR_MESSAGE: &str = "Internal server error";
const FALLBACK_INTERNAL_ERROR_JSON: &str = r#"{"detail":"Internal server error"}"#;
const FALLBACK_SERVICE_UNAVAILABLE_JSON: &str =
    r#"{"detail":"Service unavailable; retry shortly"}"#;
const FALLBACK_ERROR_JSON: &str = r#"{"detail":"Request could not be completed"}"#;

#[derive(Clone, Debug)]
struct PendingErrorResponse(ErrorResponse);

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

    #[error("Payload too large: {0}")]
    PayloadTooLarge(String),

    #[error("Unprocessable entity: {0}")]
    UnprocessableEntity(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Database error: {0}")]
    Database(rusqlite::Error),

    #[error("Database is busy")]
    DatabaseBusy,

    #[error("Service unavailable: {0}")]
    Unavailable(String),

    #[error("Too many authentication attempts; retry after {retry_after_seconds} seconds")]
    RateLimited { retry_after_seconds: u64 },

    #[error("Resource limit reached: {0}")]
    ResourceLimit(String),

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
            let mut response = pending_error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "Database is busy; retry shortly".to_string(),
                None,
            );
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
            let mut response = pending_error_response(
                StatusCode::TOO_MANY_REQUESTS,
                "Too many authentication attempts; retry later".to_string(),
                None,
            );
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
            AppError::PayloadTooLarge(message) => {
                (StatusCode::PAYLOAD_TOO_LARGE, message.clone(), None)
            }
            AppError::UnprocessableEntity(message) => {
                (StatusCode::UNPROCESSABLE_ENTITY, message.clone(), None)
            }
            AppError::Internal(message) => internal_server_error("Internal", message),
            AppError::Database(error) => internal_server_error("Database", error),
            AppError::DatabaseBusy => unreachable!(),
            AppError::Unavailable(message) => {
                (StatusCode::SERVICE_UNAVAILABLE, message.clone(), None)
            }
            AppError::RateLimited { .. } => unreachable!(),
            AppError::ResourceLimit(message) => {
                (StatusCode::TOO_MANY_REQUESTS, message.clone(), None)
            }
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

        let close_connection = matches!(self, AppError::Unavailable(_));
        let mut response = pending_error_response(status, message, error_code);
        if close_connection {
            response.headers_mut().insert(
                axum::http::header::CONNECTION,
                axum::http::HeaderValue::from_static("close"),
            );
        }
        response
    }
}

fn pending_error_response(
    status: StatusCode,
    detail: String,
    code: Option<&'static str>,
) -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = status;
    response
        .extensions_mut()
        .insert(PendingErrorResponse(ErrorResponse { detail, code }));
    response
}

pub async fn render_pending_error_response(
    cpu: &CpuExecutorHandle,
    mut response: Response,
) -> Response {
    let Some(pending) = response.extensions_mut().remove::<PendingErrorResponse>() else {
        return response;
    };
    let bytes = match cpu.serialize_control_response(pending.0.into()).await {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::warn!(error = %error, "Using the static HTTP error response");
            fallback_error_json(response.status()).as_bytes().to_vec()
        }
    };
    *response.body_mut() = Body::from(bytes);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );
    response
}

fn fallback_error_json(status: StatusCode) -> &'static str {
    match status {
        StatusCode::INTERNAL_SERVER_ERROR => FALLBACK_INTERNAL_ERROR_JSON,
        StatusCode::SERVICE_UNAVAILABLE => FALLBACK_SERVICE_UNAVAILABLE_JSON,
        _ => FALLBACK_ERROR_JSON,
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

impl From<crate::executor::ExecutorError> for AppError {
    fn from(error: crate::executor::ExecutorError) -> Self {
        use crate::executor::ExecutorErrorKind;

        match error.kind {
            ExecutorErrorKind::Overloaded => {
                Self::Unavailable("The server is at capacity; retry shortly".to_string())
            }
            ExecutorErrorKind::ShuttingDown => {
                Self::Unavailable("The server is shutting down".to_string())
            }
            ExecutorErrorKind::InvalidInput => Self::Validation(error.detail),
            ExecutorErrorKind::BadRequest => Self::BadRequest(error.detail),
            ExecutorErrorKind::Conflict => Self::Conflict(error.detail),
            ExecutorErrorKind::NotFound => Self::NotFound(error.detail),
            ExecutorErrorKind::DatabaseBusy | ExecutorErrorKind::DatabaseTimeout => {
                Self::DatabaseBusy
            }
            ExecutorErrorKind::WorkerPanic
            | ExecutorErrorKind::DatabasePermanent
            | ExecutorErrorKind::Database
            | ExecutorErrorKind::FileNotFound
            | ExecutorErrorKind::FilePermission
            | ExecutorErrorKind::FileConflict
            | ExecutorErrorKind::FileInvalidData
            | ExecutorErrorKind::FileTransient
            | ExecutorErrorKind::FileSystem
            | ExecutorErrorKind::Internal => Self::Internal(error.to_string()),
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::{
        fallback_error_json, FALLBACK_ERROR_JSON, FALLBACK_INTERNAL_ERROR_JSON,
        FALLBACK_SERVICE_UNAVAILABLE_JSON,
    };

    #[test]
    fn static_error_fallback_matches_the_original_status_class() {
        assert_eq!(
            fallback_error_json(StatusCode::INTERNAL_SERVER_ERROR),
            FALLBACK_INTERNAL_ERROR_JSON
        );
        assert_eq!(
            fallback_error_json(StatusCode::SERVICE_UNAVAILABLE),
            FALLBACK_SERVICE_UNAVAILABLE_JSON
        );
        assert_eq!(
            fallback_error_json(StatusCode::UNAUTHORIZED),
            FALLBACK_ERROR_JSON
        );
    }
}
