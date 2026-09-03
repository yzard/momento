use axum::{extract::State, response::Response, routing::post, Router};
use chrono::{Duration, Utc};
use rand::Rng;

use crate::auth::{AppState, CurrentUser};
use crate::database::operations::{
    CreateShareLink, CreateShareLinkOutcome, DeleteShareLinkOutcome, GrantShareAccess,
    GrantShareAccessOutcome, ShareTargetKind,
};
use crate::error::{AppError, AppResult};
use crate::models::{
    ShareAlbumRequest, ShareCreateRequest, ShareDeleteRequest, ShareListResponse, ShareMediaRequest,
};
use crate::routes::{render_json, render_message, CpuJson};

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

async fn create_share_link(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<ShareCreateRequest>,
) -> AppResult<Response> {
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

    let token: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(22)
        .map(char::from)
        .collect();

    let expires_at = request
        .expires_in_days
        .map(|days| (Utc::now() + Duration::days(days as i64)).to_rfc3339());

    match state
        .executors
        .sqlite
        .create_share_link_request(CreateShareLink {
            user_id: current_user.id,
            media_id: request.media_id,
            album_id: request.album_id,
            token,
            password_hash,
            expires_at,
        })
        .await?
    {
        CreateShareLinkOutcome::MediaNotFound => {
            Err(AppError::NotFound("Media not found".to_string()))
        }
        CreateShareLinkOutcome::AlbumNotFound => {
            Err(AppError::NotFound("Album not found".to_string()))
        }
        CreateShareLinkOutcome::Created(share) => render_json(&state, share).await,
    }
}

async fn list_share_links(
    State(state): State<AppState>,
    current_user: CurrentUser,
) -> AppResult<Response> {
    let shares = state
        .executors
        .sqlite
        .list_share_links_request(current_user.id)
        .await?;

    render_json(&state, ShareListResponse { shares }).await
}

async fn delete_share_link(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<ShareDeleteRequest>,
) -> AppResult<Response> {
    if state
        .executors
        .sqlite
        .delete_share_link_request(current_user.id, request.share_id)
        .await?
        == DeleteShareLinkOutcome::NotFound
    {
        return Err(AppError::NotFound("Share link not found".to_string()));
    }

    render_message(&state, "Share link deleted successfully").await
}

async fn share_media_with_user(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<ShareMediaRequest>,
) -> AppResult<Response> {
    validate_access_grant(
        &request.access_level,
        current_user.id,
        request.target_user_id,
    )?;
    require_granted(
        state
            .executors
            .sqlite
            .grant_share_access_request(GrantShareAccess {
                owner_user_id: current_user.id,
                target_user_id: request.target_user_id,
                target_id: request.media_id,
                access_level: request.access_level,
                kind: ShareTargetKind::Media,
            })
            .await?,
        "Media",
    )?;

    render_message(&state, "Media shared successfully").await
}

async fn share_album_with_user(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<ShareAlbumRequest>,
) -> AppResult<Response> {
    validate_access_grant(
        &request.access_level,
        current_user.id,
        request.target_user_id,
    )?;
    require_granted(
        state
            .executors
            .sqlite
            .grant_share_access_request(GrantShareAccess {
                owner_user_id: current_user.id,
                target_user_id: request.target_user_id,
                target_id: request.album_id,
                access_level: request.access_level,
                kind: ShareTargetKind::Album,
            })
            .await?,
        "Album",
    )?;

    render_message(&state, "Album shared successfully").await
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

fn require_granted(outcome: GrantShareAccessOutcome, target: &str) -> AppResult<()> {
    match outcome {
        GrantShareAccessOutcome::TargetUserNotFound => {
            Err(AppError::NotFound("Target user not found".to_string()))
        }
        GrantShareAccessOutcome::TargetNotFound => {
            Err(AppError::NotFound(format!("{target} not found")))
        }
        GrantShareAccessOutcome::InsufficientPermission => Err(AppError::Forbidden(
            "Insufficient permissions to share".to_string(),
        )),
        GrantShareAccessOutcome::Granted => Ok(()),
    }
}
