use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

use crate::auth::{AppState, CurrentUser};
use crate::constants::{ORIGINALS_DIR, PREVIEWS_DIR, THUMBNAILS_DIR};
use crate::database::{execute_query, fetch_all, fetch_one};
use crate::error::{AppError, AppResult};
use crate::models::{
    DeleteMediaResponse, MediaDeleteRequest, MediaGetRequest, MediaListRequest,
    MediaListResponse, MediaResponse, MediaUpdateRequest,
};
use crate::processor::thumbnails::generate_image_preview;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/media/list", post(list_media))
        .route("/media/get", post(get_media))
        .route("/media/update", post(update_media))
        .route("/media/delete", post(delete_media))
        .route("/media/file/:media_id", get(get_media_file))
}

pub fn thumbnail_router() -> Router<AppState> {
    Router::new().route("/thumbnail/:media_id", get(get_media_thumbnail))
}

pub fn preview_router() -> Router<AppState> {
    Router::new().route("/preview/:media_id", get(get_media_preview))
}

fn row_to_media_response(
    id: i64,
    filename: String,
    original_filename: String,
    media_type: String,
    mime_type: Option<String>,
    width: Option<i32>,
    height: Option<i32>,
    file_size: Option<i64>,
    duration_seconds: Option<f64>,
    date_taken: Option<String>,
    gps_latitude: Option<f64>,
    gps_longitude: Option<f64>,
    camera_make: Option<String>,
    camera_model: Option<String>,
    iso: Option<i32>,
    exposure_time: Option<String>,
    f_number: Option<f64>,
    focal_length: Option<f64>,
    gps_altitude: Option<f64>,
    location_state: Option<String>,
    location_country: Option<String>,
    keywords: Option<String>,
    created_at: String,
) -> MediaResponse {
    MediaResponse {
        id,
        filename,
        original_filename,
        media_type,
        mime_type,
        width,
        height,
        file_size,
        duration_seconds,
        date_taken,
        gps_latitude,
        gps_longitude,
        camera_make,
        camera_model,
        iso,
        exposure_time,
        f_number,
        focal_length,
        gps_altitude,
        location_state,
        location_country,
        keywords,
        created_at,
    }
}

async fn list_media(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<MediaListRequest>,
) -> AppResult<Json<MediaListResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;
    let limit = request.limit.min(100);

    let rows = if let Some(ref cursor) = request.cursor {
        let parts: Vec<&str> = cursor.split('_').collect();
        if parts.len() == 2 {
            let cursor_date = parts[0];
            let cursor_id: i64 = parts[1].parse().unwrap_or(0);
            fetch_all(
                &conn,
                r#"
                SELECT id, filename, original_filename, media_type, mime_type, width, height,
                       file_size, duration_seconds, date_taken, gps_latitude, gps_longitude,
                       camera_make, camera_model, iso, exposure_time, f_number, focal_length,
                       gps_altitude, location_state, location_country, keywords, created_at
                FROM media
                WHERE user_id = ? AND deleted_at IS NULL
                  AND (date_taken < ? OR (date_taken = ? AND id < ?))
                ORDER BY date_taken DESC, id DESC
                LIMIT ?
                "#,
                &[&current_user.id, &cursor_date, &cursor_date, &cursor_id, &(limit + 1)],
                map_media_row,
            )?
        } else {
            fetch_default_media(&conn, current_user.id, limit)?
        }
    } else {
        fetch_default_media(&conn, current_user.id, limit)?
    };

    let has_more = rows.len() > limit as usize;
    let items: Vec<MediaResponse> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more && !items.is_empty() {
        let last = items.last().unwrap();
        last.date_taken
            .as_ref()
            .map(|dt| format!("{}_{}", dt, last.id))
    } else {
        None
    };

    Ok(Json(MediaListResponse {
        items,
        next_cursor,
        has_more,
    }))
}

fn fetch_default_media(
    conn: &crate::database::DbConn,
    user_id: i64,
    limit: i32,
) -> AppResult<Vec<MediaResponse>> {
    fetch_all(
        conn,
        r#"
        SELECT id, filename, original_filename, media_type, mime_type, width, height,
               file_size, duration_seconds, date_taken, gps_latitude, gps_longitude,
               camera_make, camera_model, iso, exposure_time, f_number, focal_length,
               gps_altitude, location_state, location_country, keywords, created_at
        FROM media
        WHERE user_id = ? AND deleted_at IS NULL
        ORDER BY date_taken DESC, id DESC
        LIMIT ?
        "#,
        &[&user_id, &(limit + 1)],
        map_media_row,
    )
}

fn map_media_row(row: &rusqlite::Row) -> rusqlite::Result<MediaResponse> {
    Ok(row_to_media_response(
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
        row.get(12)?,
        row.get(13)?,
        row.get(14)?,
        row.get(15)?,
        row.get(16)?,
        row.get(17)?,
        row.get(18)?,
        row.get(19)?,
        row.get(20)?,
        row.get(21)?,
        row.get(22)?,
    ))
}

