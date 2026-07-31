use axum::{extract::State, routing::post, Json, Router};

use crate::auth::{AppState, CurrentUser, RequireAdmin};
use crate::database::{execute_query, fetch_all, fetch_one, insert_returning_id, queries};
use crate::error::{AppError, AppResult};
use crate::models::{
    TagAddToMediaRequest, TagCreateRequest, TagDeleteRequest, TagListResponse,
    TagRemoveFromMediaRequest, TagResponse,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tag/list", post(list_tags))
        .route("/tag/create", post(create_tag))
        .route("/tag/delete", post(delete_tag))
        .route("/tag/add-to-media", post(add_tag_to_media))
        .route("/tag/remove-from-media", post(remove_tag_from_media))
}

fn map_tag_row(row: &rusqlite::Row) -> rusqlite::Result<TagResponse> {
    Ok(TagResponse {
        id: row.get(0)?,
        name: row.get(1)?,
        created_at: row.get(2)?,
    })
}

async fn list_tags(
    State(state): State<AppState>,
    _current_user: CurrentUser,
) -> AppResult<Json<TagListResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let tags = fetch_all(&conn, queries::tags::SELECT_ALL, &[], map_tag_row)?;

    Ok(Json(TagListResponse { tags }))
}

async fn create_tag(
    State(state): State<AppState>,
    _current_user: CurrentUser,
    Json(request): Json<TagCreateRequest>,
) -> AppResult<Json<TagResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    // Check existing
    let existing = fetch_one(
        &conn,
        queries::tags::SELECT_ID_BY_NAME,
        &[&request.name],
        |row| row.get::<_, i64>(0),
    )?;

    if existing.is_some() {
        return Err(AppError::BadRequest("Tag already exists".to_string()));
    }

    let tag_id = insert_returning_id(&conn, queries::tags::INSERT, &[&request.name])?;

    let tag = fetch_one(&conn, queries::tags::SELECT_BY_ID, &[&tag_id], map_tag_row)?
        .ok_or_else(|| AppError::Internal("Failed to create tag".to_string()))?;

    Ok(Json(tag))
}

async fn delete_tag(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(request): Json<TagDeleteRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let exists = fetch_one(
        &conn,
        queries::tags::CHECK_EXISTS,
        &[&request.tag_id],
        |row| row.get::<_, i64>(0),
    )?;

    if exists.is_none() {
        return Err(AppError::NotFound("Tag not found".to_string()));
    }

    execute_query(&conn, queries::tags::DELETE, &[&request.tag_id])?;

    Ok(Json(
        serde_json::json!({"message": "Tag deleted successfully"}),
    ))
}

async fn add_tag_to_media(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<TagAddToMediaRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let tag_exists = fetch_one(
        &conn,
        queries::tags::CHECK_EXISTS,
        &[&request.tag_id],
        |row| row.get::<_, i64>(0),
    )?;

    if tag_exists.is_none() {
        return Err(AppError::NotFound("Tag not found".to_string()));
    }

    for media_id in &request.media_ids {
        // Check media belongs to user
        let media_exists = fetch_one(
            &conn,
            queries::media::CHECK_EXISTS,
            &[media_id, &current_user.id],
            |row| row.get::<_, i64>(0),
        )?;

        if media_exists.is_none() {
            continue;
        }

        let _ = conn.execute(
            queries::tags::ADD_TO_MEDIA,
            rusqlite::params![media_id, request.tag_id],
        );
    }

    Ok(Json(serde_json::json!({"message": "Tag added to media"})))
}

async fn remove_tag_from_media(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<TagRemoveFromMediaRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    for media_id in &request.media_ids {
        // Check media belongs to user
        let media_exists = fetch_one(
            &conn,
            queries::media::CHECK_EXISTS,
            &[media_id, &current_user.id],
            |row| row.get::<_, i64>(0),
        )?;

        if media_exists.is_some() {
            conn.execute(
                queries::tags::REMOVE_FROM_MEDIA,
                rusqlite::params![media_id, request.tag_id],
            )?;
        }
    }

    Ok(Json(
        serde_json::json!({"message": "Tag removed from media"}),
    ))
}
