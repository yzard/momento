mod auth;
mod handler;

use axum::{
    body::Body,
    extract::Request,
    middleware,
    response::IntoResponse,
    response::Response,
    routing::any,
    Router,
};
use axum::http::{uri::PathAndQuery, StatusCode, Uri};

use crate::auth::AppState;
use crate::constants::WEBDAV_DIR;

pub use auth::WebDAVUser;
use auth::{basic_auth_middleware, path_guard_middleware};
use handler::{create_dav_handler, handle_webdav_request};

async fn webdav_handler(request: Request<Body>) -> Response {
    let (mut parts, body) = request.into_parts();
    let user = parts.extensions.get::<WebDAVUser>().cloned();
    let Some(user) = user else {
        return (StatusCode::UNAUTHORIZED, "Not authenticated").into_response();
    };

    let path = parts.uri.path();
    let stripped = path.strip_prefix("/webdav").unwrap_or(path);
    let stripped = if stripped.is_empty() { "/" } else { stripped };
    let normalized_path = if stripped.starts_with('/') {
        stripped.to_string()
    } else {
        format!("/{}", stripped)
    };
    let new_path = match parts.uri.query() {
        Some(query) => format!("{}?{}", normalized_path, query),
        None => normalized_path,
    };

    let path_and_query = match PathAndQuery::from_maybe_shared(new_path) {
        Ok(value) => value,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "Invalid WebDAV path").into_response();
        }
    };

    let mut uri_parts = parts.uri.into_parts();
    uri_parts.path_and_query = Some(path_and_query);
    let uri = match Uri::from_parts(uri_parts) {
        Ok(value) => value,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "Invalid WebDAV path").into_response();
        }
    };

    parts.uri = uri;
    let request = Request::from_parts(parts, body);

    let user_root = WEBDAV_DIR.join(&user.username);
    let dav_handler = create_dav_handler(&user_root);

    handle_webdav_request(dav_handler, request).await
}

pub fn webdav_router(app_state: AppState) -> Router<AppState> {
    if !app_state.config.webdav.enabled {
        tracing::info!("WebDAV server disabled");
        return Router::new();
    }

    tracing::info!(
        "WebDAV server enabled at /webdav, root: {}/<username>",
        WEBDAV_DIR.display()
    );

    Router::new()
        .route("/webdav", any(webdav_handler))
        .route("/webdav/", any(webdav_handler))
        .route("/webdav/*path", any(webdav_handler))
        .layer(middleware::from_fn(path_guard_middleware))
        .layer(middleware::from_fn_with_state(
            app_state,
            basic_auth_middleware,
        ))
}
