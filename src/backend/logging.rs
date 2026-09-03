use std::io::{self, Write};
use std::time::Instant;

use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, Method},
    middleware::Next,
    response::Response,
};
use futures::StreamExt;
use tokio::sync::{mpsc, oneshot};
use tracing::Metadata;
use tracing::{error, info, warn};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::writer::MakeWriterExt;
use tracing_subscriber::prelude::*;

use crate::io::log::{DroppedLogEvents, LogEventProducer, LogSeverity, MAX_LOG_EVENT_BYTES};

const MULTIPART_BODY_OMITTED: &str = "[multipart body omitted]";
const BINARY_BODY_OMITTED: &str = "[binary body omitted]";
const BINARY_PAYLOAD_OMITTED: &str = "[binary payload omitted]";
const BASE64_VALUE_OMITTED: &str = "[base64 omitted]";
const SENSITIVE_VALUE_REDACTED: &str = "[redacted]";
pub const MAX_REQUEST_LOG_CAPTURE_BYTES: usize = 48 * 1024;

pub struct LoggingGuard {
    producer: LogEventProducer,
}

impl LoggingGuard {
    pub fn dropped_events(&self) -> DroppedLogEvents {
        self.producer.dropped_events()
    }
}

pub fn init_logging(producer: LogEventProducer) -> io::Result<LoggingGuard> {
    let console_writer = std::io::stderr
        .with_max_level(tracing::Level::WARN)
        .or_else(std::io::stdout);
    let console_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .event_format(momento_common::logging::EventFormatter::new(true))
        .with_writer(console_writer);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .event_format(momento_common::logging::EventFormatter::new(false))
        .with_writer(LogMakeWriter::new(producer.clone()));

    tracing_subscriber::registry()
        .with(LevelFilter::INFO)
        .with(console_layer)
        .with(file_layer)
        .try_init()
        .map_err(|error| io::Error::other(error.to_string()))?;
    Ok(LoggingGuard { producer })
}

#[derive(Clone)]
struct LogMakeWriter {
    producer: LogEventProducer,
}

impl LogMakeWriter {
    fn new(producer: LogEventProducer) -> Self {
        Self { producer }
    }
}

impl<'writer> tracing_subscriber::fmt::MakeWriter<'writer> for LogMakeWriter {
    type Writer = LogEventWriter;

    fn make_writer(&'writer self) -> Self::Writer {
        LogEventWriter::new(self.producer.clone(), LogSeverity::Info)
    }

    fn make_writer_for(&'writer self, metadata: &Metadata<'_>) -> Self::Writer {
        let severity = match *metadata.level() {
            tracing::Level::ERROR => LogSeverity::Error,
            tracing::Level::WARN => LogSeverity::Warn,
            tracing::Level::INFO => LogSeverity::Info,
            tracing::Level::DEBUG | tracing::Level::TRACE => LogSeverity::Debug,
        };
        LogEventWriter::new(self.producer.clone(), severity)
    }
}

struct LogEventWriter {
    producer: LogEventProducer,
    severity: LogSeverity,
    bytes: Vec<u8>,
    overflowed: bool,
}

impl LogEventWriter {
    fn new(producer: LogEventProducer, severity: LogSeverity) -> Self {
        Self {
            producer,
            severity,
            bytes: Vec::with_capacity(MAX_LOG_EVENT_BYTES),
            overflowed: false,
        }
    }
}

