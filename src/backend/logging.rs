use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, Method},
    middleware::Next,
    response::Response,
};
use futures::StreamExt;
use tracing::{error, info, warn};

const MULTIPART_BODY_OMITTED: &str = "[multipart body omitted]";
const BINARY_BODY_OMITTED: &str = "[binary body omitted]";
const BINARY_PAYLOAD_OMITTED: &str = "[binary payload omitted]";
const BASE64_VALUE_OMITTED: &str = "[base64 omitted]";
const SENSITIVE_VALUE_REDACTED: &str = "[redacted]";

pub async fn request_logger(
    State(maximum_capture_bytes): State<usize>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let path = uri.path().to_string();

    let is_static = path.starts_with("/assets/") || path.ends_with(".js") || path.ends_with(".css");
    let payload_capture = begin_payload_capture(&mut request, maximum_capture_bytes);

    let start = Instant::now();
    let response = next.run(request).await;
    let duration = start.elapsed();
    let status = response.status();

    if !is_static {
        let duration_ms = duration.as_secs_f64() * 1000.0;
        let duration_text = format!("{:05.2}", duration_ms);
        let payload_text = payload_capture
            .map(|capture| capture.render())
            .unwrap_or_else(|| "{}".to_string());
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

#[derive(Clone)]
pub struct PayloadCapture {
    content: PayloadCaptureContent,
}

#[derive(Clone)]
enum PayloadCaptureContent {
    Fixed(&'static str),
    Streaming(Arc<Mutex<CapturedBody>>),
}

#[derive(Debug)]
struct CapturedBody {
    bytes: Vec<u8>,
    maximum_bytes: usize,
    truncated: bool,
}

impl CapturedBody {
    fn new(maximum_bytes: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(maximum_bytes.min(16 * 1024)),
            maximum_bytes,
            truncated: false,
        }
    }

    fn record(&mut self, chunk: &[u8]) {
        let remaining = self.maximum_bytes.saturating_sub(self.bytes.len());
        let captured_length = remaining.min(chunk.len());
        self.bytes.extend_from_slice(&chunk[..captured_length]);
        if captured_length < chunk.len() {
            self.truncated = true;
        }
    }

    fn render(&self) -> String {
        if self.truncated {
            return format!(
                "[request body omitted: exceeded logging limit of {} bytes]",
                self.maximum_bytes
            );
        }
        if self.bytes.is_empty() {
            return "{}".to_string();
        }

        compact_and_redact_payload(&self.bytes)
    }
}

impl PayloadCapture {
    fn fixed(value: &'static str) -> Self {
        Self {
            content: PayloadCaptureContent::Fixed(value),
        }
    }

    fn streaming(state: Arc<Mutex<CapturedBody>>) -> Self {
        Self {
            content: PayloadCaptureContent::Streaming(state),
        }
    }

    pub fn render(&self) -> String {
        match &self.content {
            PayloadCaptureContent::Fixed(value) => (*value).to_string(),
            PayloadCaptureContent::Streaming(state) => {
                let state = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                state.render()
            }
        }
    }
}

pub fn begin_payload_capture(
    request: &mut Request<Body>,
    maximum_capture_bytes: usize,
) -> Option<PayloadCapture> {
    if request.method() != Method::POST {
        return None;
    }

    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    if content_type.is_some_and(is_multipart_content_type) {
        return Some(PayloadCapture::fixed(MULTIPART_BODY_OMITTED));
    }
    if content_type.is_some_and(is_binary_content_type) {
        return Some(PayloadCapture::fixed(BINARY_BODY_OMITTED));
    }

    let state = Arc::new(Mutex::new(CapturedBody::new(maximum_capture_bytes)));
    let capture_state = Arc::clone(&state);
    let body = std::mem::replace(request.body_mut(), Body::empty());
    let stream = body.into_data_stream().map(move |result| {
        if let Ok(bytes) = &result {
            let mut state = capture_state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.record(bytes);
        }
        result
    });
    *request.body_mut() = Body::from_stream(stream);

    Some(PayloadCapture::streaming(state))
}

fn is_multipart_content_type(content_type: &str) -> bool {
    content_type
        .split(';')
        .next()
        .is_some_and(|mime_type| mime_type.trim().eq_ignore_ascii_case("multipart/form-data"))
}

fn is_binary_content_type(content_type: &str) -> bool {
    let mime_type = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    mime_type == "application/octet-stream"
        || mime_type.starts_with("image/")
        || mime_type.starts_with("video/")
        || mime_type.starts_with("audio/")
}

fn compact_and_redact_payload(bytes: &[u8]) -> String {
    let Ok(body) = std::str::from_utf8(bytes) else {
        return BINARY_PAYLOAD_OMITTED.to_string();
    };

    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(mut value) => {
            redact_request_values(&mut value);
            value.to_string()
        }
        Err(_) if body.to_ascii_lowercase().contains("base64") => {
            BINARY_PAYLOAD_OMITTED.to_string()
        }
        Err(_) => body.trim().to_string(),
    }
}

pub fn redact_request_values(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(fields) => {
            for (key, value) in fields {
                if is_sensitive_field(key) {
                    *value = serde_json::Value::String(SENSITIVE_VALUE_REDACTED.to_string());
                    continue;
                }
                if is_binary_field(key) && value.is_string() {
                    *value = serde_json::Value::String(BASE64_VALUE_OMITTED.to_string());
                    continue;
                }
                redact_request_values(value);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                redact_request_values(value);
            }
        }
        serde_json::Value::String(text) if text.contains(";base64,") => {
            *text = BASE64_VALUE_OMITTED.to_string();
        }
        _ => {}
    }
}

fn normalized_field_name(key: &str) -> String {
    key.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_sensitive_field(key: &str) -> bool {
    matches!(
        normalized_field_name(key).as_str(),
        "password"
            | "currentpassword"
            | "newpassword"
            | "accesstoken"
            | "refreshtoken"
            | "apikey"
            | "secret"
            | "authorization"
            | "token"
    )
}

fn is_binary_field(key: &str) -> bool {
    let normalized = normalized_field_name(key);
    matches!(
        normalized.as_str(),
        "image" | "imagebase64" | "data" | "bytes" | "embedding"
    ) || normalized.ends_with("base64")
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
