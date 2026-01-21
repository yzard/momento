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
use crate::database::{execute_query, fetch_all, fetch_one, queries, DbConn};
use crate::error::{AppError, AppResult};
use crate::models::{MediaResponse, ShareVerifyRequest};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/public/share/:token", get(get_shared_content))
        .route("/public/share/:token/verify", post(verify_share_password))
        .route(
            "/public/share/:token/media/:media_id",
            get(get_shared_media_file),
        )
        .route(
            "/public/share/:token/thumbnail/:media_id",
            get(get_shared_thumbnail),
        )
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

fn validate_share_token(conn: &DbConn, token: &str, password: Option<&str>) -> AppResult<ShareRow> {
    let share = fetch_one(conn, queries::share::SELECT_BY_TOKEN, &[&token], |row| {
        Ok(ShareRow {
            id: row.get(0)?,
            media_id: row.get(1)?,
            album_id: row.get(2)?,
            password_hash: row.get(3)?,
            expires_at: row.get(4)?,
        })
    })?
    .ok_or_else(|| AppError::NotFound("Share link not found".to_string()))?;

    // Check expiration
    if let Some(expires_at) = &share.expires_at {
        if let Ok(dt) = DateTime::parse_from_rfc3339(expires_at) {
            if dt.with_timezone(&Utc) < Utc::now() {
                return Err(AppError::NotFound("Share link expired".to_string()));
            }
        }
    }

    // Check password
    if share.password_hash.is_some() {
        if let Some(pwd) = password {
            if !verify_password(pwd, share.password_hash.as_ref().unwrap()) {
                return Err(AppError::Authentication("Invalid password".to_string()));
            }
        } else {
            return Err(AppError::Authentication("Password required".to_string()));
        }
    }

    Ok(share)
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
        lens_make: row.get(14)?,
        lens_model: row.get(15)?,
        iso: row.get(16)?,
        exposure_time: row.get(17)?,
        f_number: row.get(18)?,
        focal_length: row.get(19)?,
        focal_length_35mm: row.get(20)?,
        gps_altitude: row.get(21)?,
        location_city: row.get(22)?,
        location_state: row.get(23)?,
        location_country: row.get(24)?,
        video_codec: row.get(25)?,
        keywords: row.get(26)?,
        created_at: row.get(27)?,
    })
}

async fn get_shared_content(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Query(query): Query<PasswordQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let share = validate_share_token(&conn, &token, query.password.as_deref())?;

    // Increment view count
    let _ = execute_query(&conn, queries::share::INCREMENT_VIEW_COUNT, &[&share.id]);

    if let Some(media_id) = share.media_id {
        let media = fetch_one(
            &conn,
            queries::media::SELECT_BY_ID,
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
            queries::public::SELECT_ALBUM_BASIC,
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
            queries::public::SELECT_ALBUM_MEDIA,
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

async fn verify_share_password(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Json(request): Json<ShareVerifyRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let share = fetch_one(
        &conn,
        queries::share::SELECT_PASSWORD_HASH,
        &[&token],
        |row| row.get::<_, Option<String>>(0),
    )?
    .ok_or_else(|| AppError::NotFound("Share link not found".to_string()))?;

    if share.is_none() {
        return Ok(Json(serde_json::json!({
            "valid": true,
            "message": "No password required"
        })));
    }

    let password = request.password.clone();
    if verify_password(&password, share.as_ref().unwrap()) {
        return Ok(Json(serde_json::json!({
            "valid": true,
            "message": "Password correct"
        })));
    }

    Ok(Json(serde_json::json!({
        "valid": false,
        "message": "Invalid password"
    })))
}

async fn get_shared_media_file(
    State(state): State<AppState>,
    Path((token, media_id)): Path<(String, i64)>,
    Query(query): Query<PasswordQuery>,
) -> AppResult<Response> {
    let conn = state.pool.get().map_err(AppError::Pool)?;
    let share = validate_share_token(&conn, &token, query.password.as_deref())?;

    // Verify media is in share
    if let Some(share_media_id) = share.media_id {
        if share_media_id != media_id {
            return Err(AppError::Authorization("Media not in share".to_string()));
        }
    }

    if let Some(album_id) = share.album_id {
        let in_album = fetch_one(
            &conn,
            queries::public::CHECK_ALBUM_MEDIA,
            &[&album_id, &media_id],
            |row| row.get::<_, i32>(0),
        )?;

        if in_album.is_none() {
            return Err(AppError::Authorization(
                "Media not in shared album".to_string(),
            ));
        }
    }

    let media = fetch_one(
        &conn,
        queries::public::SELECT_MEDIA_FILE_INFO,
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
        &media
            .mime_type
            .unwrap_or_else(|| "application/octet-stream".to_string()),
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
    let conn = state.pool.get().map_err(AppError::Pool)?;

    // We need password to access thumbnails too
    let password = query.password.as_deref();
    let share = validate_share_token(&conn, &token, password)?;

    // Verify media is in share
    if let Some(share_media_id) = share.media_id {
        if share_media_id != media_id {
            return Err(AppError::Authorization("Media not in share".to_string()));
        }
    }

    if let Some(album_id) = share.album_id {
        let in_album = fetch_one(
            &conn,
            queries::public::CHECK_ALBUM_MEDIA,
            &[&album_id, &media_id],
            |row| row.get::<_, i32>(0),
        )?;

        if in_album.is_none() {
            return Err(AppError::Authorization(
                "Media not in shared album".to_string(),
            ));
        }
    }

    let thumbnail_path: Option<String> = fetch_one(
        &conn,
        queries::public::SELECT_MEDIA_THUMBNAIL,
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
