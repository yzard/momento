use axum::{body::Body, extract::Request, middleware::Next, response::Response};
use std::time::Instant;
use tracing::{error, info, warn};
use tracing_subscriber::{fmt, EnvFilter};

pub fn init_logging() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("momento_api=info,tower_http=warn"));

    fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .init();
}

pub async fn request_logger(mut request: Request<Body>, next: Next) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let path = uri.path().to_string();

    let is_static = path.starts_with("/assets/") || path.ends_with(".js") || path.ends_with(".css");
    let payload = extract_compact_payload(&mut request).await;

    let start = Instant::now();
    let response = next.run(request).await;
    let duration = start.elapsed();
    let status = response.status();

    if !is_static {
        let duration_ms = duration.as_secs_f64() * 1000.0;
        let duration_text = format!("{:05.2}", duration_ms);
        let payload_text = payload.unwrap_or_else(|| "{}".to_string());
        let log_line = format!(
            "{} {} {} {}ms {}",
            method,
            path,
            status.as_u16(),
            duration_text,
            payload_text
        );

        let status_code = status.as_u16();
        let is_missing_route = status_code == 404;

        if is_missing_route {
            warn!("{}", log_line);
            return response;
        }

        match status_code {
            200..=299 => info!("{}", log_line),
            400..=499 => warn!("{}", log_line),
            500..=599 => error!("{}", log_line),
            _ => info!("{}", log_line),
        }
    }

    response
}

async fn extract_compact_payload(request: &mut Request<Body>) -> Option<String> {
    if request.method() != axum::http::Method::POST {
        return None;
    }

    let body = std::mem::replace(request.body_mut(), Body::empty());
    let bytes = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(b) => b,
        Err(_) => return None,
    };

    let body_str = match String::from_utf8(bytes.to_vec()) {
        Ok(s) => s,
        Err(_) => return None,
    };

    let compact = match serde_json::from_str::<serde_json::Value>(&body_str) {
        Ok(value) => value.to_string(),
        Err(_) => body_str.trim().to_string(),
    };

    let restored = Body::from(bytes);
    *request.body_mut() = restored;

    Some(compact)
}

pub fn log_error(context: &str, error: &dyn std::error::Error) {
    error!("{}: {}", context, error);
}

pub fn log_panic(info: &std::panic::PanicHookInfo) {
    let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = info.payload().downcast_ref::<String>() {
        s.clone()
    } else {
        "Unknown panic payload".to_string()
    };

    let location = if let Some(loc) = info.location() {
        format!("{}:{}:{}", loc.file(), loc.line(), loc.column())
    } else {
        "unknown location".to_string()
    };

    error!("PANIC at {}: {}", location, payload);
}

pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log_panic(info);
        default_hook(info);
    }));
}
