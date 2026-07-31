use axum::{
    extract::State,
    http::{header::AUTHORIZATION, HeaderMap},
    routing::post,
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};

use crate::auth::{
    create_access_token, create_refresh_token, hash_password, hash_refresh_token,
    verify_and_migrate, AppState, CurrentUser,
};
use crate::database::{execute_query, fetch_one, insert_returning_id, queries};
use crate::error::{AppError, AppResult};
use crate::models::{ChangePasswordRequest, LogoutRequest, RefreshTokenRequest, TokenResponse};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/user/authenticate", post(login))
        .route("/user/refresh", post(refresh))
        .route("/user/logout", post(logout))
        .route("/user/change-password", post(change_password))
}

async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<TokenResponse>> {
    // Extract Basic auth credentials
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

    let conn = state.pool.get().map_err(AppError::Pool)?;

    let user = fetch_one(
        &conn,
        queries::auth::SELECT_USER_BY_USERNAME,
        &[&username],
        |row| {
            Ok(UserAuthRow {
                id: row.get(0)?,
                username: row.get(1)?,
                role: row.get(3)?,
                hashed_password: row.get(4)?,
                is_active: row.get(5)?,
            })
        },
    )?
    .ok_or_else(|| AppError::Authentication("Invalid credentials".to_string()))?;

    let (valid, new_hash) = verify_and_migrate(password, &user.hashed_password);
    if !valid {
        return Err(AppError::Authentication("Invalid credentials".to_string()));
    }

    // Migrate hash if needed
    if let Some(new_hash) = new_hash {
        let _ = execute_query(
            &conn,
            queries::auth::UPDATE_PASSWORD,
            &[&new_hash, &user.id],
        );
    }

    if user.is_active == 0 {
        return Err(AppError::Authentication("User is inactive".to_string()));
    }

    let access_token = create_access_token(user.id, &user.username, &user.role, &state.config)?;
    let (raw_refresh, token_hash, expires_at) = create_refresh_token(user.id, &state.config);

    insert_returning_id(
        &conn,
        queries::auth::INSERT_REFRESH_TOKEN,
        &[&token_hash, &user.id, &expires_at.to_rfc3339()],
    )?;

    Ok(Json(TokenResponse::new(access_token, raw_refresh)))
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
    let token_hash = hash_refresh_token(&request.refresh_token);
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let token_row = fetch_one(
        &conn,
        queries::auth::VALIDATE_REFRESH_TOKEN,
        &[&token_hash],
        |row| {
            Ok(RefreshTokenRow {
                id: row.get(0)?,
                user_id: row.get(1)?,
                revoked: row.get(3)?,
                username: row.get(4)?,
                role: row.get(5)?,
                is_active: row.get(6)?,
            })
        },
    )?
    .ok_or_else(|| AppError::Authentication("Invalid refresh token".to_string()))?;

    if token_row.revoked != 0 {
        return Err(AppError::Authentication(
            "Token has been revoked".to_string(),
        ));
    }

    if token_row.is_active == 0 {
        return Err(AppError::Authentication("User is inactive".to_string()));
    }

    // Revoke old token
    execute_query(&conn, queries::auth::REVOKE_REFRESH_TOKEN, &[&token_row.id])?;
    execute_query(&conn, queries::auth::DELETE_REVOKED_TOKEN, &[&token_row.id])?;

    // Create new tokens
    let access_token = create_access_token(
        token_row.user_id,
        &token_row.username,
        &token_row.role,
        &state.config,
    )?;
    let (raw_refresh, new_token_hash, expires_at) =
        create_refresh_token(token_row.user_id, &state.config);

    insert_returning_id(
        &conn,
        queries::auth::INSERT_REFRESH_TOKEN,
        &[
            &new_token_hash,
            &token_row.user_id,
            &expires_at.to_rfc3339(),
        ],
    )?;

    Ok(Json(TokenResponse::new(access_token, raw_refresh)))
}

struct RefreshTokenRow {
    id: i64,
    user_id: i64,
    revoked: i32,
    username: String,
    role: String,
    is_active: i32,
}

async fn logout(
    State(state): State<AppState>,
    Json(request): Json<LogoutRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let token_hash = hash_refresh_token(&request.refresh_token);
    let conn = state.pool.get().map_err(AppError::Pool)?;

    execute_query(
        &conn,
        queries::auth::REVOKE_REFRESH_TOKEN_BY_HASH,
        &[&token_hash],
    )?;

    Ok(Json(
        serde_json::json!({"message": "Logged out successfully"}),
    ))
}

async fn change_password(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<ChangePasswordRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let user = fetch_one(
        &conn,
        queries::auth::SELECT_PASSWORD_HASH,
        &[&current_user.id],
        |row| row.get::<_, String>(0),
    )?
    .ok_or_else(|| AppError::NotFound("User not found".to_string()))?;

    let (valid, _) = verify_and_migrate(&request.current_password, &user);
    if !valid {
        return Err(AppError::BadRequest(
            "Current password is incorrect".to_string(),
        ));
    }

    if request.new_password.len() < 8 {
        return Err(AppError::BadRequest(
            "Password must be at least 8 characters".to_string(),
        ));
    }

    let new_hash = hash_password(&request.new_password)
        .map_err(|e| AppError::Internal(format!("Failed to hash password: {}", e)))?;

    execute_query(
        &conn,
        queries::auth::UPDATE_PASSWORD_AND_RESET_FLAG,
        &[&new_hash, &current_user.id],
    )?;

    execute_query(
        &conn,
        queries::auth::REVOKE_ALL_USER_TOKENS,
        &[&current_user.id],
    )?;

    Ok(Json(
        serde_json::json!({"message": "Password changed successfully"}),
    ))
}
