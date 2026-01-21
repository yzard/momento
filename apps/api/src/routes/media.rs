use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use chrono::{Datelike, NaiveDateTime, Utc};
use indexmap::IndexMap;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

use crate::auth::{AppState, CurrentUser};
use crate::constants::{ORIGINALS_DIR, PREVIEWS_DIR, THUMBNAILS_DIR};
use crate::database::{execute_query, fetch_all, fetch_one, queries};
use crate::error::{AppError, AppResult};
use crate::models::{
    DeleteMediaResponse, MediaDeleteRequest, MediaGetRequest, MediaListRequest, MediaListResponse,
    MediaResponse, MediaUpdateRequest, PreviewBatchRequest, PreviewBatchResponse,
    ThumbnailBatchRequest, ThumbnailBatchResponse,
};
use crate::processor::thumbnails::generate_image_preview;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use std::collections::HashMap;
use std::path::PathBuf;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/media/list", post(list_media))
        .route("/media/get", post(get_media))
        .route("/media/update", post(update_media))
        .route("/media/delete", post(delete_media))
        .route("/media/file/:media_id", get(get_media_file))
}

pub fn thumbnail_router() -> Router<AppState> {
    Router::new().route("/thumbnail/get", post(get_media_thumbnail_batch))
}

pub fn preview_router() -> Router<AppState> {
    Router::new().route("/preview/get", post(get_media_preview_batch))
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
    lens_make: Option<String>,
    lens_model: Option<String>,
    iso: Option<i32>,
    exposure_time: Option<String>,
    f_number: Option<f64>,
    focal_length: Option<f64>,
    focal_length_35mm: Option<f64>,
    gps_altitude: Option<f64>,
    location_city: Option<String>,
    location_state: Option<String>,
    location_country: Option<String>,
    video_codec: Option<String>,
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
        lens_make,
        lens_model,
        iso,
        exposure_time,
        f_number,
        focal_length,
        focal_length_35mm,
        gps_altitude,
        location_city,
        location_state,
        location_country,
        video_codec,
        keywords,
        created_at,
        content_hash: None,
    }
}

async fn list_media(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<MediaListRequest>,
) -> AppResult<Json<MediaListResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    if let Some(group_by) = request.group_by.as_deref() {
        let limit = request.limit.unwrap_or(100).min(5000);
        let rows = fetch_timeline_rows(&conn, current_user.id, limit, request.cursor.as_deref())?;
        let has_more = rows.len() > limit as usize;
        let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

        let mut grouped: IndexMap<String, Vec<MediaResponse>> = IndexMap::new();
        for (media, date_taken) in &rows {
            let key = timeline_group_key(date_taken.as_deref(), group_by);
            grouped.entry(key).or_default().push(media.clone());
        }

        let groups: Vec<crate::models::TimelineGroup> = grouped
            .into_iter()
            .map(|(date, media)| crate::models::TimelineGroup { date, media })
            .collect();

        let next_cursor = if has_more && !rows.is_empty() {
            let (last, last_date) = rows.last().unwrap();
            last_date.as_ref().map(|dt| format!("{}_{}", dt, last.id))
        } else {
            None
        };

        return Ok(Json(MediaListResponse {
            items: vec![],
            next_cursor,
            has_more,
            groups: Some(groups),
        }));
    }

    if request.limit.is_none() && request.cursor.is_none() {
        let items = fetch_all(
            &conn,
            queries::media::SELECT_ALL_FOR_USER,
            &[&current_user.id],
            map_media_row,
        )?;

        return Ok(Json(MediaListResponse {
            items,
            next_cursor: None,
            has_more: false,
            groups: None,
        }));
    }

    let limit = request.limit.unwrap_or(100).min(5000);
    let rows = if let Some(ref cursor) = request.cursor {
        let parts: Vec<&str> = cursor.split('_').collect();
        if parts.len() == 2 {
            let cursor_date = parts[0];
            let cursor_id: i64 = parts[1].parse().unwrap_or(0);
            fetch_all(
                &conn,
                queries::media::SELECT_PAGINATED_FOR_USER,
                &[
                    &current_user.id,
                    &cursor_date,
                    &cursor_date,
                    &cursor_id,
                    &(limit + 1),
                ],
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
        groups: None,
    }))
}

