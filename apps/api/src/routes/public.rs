use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tokio::fs::File;
use tokio_util::io::ReaderStream;

use crate::auth::{verify_password, AppState};
use crate::constants::{ORIGINALS_DIR, THUMBNAILS_DIR};
use crate::database::{execute_query, fetch_all, fetch_one};
use crate::error::{AppError, AppResult};
use crate::models::{MediaResponse, ShareVerifyRequest};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/public/share/:token", get(get_shared_content))
        .route("/public/share/:token/verify", post(verify_share_password))
        .route("/public/share/:token/media/:media_id", get(get_shared_media_file))
        .route("/public/share/:token/thumbnail/:media_id", get(get_shared_thumbnail))
}

#[derive(Deserialize)]
struct PasswordQuery {
    password: Option<String>,
}

struct ShareRow {
    id: i64,
    media_id: Option<i64>,
    album_id: Option<i64>,
    password_hash: Option<String>,
    expires_at: Option<String>,
}

fn validate_share_token(
    conn: &crate::database::DbConn,
    token: &str,
    password: Option<&str>,
) -> AppResult<ShareRow> {
    let share = fetch_one(
        conn,
        "SELECT id, media_id, album_id, password_hash, expires_at FROM share_links WHERE token = ?",
        &[&token],
        |row| {
            Ok(ShareRow {
                id: row.get(0)?,
                media_id: row.get(1)?,
                album_id: row.get(2)?,
                password_hash: row.get(3)?,
                expires_at: row.get(4)?,
            })
        },
    )?
    .ok_or_else(|| AppError::NotFound("Share link not found".to_string()))?;

    // Check expiry
    if let Some(ref expires_at) = share.expires_at {
        if let Ok(expires) = DateTime::parse_from_rfc3339(expires_at) {
            if Utc::now() > expires {
                return Err(AppError::Authentication("Share link has expired".to_string()));
            }
        }
    }

    // Check password
    if let Some(ref hash) = share.password_hash {
        let pwd = password.ok_or_else(|| AppError::Authentication("Password required".to_string()))?;
        if !verify_password(pwd, hash) {
            return Err(AppError::Authentication("Invalid password".to_string()));
        }
    }

    // Increment view count
    let _ = execute_query(
        conn,
        "UPDATE share_links SET view_count = view_count + 1 WHERE id = ?",
        &[&share.id],
    );

    Ok(share)
}

async fn get_shared_content(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Query(query): Query<PasswordQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let password = query
        .password
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("Password is required".to_string()))?;

    let conn = state.pool.get().map_err(AppError::Pool)?;
    let share = validate_share_token(&conn, &token, Some(password))?;

    if let Some(media_id) = share.media_id {
        let media = fetch_one(
            &conn,
            r#"
            SELECT id, filename, original_filename, media_type, mime_type, width, height,
                   file_size, duration_seconds, date_taken, gps_latitude, gps_longitude,
                   camera_make, camera_model, created_at
            FROM media WHERE id = ?
            "#,
            &[&media_id],
            map_public_media_row,
        )?
        .ok_or_else(|| AppError::NotFound("Media not found".to_string()))?;

        return Ok(Json(serde_json::json!({
            "type": "media",
            "media": media
        })));
    }

    if let Some(album_id) = share.album_id {
        let album = fetch_one(
            &conn,
            "SELECT id, name, description FROM albums WHERE id = ?",
            &[&album_id],
            |row| {
                Ok(AlbumBasic {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                })
            },
        )?
        .ok_or_else(|| AppError::NotFound("Album not found".to_string()))?;

        let media = fetch_all(
            &conn,
            r#"
            SELECT m.id, m.filename, m.original_filename, m.media_type, m.mime_type, m.width, m.height,
                   m.file_size, m.duration_seconds, m.date_taken, m.gps_latitude, m.gps_longitude,
                   m.camera_make, m.camera_model, m.created_at
            FROM media m
            JOIN album_media am ON m.id = am.media_id
            WHERE am.album_id = ?
            ORDER BY am.position
            "#,
            &[&album_id],
            map_public_media_row,
        )?;

        return Ok(Json(serde_json::json!({
            "type": "album",
            "album": {
                "id": album.id,
                "name": album.name,
                "description": album.description
            },
            "media": media
        })));
    }

    Err(AppError::Internal("Invalid share link".to_string()))
}

