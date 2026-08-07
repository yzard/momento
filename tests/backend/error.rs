use axum::http::{header::RETRY_AFTER, StatusCode};
use axum::response::IntoResponse;
use momento_api::error::AppError;

#[test]
fn sqlite_busy_errors_return_retryable_service_unavailable() {
    let sqlite_error = rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
        Some("database is locked".to_string()),
    );

    let response = AppError::from(sqlite_error).into_response();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers().get(RETRY_AFTER).unwrap(), "1");
}
