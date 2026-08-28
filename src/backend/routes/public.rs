use axum::{
    extract::{ConnectInfo, Path, State},
    http::{header, HeaderMap, HeaderValue},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use std::net::SocketAddr;

use crate::auth::{
    create_share_session_token, decode_share_session_token, share_token_hash, AppState,
};
use crate::constants::paths;
use crate::database::{execute_query, fetch_all, fetch_one, queries, DbConn};
use crate::error::{AppError, AppResult};
use crate::models::{map_media_response, ShareVerifyRequest, ShareVerifyResponse};
use crate::utils::path::resolve_existing_storage_path;

use super::file_stream::{serve_file, ContentDisposition};

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

const SHARE_SESSION_COOKIE_NAME: &str = "momento_share_session";

struct ShareRow {
    id: i64,
    media_id: Option<i64>,
    album_id: Option<i64>,
    password_hash: Option<String>,
    expires_at: Option<String>,
}

fn validate_share_access(
    conn: &DbConn,
    token: &str,
    headers: &HeaderMap,
    state: &AppState,
) -> AppResult<ShareRow> {
    let share = load_active_share(conn, token)?;
    if share.password_hash.is_none() {
        return Ok(share);
    }

    let session_token = cookie_value(headers, SHARE_SESSION_COOKIE_NAME).ok_or_else(|| {
        AppError::Authentication("Share password verification is required".to_string())
    })?;
    let config = state.config.current();
    let claims = decode_share_session_token(&session_token, &config)
        .ok_or_else(|| AppError::Authentication("Invalid or expired share session".to_string()))?;
    if claims.share_id != share.id || claims.share_token_hash != share_token_hash(token) {
        return Err(AppError::Authentication(
            "Share session does not match this link".to_string(),
        ));
    }
    Ok(share)
}

fn load_active_share(conn: &DbConn, token: &str) -> AppResult<ShareRow> {
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

    if let Some(expires_at) = &share.expires_at {
        let expires_at = DateTime::parse_from_rfc3339(expires_at)
            .map_err(|_| AppError::Internal("Share link has an invalid expiration".to_string()))?;
        if expires_at.with_timezone(&Utc) <= Utc::now() {
            return Err(AppError::NotFound("Share link expired".to_string()));
        }
    }

    Ok(share)
}

struct AlbumBasic {
    id: i64,
    name: String,
    description: Option<String>,
}

fn require_media_in_share(connection: &DbConn, share: &ShareRow, media_id: i64) -> AppResult<()> {
    if share
        .media_id
        .is_some_and(|shared_media_id| shared_media_id != media_id)
    {
        return Err(AppError::Authorization("Media not in share".to_string()));
    }

    let Some(album_id) = share.album_id else {
        return Ok(());
    };
    let media_is_in_album = fetch_one(
        connection,
        queries::public::CHECK_ALBUM_MEDIA,
        &[&album_id, &media_id],
        |row| row.get::<_, i32>(0),
    )?;
    if media_is_in_album.is_none() {
        return Err(AppError::Authorization(
            "Media not in shared album".to_string(),
        ));
    }
    Ok(())
}

async fn get_shared_content(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let conn = state.pool.get().map_err(AppError::Pool)?;
    let share = validate_share_access(&conn, &token, &headers, &state)?;

    // Increment view count
    let _ = execute_query(&conn, queries::share::INCREMENT_VIEW_COUNT, &[&share.id]);

    if let Some(media_id) = share.media_id {
        let media = fetch_one(
            &conn,
            queries::media::SELECT_BY_ID,
            &[&media_id],
            map_media_response,
        )?
        .ok_or_else(|| AppError::NotFound("Media not found".to_string()))?;

        return Ok(public_json_response(serde_json::json!({
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
            map_media_response,
        )?;

        return Ok(public_json_response(serde_json::json!({
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
    peer_address: Option<ConnectInfo<SocketAddr>>,
    Path(token): Path<String>,
    headers: HeaderMap,
    Json(request): Json<ShareVerifyRequest>,
) -> AppResult<Response> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let share = load_active_share(&conn, &token)?;

    let Some(password_hash) = share.password_hash else {
        return share_verify_response(true, "No password required", None);
    };

    let password_identity = format!("share:{}", share_token_hash(&token));
    let client_source = state
        .authentication_protection
        .client_source(&headers, peer_address.map(|peer| peer.0));
    state
        .authentication_protection
        .begin_password_attempt(&client_source, &password_identity)?;
    if !state
        .authentication_protection
        .verify_password(&request.password, Some(&password_hash))
        .await?
    {
        return share_verify_response(false, "Invalid password", None);
    }
    state
        .authentication_protection
        .record_password_success(&client_source, &password_identity);

    let share_expiration = parse_share_expiration(share.expires_at.as_deref())?;
    let config = state.config.current();
    let (session_token, expires_at) =
        create_share_session_token(share.id, &token, share_expiration, &config)?;
    let maximum_age = (expires_at - Utc::now()).num_seconds().max(1);
    let cookie = format!(
        "{SHARE_SESSION_COOKIE_NAME}={session_token}; Path=/api/v1/public/share/{token}; Max-Age={maximum_age}; HttpOnly; Secure; SameSite=Strict"
    );
    share_verify_response(true, "Password correct", Some(&cookie))
}

async fn get_shared_media_file(
    State(state): State<AppState>,
    Path((token, media_id)): Path<(String, i64)>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let conn = state.pool.get().map_err(AppError::Pool)?;
    let share = validate_share_access(&conn, &token, &headers, &state)?;
    require_media_in_share(&conn, &share, media_id)?;

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

    let full_path = resolve_existing_storage_path(&paths().originals, &media.file_path).await?;

    serve_file(
        full_path,
        &media
            .mime_type
            .unwrap_or_else(|| "application/octet-stream".to_string()),
        &headers,
        Some(&media.original_filename),
        true,
        ContentDisposition::Attachment,
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
    headers: HeaderMap,
) -> AppResult<Response> {
    let conn = state.pool.get().map_err(AppError::Pool)?;
    let share = validate_share_access(&conn, &token, &headers, &state)?;
    require_media_in_share(&conn, &share, media_id)?;

    let thumbnail_path: Option<String> = fetch_one(
        &conn,
        queries::public::SELECT_MEDIA_THUMBNAIL,
        &[&media_id],
        |row| row.get(0),
    )?
    .ok_or_else(|| AppError::NotFound("Media not found".to_string()))?;

    let thumbnail_path =
        thumbnail_path.ok_or_else(|| AppError::NotFound("Thumbnail not available".to_string()))?;

    let full_path = resolve_existing_storage_path(&paths().thumbnails, &thumbnail_path).await?;

    serve_file(
        full_path,
        "image/jpeg",
        &headers,
        None,
        false,
        ContentDisposition::Inline,
    )
    .await
}

fn parse_share_expiration(expires_at: Option<&str>) -> AppResult<Option<DateTime<Utc>>> {
    expires_at
        .map(|expiration| {
            DateTime::parse_from_rfc3339(expiration)
                .map(|parsed| parsed.with_timezone(&Utc))
                .map_err(|_| AppError::Internal("Share link has an invalid expiration".to_string()))
        })
        .transpose()
}

fn cookie_value(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|header_value| header_value.to_str().ok())
        .flat_map(|cookies| cookies.split(';'))
        .filter_map(|cookie| cookie.trim().split_once('='))
        .find_map(|(name, value)| (name == cookie_name).then(|| value.to_string()))
}

fn share_verify_response(
    valid: bool,
    message: &str,
    set_cookie: Option<&str>,
) -> AppResult<Response> {
    let mut response = Json(ShareVerifyResponse {
        valid,
        message: message.to_string(),
    })
    .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Some(cookie) = set_cookie {
        let cookie = HeaderValue::from_str(cookie)
            .map_err(|_| AppError::Internal("Failed to create share session cookie".to_string()))?;
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    Ok(response)
}

fn public_json_response(body: serde_json::Value) -> Response {
    let mut response = Json(body).into_response();
    response
        .headers_mut()
        .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    response
}
