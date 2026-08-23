use axum::{
    body::{to_bytes, Body},
    http::{header, HeaderMap, HeaderValue, Method, Request, StatusCode},
};
use momento_api::webdav::handler::{
    completed_upload_path, contains_reserved_destination, contains_reserved_path,
    create_dav_handler, guard_response_body, handle_webdav_request, invalidated_upload_paths,
    request_mutates_staging, validate_upload_size,
};
use std::sync::Arc;

async fn request(
    handler: &dav_server::DavHandler,
    webdav_root: &std::path::Path,
    maximum_upload_bytes: u64,
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
        webdav_root,
        "/webdav",
        maximum_upload_bytes,
    )
    .await;
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[test]
fn test_upload_size_validation_accepts_undeclared_put_and_bounds_declared_sizes() {
    let mut headers = HeaderMap::new();
    assert_eq!(validate_upload_size(&Method::PUT, &headers, 11), Ok(()));
    assert_eq!(
        validate_upload_size(&Method::PATCH, &headers, 11),
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
fn test_only_staging_mutations_block_import_claims() {
    for method in [Method::GET, Method::HEAD, Method::OPTIONS] {
        assert!(!request_mutates_staging(&method), "{method} must not block");
    }
    let propfind = Method::from_bytes(b"PROPFIND").expect("PROPFIND method");
    assert!(!request_mutates_staging(&propfind));

    for method in [Method::PUT, Method::PATCH, Method::DELETE] {
        assert!(request_mutates_staging(&method), "{method} must block");
    }
    for method in ["MKCOL", "MOVE", "COPY", "PROPPATCH"] {
        let method = Method::from_bytes(method.as_bytes()).expect("WebDAV mutation method");
        assert!(request_mutates_staging(&method), "{method} must block");
    }
}

#[test]
fn test_completed_upload_paths_require_a_complete_mutation() {
    let empty_headers = HeaderMap::new();
    assert_eq!(
        completed_upload_path(&Method::PUT, &empty_headers, "/webdav/video.mp4", "/webdav"),
        Some("video.mp4".to_string())
    );
    assert_eq!(
        completed_upload_path(
            &Method::PATCH,
            &empty_headers,
            "/webdav/video.mp4",
            "/webdav"
        ),
        None
    );

    let mut patch_headers = HeaderMap::new();
    patch_headers.insert(
        header::CONTENT_RANGE,
        HeaderValue::from_static("bytes 10-19/20"),
    );
    assert_eq!(
        completed_upload_path(
            &Method::PATCH,
            &patch_headers,
            "/webdav/video.mp4",
            "/webdav"
        ),
        Some("video.mp4".to_string())
    );

    let move_method = Method::from_bytes(b"MOVE").expect("MOVE method");
    let mut move_headers = HeaderMap::new();
    move_headers.insert(
        "destination",
        HeaderValue::from_static("http://photos.example/webdav/Camera%20Roll/video.mp4"),
    );
    assert_eq!(
        completed_upload_path(
            &move_method,
            &move_headers,
            "/webdav/.uploading.mp4",
            "/webdav"
        ),
        Some("Camera Roll/video.mp4".to_string())
    );
    assert_eq!(
        invalidated_upload_paths(
            &move_method,
            &move_headers,
            "/webdav/.uploading.mp4",
            "/webdav"
        ),
        vec![
            ".uploading.mp4".to_string(),
            "Camera Roll/video.mp4".to_string()
        ]
    );
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
    let mut headers = HeaderMap::new();
    headers.insert(
        "destination",
        HeaderValue::from_static("http://photos.example/webdav/%2eprocessing/file.jpg"),
    );
    assert!(contains_reserved_destination(&headers));
}

#[tokio::test]
async fn test_photosync_upload_sequence_preserves_webdav_mount_path() {
    let directory = tempfile::tempdir().expect("temporary WebDAV root");
    let handler = create_dav_handler(directory.path(), "/webdav");

    let (status, _) = request(
        &handler,
        directory.path(),
        1024,
        "OPTIONS",
        "/webdav/",
        &[],
        b"",
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = request(
        &handler,
        directory.path(),
        1024,
        "MKCOL",
        "/webdav/Camera%20Roll",
        &[],
        b"",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let jpeg = b"photo bytes";
    let (status, _) = request(
        &handler,
        directory.path(),
        1024,
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
        directory.path(),
        1024,
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
        directory.path(),
        1024,
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

    let video = b"chunked video bytes";
    let (status, _) = request(
        &handler,
        directory.path(),
        video.len() as u64,
        "PUT",
        "/webdav/Camera%20Roll/video.mp4",
        &[(header::TRANSFER_ENCODING.as_str(), "chunked")],
        video,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        std::fs::read(directory.path().join("Camera Roll/video.mp4")).expect("uploaded video"),
        video
    );

    let (status, _) = request(
        &handler,
        directory.path(),
        5,
        "PUT",
        "/webdav/Camera%20Roll/oversized.mp4",
        &[(header::TRANSFER_ENCODING.as_str(), "chunked")],
        b"too many bytes",
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(!directory.path().join("Camera Roll/oversized.mp4").exists());
}
