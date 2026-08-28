use axum::{
    extract::{ConnectInfo, State},
    http::{header::AUTHORIZATION, header::SET_COOKIE, HeaderMap},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::{OptionalExtension, TransactionBehavior};
use std::net::SocketAddr;

use crate::auth::{
    clear_access_token_cookie, clear_refresh_token_cookie, create_access_token,
    create_access_token_cookie, create_refresh_token, create_refresh_token_cookie,
    hash_refresh_token, refresh_token_cookie, AppState, CurrentUser, TEMPORARY_ADMIN_PASSWORD,
    TEMPORARY_ADMIN_USERNAME,
};
use crate::config::Config;
use crate::database::{execute_query, fetch_one, insert_returning_id, queries, DbConn};
use crate::error::{AppError, AppResult};
use crate::models::{ChangePasswordRequest, LogoutRequest, RefreshTokenRequest, TokenResponse};

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
) -> AppResult<Json<TokenResponse>> {
    Ok(Json(
        authenticate(&state, &headers, peer_address.map(|peer| peer.0)).await?,
    ))
}

async fn create_browser_session(
    State(state): State<AppState>,
    peer_address: Option<ConnectInfo<SocketAddr>>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let tokens = authenticate(&state, &headers, peer_address.map(|peer| peer.0)).await?;
    session_response(
        serde_json::json!({"message": "Authenticated successfully"}),
        &tokens,
        &state.config.current(),
    )
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
        .begin_password_attempt(&client_source, username)?;

    let connection = state.pool.get().map_err(AppError::Pool)?;

    let temporary_admin_reset = (username == TEMPORARY_ADMIN_USERNAME
        && password == TEMPORARY_ADMIN_PASSWORD)
        .then(|| state.admin_password_reset.login())
        .flatten();
    let temporary_admin_login = temporary_admin_reset.is_some();
    let user = if let Some((admin_id, _)) = &temporary_admin_reset {
        load_user_for_auth(&connection, queries::auth::SELECT_USER_BY_ID, admin_id)?
    } else {
        load_user_for_auth(
            &connection,
            queries::auth::SELECT_USER_BY_USERNAME,
            &username,
        )?
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
        user.is_active != 0
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
        .record_password_success(&client_source, username);

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
        insert_returning_id(
            &connection,
            queries::auth::INSERT_REFRESH_TOKEN,
            &[&token_hash, &user.id, &expires_at.to_rfc3339()],
        )?;
    }

    Ok(TokenResponse::new(access_token, raw_refresh))
}

fn load_user_for_auth(
    connection: &DbConn,
    query: &str,
    identifier: &dyn rusqlite::ToSql,
) -> AppResult<Option<UserAuthRow>> {
    fetch_one(connection, query, &[identifier], |row| {
        Ok(UserAuthRow {
            id: row.get(0)?,
            username: row.get(1)?,
            role: row.get(3)?,
            hashed_password: row.get(4)?,
            is_active: row.get(5)?,
        })
    })
}

struct UserAuthRow {
    id: i64,
    username: String,
    role: String,
    hashed_password: String,
    is_active: i32,
}

async fn refresh(
    State(state): State<AppState>,
    Json(request): Json<RefreshTokenRequest>,
) -> AppResult<Json<TokenResponse>> {
    Ok(Json(rotate_refresh_token(&state, &request.refresh_token)?))
}

async fn refresh_browser_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let refresh_token = refresh_token_cookie(&headers)
        .ok_or_else(|| AppError::Authentication("Refresh session is required".to_string()))?;
    let tokens = rotate_refresh_token(&state, &refresh_token)?;
    session_response(
        serde_json::json!({"message": "Session refreshed successfully"}),
        &tokens,
        &state.config.current(),
    )
}