async fn get_media(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<MediaGetRequest>,
) -> AppResult<Json<MediaResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let media = fetch_one(
        &conn,
        r#"
        SELECT id, filename, original_filename, media_type, mime_type, width, height,
               file_size, duration_seconds, date_taken, gps_latitude, gps_longitude,
               camera_make, camera_model, iso, exposure_time, f_number, focal_length,
               gps_altitude, location_state, location_country, keywords, created_at
        FROM media
        WHERE id = ? AND user_id = ? AND deleted_at IS NULL
        "#,
        &[&request.media_id, &current_user.id],
        map_media_row,
    )?
    .ok_or_else(|| AppError::NotFound("Media not found".to_string()))?;

    Ok(Json(media))
}

async fn update_media(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<MediaUpdateRequest>,
) -> AppResult<Json<MediaResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    // Check exists
    let exists = fetch_one(
        &conn,
        "SELECT id FROM media WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
        &[&request.media_id, &current_user.id],
        |row| Ok(row.get::<_, i64>(0)?),
    )?;

    if exists.is_none() {
        return Err(AppError::NotFound("Media not found".to_string()));
    }

    let mut updates = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(ref date_taken) = request.date_taken {
        updates.push("date_taken = ?");
        params.push(Box::new(date_taken.clone()));
    }

    if let Some(lat) = request.gps_latitude {
        updates.push("gps_latitude = ?");
        params.push(Box::new(lat));
    }

    if let Some(lon) = request.gps_longitude {
        updates.push("gps_longitude = ?");
        params.push(Box::new(lon));
    }

    if !updates.is_empty() {
        params.push(Box::new(request.media_id));
        let sql = format!("UPDATE media SET {} WHERE id = ?", updates.join(", "));
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        execute_query(&conn, &sql, &param_refs)?;
    }

    let media = fetch_one(
        &conn,
        r#"
        SELECT id, filename, original_filename, media_type, mime_type, width, height,
               file_size, duration_seconds, date_taken, gps_latitude, gps_longitude,
               camera_make, camera_model, iso, exposure_time, f_number, focal_length,
               gps_altitude, location_state, location_country, keywords, created_at
        FROM media WHERE id = ?
        "#,
        &[&request.media_id],
        map_media_row,
    )?
    .ok_or_else(|| AppError::Internal("Update failed".to_string()))?;

    Ok(Json(media))
}

async fn delete_media(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<MediaDeleteRequest>,
) -> AppResult<Json<DeleteMediaResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let exists = fetch_one(
        &conn,
        "SELECT id FROM media WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
        &[&request.media_id, &current_user.id],
        |row| Ok(row.get::<_, i64>(0)?),
    )?;

    if exists.is_none() {
        return Err(AppError::NotFound("Media not found".to_string()));
    }

    execute_query(
        &conn,
        "UPDATE media SET deleted_at = datetime('now') WHERE id = ?",
        &[&request.media_id],
    )?;

    Ok(Json(DeleteMediaResponse {
        message: "Media moved to trash".to_string(),
    }))
}

async fn get_media_file(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Path(media_id): Path<i64>,
) -> AppResult<Response> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let media = fetch_one(
        &conn,
        "SELECT file_path, mime_type, original_filename FROM media WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
        &[&media_id, &current_user.id],
        |row| {
            Ok(FileInfo {
                file_path: row.get(0)?,
                mime_type: row.get(1)?,
                original_filename: row.get(2)?,
            })
        },
    )?
    .ok_or_else(|| AppError::NotFound("Media not found".to_string()))?;

    let full_path = ORIGINALS_DIR.join(&media.file_path);
    if !full_path.exists() {
        return Err(AppError::NotFound("File not found".to_string()));
    }

    serve_file(
        full_path,
        &media.mime_type.unwrap_or_else(|| "application/octet-stream".to_string()),
        Some(&media.original_filename),
    )
    .await
}

struct FileInfo {
    file_path: String,
    mime_type: Option<String>,
    original_filename: String,
}

async fn get_media_thumbnail(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Path(media_id): Path<i64>,
) -> AppResult<Response> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let thumbnail_path: Option<String> = fetch_one(
        &conn,
        "SELECT thumbnail_path FROM media WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
        &[&media_id, &current_user.id],
        |row| row.get(0),
    )?
    .ok_or_else(|| AppError::NotFound("Media not found".to_string()))?;

    let thumbnail_path =
        thumbnail_path.ok_or_else(|| AppError::NotFound("Thumbnail not available".to_string()))?;

    let full_path = THUMBNAILS_DIR.join(&thumbnail_path);
    if !full_path.exists() {
        return Err(AppError::NotFound("Thumbnail file not found".to_string()));
    }

    serve_file(full_path, "image/jpeg", None).await
}

