use axum::{extract::State, routing::post, Json, Router};
use chrono::{Duration, Utc};
use rand::Rng;

use crate::auth::{AppState, CurrentUser};
use crate::database::{execute_query, fetch_all, fetch_one, insert_returning_id, queries};
use crate::error::{AppError, AppResult};
use crate::models::{
    ShareAlbumRequest, ShareCreateRequest, ShareDeleteRequest, ShareLinkResponse,
    ShareListResponse, ShareMediaRequest,
};

const MIN_ACCESS_LEVEL: i32 = 1;
const MAX_ACCESS_LEVEL: i32 = 2;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/share/create", post(create_share_link))
        .route("/share/list", post(list_share_links))
        .route("/share/delete", post(delete_share_link))
        .route("/share/media", post(share_media_with_user))
        .route("/share/album", post(share_album_with_user))
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

    if request
        .password
        .as_ref()
        .is_some_and(|password| password.trim().is_empty())
    {
        return Err(AppError::BadRequest("Password cannot be empty".to_string()));
    }

    if request.expires_in_days.is_some_and(|days| days <= 0) {
        return Err(AppError::BadRequest(
            "Expiration must be at least one day".to_string(),
        ));
    }

    let password_hash = match request.password.as_deref() {
        Some(password) => Some(
            state
                .authentication_protection
                .hash_password(password)
                .await?,
        ),
        None => None,
    };

    let conn = state.pool.get().map_err(AppError::Pool)?;

    if let Some(media_id) = request.media_id {
        let exists = fetch_one(
            &conn,
            queries::share::CHECK_MEDIA_OWNERSHIP,
            &[&media_id, &current_user.id],
            |row| row.get::<_, i64>(0),
        )?;

        if exists.is_none() {
            return Err(AppError::NotFound("Media not found".to_string()));
        }
    }

    if let Some(album_id) = request.album_id {
        let exists = fetch_one(
            &conn,
            queries::share::CHECK_ALBUM_OWNERSHIP,
            &[&album_id, &current_user.id],
            |row| row.get::<_, i64>(0),
        )?;

        if exists.is_none() {
            return Err(AppError::NotFound("Album not found".to_string()));
        }
    }

    let token: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(22)
        .map(char::from)
        .collect();

    let expires_at = request
        .expires_in_days
        .map(|days| (Utc::now() + Duration::days(days as i64)).to_rfc3339());

    let share_id = insert_returning_id(
        &conn,
        queries::share::INSERT,
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
        queries::share::SELECT_BY_ID,
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
        queries::share::SELECT_ALL_FOR_USER,
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
        queries::share::CHECK_OWNERSHIP,
        &[&request.share_id, &current_user.id],
        |row| row.get::<_, i64>(0),
    )?;

    if exists.is_none() {
        return Err(AppError::NotFound("Share link not found".to_string()));
    }

    execute_query(&conn, queries::share::DELETE, &[&request.share_id])?;

    Ok(Json(
        serde_json::json!({"message": "Share link deleted successfully"}),
    ))
}

async fn share_media_with_user(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<ShareMediaRequest>,
) -> AppResult<Json<serde_json::Value>> {
    validate_access_grant(
        &request.access_level,
        current_user.id,
        request.target_user_id,
    )?;
    let conn = state.pool.get().map_err(AppError::Pool)?;

    ensure_active_target_user(&conn, request.target_user_id)?;

    let access_level: i32 = fetch_one(
        &conn,
        queries::access::CHECK_MEDIA_ACCESS,
        &[&request.media_id, &current_user.id],
        |row| row.get(0),
    )?
    .ok_or_else(|| AppError::NotFound("Media not found".to_string()))?;

    if access_level < 2 {
        return Err(AppError::Forbidden(
            "Insufficient permissions to share".to_string(),
        ));
    }

    execute_query(
        &conn,
        queries::access::UPSERT_SHARED_MEDIA_ACCESS,
        &[
            &request.media_id,
            &request.target_user_id,
            &request.access_level,
        ],
    )?;

    Ok(Json(
        serde_json::json!({"message": "Media shared successfully"}),
    ))
}

async fn share_album_with_user(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<ShareAlbumRequest>,
) -> AppResult<Json<serde_json::Value>> {
    validate_access_grant(
        &request.access_level,
        current_user.id,
        request.target_user_id,
    )?;
    let conn = state.pool.get().map_err(AppError::Pool)?;

    ensure_active_target_user(&conn, request.target_user_id)?;

    let exists = fetch_one(
        &conn,
        queries::albums::CHECK_OWNERSHIP,
        &[&request.album_id, &current_user.id],
        |row| row.get::<_, i64>(0),
    )?;

    if exists.is_none() {
        return Err(AppError::NotFound("Album not found".to_string()));
    }

    execute_query(
        &conn,
        queries::access::UPSERT_SHARED_ALBUM_ACCESS,
        &[
            &request.album_id,
            &request.target_user_id,
            &request.access_level,
        ],
    )?;

    Ok(Json(
        serde_json::json!({"message": "Album shared successfully"}),
    ))
}

fn validate_access_grant(access_level: &i32, user_id: i64, target_user_id: i64) -> AppResult<()> {
    if !(MIN_ACCESS_LEVEL..=MAX_ACCESS_LEVEL).contains(access_level) {
        return Err(AppError::BadRequest(
            "Access level must be 1 or 2".to_string(),
        ));
    }

    if user_id == target_user_id {
        return Err(AppError::BadRequest(
            "Cannot share with yourself".to_string(),
        ));
    }

    Ok(())
}

fn ensure_active_target_user(conn: &crate::database::DbConn, target_user_id: i64) -> AppResult<()> {
    let active = fetch_one(
        conn,
        queries::users::SELECT_BY_ID,
        &[&target_user_id],
        |row| row.get::<_, bool>(5),
    )?;

    if active != Some(true) {
        return Err(AppError::NotFound("Target user not found".to_string()));
    }

    Ok(())
}
