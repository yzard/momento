use axum::{
    extract::{ConnectInfo, State},
    http::{header::AUTHORIZATION, header::SET_COOKIE, HeaderMap},
    response::Response,
    routing::post,
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::net::SocketAddr;

use crate::auth::{
    clear_access_token_cookie, clear_refresh_token_cookie, create_access_token,
    create_access_token_cookie, create_refresh_token, create_refresh_token_cookie,
    hash_refresh_token, refresh_token_cookie, AppState, CurrentUser, TEMPORARY_ADMIN_PASSWORD,
    TEMPORARY_ADMIN_USERNAME,
};
use crate::database::operations::{
    InsertRefreshToken, ReplacePassword, RotateRefreshToken, UserAuthIdentifier,
};
use crate::error::{AppError, AppResult};
use crate::models::{ChangePasswordRequest, LogoutRequest, RefreshTokenRequest, TokenResponse};
use crate::routes::{render_json, render_message, CpuJson};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/user/authenticate", post(login))
        .route("/user/refresh", post(refresh))
        .route("/user/logout", post(logout))
        .route("/user/session/create", post(create_browser_session))
        .route("/user/session/refresh", post(refresh_browser_session))
        .route("/user/session/delete", post(delete_browser_session))
        .route("/user/change-password", post(change_password))
}

async fn login(
    State(state): State<AppState>,
    peer_address: Option<ConnectInfo<SocketAddr>>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let response = authenticate(&state, &headers, peer_address.map(|peer| peer.0)).await?;
    render_json(&state, response).await
}

async fn create_browser_session(
    State(state): State<AppState>,
    peer_address: Option<ConnectInfo<SocketAddr>>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let tokens = authenticate(&state, &headers, peer_address.map(|peer| peer.0)).await?;
    session_response(&state, "Authenticated successfully", &tokens).await
}

async fn authenticate(
    state: &AppState,
    headers: &HeaderMap,
    peer_address: Option<SocketAddr>,
) -> AppResult<TokenResponse> {
    let auth_header = headers
        .get(AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| AppError::Authentication("Missing authorization header".to_string()))?;

    let credentials = auth_header
        .strip_prefix("Basic ")
        .ok_or_else(|| AppError::Authentication("Invalid authorization header".to_string()))?;

    let decoded = STANDARD
        .decode(credentials)
        .map_err(|_| AppError::Authentication("Invalid credentials encoding".to_string()))?;

    let credentials_str = String::from_utf8(decoded)
        .map_err(|_| AppError::Authentication("Invalid credentials encoding".to_string()))?;

    let (username, password) = credentials_str
        .split_once(':')
        .ok_or_else(|| AppError::Authentication("Invalid credentials format".to_string()))?;

    let client_source = state
        .authentication_protection
        .client_source(headers, peer_address);
    state
        .authentication_protection
        .begin_password_attempt(&client_source, username)
        .await?;

    let temporary_admin_reset = (username == TEMPORARY_ADMIN_USERNAME
        && password == TEMPORARY_ADMIN_PASSWORD)
        .then(|| state.admin_password_reset.login())
        .flatten();
    let temporary_admin_login = temporary_admin_reset.is_some();
    let user = if let Some((admin_id, _)) = &temporary_admin_reset {
        state
            .executors
            .sqlite
            .load_user_for_authentication_request(UserAuthIdentifier::Id(*admin_id))
            .await
            .map_err(AppError::from)?
    } else {
        state
            .executors
            .sqlite
            .load_user_for_authentication_request(UserAuthIdentifier::Username(
                username.to_string(),
            ))
            .await
            .map_err(AppError::from)?
    };
    let verified = state
        .authentication_protection
        .verify_password(
            password,
            (!temporary_admin_login)
                .then(|| user.as_ref().map(|row| row.hashed_password.as_str()))
                .flatten(),
        )
        .await?;
    let user = user.filter(|user| {
        user.is_active
            && if temporary_admin_login {
                user.role == "admin"
            } else {
                verified
            }
    });
    let Some(user) = user else {
        return Err(AppError::Authentication("Invalid credentials".to_string()));
    };
    state
        .authentication_protection
        .record_password_success(&client_source, username)
        .await?;

    let config = state.config.current();
    let access_token = create_access_token(
        user.id,
        &user.username,
        &user.role,
        &config,
        temporary_admin_reset
            .as_ref()
            .map(|(_, reset_id)| reset_id.as_str()),
    )?;
    let (raw_refresh, token_hash, expires_at) = create_refresh_token(user.id, &config);

    if !temporary_admin_login {
        state
            .executors
            .sqlite
            .insert_refresh_token_request(InsertRefreshToken {
                token_hash,
                user_id: user.id,
                expires_at: expires_at.to_rfc3339(),
            })
            .await
            .map_err(AppError::from)?;
    }

    Ok(TokenResponse::new(access_token, raw_refresh))
}

async fn refresh(
    State(state): State<AppState>,
    CpuJson(request): CpuJson<RefreshTokenRequest>,
) -> AppResult<Response> {
    let response = rotate_refresh_token(&state, &request.refresh_token).await?;
    render_json(&state, response).await
}

