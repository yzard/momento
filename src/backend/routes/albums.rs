use axum::{extract::State, response::Response, routing::post, Router};

use crate::auth::{AppState, CurrentUser};
use crate::database::operations::{
    AlbumDetailOutcome, AlbumMediaMutation, AlbumMutationOutcome, AlbumUpdateOutcome, CreateAlbum,
    UpdateAlbum, UserAlbum,
};
use crate::error::{AppError, AppResult};
use crate::models::{
    AlbumAddMediaRequest, AlbumCreateRequest, AlbumDeleteRequest, AlbumGetRequest,
    AlbumListResponse, AlbumRemoveMediaRequest, AlbumReorderRequest, AlbumUpdateRequest,
};
use crate::routes::{render_json, render_message, CpuJson};

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

async fn create_album(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<AlbumCreateRequest>,
) -> AppResult<Response> {
    let album = state
        .executors
        .sqlite
        .create_album_request(CreateAlbum {
            user_id: current_user.id,
            name: request.name,
            description: request.description,
            media_ids: request.media_ids,
        })
        .await?;
    render_json(&state, album).await
}

async fn update_album(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<AlbumUpdateRequest>,
) -> AppResult<Response> {
    match state
        .executors
        .sqlite
        .update_album_request(UpdateAlbum {
            user_id: current_user.id,
            album_id: request.album_id,
            name: request.name,
            description: request.description,
            cover_media_id: request.cover_media_id,
        })
        .await?
    {
        AlbumUpdateOutcome::NotFound => Err(AppError::NotFound("Album not found".to_string())),
        AlbumUpdateOutcome::Updated(album) => render_json(&state, album).await,
    }
}

async fn delete_album(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<AlbumDeleteRequest>,
) -> AppResult<Response> {
    require_completed(
        state
            .executors
            .sqlite
            .delete_album_request(UserAlbum {
                user_id: current_user.id,
                album_id: request.album_id,
            })
            .await?,
    )?;

    render_message(&state, "Album deleted successfully").await
}

async fn add_media_to_album(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<AlbumAddMediaRequest>,
) -> AppResult<Response> {
    require_completed(
        state
            .executors
            .sqlite
            .add_album_media_request(AlbumMediaMutation {
                user_id: current_user.id,
                album_id: request.album_id,
                media_ids: request.media_ids,
            })
            .await?,
    )?;

    render_message(&state, "Media added to album").await
}

async fn remove_media_from_album(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<AlbumRemoveMediaRequest>,
) -> AppResult<Response> {
    require_completed(
        state
            .executors
            .sqlite
            .remove_album_media_request(AlbumMediaMutation {
                user_id: current_user.id,
                album_id: request.album_id,
                media_ids: request.media_ids,
            })
            .await?,
    )?;

    render_message(&state, "Media removed from album").await
}

async fn list_albums(
    State(state): State<AppState>,
    current_user: CurrentUser,
) -> AppResult<Response> {
    let albums = state
        .executors
        .sqlite
        .list_albums_request(current_user.id)
        .await?;

    render_json(&state, AlbumListResponse { albums }).await
}

async fn get_album(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<AlbumGetRequest>,
) -> AppResult<Response> {
    match state
        .executors
        .sqlite
        .load_album_request(UserAlbum {
            user_id: current_user.id,
            album_id: request.album_id,
        })
        .await?
    {
        AlbumDetailOutcome::NotFound => Err(AppError::NotFound("Album not found".to_string())),
        AlbumDetailOutcome::Found(album) => render_json(&state, album).await,
    }
}

async fn reorder_album_media(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<AlbumReorderRequest>,
) -> AppResult<Response> {
    require_completed(
        state
            .executors
            .sqlite
            .reorder_album_media_request(AlbumMediaMutation {
                user_id: current_user.id,
                album_id: request.album_id,
                media_ids: request.media_ids,
            })
            .await?,
    )?;

    render_message(&state, "Album reordered successfully").await
}

fn require_completed(outcome: AlbumMutationOutcome) -> AppResult<()> {
    match outcome {
        AlbumMutationOutcome::NotFound => Err(AppError::NotFound("Album not found".to_string())),
        AlbumMutationOutcome::InvalidPermutation => Err(AppError::BadRequest(
            "mediaIds must be an exact permutation of the album media".to_string(),
        )),
        AlbumMutationOutcome::Completed => Ok(()),
    }
}
