use axum::body::{to_bytes, Body};
use momento_api::logging::{
    begin_payload_capture, redact_request_values, MAX_REQUEST_LOG_CAPTURE_BYTES,
};

#[tokio::test]
async fn request_failure_logs_include_the_response_cause_and_request_path() {
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;
    use tracing::instrument::WithSubscriber;
    #[derive(Clone)]
    struct LogBuffer(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for LogBuffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let buffer = LogBuffer(Arc::new(Mutex::new(Vec::new())));
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let app = axum::Router::new()
        .route(
            "/thumbnail",
            axum::routing::get(|| async {
                Err::<(), _>(momento_api::error::AppError::StreamUnavailable(
                    "stream-session admission is at capacity".into(),
                ))
            }),
        )
        .layer(axum::middleware::from_fn_with_state(
            momento_api::logging::RequestLoggerState {
                cpu: test_cpu_executor(),
            },
            momento_api::logging::request_logger,
        ));
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/thumbnail")
                .body(Body::empty())
                .unwrap(),
        )
        .with_subscriber(subscriber)
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    );
    let bytes = buffer.0.lock().unwrap().clone();
    let log = String::from_utf8(bytes).unwrap();
    assert!(log.contains("GET /thumbnail 503"), "{log}");
    assert!(log.contains("stream_unavailable"), "{log}");
    assert!(
        log.contains("stream-session admission is at capacity"),
        "{log}"
    );
}

fn test_cpu_executor() -> momento_api::executor::CpuExecutorHandle {
    crate::test_utils::test_executor_handles(crate::test_utils::create_test_db()).cpu
}

#[test]
fn redacts_binary_and_sensitive_request_values_recursively() {
    let mut payload = serde_json::json!({
        "image": "aGVsbG8=",
        "password": "plain text",
        "current_password": "old",
        "nested": {
            "url": "data:image/jpeg;base64,aGVsbG8=",
            "accessToken": "access-token",
            "faces": [{
                "embedding": "aGVsbG8=",
                "embeddingDimensions": 512,
                "api-key": "api-key"
            }]
        },
        "label": "keep"
    });

    redact_request_values(&mut payload);

    assert_eq!(payload["image"], "[base64 omitted]");
    assert_eq!(payload["password"], "[redacted]");
    assert_eq!(payload["current_password"], "[redacted]");
    assert_eq!(payload["nested"]["url"], "[base64 omitted]");
    assert_eq!(payload["nested"]["accessToken"], "[redacted]");
    assert_eq!(
        payload["nested"]["faces"][0]["embedding"],
        "[base64 omitted]"
    );
    assert_eq!(payload["nested"]["faces"][0]["api-key"], "[redacted]");
    assert_eq!(payload["nested"]["faces"][0]["embeddingDimensions"], 512);
    assert_eq!(payload["label"], "keep");
}

#[tokio::test]
async fn captures_and_restores_a_json_payload_for_the_handler() {
    let original = br#"{ "password": "secret", "label": "keep" }"#;
    let mut request = axum::http::Request::builder()
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(original.as_slice()))
        .expect("request");

    let capture = begin_payload_capture(&mut request).expect("payload capture");
    let downstream = to_bytes(request.into_body(), usize::MAX)
        .await
        .expect("downstream body");

    assert_eq!(downstream.as_ref(), original);
    assert_eq!(
        capture.render(&test_cpu_executor()).await,
        r#"{"label":"keep","password":"[redacted]"}"#
    );
}

#[tokio::test]
async fn truncates_logging_without_truncating_the_handler_body() {
    let original = vec![b'x'; MAX_REQUEST_LOG_CAPTURE_BYTES + 1];
    let mut request = axum::http::Request::builder()
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(original.clone()))
        .expect("request");

    let capture = begin_payload_capture(&mut request).expect("payload capture");
    let downstream = to_bytes(request.into_body(), usize::MAX)
        .await
        .expect("downstream body");

    assert_eq!(downstream.as_ref(), original.as_slice());
    assert_eq!(
        capture.render(&test_cpu_executor()).await,
        "[request body omitted: exceeded logging limit of 49152 bytes]"
    );
}

#[tokio::test]
async fn omits_multipart_payload_without_reading_it() {
    let original = b"large binary request";
    let mut request = axum::http::Request::builder()
        .method("POST")
        .header("content-type", "multipart/form-data; boundary=test")
        .body(Body::from(original.as_slice()))
        .expect("request");

    let capture = begin_payload_capture(&mut request).expect("payload capture");
    let downstream = to_bytes(request.into_body(), usize::MAX)
        .await
        .expect("downstream body");

    assert_eq!(
        capture.render(&test_cpu_executor()).await,
        "[multipart body omitted]"
    );
    assert_eq!(downstream.as_ref(), original);
}

#[tokio::test]
async fn omits_binary_payload_without_reading_it() {
    let original = b"binary request";
    let mut request = axum::http::Request::builder()
        .method("POST")
        .header("content-type", "video/mp4")
        .body(Body::from(original.as_slice()))
        .expect("request");

    let capture = begin_payload_capture(&mut request).expect("payload capture");
    let downstream = to_bytes(request.into_body(), usize::MAX)
        .await
        .expect("downstream body");

    assert_eq!(
        capture.render(&test_cpu_executor()).await,
        "[binary body omitted]"
    );
    assert_eq!(downstream.as_ref(), original);
}

#[test]
fn does_not_capture_non_post_requests() {
    let mut request = axum::http::Request::builder()
        .method("PUT")
        .header("content-type", "video/mp4")
        .body(Body::from("large streamed upload"))
        .expect("request");

    assert!(begin_payload_capture(&mut request).is_none());
}