async fn refresh_browser_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let refresh_token = refresh_token_cookie(&headers)
        .ok_or_else(|| AppError::Authentication("Refresh session is required".to_string()))?;
    let tokens = rotate_refresh_token(&state, &refresh_token).await?;
    session_response(&state, "Session refreshed successfully", &tokens).await
}

async fn rotate_refresh_token(state: &AppState, refresh_token: &str) -> AppResult<TokenResponse> {
    let token_hash = hash_refresh_token(refresh_token);
    let now = chrono::Utc::now().to_rfc3339();
    let config = state.config.current();
    let (raw_refresh, new_token_hash, expires_at) = create_refresh_token(0, &config);
    let token_row = state
        .executors
        .sqlite
        .rotate_refresh_token_request(RotateRefreshToken {
            current_token_hash: token_hash,
            replacement_token_hash: new_token_hash,
            replacement_expires_at: expires_at.to_rfc3339(),
            now,
        })
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::Authentication("Invalid refresh token".to_string()))?;
    let access_token = create_access_token(
        token_row.user_id,
        &token_row.username,
        &token_row.role,
        &config,
        None,
    )?;
    Ok(TokenResponse::new(access_token, raw_refresh))
}

async fn logout(
    State(state): State<AppState>,
    CpuJson(request): CpuJson<LogoutRequest>,
) -> AppResult<Response> {
    revoke_refresh_token(&state, &request.refresh_token).await?;

    render_message(&state, "Logged out successfully").await
}

async fn delete_browser_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Response> {
    if let Some(refresh_token) = refresh_token_cookie(&headers) {
        revoke_refresh_token(&state, &refresh_token).await?;
    }
    cleared_session_response(&state, "Logged out successfully").await
}

async fn revoke_refresh_token(state: &AppState, refresh_token: &str) -> AppResult<()> {
    let token_hash = hash_refresh_token(refresh_token);
    state
        .executors
        .sqlite
        .revoke_refresh_token_request(token_hash)
        .await
        .map_err(AppError::from)
}

async fn change_password(
    State(state): State<AppState>,
    peer_address: Option<ConnectInfo<SocketAddr>>,
    current_user: CurrentUser,
    headers: HeaderMap,
    CpuJson(request): CpuJson<ChangePasswordRequest>,
) -> AppResult<Response> {
    if request.new_password.len() < 8 {
        return Err(AppError::BadRequest(
            "Password must be at least 8 characters".to_string(),
        ));
    }

    let user = state
        .executors
        .sqlite
        .load_password_hash_request(current_user.id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::NotFound("User not found".to_string()))?;
    let temporary_admin_password = state
        .admin_password_reset
        .requires_password_change(current_user.id)
        && current_user.must_change_password
        && request.current_password == TEMPORARY_ADMIN_PASSWORD;
    let password_identity = format!("user:{}", current_user.id);
    let client_source = state
        .authentication_protection
        .client_source(&headers, peer_address.map(|peer| peer.0));
    state
        .authentication_protection
        .begin_password_attempt(&client_source, &password_identity)
        .await?;
    let verified = state
        .authentication_protection
        .verify_password(
            &request.current_password,
            (!temporary_admin_password).then_some(user.as_str()),
        )
        .await?;
    if !temporary_admin_password && !verified {
        return Err(AppError::BadRequest(
            "Current password is incorrect".to_string(),
        ));
    }
    state
        .authentication_protection
        .record_password_success(&client_source, &password_identity)
        .await?;
    let new_hash = state
        .authentication_protection
        .hash_password(&request.new_password)
        .await?;
    let changed = state
        .executors
        .sqlite
        .replace_password_request(ReplacePassword {
            user_id: current_user.id,
            expected_hash: user,
            replacement_hash: new_hash,
        })
        .await
        .map_err(AppError::from)?;
    if !changed {
        return Err(AppError::Conflict(
            "Password changed concurrently; retry with the current password".to_string(),
        ));
    }
    state.admin_password_reset.complete(current_user.id);

    cleared_session_response(&state, "Password changed successfully").await
}

async fn session_response(
    state: &AppState,
    message: &str,
    tokens: &TokenResponse,
) -> AppResult<Response> {
    let config = state.config.current();
    let mut response = render_message(state, message).await?;
    response.headers_mut().append(
        SET_COOKIE,
        create_access_token_cookie(
            &tokens.access_token,
            config.security.access_token_expire_minutes * 60,
        )?,
    );
    response.headers_mut().append(
        SET_COOKIE,
        create_refresh_token_cookie(
            &tokens.refresh_token,
            config.security.refresh_token_expire_days * 24 * 60 * 60,
        )?,
    );
    Ok(response)
}

async fn cleared_session_response(state: &AppState, message: &str) -> AppResult<Response> {
    let mut response = render_message(state, message).await?;
    response
        .headers_mut()
        .append(SET_COOKIE, clear_access_token_cookie());
    response
        .headers_mut()
        .append(SET_COOKIE, clear_refresh_token_cookie());
    Ok(response)
}