async fn get_media(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<MediaGetRequest>,
) -> AppResult<Json<MediaResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let media = fetch_one(
        &conn,
        queries::media::SELECT_BY_ID_AND_USER,
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

    let exists = fetch_one(
        &conn,
        queries::media::CHECK_EXISTS,
        &[&request.media_id, &current_user.id],
        |row| row.get::<_, i64>(0),
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

    if let Some(gps_latitude) = request.gps_latitude {
        updates.push("gps_latitude = ?");
        params.push(Box::new(gps_latitude));
    }

    if let Some(gps_longitude) = request.gps_longitude {
        updates.push("gps_longitude = ?");
        params.push(Box::new(gps_longitude));
    }

    if !updates.is_empty() {
        params.push(Box::new(request.media_id));
        
        let sql = format!(
            "UPDATE media SET {} WHERE id = ?",
            updates.join(", ")
        );
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        execute_query(&conn, &sql, &param_refs)?;
    }

    let media = fetch_one(
        &conn,
        queries::media::SELECT_BY_ID_AND_USER,
        &[&request.media_id, &current_user.id],
        map_media_row,
    )?
    .ok_or_else(|| AppError::NotFound("Media not found".to_string()))?;

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
        queries::media::CHECK_EXISTS,
        &[&request.media_id, &current_user.id],
        |row| row.get::<_, i64>(0),
    )?;

    if exists.is_none() {
        return Err(AppError::NotFound("Media not found".to_string()));
    }

    let deleted_at = Utc::now().to_rfc3339();
    execute_query(
        &conn,
        queries::media::UPDATE_DELETED_AT,
        &[&deleted_at, &request.media_id, &current_user.id],
    )?;

    Ok(Json(DeleteMediaResponse {
        message: "Media deleted".to_string(),
    }))
}

async fn get_media_file(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Path(media_id): Path<i64>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let media = fetch_one(
        &conn,
        queries::media::SELECT_FILE_INFO,
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

    serve_file_with_range(
        full_path,
        &media
            .mime_type
            .unwrap_or_else(|| "application/octet-stream".to_string()),
        &headers,
        Some(&media.original_filename),
    )
    .await
}

fn fetch_default_media(
    conn: &crate::database::DbConn,
    user_id: i64,
    limit: i32,
) -> AppResult<Vec<MediaResponse>> {
    fetch_all(
        conn,
        queries::media::SELECT_PAGINATED_FOR_USER,
        &[
            &user_id,
            &Utc::now().to_rfc3339(),
            &Utc::now().to_rfc3339(),
            &i64::MAX,
            &(limit + 1)
        ],
        map_media_row,
    ).or_else(|_| {
        let future_date = "9999-12-31T23:59:59";
        fetch_all(
            conn,
            queries::media::SELECT_PAGINATED_FOR_USER,
            &[
                &user_id,
                &future_date,
                &future_date,
                &i64::MAX,
                &(limit + 1)
            ],
            map_media_row,
        )
    })
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
        row.get(23)?,
        row.get(24)?,
        row.get(25)?,
        row.get(26)?,
        row.get(27)?,
    ))
}

fn timeline_group_key(date_taken: Option<&str>, group_by: &str) -> String {
    let date_taken = match date_taken {
        Some(dt) => dt,
        None => return "Unknown".to_string(),
    };

    let dt = if let Ok(dt) = NaiveDateTime::parse_from_str(date_taken, "%Y-%m-%dT%H:%M:%S") {
        dt
    } else if let Ok(dt) =
        NaiveDateTime::parse_from_str(&date_taken.replace("Z", ""), "%Y-%m-%dT%H:%M:%S%.f")
    {
        dt
    } else if date_taken.len() >= 10 {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(&date_taken[..10], "%Y-%m-%d") {
            d.and_hms_opt(0, 0, 0).unwrap()
        } else {
            return "Unknown".to_string();
        }
    } else {
        return "Unknown".to_string();
    };

    match group_by {
        "year" => dt.year().to_string(),
        "month" => format!("{}-{:02}", dt.year(), dt.month()),
        "week" => {
            let week = dt.iso_week();
            format!("{}-W{:02}", week.year(), week.week())
        }
        _ => date_taken.chars().take(10).collect(),
    }
}

