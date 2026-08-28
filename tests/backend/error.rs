use axum::response::IntoResponse;
use axum::{
    body::to_bytes,
    http::{header::RETRY_AFTER, StatusCode},
};
use momento_api::error::AppError;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::time::Duration;

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
async fn password_change_errors_have_a_stable_machine_readable_code() {
    let response = AppError::PasswordChangeRequired.into_response();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let body: serde_json::Value = serde_json::from_slice(&body).expect("JSON body");
    assert_eq!(body["detail"], "Password change required");
    assert_eq!(body["code"], "password_change_required");
}

async fn assert_generic_internal_error(error: AppError) {
    let response = error.into_response();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let response_body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let response_body: serde_json::Value =
        serde_json::from_slice(&response_body).expect("JSON body");
    assert_eq!(response_body["detail"], "Internal server error");
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
