use axum::{
    body::Body,
    extract::Request,
    http::{header, HeaderMap, Method, StatusCode},
    response::Response,
};
use dav_server::{fakels::FakeLs, localfs::LocalFs, DavHandler};
use http_body_util::BodyExt;
use percent_encoding::percent_decode_str;
use std::path::Path;
use tokio::sync::OwnedSemaphorePermit;
use tracing::{debug, error, info, trace};

pub fn validate_upload_size(
    method: &Method,
    headers: &HeaderMap,
    max_upload_bytes: u64,
) -> Result<(), StatusCode> {
    if !matches!(*method, Method::PUT | Method::PATCH) {
        return Ok(());
    }

    let declared_size = headers
        .get(header::CONTENT_LENGTH)
        .or_else(|| headers.get("x-expected-entity-length"))
        .ok_or(StatusCode::LENGTH_REQUIRED)?
        .to_str()
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or(StatusCode::BAD_REQUEST)?;
    if declared_size > max_upload_bytes {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    Ok(())
}

pub fn contains_reserved_path(path: &str) -> bool {
    let Ok(decoded_path) = percent_decode_str(path).decode_utf8() else {
        return true;
    };
    decoded_path
        .split('/')
        .any(|segment| segment == ".processing")
}

pub fn create_dav_handler(webdav_root: &Path, mount_path: &str) -> DavHandler {
    std::fs::create_dir_all(webdav_root).ok();

    DavHandler::builder()
        .strip_prefix(mount_path)
        .filesystem(LocalFs::new(webdav_root, false, false, false))
        .locksystem(FakeLs::new())
        .autoindex(true)
        .build_handler()
}

pub fn guard_response_body(response: Response, request_permit: OwnedSemaphorePermit) -> Response {
    let (parts, body) = response.into_parts();
    let guarded_body = body.map_frame(move |frame| {
        let _request_permit = &request_permit;
        frame
    });
    Response::from_parts(parts, Body::new(guarded_body))
}

pub async fn handle_webdav_request(dav_handler: DavHandler, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let method = parts.method.clone();
    let path = parts.uri.path().to_string();
    let content_length = parts
        .headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let content_type = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("unknown");
    let transfer_encoding = parts
        .headers
        .get(header::TRANSFER_ENCODING)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("none");

    trace!(
        "WebDAV request headers: method={} path={} content_type={} content_length={:?} transfer_encoding={}",
        method,
        path,
        content_type,
        content_length,
        transfer_encoding
    );

    if method == Method::PUT {
        info!(
            "WebDAV upload request: {} ({} bytes)",
            path,
            content_length
                .map(|bytes| bytes.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        );
    } else {
        debug!("WebDAV request: {} {}", method, path);
    }

    let dav_request = axum::http::Request::from_parts(parts, body);

    let dav_response = dav_handler.handle(dav_request).await;
    let (mut resp_parts, resp_body) = dav_response.into_parts();

    if method.as_str() == "MKCOL" && resp_parts.status == StatusCode::METHOD_NOT_ALLOWED {
        info!("WebDAV MKCOL already exists, returning 204 for {}", path);
        resp_parts.status = StatusCode::NO_CONTENT;
    }

    if resp_parts.status.is_server_error() {
        error!(
            "WebDAV server error: {} {} -> {}",
            method, path, resp_parts.status
        );
        trace!("WebDAV server error headers: {:?}", resp_parts.headers);
    }

    if method == Method::PUT {
        info!("WebDAV upload response: {} -> {}", path, resp_parts.status);
    } else {
        debug!(
            "WebDAV response: {} {} -> {}",
            method, path, resp_parts.status
        );
    }

    Response::from_parts(resp_parts, Body::new(resp_body))
}
