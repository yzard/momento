use axum::body::{to_bytes, Body};
use momento_api::logging::{begin_payload_capture, redact_request_values};

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

    let capture = begin_payload_capture(&mut request, 1024).expect("payload capture");
    let downstream = to_bytes(request.into_body(), usize::MAX)
        .await
        .expect("downstream body");

    assert_eq!(downstream.as_ref(), original);
    assert_eq!(
        capture.render(),
        r#"{"label":"keep","password":"[redacted]"}"#
    );
}

#[tokio::test]
async fn truncates_logging_without_truncating_the_handler_body() {
    let original = br#"{"label":"a body larger than the capture limit"}"#;
    let mut request = axum::http::Request::builder()
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(original.as_slice()))
        .expect("request");

    let capture = begin_payload_capture(&mut request, 8).expect("payload capture");
    let downstream = to_bytes(request.into_body(), usize::MAX)
        .await
        .expect("downstream body");

    assert_eq!(downstream.as_ref(), original);
    assert_eq!(
        capture.render(),
        "[request body omitted: exceeded logging limit of 8 bytes]"
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

    let capture = begin_payload_capture(&mut request, 8).expect("payload capture");
    let downstream = to_bytes(request.into_body(), usize::MAX)
        .await
        .expect("downstream body");

    assert_eq!(capture.render(), "[multipart body omitted]");
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

    let capture = begin_payload_capture(&mut request, 8).expect("payload capture");
    let downstream = to_bytes(request.into_body(), usize::MAX)
        .await
        .expect("downstream body");

    assert_eq!(capture.render(), "[binary body omitted]");
    assert_eq!(downstream.as_ref(), original);
}

#[test]
fn does_not_capture_non_post_requests() {
    let mut request = axum::http::Request::builder()
        .method("PUT")
        .header("content-type", "video/mp4")
        .body(Body::from("large streamed upload"))
        .expect("request");

    assert!(begin_payload_capture(&mut request, 8).is_none());
}
