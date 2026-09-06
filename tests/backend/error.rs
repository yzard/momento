use axum::response::IntoResponse;
use axum::{
    body::to_bytes,
    http::{header::RETRY_AFTER, StatusCode},
};
use momento_api::error::AppError;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::time::Duration;

#[tokio::test]
async fn unavailable_responses_explain_the_cause_without_cpu_serialization_or_internal_details() {
    use momento_api::executor::{ExecutorError, ExecutorErrorKind};
    for (error, code) in [
        (AppError::DatabaseBusy, "database_busy"),
        (
            AppError::StreamUnavailable("stream-session admission is at capacity".into()),
            "stream_unavailable",
        ),
        (
            AppError::from(ExecutorError {
                kind: ExecutorErrorKind::Overloaded,
                operation: "open_storage_read_session",
                detail: "private-path".into(),
            }),
            "executor_overloaded",
        ),
        (
            AppError::from(ExecutorError {
                kind: ExecutorErrorKind::ShuttingDown,
                operation: "read",
                detail: "private-path".into(),
            }),
            "server_shutting_down",
        ),
    ] {
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()[RETRY_AFTER], "1");
        assert_eq!(response.headers()["cache-control"], "no-store");
        let body = to_bytes(response.into_body(), 2048).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["code"], code);
        assert!(value["detail"].as_str().unwrap().contains("retry"));
        assert!(!String::from_utf8_lossy(&body).contains("private-path"));
    }
}

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

#[tokio::test]
async fn sqlite_execution_timeout_is_not_reported_as_capacity_contention() {
    let error = AppError::from(momento_api::executor::ExecutorError {
        kind: momento_api::executor::ExecutorErrorKind::DatabaseTimeout,
        operation: "load_binary_media",
        detail: "private SQL detail".into(),
    });
    assert!(matches!(error, AppError::DatabaseTimeout(_)));
    let response = error.into_response();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!response.headers().contains_key(RETRY_AFTER));
    let pool = crate::test_utils::create_test_db();
    let executors = crate::test_utils::test_executor_handles(pool);
    let response =
        momento_api::error::render_pending_error_response(&executors.cpu, response).await;
    let body = to_bytes(response.into_body(), 2048).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["code"], "database_timeout");
    assert_eq!(value["detail"], "Database operation timed out");
}

#[test]
fn authentication_rate_limits_return_retry_after_without_internal_details() {
    let response = AppError::RateLimited {
        retry_after_seconds: 42,
    }
    .into_response();

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers().get(RETRY_AFTER).unwrap(), "42");
}

#[tokio::test]
async fn thumbnail_not_ready_has_a_stable_response_without_an_executor() {
    let response = AppError::ThumbnailNotReady.into_response();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(response.headers()["content-type"], "application/json");
    let body = to_bytes(response.into_body(), 1024)
        .await
        .expect("response body");
    let body: serde_json::Value = serde_json::from_slice(&body).expect("JSON body");
    assert_eq!(body["code"], "thumbnail_not_ready");
    assert_eq!(body["detail"], "Thumbnail is not ready");
}

#[tokio::test]
async fn password_change_errors_have_a_stable_machine_readable_code() {
    let response = render_error(AppError::PasswordChangeRequired).await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let body: serde_json::Value = serde_json::from_slice(&body).expect("JSON body");
    assert_eq!(body["detail"], "Password change required");
    assert_eq!(body["code"], "password_change_required");
}

async fn assert_generic_internal_error(error: AppError) {
    let response = render_error(error).await;

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let response_body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let response_body: serde_json::Value =
        serde_json::from_slice(&response_body).expect("JSON body");
    assert_eq!(response_body["detail"], "Internal server error");
}

async fn render_error(error: AppError) -> axum::response::Response {
    let pool = crate::test_utils::create_test_db();
    let executors = crate::test_utils::test_executor_handles(pool);
    momento_api::error::render_pending_error_response(&executors.cpu, error.into_response()).await
}

#[tokio::test]
async fn all_internal_error_categories_return_the_same_generic_response() {
    let sqlite_error = rusqlite::Error::InvalidParameterName("secret-column".to_string());
    let io_error = std::io::Error::other("secret path");
    let manager = SqliteConnectionManager::memory();
    let pool = Pool::builder().max_size(1).build(manager).expect("pool");
    let held_connection = pool.get().expect("held connection");
    let pool_error = match pool.get_timeout(Duration::ZERO) {
        Ok(_) => panic!("second connection must time out"),
        Err(error) => error,
    };
    drop(held_connection);

    for error in [
        AppError::Internal("secret internal detail".to_string()),
        AppError::Database(sqlite_error),
        AppError::Pool(pool_error),
        AppError::Io(io_error),
    ] {
        assert_generic_internal_error(error).await;
    }
}
