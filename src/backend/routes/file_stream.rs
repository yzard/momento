use axum::{
    body::Body,
    http::{header, HeaderMap, StatusCode},
    response::Response,
};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

use crate::error::{AppError, AppResult};

#[derive(Clone, Copy)]
pub(super) enum ContentDisposition {
    Inline,
    Attachment,
}

pub(super) async fn serve_file(
    path: std::path::PathBuf,
    content_type: &str,
    headers: &HeaderMap,
    filename: Option<&str>,
    allow_ranges: bool,
    content_disposition: ContentDisposition,
) -> AppResult<Response> {
    let metadata = tokio::fs::metadata(&path).await?;
    let file_size = metadata.len();
    let etag = file_etag(&metadata);
    if matches_if_none_match(headers, &etag) {
        return Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header(header::ETAG, etag)
            .header(header::CACHE_CONTROL, "private")
            .header("referrer-policy", "no-referrer")
            .body(Body::empty())
            .map_err(|error| AppError::Internal(error.to_string()));
    }

    let range_header = allow_ranges
        .then(|| if_range_allows_range(headers, &etag))
        .and_then(|allowed| {
            allowed.then(|| {
                headers
                    .get(header::RANGE)
                    .and_then(|header_value| header_value.to_str().ok())
                    .map(str::to_string)
            })
        })
        .flatten();

    if let Some(range_header) = range_header {
        return serve_range(
            path,
            content_type,
            filename,
            content_disposition,
            &etag,
            &range_header,
            file_size,
        )
        .await;
    }

    let file = File::open(&path).await?;
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, file_size)
        .header(header::ETAG, &etag)
        .header(header::CACHE_CONTROL, "private")
        .header("referrer-policy", "no-referrer");
    if allow_ranges {
        response = response.header(header::ACCEPT_RANGES, "bytes");
    }
    if let Some(filename) = filename {
        response = response.header(
            header::CONTENT_DISPOSITION,
            content_disposition_header(content_disposition, filename),
        );
    }
    response
        .body(body)
        .map_err(|error| AppError::Internal(error.to_string()))
}

async fn serve_range(
    path: std::path::PathBuf,
    content_type: &str,
    filename: Option<&str>,
    content_disposition: ContentDisposition,
    etag: &str,
    range_header: &str,
    file_size: u64,
) -> AppResult<Response> {
    let Some((start, end)) = parse_range(range_header, file_size) else {
        return range_not_satisfiable(file_size);
    };

    let mut file = File::open(&path).await?;
    file.seek(std::io::SeekFrom::Start(start)).await?;
    let length = end - start + 1;
    let stream = ReaderStream::new(file.take(length));
    let body = Body::from_stream(stream);
    let mut response = Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, length)
        .header(header::ETAG, etag)
        .header(header::CACHE_CONTROL, "private")
        .header("referrer-policy", "no-referrer")
        .header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{file_size}"),
        );
    if let Some(filename) = filename {
        response = response.header(
            header::CONTENT_DISPOSITION,
            content_disposition_header(content_disposition, filename),
        );
    }
    response
        .body(body)
        .map_err(|error| AppError::Internal(error.to_string()))
}

fn file_etag(metadata: &std::fs::Metadata) -> String {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("W/\"{}-{modified}\"", metadata.len())
}

fn matches_if_none_match(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|candidate| candidate.trim() == "*" || candidate.trim() == etag)
        })
}

fn if_range_allows_range(headers: &HeaderMap, etag: &str) -> bool {
    match headers
        .get(header::IF_RANGE)
        .and_then(|value| value.to_str().ok())
    {
        None => true,
        Some(value) => value == etag,
    }
}

fn range_not_satisfiable(file_size: u64) -> AppResult<Response> {
    Response::builder()
        .status(StatusCode::RANGE_NOT_SATISFIABLE)
        .header(header::CONTENT_RANGE, format!("bytes */{file_size}"))
        .header(header::CACHE_CONTROL, "private")
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
