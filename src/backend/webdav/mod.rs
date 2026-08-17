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
    contains_reserved_path, create_dav_handler, guard_response_body, handle_webdav_request,
    validate_upload_size,
};

pub type WebDAVRequestGate = Arc<Semaphore>;

async fn webdav_handler(State(state): State<AppState>, request: Request<Body>) -> Response {
    let user = request.extensions().get::<WebDAVUser>().cloned();
    let Some(user) = user else {
        return (StatusCode::UNAUTHORIZED, "Not authenticated").into_response();
    };
    if contains_reserved_path(request.uri().path()) {
        return StatusCode::FORBIDDEN.into_response();
    }

    if let Err(status) = validate_upload_size(
        request.method(),
        request.headers(),
        state.config.webdav.max_upload_bytes,
    ) {
        return status.into_response();
    }

    let Ok(request_permit) = Arc::clone(&state.webdav_request_gate).acquire_owned().await else {
        return (StatusCode::SERVICE_UNAVAILABLE, "WebDAV is unavailable").into_response();
    };

    let user_root = paths().webdav.join(&user.username);
    let dav_handler = create_dav_handler(&user_root, &state.config.webdav.mount_path);
    let response = handle_webdav_request(dav_handler, request).await;

    guard_response_body(response, request_permit)
}

pub fn webdav_router(app_state: AppState) -> Router<AppState> {
    let mount_path = app_state.config.webdav.mount_path.clone();
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
