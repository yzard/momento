use axum::{
    body::Body,
    extract::DefaultBodyLimit,
    http::{header, header::HeaderName, HeaderMap, HeaderValue, Method, Request, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use percent_encoding::percent_decode_str;

use crate::auth::{password_change_guard, AdminPasswordReset, AppState, AuthenticationProtection};
use crate::config::ConfigManager;
use crate::logging::{request_logger, RequestLoggerState};
use crate::routes::{
    api_router,
    file_stream::{serve_file, ContentDisposition, FileResponseOptions},
};
use crate::runtime::HttpRequestAdmission;
use crate::runtime::{schedule_client_request, SchedulerHandle};
use crate::webdav::webdav_router;
use crate::VERSION;
use crate::{
    error::{render_pending_error_response, AppError, AppResult},
    executor::{FileIoExecutorHandle, HealthcheckResponse},
    io::file::{NormalizedStoragePath, StorageRootId},
};

async fn healthcheck(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> AppResult<Response> {
    crate::routes::render_json(
        &state,
        HealthcheckResponse {
            status: "healthy".to_string(),
            version: VERSION.to_string(),
        },
    )
    .await
}

async fn api_not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}

async fn static_fallback(
    request: Request<Body>,
    file_io: FileIoExecutorHandle,
    webdav_mount_path: String,
) -> Response {
    let admission = match request.extensions().get::<HttpRequestAdmission>() {
        Some(admission) => admission.clone(),
        None => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let method = request.method().clone();
    if !matches!(method, Method::GET | Method::HEAD) {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let raw_path = request.uri().path().trim_start_matches('/');
    if raw_path == "api"
        || raw_path.starts_with("api/")
        || raw_path == webdav_mount_path
        || raw_path
            .strip_prefix(&webdav_mount_path)
            .is_some_and(|suffix| suffix.starts_with('/'))
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    let decoded_path = match percent_decode_str(raw_path).decode_utf8() {
        Ok(path) => path,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    let requested_path = if decoded_path.is_empty() {
        "index.html"
    } else {
        decoded_path.as_ref()
    };
    match serve_static_path(
        &file_io,
        requested_path,
        request.headers(),
        &admission,
        method == Method::HEAD,
    )
    .await
    {
        Ok(response) => response,
        Err(AppError::NotFound(_))
            if accepts_html(request.headers()) && !names_static_asset(requested_path) =>
        {
            serve_static_path(
                &file_io,
                "index.html",
                request.headers(),
                &admission,
                method == Method::HEAD,
            )
            .await
            .unwrap_or_else(IntoResponse::into_response)
        }
        Err(error) => error.into_response(),
    }
}

async fn serve_static_path(
    file_io: &FileIoExecutorHandle,
    path: &str,
    headers: &HeaderMap,
    admission: &HttpRequestAdmission,
    head_only: bool,
) -> AppResult<Response> {
    let path = NormalizedStoragePath::parse(path)
        .map_err(|_| AppError::NotFound("Static asset not found".to_string()))?;
    let content_type = mime_guess::from_path(path.relative_path())
        .first_or_octet_stream()
        .to_string();
    serve_file(
        file_io,
        StorageRootId::Static,
        path,
        FileResponseOptions {
            admission,
            content_type: &content_type,
            headers,
            filename: None,
            allow_ranges: true,
            content_disposition: ContentDisposition::Inline,
            cache_control: "public, max-age=0, must-revalidate",
            head_only,
        },
    )
    .await
}

fn accepts_html(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|part| matches!(part.trim().split(';').next(), Some("text/html" | "*/*")))
        })
}

fn names_static_asset(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(|name| name.contains('.'))
}

const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob: https://tile.openstreetmap.org; font-src 'self'; media-src 'self' blob:; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'";
const MAX_REQUEST_URI_BYTES: usize = 8 * 1024;

async fn http_protocol_guard(request: Request<Body>, next: middleware::Next) -> Response {
    if request.uri().to_string().len() > MAX_REQUEST_URI_BYTES {
        return close_connection_response(StatusCode::URI_TOO_LONG, "Request URI is too long");
    }
    if request
        .headers()
        .get_all(header::CONTENT_ENCODING)
        .iter()
        .any(|value| match value.to_str() {
            Ok(value) => value
                .split(',')
                .any(|encoding| !encoding.trim().eq_ignore_ascii_case("identity")),
            Err(_) => true,
        })
    {
        return close_connection_response(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Request content encoding is not supported",
        );
    }
    next.run(request).await
}

fn close_connection_response(status: StatusCode, message: &'static str) -> Response {
    let mut response = (status, message).into_response();
    response
        .headers_mut()
        .insert(header::CONNECTION, HeaderValue::from_static("close"));
    response
}

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
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(), geolocation=(), microphone=()"),
    );
    response
}

async fn render_error_responses(
    axum::extract::State(state): axum::extract::State<AppState>,
    request: Request<Body>,
    next: middleware::Next,
) -> Response {
    render_pending_error_response(&state.executors.cpu, next.run(request).await).await
}

pub struct AppDependencies {
    pub executors: crate::runtime::ExecutorHandles,
    pub authentication_dummy_hash: String,
    pub llm_transport: crate::processor::ai::transport::TransportHandle,
    pub webdav_request_gate: crate::webdav::WebDAVRequestGate,
    pub admin_password_reset_user_id: Option<i64>,
}

pub fn create_app(config_manager: ConfigManager, dependencies: AppDependencies) -> Router {
    let config = config_manager.current();
    let scheduler: SchedulerHandle = dependencies.executors.scheduler.clone();
    let authentication_protection = AuthenticationProtection::new(
        &config.security,
        dependencies.executors.cpu.clone(),
        dependencies.executors.sqlite.clone(),
        dependencies.authentication_dummy_hash,
    );
    let state = AppState {
        config: config_manager,
        executors: dependencies.executors,
        scheduler: scheduler.clone(),
        llm_transport: dependencies.llm_transport,
        webdav_request_gate: dependencies.webdav_request_gate,
        admin_password_reset: AdminPasswordReset::new(dependencies.admin_password_reset_user_id),
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

    let file_io = state.executors.file_io.clone();
    let webdav_mount_path = config.webdav.mount_path.trim_start_matches('/').to_string();
    Router::new()
        .nest("/api/v1", api_routes)
        .merge(webdav_router(state.clone()))
        .fallback(move |request: Request<Body>| {
            static_fallback(request, file_io.clone(), webdav_mount_path.clone())
        })
        .layer(middleware::from_fn_with_state(
            RequestLoggerState {
                cpu: state.executors.cpu.clone(),
            },
            request_logger,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            render_error_responses,
        ))
        .layer(middleware::from_fn(browser_security_headers))
        .layer(middleware::from_fn(http_protocol_guard))
        .layer(middleware::from_fn_with_state(
            scheduler,
            schedule_client_request,
        ))
        .with_state(state)
}
