use crate::auth::{
    jwt::{decode_access_token, decode_media_access_ticket},
    AdminPasswordReset,
};
use crate::config::ConfigManager;
use crate::database::{fetch_one, queries, DbPool};
use crate::error::AppError;
use crate::models::MediaAccessResource;
use axum::{
    body::Body,
    extract::{FromRequestParts, Request, State},
    http::{header::AUTHORIZATION, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

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
    pub config: ConfigManager,
    pub pool: DbPool,
    pub llm_transport: crate::processor::ai::transport::TransportHandle,
    pub webdav_request_gate: crate::webdav::WebDAVRequestGate,
    pub admin_password_reset: AdminPasswordReset,
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
        if let Some(current_user) = parts.extensions.get::<CurrentUser>() {
            return Ok(current_user.clone());
        }

        let token = bearer_token(parts)?;
        let config = app_state.config.current();
        let claims = decode_access_token(token, &config)
            .ok_or_else(|| AppError::Authentication("Invalid or expired token".to_string()))?;

        let user_id: i64 = claims
            .sub
            .parse()
            .map_err(|_| AppError::Authentication("Invalid token".to_string()))?;
        if let Some(reset_id) = claims.admin_reset_id {
            if !app_state
                .admin_password_reset
                .accepts_temporary_token(user_id, &reset_id)
            {
                return Err(AppError::Authentication(
                    "Administrator reset token is no longer valid".to_string(),
                ));
            }
        }

        load_current_user(&app_state, user_id)
    }
}

#[derive(Deserialize)]
struct MediaAccessTicketQuery {
    ticket: Option<String>,
}

pub struct MediaAccessAuthorization {
    user_id: i64,
    ticket_scope: Option<(i64, MediaAccessResource)>,
}

impl MediaAccessAuthorization {
    pub fn authorize(&self, media_id: i64, resource: MediaAccessResource) -> Result<i64, AppError> {
        if let Some((ticket_media_id, ticket_resource)) = self.ticket_scope {
            if ticket_media_id != media_id || ticket_resource != resource {
                return Err(AppError::Authorization(
                    "Media access ticket does not match the requested resource".to_string(),
                ));
            }
        }
        Ok(self.user_id)
    }
}

#[axum::async_trait]
impl<S> FromRequestParts<S> for MediaAccessAuthorization
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);
        if let Some(ticket) = media_access_ticket(parts.uri.query())? {
            let config = app_state.config.current();
            let claims = decode_media_access_ticket(&ticket, &config).ok_or_else(|| {
                AppError::Authentication("Invalid or expired media access ticket".to_string())
            })?;
            let user_id = claims
                .sub
                .parse::<i64>()
                .map_err(|_| AppError::Authentication("Invalid media access ticket".to_string()))?;
            let current_user = load_current_user(&app_state, user_id)?;
            if current_user.must_change_password {
                return Err(AppError::Forbidden("Password change required".to_string()));
            }

            return Ok(Self {
                user_id,
                ticket_scope: Some((claims.media_id, claims.resource)),
            });
        }

        if parts.headers.contains_key(AUTHORIZATION) {
            let current_user = CurrentUser::from_request_parts(parts, state).await?;
            return Ok(Self {
                user_id: current_user.id,
                ticket_scope: None,
            });
        }

        Err(AppError::Authentication(
            "Media access ticket is required".to_string(),
        ))
    }
}

fn media_access_ticket(query: Option<&str>) -> Result<Option<String>, AppError> {
    let Some(query) = query else {
        return Ok(None);
    };
    let ticket_query = serde_urlencoded::from_str::<MediaAccessTicketQuery>(query)
        .map_err(|_| AppError::Authentication("Invalid media access ticket".to_string()))?;
    Ok(ticket_query.ticket)
}

fn bearer_token(parts: &Parts) -> Result<&str, AppError> {
    parts
        .headers
        .get(AUTHORIZATION)
        .and_then(|header| header.to_str().ok())
        .and_then(|authorization| authorization.strip_prefix("Bearer "))
        .ok_or_else(|| AppError::Authentication("Not authenticated".to_string()))
}

fn load_current_user(app_state: &AppState, user_id: i64) -> Result<CurrentUser, AppError> {
    let connection = app_state.pool.get().map_err(AppError::Pool)?;
    let user = fetch_one(
        &connection,
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
        must_change_password: user.must_change_password != 0
            || app_state
                .admin_password_reset
                .requires_password_change(user.id),
    })
}

pub async fn password_change_guard(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if is_password_change_route(request.uri().path()) {
        return next.run(request).await;
    }

    let (mut parts, body) = request.into_parts();
    if let Ok(current_user) = CurrentUser::from_request_parts(&mut parts, &state).await {
        if current_user.must_change_password {
            return AppError::Forbidden("Password change required".to_string()).into_response();
        }
        parts.extensions.insert(current_user);
    }
    next.run(Request::from_parts(parts, body)).await
}

fn is_password_change_route(path: &str) -> bool {
    [
        "/healthcheck",
        "/user/authenticate",
        "/user/refresh",
        "/user/logout",
        "/user/get",
        "/user/change-password",
        "/api/v1/healthcheck",
        "/api/v1/user/authenticate",
        "/api/v1/user/refresh",
        "/api/v1/user/logout",
        "/api/v1/user/get",
        "/api/v1/user/change-password",
    ]
    .contains(&path)
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
