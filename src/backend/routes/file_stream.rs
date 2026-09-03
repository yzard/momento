use axum::{
    body::Body,
    http::{header, HeaderMap, StatusCode},
    response::Response,
};

use crate::error::{AppError, AppResult};
use crate::executor::{ExecutorError, ExecutorErrorKind, FileIoExecutorHandle};
use crate::io::file::{NormalizedStoragePath, StorageRootId};
use crate::io::{StorageFileSession, StorageFileSnapshot};
use crate::runtime::{FileChunkAdmission, HttpRequestAdmission, SchedulerHandle};

#[derive(Clone, Copy)]
pub(crate) enum ContentDisposition {
    Inline,
    Attachment,
}

pub(crate) struct FileResponseOptions<'a> {
    pub admission: &'a HttpRequestAdmission,
    pub content_type: &'a str,
    pub headers: &'a HeaderMap,
    pub filename: Option<&'a str>,
    pub allow_ranges: bool,
    pub content_disposition: ContentDisposition,
    pub cache_control: &'a str,
    pub head_only: bool,
}

pub(crate) async fn serve_file(
    file_io: &FileIoExecutorHandle,
    storage_root: StorageRootId,
    path: NormalizedStoragePath,
    options: FileResponseOptions<'_>,
) -> AppResult<Response> {
    let (session, snapshot) = file_io
        .open_storage_read_session_request(storage_root, path)
        .await
        .map_err(map_open_error)?;
    let file_size = snapshot.byte_size;
    let etag = file_etag(snapshot);
    let last_modified = last_modified(snapshot);
    if let Some(status) = precondition_status(options.headers, &etag, snapshot) {
        drop(session);
        return Response::builder()
            .status(status)
            .header(header::ETAG, etag)
            .header(header::LAST_MODIFIED, &last_modified)
            .header(header::CACHE_CONTROL, options.cache_control)
            .header("referrer-policy", "no-referrer")
            .body(Body::empty())
            .map_err(|error| AppError::Internal(error.to_string()));
    }

    let range_header = options
        .allow_ranges
        .then(|| if_range_allows_range(options.headers, &etag, snapshot))
        .and_then(|allowed| {
            allowed.then(|| {
                options
                    .headers
                    .get(header::RANGE)
                    .and_then(|header_value| header_value.to_str().ok())
                    .map(str::to_string)
            })
        })
        .flatten();

    if let Some(range_header) = range_header {
        return serve_range(
            file_io,
            session,
            &options,
            &etag,
            &last_modified,
            &range_header,
            file_size,
        )
        .await;
    }

    let body = if options.head_only {
        drop(session);
        Body::empty()
    } else {
        if let Err(error) = options.admission.convert_to_stream() {
            drop(session);
            return Err(AppError::Unavailable(error));
        }
        streaming_body(
            file_io.clone(),
            options.admission.clone(),
            session,
            file_size,
        )
    };
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, options.content_type)
        .header(header::CONTENT_LENGTH, file_size)
        .header(header::ETAG, &etag)
        .header(header::LAST_MODIFIED, &last_modified)
        .header(header::CACHE_CONTROL, options.cache_control)
        .header("referrer-policy", "no-referrer");
    if options.allow_ranges {
        response = response.header(header::ACCEPT_RANGES, "bytes");
    }
    if let Some(filename) = options.filename {
        response = response.header(
            header::CONTENT_DISPOSITION,
            content_disposition_header(options.content_disposition, filename),
        );
    }
    response
        .body(body)
        .map_err(|error| AppError::Internal(error.to_string()))
}

