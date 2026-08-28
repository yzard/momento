use axum::{extract::State, routing::post, Json, Router};
use std::collections::HashSet;

use rusqlite::OptionalExtension;

use crate::auth::{AppState, CurrentUser};
use crate::database::{execute_query, fetch_all, fetch_one, queries};
use crate::error::{AppError, AppResult};
use crate::models::{
    map_media_response, AlbumAddMediaRequest, AlbumCreateRequest, AlbumDeleteRequest,
    AlbumDetailResponse, AlbumGetRequest, AlbumListResponse, AlbumRemoveMediaRequest,
    AlbumReorderRequest, AlbumResponse, AlbumUpdateRequest,
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
    let thumbnail_media_ids = (6..10)
        .filter_map(|column| row.get::<_, Option<i64>>(column).transpose())
        .collect::<Result<Vec<_>, _>>()?;

    Ok(AlbumResponse {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        cover_media_id: row.get(3)?,
        thumbnail_media_ids,
        media_count: row.get(4)?,
        created_at: row.get(5)?,
    })
}

async fn create_album(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<AlbumCreateRequest>,
) -> AppResult<Json<AlbumDetailResponse>> {
    validate_media_batch(&request.media_ids)?;
    let connection = state.pool.get().map_err(AppError::Pool)?;
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(
        queries::albums::INSERT,
        rusqlite::params![current_user.id, request.name, request.description],
    )?;
    let album_id = transaction.last_insert_rowid();
    transaction.execute(
        queries::access::INSERT_ALBUM_ACCESS,
        rusqlite::params![album_id, current_user.id, 2],
    )?;
    insert_accessible_media(&transaction, current_user.id, album_id, &request.media_ids)?;
    let album = load_album_detail(&transaction, album_id)?;
    transaction.commit()?;

    Ok(Json(album))
}

struct AlbumBasic {
    id: i64,
    name: String,
    description: Option<String>,
    cover_media_id: Option<i64>,
    created_at: String,
}

fn validate_media_batch(media_ids: &[i64]) -> AppResult<()> {
    if media_ids.len() > 500 {
        return Err(AppError::BadRequest(
            "mediaIds must contain at most 500 IDs".to_string(),
        ));
    }
    Ok(())
}

fn require_album_ownership(
    connection: &rusqlite::Connection,
    album_id: i64,
    user_id: i64,
) -> AppResult<()> {
    let owned_album_id = connection
        .query_row(
            queries::albums::CHECK_OWNERSHIP,
            [album_id, user_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    if owned_album_id.is_none() {
        return Err(AppError::NotFound("Album not found".to_string()));
    }
    Ok(())
}

fn insert_accessible_media(
    connection: &rusqlite::Connection,
    user_id: i64,
    album_id: i64,
    media_ids: &[i64],
) -> AppResult<()> {
    if media_ids.is_empty() {
        return Ok(());
    }

    let query = queries::albums::build_add_media_batch_query(media_ids.len());
    let mut parameters: Vec<Box<dyn rusqlite::ToSql>> = Vec::with_capacity(media_ids.len() * 2 + 3);
    for (position, media_id) in media_ids.iter().enumerate() {
        parameters.push(Box::new(*media_id));
        parameters.push(Box::new(position as i64));
    }
    parameters.push(Box::new(user_id));
    parameters.push(Box::new(album_id));
    parameters.push(Box::new(album_id));
    let parameter_refs = parameters
        .iter()
        .map(|parameter| parameter.as_ref())
        .collect::<Vec<_>>();
    connection.execute(&query, parameter_refs.as_slice())?;
    Ok(())
}

fn load_album_detail(
    connection: &rusqlite::Connection,
    album_id: i64,
) -> AppResult<AlbumDetailResponse> {
    let album = connection
        .query_row(queries::albums::SELECT_BY_ID, [album_id], |row| {
            Ok(AlbumBasic {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                cover_media_id: row.get(3)?,
                created_at: row.get(5)?,
            })
        })
        .optional()?
        .ok_or_else(|| AppError::NotFound("Album not found".to_string()))?;
    let media = connection
        .prepare(queries::albums::SELECT_MEDIA)?
        .query_map([album_id], map_media_response)?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(AlbumDetailResponse {
        id: album.id,
        name: album.name,
        description: album.description,
        cover_media_id: album.cover_media_id,
        media,
        created_at: album.created_at,
    })
}

async fn update_album(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<AlbumUpdateRequest>,
) -> AppResult<Json<AlbumResponse>> {
    let connection = state.pool.get().map_err(AppError::Pool)?;

    require_album_ownership(&connection, request.album_id, current_user.id)?;

    execute_query(
        &connection,
        queries::albums::UPDATE,
        &[
            &request.name,
            &request.description,
            &request.cover_media_id,
            &request.album_id,
        ],
    )?;

    let album = fetch_one(
        &connection,
        &queries::albums::select_with_count_query(),
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
    let connection = state.pool.get().map_err(AppError::Pool)?;

    require_album_ownership(&connection, request.album_id, current_user.id)?;

    execute_query(
        &connection,
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
    let connection = state.pool.get().map_err(AppError::Pool)?;

    require_album_ownership(&connection, request.album_id, current_user.id)?;

    validate_media_batch(&request.media_ids)?;
    insert_accessible_media(
        &connection,
        current_user.id,
        request.album_id,
        &request.media_ids,
    )?;

    Ok(Json(serde_json::json!({"message": "Media added to album"})))
}

async fn remove_media_from_album(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<AlbumRemoveMediaRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let connection = state.pool.get().map_err(AppError::Pool)?;

    require_album_ownership(&connection, request.album_id, current_user.id)?;

    for media_id in &request.media_ids {
        connection.execute(
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
    let connection = state.pool.get().map_err(AppError::Pool)?;

    let albums = fetch_all(
        &connection,
        &queries::albums::select_all_for_user_query(),
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
    let connection = state.pool.get().map_err(AppError::Pool)?;

    require_album_ownership(&connection, request.album_id, current_user.id)?;

    Ok(Json(load_album_detail(&connection, request.album_id)?))
}

async fn reorder_album_media(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<AlbumReorderRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let connection = state.pool.get().map_err(AppError::Pool)?;
    let transaction = connection.unchecked_transaction()?;
    require_album_ownership(&transaction, request.album_id, current_user.id)?;

    let current_ids = transaction
        .prepare(queries::albums::SELECT_MEDIA_IDS)?
        .query_map([request.album_id], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let requested_ids = request.media_ids.iter().copied().collect::<HashSet<_>>();
    let current_id_set = current_ids.iter().copied().collect::<HashSet<_>>();
    if request.media_ids.len() != current_ids.len()
        || requested_ids.len() != request.media_ids.len()
        || requested_ids != current_id_set
    {
        return Err(AppError::BadRequest(
            "mediaIds must be an exact permutation of the album media".to_string(),
        ));
    }

    for (i, media_id) in request.media_ids.iter().enumerate() {
        transaction.execute(
            queries::albums::UPDATE_POSITION,
            rusqlite::params![i as i64, request.album_id, media_id],
        )?;
    }
    transaction.commit()?;

    Ok(Json(
        serde_json::json!({"message": "Album reordered successfully"}),
    ))
}