async fn get_media_preview(
    State(state): State<AppState>,
    current_user: CurrentUser,
    headers: HeaderMap,
    Path(media_id): Path<i64>,
) -> AppResult<Response> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let media = fetch_one(
        &conn,
        "SELECT file_path, media_type, mime_type FROM media WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
        &[&media_id, &current_user.id],
        |row| {
            Ok(PreviewInfo {
                file_path: row.get(0)?,
                media_type: row.get(1)?,
                mime_type: row.get(2)?,
            })
        },
    )?
    .ok_or_else(|| AppError::NotFound("Media not found".to_string()))?;

    let original_path = ORIGINALS_DIR.join(&media.file_path);
    if !original_path.exists() {
        return Err(AppError::NotFound("File not found".to_string()));
    }

    // Videos served with range support
    if media.media_type == "video" {
        return serve_file_with_range(
            original_path,
            &media.mime_type.unwrap_or_else(|| "video/mp4".to_string()),
            &headers,
        )
        .await;
    }

    // Web-compatible images served as-is
    let web_compatible = ["image/jpeg", "image/png", "image/webp", "image/gif"];
    if let Some(ref mime) = media.mime_type {
        if web_compatible.contains(&mime.as_str()) {
            return serve_file(original_path.clone(), mime, None).await;
        }
    }

    // Generate preview for other formats
    let preview_filename = format!(
        "{}_preview.jpg",
        original_path.file_stem().unwrap().to_string_lossy()
    );
    let preview_path = PREVIEWS_DIR.join(current_user.id.to_string()).join(&preview_filename);

    if !preview_path.exists() {
        tokio::fs::create_dir_all(preview_path.parent().unwrap())
            .await
            .ok();
        generate_image_preview(&original_path, &preview_path, 2048, 90);
    }

    if preview_path.exists() {
        serve_file(preview_path, "image/jpeg", None).await
    } else {
        // Fall back to thumbnail
        let thumb_row: Option<Option<String>> = fetch_one(
            &conn,
            "SELECT thumbnail_path FROM media WHERE id = ?",
            &[&media_id],
            |row| row.get(0),
        )?;

        if let Some(Some(thumbnail_path)) = thumb_row {
            let thumb_full = THUMBNAILS_DIR.join(&thumbnail_path);
            if thumb_full.exists() {
                return serve_file(thumb_full, "image/jpeg", None).await;
            }
        }

        Err(AppError::NotFound("Preview not available".to_string()))
    }
}

struct PreviewInfo {
    file_path: String,
    media_type: String,
    mime_type: Option<String>,
}

async fn serve_file(
    path: std::path::PathBuf,
    content_type: &str,
    filename: Option<&str>,
) -> AppResult<Response> {
    let file = File::open(&path).await?;
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type);

    if let Some(name) = filename {
        response = response.header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", name),
        );
    }

    response
        .body(body)
        .map_err(|e| AppError::Internal(e.to_string()))
}

async fn serve_file_with_range(
    path: std::path::PathBuf,
    content_type: &str,
    headers: &HeaderMap,
) -> AppResult<Response> {
    let metadata = tokio::fs::metadata(&path).await?;
    let file_size = metadata.len();

    // Parse Range header
    let range_header = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("bytes="));

    if let Some(range_str) = range_header {
        // Parse range like "0-1023" or "1024-" or "-500"
        let (start, end) = parse_range(range_str, file_size);

        let mut file = File::open(&path).await?;
        file.seek(std::io::SeekFrom::Start(start)).await?;

        let length = end - start + 1;
        let stream = ReaderStream::new(file.take(length));
        let body = Body::from_stream(stream);

        Response::builder()
            .status(StatusCode::PARTIAL_CONTENT)
            .header(header::CONTENT_TYPE, content_type)
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_LENGTH, length)
            .header(
                header::CONTENT_RANGE,
                format!("bytes {}-{}/{}", start, end, file_size),
            )
            .body(body)
            .map_err(|e| AppError::Internal(e.to_string()))
    } else {
        // No range requested, serve full file with Accept-Ranges header
        let file = File::open(&path).await?;
        let stream = ReaderStream::new(file);
        let body = Body::from_stream(stream);

        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type)
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_LENGTH, file_size)
            .body(body)
            .map_err(|e| AppError::Internal(e.to_string()))
    }
}

fn parse_range(range_str: &str, file_size: u64) -> (u64, u64) {
    let parts: Vec<&str> = range_str.split('-').collect();
    if parts.len() != 2 {
        return (0, file_size - 1);
    }

    let start = if parts[0].is_empty() {
        // "-500" means last 500 bytes
        let suffix_len: u64 = parts[1].parse().unwrap_or(0);
        file_size.saturating_sub(suffix_len)
    } else {
        parts[0].parse().unwrap_or(0)
    };

    let end = if parts[1].is_empty() {
        // "1024-" means from 1024 to end
        file_size - 1
    } else {
        parts[1].parse().unwrap_or(file_size - 1)
    };

    // Ensure valid range
    let start = start.min(file_size.saturating_sub(1));
    let end = end.min(file_size - 1).max(start);

    (start, end)
}
