use axum::{
    body::Body,
    extract::DefaultBodyLimit,
    http::{header::HeaderName, HeaderValue, Request, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Serialize;
use tower::ServiceExt;
use tower_http::services::{ServeDir, ServeFile};

use crate::auth::{password_change_guard, AdminPasswordReset, AppState, AuthenticationProtection};
use crate::config::ConfigManager;
use crate::database::DbPool;
use crate::logging::request_logger;
use crate::routes::api_router;
use crate::webdav::webdav_router;
use crate::VERSION;

#[derive(Serialize)]
struct HealthcheckResponse {
    status: String,
    version: String,
}

async fn healthcheck() -> Json<HealthcheckResponse> {
    Json(HealthcheckResponse {
        status: "healthy".to_string(),
        version: VERSION.to_string(),
    })
}

async fn api_not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}

const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self'; media-src 'self' blob:; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'";

async fn browser_security_headers(request: Request<Body>, next: middleware::Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static(CONTENT_SECURITY_POLICY),
    );
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        HeaderName::from_static("x-frame-options"),
        HeaderValue::from_static("DENY"),
    );
    headers.insert(
        HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(), geolocation=(), microphone=()"),
    );
    response
}

pub fn create_app(
    config_manager: ConfigManager,
    pool: DbPool,
    llm_transport: crate::processor::ai::transport::TransportHandle,
    webdav_request_gate: crate::webdav::WebDAVRequestGate,
    admin_password_reset_user_id: Option<i64>,
) -> Router {
    let config = config_manager.current();
    let authentication_protection = AuthenticationProtection::new(&config.security);
    let state = AppState {
        config: config_manager,
        pool,
        llm_transport,
        webdav_request_gate,
        admin_password_reset: AdminPasswordReset::new(admin_password_reset_user_id),
        authentication_protection,
    };

    let api_routes = Router::new()
        .route("/healthcheck", get(healthcheck))
        .merge(api_router())
        .layer(DefaultBodyLimit::max(
            config.server.api_request_body_max_bytes,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            password_change_guard,
        ))
        .fallback(api_not_found);

    let mut app = Router::new()
        .nest("/api/v1", api_routes)
        .merge(webdav_router(state.clone()))
        .layer(middleware::from_fn_with_state(
            config.server.request_log_body_max_bytes,
            request_logger,
        ))
        .layer(middleware::from_fn(browser_security_headers))
        .with_state(state);

    // Serve static files if frontend exists
    let static_dir = config.server.static_dir.clone();

    if static_dir.exists() {
        let static_service = ServeDir::new(&static_dir)
            .not_found_service(ServeFile::new(static_dir.join("index.html")));
        let webdav_mount_path = config.webdav.mount_path.trim_start_matches('/').to_string();
        app = app.fallback(move |req: Request<Body>| {
            let static_service = static_service.clone();
            let webdav_mount_path = webdav_mount_path.clone();
            async move {
                let path = req.uri().path().trim_start_matches('/');

                if path == webdav_mount_path
                    || path
                        .strip_prefix(&webdav_mount_path)
                        .is_some_and(|suffix| suffix.starts_with('/'))
                {
                    return (StatusCode::NOT_FOUND, "Not Found").into_response();
                }

                match static_service.oneshot(req).await {
                    Ok(response) => response.into_response(),
                    Err(error) => match error {},
                }
            }
        });
    }

    app
}