impl Write for LogEventWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let remaining = MAX_LOG_EVENT_BYTES.saturating_sub(self.bytes.len());
        if bytes.len() > remaining {
            self.overflowed = true;
            return Ok(bytes.len());
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for LogEventWriter {
    fn drop(&mut self) {
        if self.overflowed {
            self.bytes.clear();
        }
        self.producer
            .try_emit(self.severity, std::mem::take(&mut self.bytes));
    }
}

#[derive(Clone)]
pub struct RequestLoggerState {
    pub cpu: crate::executor::CpuExecutorHandle,
}

pub async fn request_logger(
    State(state): State<RequestLoggerState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let path = uri.path().to_string();

    let is_static = path.starts_with("/assets/") || path.ends_with(".js") || path.ends_with(".css");
    let payload_capture = begin_payload_capture(&mut request);

    let start = Instant::now();
    let response = next.run(request).await;
    let duration = start.elapsed();
    let status = response.status();

    if !is_static {
        let duration_ms = duration.as_secs_f64() * 1000.0;
        let duration_text = format!("{:05.2}", duration_ms);
        let payload_text = match payload_capture {
            Some(capture) => capture.render(&state.cpu).await,
            None => "{}".to_string(),
        };
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

pub struct PayloadCapture {
    content: PayloadCaptureContent,
}

enum PayloadCaptureContent {
    Fixed(&'static str),
    Streaming {
        raw: oneshot::Receiver<CapturedBody>,
        typed: mpsc::Receiver<String>,
    },
}

#[derive(Clone)]
pub(crate) struct TypedControlLogPayloadSender(mpsc::Sender<String>);

impl TypedControlLogPayloadSender {
    pub(crate) fn send(&self, payload: String) {
        let _ = self.0.try_send(payload);
    }
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
}

impl PayloadCapture {
    fn fixed(value: &'static str) -> Self {
        Self {
            content: PayloadCaptureContent::Fixed(value),
        }
    }

    fn streaming(raw: oneshot::Receiver<CapturedBody>, typed: mpsc::Receiver<String>) -> Self {
        Self {
            content: PayloadCaptureContent::Streaming { raw, typed },
        }
    }

    pub async fn render(self, cpu: &crate::executor::CpuExecutorHandle) -> String {
        match self.content {
            PayloadCaptureContent::Fixed(value) => value.to_string(),
            PayloadCaptureContent::Streaming { raw, mut typed } => {
                if let Ok(payload) = typed.try_recv() {
                    return payload;
                }
                match raw.await {
                    Ok(captured) => cpu
                        .render_request_log_payload_request(
                            captured.bytes,
                            captured.maximum_bytes,
                            captured.truncated,
                        )
                        .await
                        .unwrap_or_else(|error| {
                            format!("[request body logging unavailable: {error}]")
                        }),
                    Err(_) => "[request body was not fully consumed]".to_string(),
                }
            }
        }
    }
}

pub fn begin_payload_capture(request: &mut Request<Body>) -> Option<PayloadCapture> {
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

    let (sender, receiver) = oneshot::channel();
    let (typed_sender, typed_receiver) = mpsc::channel(1);
    request
        .extensions_mut()
        .insert(TypedControlLogPayloadSender(typed_sender));
    let body = std::mem::replace(request.body_mut(), Body::empty());
    let stream = futures::stream::unfold(
        (
            body.into_data_stream(),
            CapturedBody::new(MAX_REQUEST_LOG_CAPTURE_BYTES),
            Some(sender),
        ),
        |(mut body, mut captured, mut sender)| async move {
            match body.next().await {
                Some(result) => {
                    if let Ok(bytes) = &result {
                        captured.record(bytes);
                    }
                    Some((result, (body, captured, sender)))
                }
                None => {
                    if let Some(sender) = sender.take() {
                        let _ = sender.send(captured);
                    }
                    None
                }
            }
        },
    );
    *request.body_mut() = Body::from_stream(stream);

    Some(PayloadCapture::streaming(receiver, typed_receiver))
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

pub(crate) fn render_captured_request_payload(
    bytes: &[u8],
    maximum_bytes: usize,
    truncated: bool,
) -> String {
    if truncated {
        return format!("[request body omitted: exceeded logging limit of {maximum_bytes} bytes]");
    }
    if bytes.is_empty() {
        return "{}".to_string();
    }
    match serde_json::from_slice::<serde_json::Value>(bytes) {
        Ok(mut value) => {
            redact_request_values(&mut value);
            value.to_string()
        }
        Err(_) => render_invalid_control_payload(bytes),
    }
}

pub(crate) fn render_invalid_control_payload(bytes: &[u8]) -> String {
    let Ok(body) = std::str::from_utf8(bytes) else {
        return BINARY_PAYLOAD_OMITTED.to_string();
    };
    if body.to_ascii_lowercase().contains("base64") {
        return BINARY_PAYLOAD_OMITTED.to_string();
    }
    body.trim().to_string()
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
