use axum::{extract::State, routing::post, Json, Router};

use crate::auth::{AppState, CurrentUser};
use crate::database::{execute_query, fetch_all, fetch_one, insert_returning_id, queries};
use crate::error::{AppError, AppResult};
use crate::models::{
    AlbumAddMediaRequest, AlbumCreateRequest, AlbumDeleteRequest, AlbumDetailResponse,
    AlbumGetRequest, AlbumListResponse, AlbumRemoveMediaRequest, AlbumReorderRequest,
    AlbumResponse, AlbumUpdateRequest, MediaResponse,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/album/create", post(create_album))
        .route("/album/list", post(list_albums))
        .route("/album/get", post(get_album))
        .route("/album/update", post(update_album))
        .route("/album/delete", post(delete_album))
        .route("/album/add-media", post(add_media_to_album))
        .route("/album/remove-media", post(remove_media_from_album))
        .route("/album/reorder", post(reorder_album_media))
}

fn map_album_row(row: &rusqlite::Row) -> rusqlite::Result<AlbumResponse> {
    Ok(AlbumResponse {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        cover_media_id: row.get(3)?,
        media_count: row.get(4)?,
        created_at: row.get(5)?,
    })
}

async fn create_album(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<AlbumCreateRequest>,
) -> AppResult<Json<AlbumDetailResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let album_id = insert_returning_id(
        &conn,
        queries::albums::INSERT,
        &[&current_user.id, &request.name, &request.description],
    )?;

    execute_query(
        &conn,
        queries::access::INSERT_ALBUM_ACCESS,
        &[&album_id, &current_user.id, &2],
    )?;

    let album = fetch_one(&conn, queries::albums::SELECT_BY_ID, &[&album_id], |row| {
        Ok(AlbumBasic {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            cover_media_id: row.get(3)?,
            created_at: row.get(5)?,
        })
    })?
    .ok_or_else(|| AppError::NotFound("Album not found".to_string()))?;

    let media = fetch_all(
        &conn,
        queries::albums::SELECT_MEDIA,
        &[&album_id],
        map_media_row,
    )?;

    Ok(Json(AlbumDetailResponse {
        id: album.id,
        name: album.name,
        description: album.description,
        cover_media_id: album.cover_media_id,
        media,
        created_at: album.created_at,
    }))
}

struct AlbumBasic {
    id: i64,
    name: String,
    description: Option<String>,
    cover_media_id: Option<i64>,
    created_at: String,
}

fn map_media_row(row: &rusqlite::Row) -> rusqlite::Result<MediaResponse> {
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
        content_hash: None,
    })
}

async fn update_album(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<AlbumUpdateRequest>,
) -> AppResult<Json<AlbumResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let exists = fetch_one(
        &conn,
        queries::albums::CHECK_OWNERSHIP,
        &[&request.album_id, &current_user.id],
        |row| row.get::<_, i64>(0),
    )?;

    if exists.is_none() {
        return Err(AppError::NotFound("Album not found".to_string()));
    }

    let mut updates = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(ref name) = request.name {
        updates.push("name = ?");
        params.push(Box::new(name.clone()));
    }

    if let Some(ref desc) = request.description {
        updates.push("description = ?");
        params.push(Box::new(desc.clone()));
    }

    if let Some(cover_id) = request.cover_media_id {
        updates.push("cover_media_id = ?");
        params.push(Box::new(cover_id));
    }

    if !updates.is_empty() {
        params.push(Box::new(request.album_id));
        let sql = format!("UPDATE albums SET {} WHERE id = ?", updates.join(", "));
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        execute_query(&conn, &sql, &param_refs)?;
    }

    let album = fetch_one(
        &conn,
        queries::albums::SELECT_WITH_COUNT,
        &[&request.album_id],
        map_album_row,
    )?
    .ok_or_else(|| AppError::Internal("Failed to update album".to_string()))?;

    Ok(Json(album))
}

