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
use tokio::sync::RwLock;

use crate::auth::AppState;
use crate::database::operations::{InvalidateWebdavReadiness, MarkWebdavReady};
use crate::runtime::HttpRequestAdmission;

pub use auth::WebDAVUser;
use auth::{basic_auth_middleware, path_guard_middleware};
use handler::{
    completed_upload_path, contains_reserved_destination, contains_reserved_path,
    guard_response_body, handle_webdav_request, invalidated_upload_paths, request_mutates_staging,
    validate_upload_size,
};

pub type WebDAVRequestGate = Arc<RwLock<()>>;

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

    let admission = match request.extensions().get::<HttpRequestAdmission>() {
        Some(admission) => admission.clone(),
        None => return stream_admission_unavailable(),
    };
    if (request_mutates_staging(request.method()) || request.method().as_str() == "PROPFIND")
        && admission.convert_to_stream().is_err()
    {
        return stream_admission_unavailable();
    }

    let request_permit = if request_mutates_staging(request.method()) {
        Some(Arc::clone(&state.webdav_request_gate).read_owned().await)
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
    if !invalidated_paths.is_empty()
        && state
            .executors
            .sqlite
            .invalidate_webdav_readiness_request(InvalidateWebdavReadiness {
                user_id: user.id,
                paths: invalidated_paths,
            })
            .await
            .is_err()
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "WebDAV readiness is unavailable",
        )
            .into_response();
    }

    let mut response = handle_webdav_request(
        &state.executors,
        &user.username,
        &admission,
        request,
        &config.webdav.mount_path,
        config.webdav.max_upload_bytes,
    )
    .await;
    if response.status().is_success() {
        if let Some(completed_path) = completed_path {
            let readiness_saved = state
                .executors
                .sqlite
                .mark_webdav_ready_request(MarkWebdavReady {
                    user_id: user.id,
                    path: completed_path,
                })
                .await
                .is_ok();
            if !readiness_saved {
                tracing::error!(path = %request_path, "Could not persist completed WebDAV upload readiness");
                *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
            } else {
                state.scheduler.wake_webdav_import();
            }
        }
    }

    match request_permit {
        Some(request_permit) => guard_response_body(response, request_permit),
        None => response,
    }
}

fn stream_admission_unavailable() -> Response {
    let mut response = (
        StatusCode::SERVICE_UNAVAILABLE,
        "WebDAV stream capacity is unavailable",
    )
        .into_response();
    response.headers_mut().insert(
        axum::http::header::CONNECTION,
        axum::http::HeaderValue::from_static("close"),
    );
    response
}

pub fn webdav_router(app_state: AppState) -> Router<AppState> {
    let mount_path = app_state.config.current().webdav.mount_path.clone();
    let mount_path_with_slash = format!("{mount_path}/");
    let mount_path_with_wildcard = format!("{mount_path}/*path");
    tracing::info!(
        "WebDAV server listening at {}, using the typed WebDAV storage root",
        mount_path
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
