use momento_api::logging::redact_binary_values;

#[test]
fn redacts_base64_request_values() {
    let mut payload = serde_json::json!({
        "image": "aGVsbG8=",
        "nested": { "url": "data:image/jpeg;base64,aGVsbG8=" },
        "label": "keep"
    });

    redact_binary_values(&mut payload);

    assert_eq!(payload["image"], "[base64 omitted]");
    assert_eq!(payload["nested"]["url"], "[base64 omitted]");
    assert_eq!(payload["label"], "keep");
}

#[tokio::test]
async fn omits_multipart_payload_without_reading_it() {
    let mut request = axum::http::Request::builder()
        .method("POST")
        .header("content-type", "multipart/form-data; boundary=test")
        .body(axum::body::Body::from("large binary request"))
        .expect("request");

    let payload = momento_api::logging::extract_compact_payload(&mut request).await;

    assert_eq!(payload.as_deref(), Some("[multipart body omitted]"));
}
