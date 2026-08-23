use axum::{
    body::{Body, BodyDataStream, Bytes},
    extract::Request,
    http::{header, HeaderMap, Method, StatusCode, Uri},
    response::Response,
};
use dav_server::{davpath::DavPath, fakels::FakeLs, localfs::LocalFs, DavHandler};
use futures::Stream;
use http_body_util::BodyExt;
use percent_encoding::percent_decode_str;
use std::{
    path::Path,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll},
};
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
        .map(|value| {
            value
                .to_str()
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or(StatusCode::BAD_REQUEST)
        })
        .transpose()?;
    let Some(declared_size) = declared_size else {
        return if *method == Method::PATCH {
            Err(StatusCode::LENGTH_REQUIRED)
        } else {
            Ok(())
        };
    };
    if declared_size > max_upload_bytes {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    Ok(())
}

pub fn request_mutates_staging(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
        && method.as_str() != "PROPFIND"
}

struct UploadLimitStream {
    inner: BodyDataStream,
    remaining_bytes: u64,
    limit_exceeded: Arc<AtomicBool>,
}

impl UploadLimitStream {
    fn new(body: Body, maximum_bytes: u64, limit_exceeded: Arc<AtomicBool>) -> Self {
        Self {
            inner: body.into_data_stream(),
            remaining_bytes: maximum_bytes,
            limit_exceeded,
        }
    }
}

impl Stream for UploadLimitStream {
    type Item = Result<Bytes, axum::Error>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let Poll::Ready(frame) = Pin::new(&mut self.inner).poll_next(context) else {
            return Poll::Pending;
        };
        let Some(frame) = frame else {
            return Poll::Ready(None);
        };
        let bytes = match frame {
            Ok(bytes) => bytes,
            Err(error) => return Poll::Ready(Some(Err(error))),
        };
        if bytes.len() as u64 <= self.remaining_bytes {
            self.remaining_bytes -= bytes.len() as u64;
            return Poll::Ready(Some(Ok(bytes)));
        }

        self.limit_exceeded.store(true, Ordering::Release);
        let allowed_length = usize::try_from(self.remaining_bytes)
            .unwrap_or(usize::MAX)
            .min(bytes.len());
        self.remaining_bytes = 0;
        Poll::Ready(Some(Ok(bytes.slice(..allowed_length))))
    }
}

pub fn contains_reserved_path(path: &str) -> bool {
    let Ok(decoded_path) = percent_decode_str(path).decode_utf8() else {
        return true;
    };
    decoded_path
        .split('/')
        .any(|segment| segment == ".processing")
}

pub fn contains_reserved_destination(headers: &HeaderMap) -> bool {
    headers
        .get("destination")
        .and_then(|value| value.to_str().ok())
        .and_then(destination_request_path)
        .is_some_and(|path| contains_reserved_path(&path))
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

pub async fn handle_webdav_request(
    dav_handler: DavHandler,
    request: Request,
    webdav_root: &Path,
    mount_path: &str,
    maximum_upload_bytes: u64,
) -> Response {
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

    let limit_exceeded = Arc::new(AtomicBool::new(false));
    let body = if method == Method::PUT && content_length.is_none() {
        Body::from_stream(UploadLimitStream::new(
            body,
            maximum_upload_bytes,
            Arc::clone(&limit_exceeded),
        ))
    } else {
        body
    };
    let dav_request = axum::http::Request::from_parts(parts, body);

    let dav_response = dav_handler.handle(dav_request).await;
    let (mut resp_parts, resp_body) = dav_response.into_parts();

    if limit_exceeded.load(Ordering::Acquire) {
        remove_oversized_upload(webdav_root, mount_path, &path).await;
        resp_parts.status = StatusCode::PAYLOAD_TOO_LARGE;
        resp_parts.headers.remove(header::ETAG);
        resp_parts.headers.remove(header::LAST_MODIFIED);
        return Response::from_parts(resp_parts, Body::empty());
    }

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

async fn remove_oversized_upload(webdav_root: &Path, mount_path: &str, request_path: &str) {
    let Some(upload_path) = webdav_storage_path(webdav_root, mount_path, request_path) else {
        return;
    };
    match tokio::fs::remove_file(&upload_path).await {
        Ok(()) => info!(path = %upload_path.display(), "Removed oversized WebDAV upload"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            error!(path = %upload_path.display(), "Could not remove oversized WebDAV upload: {error}")
        }
    }
}

fn destination_request_path(value: &str) -> Option<String> {
    let uri = value.parse::<Uri>().ok()?;
    let path = uri.path();
    (!path.is_empty()).then(|| path.to_string())
}

fn content_range_completes_upload(value: &str) -> bool {
    let Some((unit, range_and_total)) = value.split_once(' ') else {
        return false;
    };
    if unit != "bytes" {
        return false;
    }
    let Some((range, total)) = range_and_total.split_once('/') else {
        return false;
    };
    let Some((_, end)) = range.split_once('-') else {
        return false;
    };
    let (Ok(end), Ok(total)) = (end.parse::<u64>(), total.parse::<u64>()) else {
        return false;
    };
    end.checked_add(1) == Some(total)
}

fn webdav_storage_path(
    webdav_root: &Path,
    mount_path: &str,
    request_path: &str,
) -> Option<std::path::PathBuf> {
    let mut dav_path = DavPath::new(request_path).ok()?;
    dav_path.set_prefix(mount_path).ok()?;
    Some(webdav_root.join(dav_path.as_rel_ospath()))
}

pub fn invalidated_upload_paths(
    method: &Method,
    headers: &HeaderMap,
    request_path: &str,
    mount_path: &str,
) -> Vec<String> {
    let source_path = relative_webdav_path(mount_path, request_path);
    let destination_path = headers
        .get("destination")
        .and_then(|value| value.to_str().ok())
        .and_then(destination_request_path)
        .and_then(|path| relative_webdav_path(mount_path, &path));
    match method.as_str() {
        "PUT" | "PATCH" | "DELETE" => source_path.into_iter().collect(),
        "MOVE" => source_path.into_iter().chain(destination_path).collect(),
        "COPY" => destination_path.into_iter().collect(),
        _ => Vec::new(),
    }
}

pub fn completed_upload_path(
    method: &Method,
    headers: &HeaderMap,
    request_path: &str,
    mount_path: &str,
) -> Option<String> {
    if *method == Method::PUT {
        return relative_webdav_path(mount_path, request_path);
    }
    if *method == Method::PATCH {
        let is_complete = headers
            .get(header::CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(content_range_completes_upload);
        return is_complete
            .then(|| relative_webdav_path(mount_path, request_path))
            .flatten();
    }
    if !matches!(method.as_str(), "MOVE" | "COPY") {
        return None;
    }
    headers
        .get("destination")
        .and_then(|value| value.to_str().ok())
        .and_then(destination_request_path)
        .and_then(|path| relative_webdav_path(mount_path, &path))
}

fn relative_webdav_path(mount_path: &str, request_path: &str) -> Option<String> {
    let mut dav_path = DavPath::new(request_path).ok()?;
    dav_path.set_prefix(mount_path).ok()?;
    let relative_path = dav_path.as_rel_ospath().to_string_lossy().to_string();
    (!relative_path.is_empty()).then_some(relative_path)
}
