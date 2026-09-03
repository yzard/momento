use axum::{
    extract::{ConnectInfo, Extension, Path, State},
    http::{header, HeaderMap, HeaderValue},
    response::Response,
    routing::{get, post},
    Router,
};
use chrono::{DateTime, Utc};
use std::net::SocketAddr;

use crate::auth::{
    create_share_session_token, decode_share_session_token, share_token_hash, AppState,
};
use crate::database::operations::{
    ActiveShareRecord, PublicFileAccessOutcome, PublicShareContent, PublicSharedMediaQuery,
    PublicThumbnailAccessOutcome,
};
use crate::error::{AppError, AppResult};
use crate::executor::{
    PublicAlbumContentResponse, PublicAlbumSummaryResponse, PublicMediaContentResponse,
};
use crate::io::file::{NormalizedStoragePath, StorageRootId};
use crate::models::{ShareVerifyRequest, ShareVerifyResponse};
use crate::routes::{render_json, CpuJson};
use crate::runtime::HttpRequestAdmission;

use super::file_stream::{serve_file, ContentDisposition, FileResponseOptions};

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

async fn validate_share_access(
    token: &str,
    headers: &HeaderMap,
    state: &AppState,
) -> AppResult<ActiveShareRecord> {
    let share = load_active_share(state, token).await?;
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

async fn load_active_share(state: &AppState, token: &str) -> AppResult<ActiveShareRecord> {
    let share = state
        .executors
        .sqlite
        .load_active_share_request(token.to_string())
        .await?
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

async fn get_shared_content(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let share = validate_share_access(&token, &headers, &state).await?;
    match state
        .executors
        .sqlite
        .load_public_share_content_request(share)
        .await?
    {
        PublicShareContent::Media(media) => {
            public_json_response(
                &state,
                PublicMediaContentResponse {
                    content_type: "media".to_string(),
                    media: *media,
                },
            )
            .await
        }
        PublicShareContent::Album {
            id,
            name,
            description,
            media,
        } => {
            public_json_response(
                &state,
                PublicAlbumContentResponse {
                    content_type: "album".to_string(),
                    album: PublicAlbumSummaryResponse {
                        id,
                        name,
                        description,
                    },
                    media,
                },
            )
            .await
        }
        PublicShareContent::NotFound => {
            Err(AppError::NotFound("Shared content not found".to_string()))
        }
        PublicShareContent::Invalid => Err(AppError::Internal("Invalid share link".to_string())),
    }
}

async fn verify_share_password(
    State(state): State<AppState>,
    peer_address: Option<ConnectInfo<SocketAddr>>,
    Path(token): Path<String>,
    headers: HeaderMap,
    CpuJson(request): CpuJson<ShareVerifyRequest>,
) -> AppResult<Response> {
    let share = load_active_share(&state, &token).await?;

    let Some(password_hash) = share.password_hash else {
        return share_verify_response(&state, true, "No password required", None).await;
    };

    let password_identity = format!("share:{}", share_token_hash(&token));
    let client_source = state
        .authentication_protection
        .client_source(&headers, peer_address.map(|peer| peer.0));
    state
        .authentication_protection
        .begin_password_attempt(&client_source, &password_identity)
        .await?;
    if !state
        .authentication_protection
        .verify_password(&request.password, Some(&password_hash))
        .await?
    {
        return share_verify_response(&state, false, "Invalid password", None).await;
    }
    state
        .authentication_protection
        .record_password_success(&client_source, &password_identity)
        .await?;

    let share_expiration = parse_share_expiration(share.expires_at.as_deref())?;
    let config = state.config.current();
    let (session_token, expires_at) =
        create_share_session_token(share.id, &token, share_expiration, &config)?;
    let maximum_age = (expires_at - Utc::now()).num_seconds().max(1);
    let cookie = format!(
        "{SHARE_SESSION_COOKIE_NAME}={session_token}; Path=/api/v1/public/share/{token}; Max-Age={maximum_age}; HttpOnly; Secure; SameSite=Strict"
    );
    share_verify_response(&state, true, "Password correct", Some(&cookie)).await
}

async fn get_shared_media_file(
    State(state): State<AppState>,
    Extension(admission): Extension<HttpRequestAdmission>,
    Path((token, media_id)): Path<(String, i64)>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let share = validate_share_access(&token, &headers, &state).await?;
    let media = match state
        .executors
        .sqlite
        .load_public_shared_file_request(PublicSharedMediaQuery { share, media_id })
        .await?
    {
        PublicFileAccessOutcome::NotInShare => {
            return Err(AppError::Authorization("Media not in share".to_string()));
        }
        PublicFileAccessOutcome::NotFound => {
            return Err(AppError::NotFound("Media not found".to_string()));
        }
        PublicFileAccessOutcome::Found(media) => media,
    };

    let relative_path = NormalizedStoragePath::parse(&media.file_path)
        .map_err(|_| AppError::NotFound("Media file path is invalid".to_string()))?;

    serve_file(
        &state.executors.file_io,
        StorageRootId::Originals,
        relative_path,
        FileResponseOptions {
            admission: &admission,
            content_type: &media
                .mime_type
                .unwrap_or_else(|| "application/octet-stream".to_string()),
            headers: &headers,
            filename: Some(&media.original_filename),
            allow_ranges: true,
            content_disposition: ContentDisposition::Attachment,
            cache_control: "private",
            head_only: false,
        },
    )
    .await
}

async fn get_shared_thumbnail(
    State(state): State<AppState>,
    Extension(admission): Extension<HttpRequestAdmission>,
    Path((token, media_id)): Path<(String, i64)>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let share = validate_share_access(&token, &headers, &state).await?;
    let thumbnail_path = match state
        .executors
        .sqlite
        .load_public_shared_thumbnail_request(PublicSharedMediaQuery { share, media_id })
        .await?
    {
        PublicThumbnailAccessOutcome::NotInShare => {
            return Err(AppError::Authorization("Media not in share".to_string()));
        }
        PublicThumbnailAccessOutcome::NotFound => {
            return Err(AppError::NotFound("Media not found".to_string()));
        }
        PublicThumbnailAccessOutcome::Unavailable => {
            return Err(AppError::NotFound("Thumbnail not available".to_string()));
        }
        PublicThumbnailAccessOutcome::Found(path) => path,
    };

    let relative_path = NormalizedStoragePath::parse(&thumbnail_path)
        .map_err(|_| AppError::NotFound("Thumbnail path is invalid".to_string()))?;

    serve_file(
        &state.executors.file_io,
        StorageRootId::Thumbnails,
        relative_path,
        FileResponseOptions {
            admission: &admission,
            content_type: "image/jpeg",
            headers: &headers,
            filename: None,
            allow_ranges: false,
            content_disposition: ContentDisposition::Inline,
            cache_control: "private",
            head_only: false,
        },
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

async fn share_verify_response(
    state: &AppState,
    valid: bool,
    message: &str,
    set_cookie: Option<&str>,
) -> AppResult<Response> {
    let mut response = render_json(
        state,
        ShareVerifyResponse {
            valid,
            message: message.to_string(),
        },
    )
    .await?;
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

async fn public_json_response<ResponseDto>(
    state: &AppState,
    body: ResponseDto,
) -> AppResult<Response>
where
    ResponseDto: Into<crate::executor::ControlResponse>,
{
    let mut response = render_json(state, body).await?;
    response
        .headers_mut()
        .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    Ok(response)
}