async fn serve_range(
    file_io: &FileIoExecutorHandle,
    session: StorageFileSession,
    options: &FileResponseOptions<'_>,
    etag: &str,
    last_modified: &str,
    range_header: &str,
    file_size: u64,
) -> AppResult<Response> {
    let Some((start, end)) = parse_range(range_header, file_size) else {
        drop(session);
        return range_not_satisfiable(file_size, options, etag, last_modified);
    };

    let session = if start == 0 {
        session
    } else {
        file_io
            .seek_storage_read_session_request(session, start)
            .await?
    };
    let length = end - start + 1;
    let body = if options.head_only {
        drop(session);
        Body::empty()
    } else {
        if let Err(error) = options.admission.convert_to_stream() {
            drop(session);
            return Err(AppError::Unavailable(error));
        }
        streaming_body(file_io.clone(), options.admission.clone(), session, length)
    };
    let mut response = Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header(header::CONTENT_TYPE, options.content_type)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, length)
        .header(header::ETAG, etag)
        .header(header::LAST_MODIFIED, last_modified)
        .header(header::CACHE_CONTROL, options.cache_control)
        .header("referrer-policy", "no-referrer")
        .header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{file_size}"),
        );
    if let Some(filename) = options.filename {
        response = response.header(
            header::CONTENT_DISPOSITION,
            content_disposition_header(options.content_disposition, filename),
        );
    }
    response
        .body(body)
        .map_err(|error| AppError::Internal(error.to_string()))
}

struct FileStreamState {
    file_io: FileIoExecutorHandle,
    scheduler: SchedulerHandle,
    previous_chunk_admission: Option<FileChunkAdmission>,
    session: Option<StorageFileSession>,
    remaining: u64,
}

fn streaming_body(
    file_io: FileIoExecutorHandle,
    admission: HttpRequestAdmission,
    session: StorageFileSession,
    remaining: u64,
) -> Body {
    let state = FileStreamState {
        scheduler: admission
            .scheduler()
            .expect("stream admission retains its scheduler"),
        file_io,
        previous_chunk_admission: None,
        session: Some(session),
        remaining,
    };
    let stream = futures::stream::unfold(state, |mut state| async move {
        drop(state.previous_chunk_admission.take());
        if state.remaining == 0 {
            drop(state.session.take());
            return None;
        }
        let session = state.session.take()?;
        let maximum_bytes =
            usize::try_from(state.remaining.min(crate::runtime::FILE_IO_CHUNK_BYTES))
                .expect("file chunk limit fits usize");
        let chunk_admission = match state.scheduler.acquire_file_chunk().await {
            Ok(admission) => admission,
            Err(error) => {
                drop(session);
                state.remaining = 0;
                return Some((Err(std::io::Error::other(error)), state));
            }
        };
        match state
            .file_io
            .read_storage_session_request(session, maximum_bytes)
            .await
        {
            Ok((session, bytes)) if bytes.is_empty() => {
                drop(session);
                state.remaining = 0;
                Some((
                    Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "storage file ended before its descriptor snapshot length",
                    )),
                    state,
                ))
            }
            Ok((session, bytes)) => {
                state.remaining -= u64::try_from(bytes.len()).expect("file chunk length fits u64");
                state.session = Some(session);
                state.previous_chunk_admission = Some(chunk_admission);
                Some((Ok(bytes), state))
            }
            Err(error) => {
                state.remaining = 0;
                Some((Err(std::io::Error::other(error.to_string())), state))
            }
        }
    });
    Body::from_stream(stream)
}

fn map_open_error(error: ExecutorError) -> AppError {
    if error.kind == ExecutorErrorKind::FileNotFound {
        AppError::NotFound("Media file not found".to_string())
    } else {
        error.into()
    }
}

fn file_etag(snapshot: StorageFileSnapshot) -> String {
    format!(
        "W/\"{:x}-{:x}-{:x}-{:x}-{:x}-{:x}-{:x}-{:x}-{:x}\"",
        snapshot.device_major,
        snapshot.device_minor,
        snapshot.mount_id,
        snapshot.inode,
        snapshot.byte_size,
        snapshot.modified_seconds,
        snapshot.modified_nanoseconds,
        snapshot.changed_seconds,
        snapshot.changed_nanoseconds,
    )
}

fn last_modified(snapshot: StorageFileSnapshot) -> String {
    let seconds = u64::try_from(snapshot.modified_seconds).unwrap_or_default();
    httpdate::fmt_http_date(std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds))
}