async fn delete_album(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<AlbumDeleteRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

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
        queries::albums::DELETE_ACCESS,
        &[&request.album_id, &current_user.id],
    )?;

    Ok(Json(
        serde_json::json!({"message": "Album deleted successfully"}),
    ))
}

async fn add_media_to_album(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<AlbumAddMediaRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let exists = fetch_one(
        &conn,
        queries::albums::CHECK_OWNERSHIP,
        &[&request.album_id, &current_user.id],
        |row| row.get::<_, i64>(0),
    )?;

    if exists.is_none() {
        return Err(AppError::NotFound("Album not found".to_string()));
    }

    let max_pos: i64 = fetch_one(
        &conn,
        queries::albums::SELECT_MAX_POSITION,
        &[&request.album_id],
        |row| row.get(0),
    )?
    .unwrap_or(-1);

    let mut next_pos = max_pos + 1;

    for media_id in &request.media_ids {
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
            queries::albums::ADD_MEDIA,
            rusqlite::params![request.album_id, media_id, next_pos],
        );
        next_pos += 1;
    }

    Ok(Json(serde_json::json!({"message": "Media added to album"})))
}

async fn remove_media_from_album(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<AlbumRemoveMediaRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let exists = fetch_one(
        &conn,
        queries::albums::CHECK_OWNERSHIP,
        &[&request.album_id, &current_user.id],
        |row| row.get::<_, i64>(0),
    )?;

    if exists.is_none() {
        return Err(AppError::NotFound("Album not found".to_string()));
    }

    for media_id in &request.media_ids {
        conn.execute(
            queries::albums::REMOVE_MEDIA,
            rusqlite::params![request.album_id, media_id],
        )?;
    }

    Ok(Json(
        serde_json::json!({"message": "Media removed from album"}),
    ))
}

async fn list_albums(
    State(state): State<AppState>,
    current_user: CurrentUser,
) -> AppResult<Json<AlbumListResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let albums = fetch_all(
        &conn,
        queries::albums::SELECT_ALL_FOR_USER,
        &[&current_user.id],
        map_album_row,
    )?;

    Ok(Json(AlbumListResponse { albums }))
}

async fn get_album(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<AlbumGetRequest>,
) -> AppResult<Json<AlbumDetailResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let exists = fetch_one(
        &conn,
        queries::albums::CHECK_OWNERSHIP,
        &[&request.album_id, &current_user.id],
        |row| row.get::<_, i64>(0),
    )?;

    if exists.is_none() {
        return Err(AppError::NotFound("Album not found".to_string()));
    }

    let album = fetch_one(
        &conn,
        queries::albums::SELECT_BY_ID,
        &[&request.album_id],
        |row| {
            Ok(AlbumBasic {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                cover_media_id: row.get(3)?,
                created_at: row.get(5)?,
            })
        },
    )?
    .ok_or_else(|| AppError::NotFound("Album not found".to_string()))?;

    let media = fetch_all(
        &conn,
        queries::albums::SELECT_MEDIA,
        &[&request.album_id],
        map_media_row,
    )?;

    Ok(Json(AlbumDetailResponse {
        id: album.id,
        name: album.name,
        description: album.description,
        cover_media_id: album.cover_media_id,
        media,
        created_at: album.created_at,
    }))
}

async fn reorder_album_media(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<AlbumReorderRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let exists = fetch_one(
        &conn,
        queries::albums::CHECK_OWNERSHIP,
        &[&request.album_id, &current_user.id],
        |row| row.get::<_, i64>(0),
    )?;

    if exists.is_none() {
        return Err(AppError::NotFound("Album not found".to_string()));
    }

    for (i, media_id) in request.media_ids.iter().enumerate() {
        conn.execute(
            queries::albums::UPDATE_POSITION,
            rusqlite::params![i as i64, request.album_id, media_id],
        )?;
    }

    Ok(Json(
        serde_json::json!({"message": "Album reordered successfully"}),
    ))
}
