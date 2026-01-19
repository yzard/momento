use axum::{body::Body, extract::Request, middleware::Next, response::Response};
use std::time::Instant;
use tracing::{error, info, warn};
use tracing_subscriber::{fmt, EnvFilter};

/// Initialize the logging system with structured output
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

/// Middleware to log all HTTP requests with timestamp, method, URI, status code, and duration
pub async fn request_logger(request: Request<Body>, next: Next) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let path = uri.path().to_string();

    // Skip logging for static assets to reduce noise
    let is_static = path.starts_with("/assets/") || path.ends_with(".js") || path.ends_with(".css");

    let start = Instant::now();
    let response = next.run(request).await;
    let duration = start.elapsed();
    let status = response.status();

    if !is_static {
        let duration_ms = duration.as_secs_f64() * 1000.0;

        match status.as_u16() {
            200..=299 => {
                info!(
                    "{} {} {} {:.2}ms",
                    method,
                    path,
                    status.as_u16(),
                    duration_ms
                );
            }
            400..=499 => {
                warn!(
                    "{} {} {} {:.2}ms",
                    method,
                    path,
                    status.as_u16(),
                    duration_ms
                );
            }
            500..=599 => {
                error!(
                    "{} {} {} {:.2}ms",
                    method,
                    path,
                    status.as_u16(),
                    duration_ms
                );
            }
            _ => {
                info!(
                    "{} {} {} {:.2}ms",
                    method,
                    path,
                    status.as_u16(),
                    duration_ms
                );
            }
        }
    }

    response
}

/// Log an error with context
pub fn log_error(context: &str, error: &dyn std::error::Error) {
    error!("{}: {}", context, error);
}

/// Log an uncaught panic
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

/// Install a panic hook to log uncaught panics
pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log_panic(info);
        default_hook(info);
    }));
}
