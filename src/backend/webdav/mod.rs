mod auth;
pub mod handler;

use axum::{
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    middleware,
    response::{IntoResponse, Response},
    routing::any,
    Router,
};
use std::sync::Arc;
use tokio::sync::Semaphore;

use crate::auth::AppState;
use crate::constants::paths;

pub use auth::WebDAVUser;
use auth::{basic_auth_middleware, path_guard_middleware};
use handler::{
    completed_upload_path, contains_reserved_destination, contains_reserved_path,
    create_dav_handler, guard_response_body, handle_webdav_request, invalidated_upload_paths,
    request_mutates_staging, validate_upload_size,
};

pub type WebDAVRequestGate = Arc<Semaphore>;

async fn webdav_handler(State(state): State<AppState>, request: Request<Body>) -> Response {
    let config = state.config.current();
    let user = request.extensions().get::<WebDAVUser>().cloned();
    let Some(user) = user else {
        return (StatusCode::UNAUTHORIZED, "Not authenticated").into_response();
    };
    if contains_reserved_path(request.uri().path())
        || contains_reserved_destination(request.headers())
    {
        return StatusCode::FORBIDDEN.into_response();
    }

    if let Err(status) = validate_upload_size(
        request.method(),
        request.headers(),
        config.webdav.max_upload_bytes,
    ) {
        return status.into_response();
    }

    let request_permit = if request_mutates_staging(request.method()) {
        let Ok(request_permit) = Arc::clone(&state.webdav_request_gate).acquire_owned().await
        else {
            return (StatusCode::SERVICE_UNAVAILABLE, "WebDAV is unavailable").into_response();
        };
        Some(request_permit)
    } else {
        None
    };
    let request_path = request.uri().path().to_string();
    let invalidated_paths = invalidated_upload_paths(
        request.method(),
        request.headers(),
        &request_path,
        &config.webdav.mount_path,
    );
    let completed_path = completed_upload_path(
        request.method(),
        request.headers(),
        &request_path,
        &config.webdav.mount_path,
    );
    if !invalidated_paths.is_empty() {
        let Ok(connection) = state.pool.get() else {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "WebDAV readiness is unavailable",
            )
                .into_response();
        };
        let Ok(transaction) = connection.unchecked_transaction() else {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "WebDAV readiness is unavailable",
            )
                .into_response();
        };
        for path in invalidated_paths {
            if transaction
                .execute(
                    crate::database::queries::webdav_ready::DELETE,
                    rusqlite::params![user.id, path],
                )
                .is_err()
            {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "WebDAV readiness is unavailable",
                )
                    .into_response();
            }
        }
        if transaction.commit().is_err() {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "WebDAV readiness is unavailable",
            )
                .into_response();
        }
    }

    let user_root = paths().webdav.join(&user.username);
    let dav_handler = create_dav_handler(&user_root, &config.webdav.mount_path);
    let mut response = handle_webdav_request(
        dav_handler,
        request,
        &user_root,
        &config.webdav.mount_path,
        config.webdav.max_upload_bytes,
    )
    .await;
    if response.status().is_success() {
        if let Some(completed_path) = completed_path {
            let readiness_saved = state.pool.get().ok().is_some_and(|connection| {
                connection
                    .execute(
                        crate::database::queries::webdav_ready::UPSERT,
                        rusqlite::params![user.id, completed_path],
                    )
                    .is_ok()
            });
            if !readiness_saved {
                tracing::error!(path = %request_path, "Could not persist completed WebDAV upload readiness");
                *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
            }
        }
    }

    match request_permit {
        Some(request_permit) => guard_response_body(response, request_permit),
        None => response,
    }
}

pub fn webdav_router(app_state: AppState) -> Router<AppState> {
    let mount_path = app_state.config.current().webdav.mount_path.clone();
    let mount_path_with_slash = format!("{mount_path}/");
    let mount_path_with_wildcard = format!("{mount_path}/*path");
    tracing::info!(
        "WebDAV server listening at {}, root: {}/<username>",
        mount_path,
        paths().webdav.display()
    );

    Router::new()
        .route(&mount_path, any(webdav_handler))
        .route(&mount_path_with_slash, any(webdav_handler))
        .route(&mount_path_with_wildcard, any(webdav_handler))
        .layer(middleware::from_fn(path_guard_middleware))
        .layer(middleware::from_fn_with_state(
            app_state,
            basic_auth_middleware,
        ))
}