fn fetch_timeline_rows(
    conn: &crate::database::DbConn,
    user_id: i64,
    limit: i32,
    cursor: Option<&str>,
) -> AppResult<Vec<(MediaResponse, Option<String>)>> {
    if let Some(cursor) = cursor {
        let parts: Vec<&str> = cursor.split('_').collect();
        if parts.len() == 2 {
            let cursor_date = parts[0];
            let cursor_id: i64 = parts[1].parse().unwrap_or(0);
            return fetch_all(
                conn,
                queries::timeline::SELECT_PAGINATED,
                &[
                    &user_id,
                    &cursor_date,
                    &cursor_date,
                    &cursor_id,
                    &(limit + 1),
                ],
                map_timeline_row,
            );
        }
    }

    fetch_default_timeline(conn, user_id, limit)
}

fn fetch_default_timeline(
    conn: &crate::database::DbConn,
    user_id: i64,
    limit: i32,
) -> AppResult<Vec<(MediaResponse, Option<String>)>> {
    fetch_all(
        conn,
        queries::timeline::SELECT_DEFAULT,
        &[&user_id, &(limit + 1)],
        map_timeline_row,
    )
}

fn map_timeline_row(row: &rusqlite::Row) -> rusqlite::Result<(MediaResponse, Option<String>)> {
    let date_taken: Option<String> = row.get(9)?;
    let media = row_to_media_response(
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        date_taken.clone(),
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
        row.get(23)?,
        row.get(24)?,
        row.get(25)?,
        row.get(26)?,
        row.get(27)?,
    );

    Ok((media, date_taken))
}

struct FileInfo {
    file_path: String,
    mime_type: Option<String>,
    original_filename: String,
}

async fn get_media_thumbnail_batch(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<ThumbnailBatchRequest>,
) -> AppResult<Json<ThumbnailBatchResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;
    if request.media_ids.is_empty() {
        return Ok(Json(ThumbnailBatchResponse {
            thumbnails: HashMap::new(),
        }));
    }

    let rows: Vec<(i64, Option<String>, String, String, i64)> = fetch_all(
        &conn,
        queries::media::SELECT_THUMBNAIL_BATCH,
        &[&current_user.id],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        },
    )?;

    let requested_ids: std::collections::HashSet<i64> = request.media_ids.into_iter().collect();
    let rows = rows
        .into_iter()
        .filter(|(id, _, _, _, _)| requested_ids.contains(id))
        .collect::<Vec<_>>();

    let mut thumbnails: HashMap<i64, Option<String>> = HashMap::new();

    for (media_id, thumbnail_path, file_path, _media_type, _user_id) in rows {
        let stem = PathBuf::from(&file_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("thumb")
            .to_string();

        let thumbnail_relative = thumbnail_path
            .clone()
            .unwrap_or_else(|| {
                 let parent = PathBuf::from(&file_path)
                     .parent()
                     .and_then(|p| p.file_name())
                     .and_then(|n| n.to_str())
                     .unwrap_or("unknown")
                     .to_string();
                 format!("{}/{}.jpg", parent, stem)
            });
            
        let full_path = THUMBNAILS_DIR.join(&thumbnail_relative);

        if full_path.exists() {
            if let Ok(data) = tokio::fs::read(&full_path).await {
                let encoded = STANDARD.encode(data);
                thumbnails.insert(
                    media_id,
                    Some(format!("data:image/jpeg;base64,{}", encoded)),
                );
                continue;
            }
        }

        thumbnails.insert(media_id, None);
    }

    Ok(Json(ThumbnailBatchResponse { thumbnails }))
}