struct AlbumBasic {
    id: i64,
    name: String,
    description: Option<String>,
}

fn map_public_media_row(row: &rusqlite::Row) -> rusqlite::Result<MediaResponse> {
    Ok(MediaResponse {
        id: row.get(0)?,
        filename: row.get(1)?,
        original_filename: row.get(2)?,
        media_type: row.get(3)?,
        mime_type: row.get(4)?,
        width: row.get(5)?,
        height: row.get(6)?,
        file_size: row.get(7)?,
        duration_seconds: row.get(8)?,
        date_taken: row.get(9)?,
        gps_latitude: row.get(10)?,
        gps_longitude: row.get(11)?,
        camera_make: row.get(12)?,
        camera_model: row.get(13)?,
        iso: None,
        exposure_time: None,
        f_number: None,
        focal_length: None,
        gps_altitude: None,
        location_state: None,
        location_country: None,
        keywords: None,
        created_at: row.get(14)?,
    })
}

async fn verify_share_password(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Json(request): Json<ShareVerifyRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let share = fetch_one(
        &conn,
        "SELECT password_hash FROM share_links WHERE token = ?",
        &[&token],
        |row| Ok(row.get::<_, Option<String>>(0)?),
    )?
    .ok_or_else(|| AppError::NotFound("Share link not found".to_string()))?;

    if share.is_none() {
        return Ok(Json(serde_json::json!({
            "valid": true,
            "message": "No password required"
        })));
    }

    if verify_password(&request.password, &share.unwrap()) {
        return Ok(Json(serde_json::json!({"valid": true})));
    }

    Err(AppError::Authentication("Invalid password".to_string()))
}

async fn get_shared_media_file(
    State(state): State<AppState>,
    Path((token, media_id)): Path<(String, i64)>,
    Query(query): Query<PasswordQuery>,
) -> AppResult<Response> {
    let password = query
        .password
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("Password is required".to_string()))?;

    let conn = state.pool.get().map_err(AppError::Pool)?;
    let share = validate_share_token(&conn, &token, Some(password))?;

    // Verify media is in share
    if let Some(share_media_id) = share.media_id {
        if share_media_id != media_id {
            return Err(AppError::Authorization("Media not in share".to_string()));
        }
    }

    if let Some(album_id) = share.album_id {
        let in_album = fetch_one(
            &conn,
            "SELECT 1 FROM album_media WHERE album_id = ? AND media_id = ?",
            &[&album_id, &media_id],
            |row| Ok(row.get::<_, i32>(0)?),
        )?;

        if in_album.is_none() {
            return Err(AppError::Authorization("Media not in shared album".to_string()));
        }
    }

    let media = fetch_one(
        &conn,
        "SELECT file_path, mime_type, original_filename FROM media WHERE id = ?",
        &[&media_id],
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

async fn get_shared_thumbnail(
    State(state): State<AppState>,
    Path((token, media_id)): Path<(String, i64)>,
    Query(query): Query<PasswordQuery>,
) -> AppResult<Response> {
    let password = query
        .password
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("Password is required".to_string()))?;

    let conn = state.pool.get().map_err(AppError::Pool)?;
    let share = validate_share_token(&conn, &token, Some(password))?;

    // Verify media is in share
    if let Some(share_media_id) = share.media_id {
        if share_media_id != media_id {
            return Err(AppError::Authorization("Media not in share".to_string()));
        }
    }

    if let Some(album_id) = share.album_id {
        let in_album = fetch_one(
            &conn,
            "SELECT 1 FROM album_media WHERE album_id = ? AND media_id = ?",
            &[&album_id, &media_id],
            |row| Ok(row.get::<_, i32>(0)?),
        )?;

        if in_album.is_none() {
            return Err(AppError::Authorization("Media not in shared album".to_string()));
        }
    }

    let thumbnail_path: Option<String> = fetch_one(
        &conn,
        "SELECT thumbnail_path FROM media WHERE id = ?",
        &[&media_id],
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