fn precondition_status(
    headers: &HeaderMap,
    etag: &str,
    snapshot: StorageFileSnapshot,
) -> Option<StatusCode> {
    let if_match = headers
        .get(header::IF_MATCH)
        .and_then(|value| value.to_str().ok());
    if let Some(value) = if_match {
        if value.trim() != "*"
            && !value
                .split(',')
                .any(|candidate| strong_match(candidate, etag))
        {
            return Some(StatusCode::PRECONDITION_FAILED);
        }
    } else if headers
        .get(header::IF_UNMODIFIED_SINCE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| httpdate::parse_http_date(value).ok())
        .is_some_and(|date| modified_after(snapshot, date))
    {
        return Some(StatusCode::PRECONDITION_FAILED);
    }

    if let Some(value) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
    {
        if value
            .split(',')
            .any(|candidate| candidate.trim() == "*" || weak_match(candidate, etag))
        {
            return Some(StatusCode::NOT_MODIFIED);
        }
    } else if headers
        .get(header::IF_MODIFIED_SINCE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| httpdate::parse_http_date(value).ok())
        .is_some_and(|date| !modified_after(snapshot, date))
    {
        return Some(StatusCode::NOT_MODIFIED);
    }
    None
}

fn if_range_allows_range(headers: &HeaderMap, etag: &str, snapshot: StorageFileSnapshot) -> bool {
    let Some(value) = headers
        .get(header::IF_RANGE)
        .and_then(|value| value.to_str().ok())
    else {
        return true;
    };
    if let Ok(date) = httpdate::parse_http_date(value) {
        return !modified_after(snapshot, date);
    }
    strong_match(value, etag)
}

fn strong_match(candidate: &str, current: &str) -> bool {
    let candidate = candidate.trim();
    !candidate.starts_with("W/") && !current.starts_with("W/") && candidate == current
}

fn weak_match(candidate: &str, current: &str) -> bool {
    candidate
        .trim()
        .strip_prefix("W/")
        .unwrap_or(candidate.trim())
        == current.strip_prefix("W/").unwrap_or(current)
}

fn modified_after(snapshot: StorageFileSnapshot, date: std::time::SystemTime) -> bool {
    let modified_seconds = u64::try_from(snapshot.modified_seconds).unwrap_or_default();
    let date_seconds = date
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    modified_seconds > date_seconds
}

fn range_not_satisfiable(
    file_size: u64,
    options: &FileResponseOptions<'_>,
    etag: &str,
    last_modified: &str,
) -> AppResult<Response> {
    Response::builder()
        .status(StatusCode::RANGE_NOT_SATISFIABLE)
        .header(header::CONTENT_RANGE, format!("bytes */{file_size}"))
        .header(header::ETAG, etag)
        .header(header::LAST_MODIFIED, last_modified)
        .header(header::CACHE_CONTROL, options.cache_control)
        .header("referrer-policy", "no-referrer")
        .body(Body::empty())
        .map_err(|error| AppError::Internal(error.to_string()))
}

fn parse_range(range_header: &str, file_size: u64) -> Option<(u64, u64)> {
    if file_size == 0 {
        return None;
    }
    let range = range_header.strip_prefix("bytes=")?;
    if range.contains(',') {
        return None;
    }
    let (start, end) = range.split_once('-')?;
    if start.is_empty() {
        let suffix_length = end.parse::<u64>().ok()?;
        if suffix_length == 0 {
            return None;
        }
        return Some((file_size.saturating_sub(suffix_length), file_size - 1));
    }
    let start = start.parse::<u64>().ok()?;
    if start >= file_size {
        return None;
    }
    if end.is_empty() {
        return Some((start, file_size - 1));
    }
    let end = end.parse::<u64>().ok()?;
    if end < start {
        return None;
    }
    Some((start, end.min(file_size - 1)))
}

fn content_disposition_header(disposition: ContentDisposition, filename: &str) -> String {
    let disposition = match disposition {
        ContentDisposition::Inline => "inline",
        ContentDisposition::Attachment => "attachment",
    };
    format!(
        "{disposition}; filename=\"{}\"",
        safe_content_disposition_filename(filename)
    )
}

fn safe_content_disposition_filename(filename: &str) -> String {
    filename
        .chars()
        .map(|character| {
            if character == '"' || character == '\\' || character.is_control() {
                '_'
            } else {
                character
            }
        })
        .collect()
}
