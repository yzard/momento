use axum::{extract::State, routing::post, Json, Router};
use chrono::{Duration, Utc};
use rand::Rng;

use crate::auth::{hash_password, AppState, CurrentUser};
use crate::database::{execute_query, fetch_all, fetch_one, insert_returning_id};
use crate::error::{AppError, AppResult};
use crate::models::{ShareCreateRequest, ShareDeleteRequest, ShareLinkResponse, ShareListResponse};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/share/create", post(create_share_link))
        .route("/share/list", post(list_share_links))
        .route("/share/delete", post(delete_share_link))
}

fn map_share_row(row: &rusqlite::Row) -> rusqlite::Result<ShareLinkResponse> {
    let password_hash: Option<String> = row.get(4)?;
    Ok(ShareLinkResponse {
        id: row.get(0)?,
        token: row.get(1)?,
        media_id: row.get(2)?,
        album_id: row.get(3)?,
        has_password: password_hash.is_some(),
        expires_at: row.get(5)?,
        view_count: row.get(6)?,
        created_at: row.get(7)?,
    })
}

async fn create_share_link(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<ShareCreateRequest>,
) -> AppResult<Json<ShareLinkResponse>> {
    if request.media_id.is_none() && request.album_id.is_none() {
        return Err(AppError::BadRequest(
            "Must specify media_id or album_id".to_string(),
        ));
    }

    if request.media_id.is_some() && request.album_id.is_some() {
        return Err(AppError::BadRequest(
            "Cannot specify both media_id and album_id".to_string(),
        ));
    }

    let conn = state.pool.get().map_err(AppError::Pool)?;

    if let Some(media_id) = request.media_id {
        let exists = fetch_one(
            &conn,
            "SELECT id FROM media WHERE id = ? AND user_id = ?",
            &[&media_id, &current_user.id],
            |row| Ok(row.get::<_, i64>(0)?),
        )?;

        if exists.is_none() {
            return Err(AppError::NotFound("Media not found".to_string()));
        }
    }

    if let Some(album_id) = request.album_id {
        let exists = fetch_one(
            &conn,
            "SELECT id FROM albums WHERE id = ? AND user_id = ?",
            &[&album_id, &current_user.id],
            |row| Ok(row.get::<_, i64>(0)?),
        )?;

        if exists.is_none() {
            return Err(AppError::NotFound("Album not found".to_string()));
        }
    }

    // Generate token
    let token: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(22)
        .map(char::from)
        .collect();

    let password_hash = request
        .password
        .as_ref()
        .map(|p| hash_password(p))
        .transpose()
        .map_err(|e| AppError::Internal(format!("Failed to hash password: {}", e)))?;

    let expires_at = request
        .expires_in_days
        .map(|days| (Utc::now() + Duration::days(days as i64)).to_rfc3339());

    let share_id = insert_returning_id(
        &conn,
        "INSERT INTO share_links (user_id, media_id, album_id, token, password_hash, expires_at) VALUES (?, ?, ?, ?, ?, ?)",
        &[
            &current_user.id,
            &request.media_id,
            &request.album_id,
            &token,
            &password_hash,
            &expires_at,
        ],
    )?;

    let share = fetch_one(
        &conn,
        "SELECT id, token, media_id, album_id, password_hash, expires_at, view_count, created_at FROM share_links WHERE id = ?",
        &[&share_id],
        map_share_row,
    )?
    .ok_or_else(|| AppError::Internal("Failed to create share link".to_string()))?;

    Ok(Json(share))
}

async fn list_share_links(
    State(state): State<AppState>,
    current_user: CurrentUser,
) -> AppResult<Json<ShareListResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let shares = fetch_all(
        &conn,
        "SELECT id, token, media_id, album_id, password_hash, expires_at, view_count, created_at FROM share_links WHERE user_id = ? ORDER BY created_at DESC",
        &[&current_user.id],
        map_share_row,
    )?;

    Ok(Json(ShareListResponse { shares }))
}

async fn delete_share_link(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<ShareDeleteRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let exists = fetch_one(
        &conn,
        "SELECT id FROM share_links WHERE id = ? AND user_id = ?",
        &[&request.share_id, &current_user.id],
        |row| Ok(row.get::<_, i64>(0)?),
    )?;

    if exists.is_none() {
        return Err(AppError::NotFound("Share link not found".to_string()));
    }

    execute_query(
        &conn,
        "DELETE FROM share_links WHERE id = ?",
        &[&request.share_id],
    )?;

    Ok(Json(serde_json::json!({"message": "Share link deleted successfully"})))
}
