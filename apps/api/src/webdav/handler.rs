use axum::{
    body::{Body, Bytes},
    extract::Request,
    http::{header, Method, StatusCode},
    response::Response,
};
use dav_server::{fakels::FakeLs, localfs::LocalFs, DavHandler};
use http_body_util::BodyExt;
use std::path::Path;
use tracing::{debug, error, info, trace};

pub fn create_dav_handler(webdav_root: &Path) -> DavHandler {
    std::fs::create_dir_all(webdav_root).ok();

    DavHandler::builder()
        .filesystem(LocalFs::new(webdav_root, false, false, false))
        .locksystem(FakeLs::new())
        .autoindex(true)
        .build_handler()
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
        info!(
            "WebDAV MKCOL already exists, returning 204 for {}",
            path
        );
        resp_parts.status = StatusCode::NO_CONTENT;
    }

    let resp_bytes: Bytes = match BodyExt::collect(resp_body).await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            error!(
                "WebDAV response body read failed: {} {} (status={}, error={})",
                method,
                path,
                resp_parts.status,
                e
            );
            trace!(
                "WebDAV response body read error details: {:?}",
                e
            );
            return Response::builder()
                .status(500)
                .body(Body::from("Failed to read response body"))
                .unwrap();
        }
    };

    if resp_parts.status.is_server_error() {
        error!(
            "WebDAV server error: {} {} -> {}",
            method,
            path,
            resp_parts.status
        );
        trace!(
            "WebDAV server error headers: {:?}",
            resp_parts.headers
        );
    }

    if method == Method::PUT {
        info!(
            "WebDAV upload response: {} -> {}",
            path,
            resp_parts.status
        );
    } else {
        debug!(
            "WebDAV response: {} {} -> {}",
            method,
            path,
            resp_parts.status
        );
    }

    Response::from_parts(resp_parts, Body::from(resp_bytes))
}
