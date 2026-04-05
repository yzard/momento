use crate::auth::jwt::decode_access_token;
use crate::config::Config;
use crate::database::{fetch_one, queries, DbPool};
use crate::error::AppError;
use axum::{
    extract::FromRequestParts,
    http::{header::AUTHORIZATION, request::Parts},
};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct CurrentUser {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub role: String,
    pub must_change_password: bool,
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub pool: DbPool,
}

#[derive(Deserialize)]
struct TokenQuery {
    token: Option<String>,
}

#[axum::async_trait]
impl<S> FromRequestParts<S> for CurrentUser
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);

        // Try to get token from Authorization header
        let mut token_str: Option<String> = None;

        if let Some(auth_header) = parts.headers.get(AUTHORIZATION) {
            if let Ok(auth_value) = auth_header.to_str() {
                if let Some(bearer_token) = auth_value.strip_prefix("Bearer ") {
                    token_str = Some(bearer_token.to_string());
                }
            }
        }

        // Fall back to query parameter
        if token_str.is_none() {
            if let Some(query) = parts.uri.query() {
                if let Ok(params) = serde_urlencoded::from_str::<TokenQuery>(query) {
                    token_str = params.token;
                }
            }
        }

        let token =
            token_str.ok_or_else(|| AppError::Authentication("Not authenticated".to_string()))?;

        let claims = decode_access_token(&token, &app_state.config)
            .ok_or_else(|| AppError::Authentication("Invalid or expired token".to_string()))?;

        let user_id: i64 = claims
            .sub
            .parse()
            .map_err(|_| AppError::Authentication("Invalid token".to_string()))?;

        let conn = app_state.pool.get().map_err(AppError::Pool)?;

        let user = fetch_one(
            &conn,
            queries::auth::SELECT_USER_FOR_TOKEN,
            &[&user_id],
            |row| {
                Ok(UserRow {
                    id: row.get(0)?,
                    username: row.get(1)?,
                    email: row.get(2)?,
                    role: row.get(3)?,
                    must_change_password: row.get(4)?,
                    is_active: row.get(5)?,
                })
            },
        )?
        .ok_or_else(|| AppError::Authentication("User not found".to_string()))?;

        if user.is_active == 0 {
            return Err(AppError::Authentication("User is inactive".to_string()));
        }

        Ok(CurrentUser {
            id: user.id,
            username: user.username,
            email: user.email,
            role: user.role,
            must_change_password: user.must_change_password != 0,
        })
    }
}

struct UserRow {
    id: i64,
    username: String,
    email: String,
    role: String,
    must_change_password: i32,
    is_active: i32,
}

// Helper trait for extracting AppState from state
pub trait FromRef<T> {
    fn from_ref(input: &T) -> Self;
}

impl FromRef<AppState> for AppState {
    fn from_ref(input: &AppState) -> Self {
        input.clone()
    }
}

// Admin extractor
pub struct RequireAdmin(pub CurrentUser);

#[axum::async_trait]
impl<S> FromRequestParts<S> for RequireAdmin
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let user = CurrentUser::from_request_parts(parts, state).await?;

        if user.role != "admin" {
            return Err(AppError::Authorization("Admin access required".to_string()));
        }

        Ok(RequireAdmin(user))
    }
}
