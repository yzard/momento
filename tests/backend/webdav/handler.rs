use axum::{
    body::{to_bytes, Body},
    http::{header, HeaderMap, HeaderValue, Method, Request, StatusCode},
};
use momento_api::webdav::handler::{
    contains_reserved_path, create_dav_handler, guard_response_body, handle_webdav_request,
    validate_upload_size,
};
use std::sync::Arc;

async fn request(
    handler: &dav_server::DavHandler,
    method: &str,
    uri: &str,
    headers: &[(&str, &str)],
    body: &'static [u8],
) -> (StatusCode, String) {
    let mut builder = Request::builder()
        .method(Method::from_bytes(method.as_bytes()).expect("method"))
        .uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let response = handle_webdav_request(
        handler.clone(),
        builder.body(Body::from(body)).expect("request"),
    )
    .await;
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[test]
fn test_upload_size_validation_requires_valid_bounded_size() {
    let mut headers = HeaderMap::new();
    assert_eq!(
        validate_upload_size(&Method::PUT, &headers, 11),
        Err(StatusCode::LENGTH_REQUIRED)
    );

    headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("invalid"));
    assert_eq!(
        validate_upload_size(&Method::PUT, &headers, 11),
        Err(StatusCode::BAD_REQUEST)
    );

    headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("12"));
    assert_eq!(
        validate_upload_size(&Method::PUT, &headers, 11),
        Err(StatusCode::PAYLOAD_TOO_LARGE)
    );

    headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("11"));
    assert_eq!(validate_upload_size(&Method::PUT, &headers, 11), Ok(()));
    let propfind = Method::from_bytes(b"PROPFIND").expect("PROPFIND method");
    assert_eq!(validate_upload_size(&propfind, &headers, 1), Ok(()));
}

#[test]
fn test_response_body_owns_request_permit_until_dropped() {
    let gate = Arc::new(tokio::sync::Semaphore::new(1));
    let permit = Arc::clone(&gate)
        .try_acquire_owned()
        .expect("request permit");
    let response = axum::response::Response::new(Body::empty());

    let guarded_response = guard_response_body(response, permit);

    assert_eq!(gate.available_permits(), 0);
    drop(guarded_response);
    assert_eq!(gate.available_permits(), 1);
}

#[test]
fn test_internal_processing_path_is_reserved_after_percent_decoding() {
    assert!(contains_reserved_path(
        "/webdav/.processing/claim/photo.jpg"
    ));
    assert!(contains_reserved_path(
        "/webdav/%2eprocessing/claim/photo.jpg"
    ));
    assert!(!contains_reserved_path(
        "/webdav/Camera%20Roll/.uploading.jpg"
    ));
}

#[tokio::test]
async fn test_photosync_upload_sequence_preserves_webdav_mount_path() {
    let directory = tempfile::tempdir().expect("temporary WebDAV root");
    let handler = create_dav_handler(directory.path(), "/webdav");

    let (status, _) = request(&handler, "OPTIONS", "/webdav/", &[], b"").await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = request(&handler, "MKCOL", "/webdav/Camera%20Roll", &[], b"").await;
    assert_eq!(status, StatusCode::CREATED);

    let jpeg = b"photo bytes";
    let (status, _) = request(
        &handler,
        "PUT",
        "/webdav/Camera%20Roll/.uploading.jpg",
        &[
            (header::CONTENT_TYPE.as_str(), "image/jpeg"),
            (header::CONTENT_LENGTH.as_str(), "11"),
        ],
        jpeg,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = request(
        &handler,
        "PROPFIND",
        "/webdav/Camera%20Roll/",
        &[("depth", "1")],
        b"",
    )
    .await;
    assert_eq!(status, StatusCode::MULTI_STATUS);
    assert!(body.contains("/webdav/Camera%20Roll/"), "{body}");
    assert!(
        body.contains("/webdav/Camera%20Roll/.uploading.jpg"),
        "{body}"
    );

    let (status, _) = request(
        &handler,
        "MOVE",
        "/webdav/Camera%20Roll/.uploading.jpg",
        &[(
            "destination",
            "http://photos.example/webdav/Camera%20Roll/photo.jpg",
        )],
        b"",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        std::fs::read(directory.path().join("Camera Roll/photo.jpg")).expect("uploaded file"),
        jpeg
    );
}