async fn get_media_preview_batch(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<PreviewBatchRequest>,
) -> AppResult<Json<PreviewBatchResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;
    if request.ids.is_empty() {
        return Ok(Json(PreviewBatchResponse {
            previews: HashMap::new(),
        }));
    }

    let rows: Vec<(i64, String, String, Option<String>)> = fetch_all(
        &conn,
        queries::media::SELECT_PREVIEW_BATCH,
        &[&current_user.id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;

    let requested_ids: std::collections::HashSet<i64> = request.ids.into_iter().collect();
    let rows = rows
        .into_iter()
        .filter(|(id, _, _, _)| requested_ids.contains(id))
        .collect::<Vec<_>>();

    let mut previews: HashMap<i64, Option<String>> = HashMap::new();

    for (media_id, file_path, media_type, mime_type) in rows {
        let original_path = ORIGINALS_DIR.join(&file_path);
        if !original_path.exists() {
            previews.insert(media_id, None);
            continue;
        }

        if media_type == "video" {
            previews.insert(media_id, None);
            continue;
        }

        let web_compatible = ["image/jpeg", "image/png", "image/webp", "image/gif"];
        if let Some(ref mime) = mime_type {
            if web_compatible.contains(&mime.as_str()) {
                if let Ok(data) = tokio::fs::read(&original_path).await {
                    let encoded = STANDARD.encode(data);
                    previews.insert(media_id, Some(format!("data:{};base64,{}", mime, encoded)));
                    continue;
                }
            }
        }

        let preview_filename = format!(
            "{}_preview.jpg",
            original_path.file_stem().unwrap().to_string_lossy()
        );
        let preview_path = PREVIEWS_DIR
            .join(current_user.id.to_string())
            .join(&preview_filename);

        if !preview_path.exists() {
            tokio::fs::create_dir_all(preview_path.parent().unwrap())
                .await
                .ok();
            generate_image_preview(&original_path, &preview_path, 2048, 90).await;
        }

        if preview_path.exists() {
            if let Ok(data) = tokio::fs::read(&preview_path).await {
                let encoded = STANDARD.encode(data);
                previews.insert(
                    media_id,
                    Some(format!("data:image/jpeg;base64,{}", encoded)),
                );
                continue;
            }
        }

        previews.insert(media_id, None);
    }

    Ok(Json(PreviewBatchResponse { previews }))
}

async fn serve_file_with_range(
    path: std::path::PathBuf,
    content_type: &str,
    headers: &HeaderMap,
    filename: Option<&str>,
) -> AppResult<Response> {
    let metadata = tokio::fs::metadata(&path).await?;
    let file_size = metadata.len();

    let range_header = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("bytes="));

    if let Some(range_str) = range_header {
        let (start, end) = parse_range(range_str, file_size);

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
            .header(
                header::CONTENT_RANGE,
                format!("bytes {}-{}/{}", start, end, file_size),
            );

        if let Some(name) = filename {
            response = response.header(
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", name),
            );
        }

        response
            .body(body)
            .map_err(|e| AppError::Internal(e.to_string()))
    } else {
        let file = File::open(&path).await?;
        let stream = ReaderStream::new(file);
        let body = Body::from_stream(stream);

        let mut response = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type)
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_LENGTH, file_size);

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
}

fn parse_range(range_str: &str, file_size: u64) -> (u64, u64) {
    let parts: Vec<&str> = range_str.split('-').collect();
    if parts.len() != 2 {
        return (0, file_size - 1);
    }

    let start = if parts[0].is_empty() {
        let suffix_len: u64 = parts[1].parse().unwrap_or(0);
        file_size.saturating_sub(suffix_len)
    } else {
        parts[0].parse().unwrap_or(0)
    };

    let end = if parts[1].is_empty() {
        file_size - 1
    } else {
        parts[1].parse().unwrap_or(file_size - 1)
    };

    let start = start.min(file_size.saturating_sub(1));
    let end = end.min(file_size - 1).max(start);

    (start, end)
}