fn rotate_refresh_token(state: &AppState, refresh_token: &str) -> AppResult<TokenResponse> {
    let token_hash = hash_refresh_token(refresh_token);
    let mut connection = state.pool.get().map_err(AppError::Pool)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let now = chrono::Utc::now().to_rfc3339();
    let token_row = transaction
        .query_row(
            queries::auth::VALIDATE_REFRESH_TOKEN,
            rusqlite::params![token_hash, now],
            |row| {
                Ok(RefreshTokenRow {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    username: row.get(4)?,
                    role: row.get(5)?,
                    is_active: row.get(6)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| AppError::Authentication("Invalid refresh token".to_string()))?;

    if token_row.is_active == 0 {
        return Err(AppError::Authentication("User is inactive".to_string()));
    }

    let config = state.config.current();
    let access_token = create_access_token(
        token_row.user_id,
        &token_row.username,
        &token_row.role,
        &config,
        None,
    )?;
    let (raw_refresh, new_token_hash, expires_at) =
        create_refresh_token(token_row.user_id, &config);

    let consumed = transaction.execute(
        queries::auth::REVOKE_REFRESH_TOKEN,
        rusqlite::params![token_row.id, now],
    )?;
    if consumed != 1 {
        return Err(AppError::Authentication(
            "Invalid refresh token".to_string(),
        ));
    }
    transaction.execute(
        queries::auth::INSERT_REFRESH_TOKEN,
        rusqlite::params![new_token_hash, token_row.user_id, expires_at.to_rfc3339()],
    )?;
    transaction.execute(
        queries::auth::DELETE_REVOKED_TOKEN,
        rusqlite::params![token_row.id],
    )?;
    transaction.commit()?;

    Ok(TokenResponse::new(access_token, raw_refresh))
}

struct RefreshTokenRow {
    id: i64,
    user_id: i64,
    username: String,
    role: String,
    is_active: i32,
}

async fn logout(
    State(state): State<AppState>,
    Json(request): Json<LogoutRequest>,
) -> AppResult<Json<serde_json::Value>> {
    revoke_refresh_token(&state, &request.refresh_token)?;

    Ok(Json(
        serde_json::json!({"message": "Logged out successfully"}),
    ))
}

async fn delete_browser_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Response> {
    if let Some(refresh_token) = refresh_token_cookie(&headers) {
        revoke_refresh_token(&state, &refresh_token)?;
    }
    cleared_session_response(serde_json::json!({"message": "Logged out successfully"}))
}

fn revoke_refresh_token(state: &AppState, refresh_token: &str) -> AppResult<()> {
    let token_hash = hash_refresh_token(refresh_token);
    let connection = state.pool.get().map_err(AppError::Pool)?;
    execute_query(
        &connection,
        queries::auth::DELETE_REFRESH_TOKEN_BY_HASH,
        &[&token_hash],
    )?;
    Ok(())
}

async fn change_password(
    State(state): State<AppState>,
    peer_address: Option<ConnectInfo<SocketAddr>>,
    current_user: CurrentUser,
    headers: HeaderMap,
    Json(request): Json<ChangePasswordRequest>,
) -> AppResult<Response> {
    if request.new_password.len() < 8 {
        return Err(AppError::BadRequest(
            "Password must be at least 8 characters".to_string(),
        ));
    }

    let mut connection = state.pool.get().map_err(AppError::Pool)?;
    let user = connection
        .query_row(
            queries::auth::SELECT_PASSWORD_HASH,
            [current_user.id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
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
        .begin_password_attempt(&client_source, &password_identity)?;
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
        .record_password_success(&client_source, &password_identity);
    let new_hash = state
        .authentication_protection
        .hash_password(&request.new_password)
        .await?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let changed = transaction.execute(
        queries::auth::UPDATE_PASSWORD_AND_RESET_FLAG_IF_UNCHANGED,
        rusqlite::params![new_hash, current_user.id, user],
    )?;
    if changed != 1 {
        return Err(AppError::Conflict(
            "Password changed concurrently; retry with the current password".to_string(),
        ));
    }
    transaction.execute(queries::auth::DELETE_ALL_USER_TOKENS, [current_user.id])?;
    transaction.commit()?;
    state.admin_password_reset.complete(current_user.id);

    cleared_session_response(serde_json::json!({
        "message": "Password changed successfully"
    }))
}

fn session_response(
    body: serde_json::Value,
    tokens: &TokenResponse,
    config: &Config,
) -> AppResult<Response> {
    let mut response = Json(body).into_response();
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

fn cleared_session_response(body: serde_json::Value) -> AppResult<Response> {
    let mut response = Json(body).into_response();
    response
        .headers_mut()
        .append(SET_COOKIE, clear_access_token_cookie());
    response
        .headers_mut()
        .append(SET_COOKIE, clear_refresh_token_cookie());
    Ok(response)
}
