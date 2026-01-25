use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use base64::Engine;
use tracing::{error, warn};

use crate::auth::{verify_password, AppState};
use crate::database::{fetch_one, queries};

#[derive(Clone)]
pub struct WebDAVUser {
    pub id: i64,
    pub username: String,
}

pub async fn basic_auth_middleware(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok());
    let client_ip = client_ip(request.headers());

    let Some(auth_value) = auth_header else {
        warn!(
            "WebDAV auth failed: missing Authorization header from {}",
            client_ip
        );
        return unauthorized_response(&state.config.webdav.realm);
    };

    let Some(credentials) = auth_value.strip_prefix("Basic ") else {
        warn!(
            "WebDAV auth failed: unsupported auth scheme from {}",
            client_ip
        );
        return unauthorized_response(&state.config.webdav.realm);
    };

    let decoded = match base64::engine::general_purpose::STANDARD.decode(credentials) {
        Ok(d) => d,
        Err(_) => {
            warn!(
                "WebDAV auth failed: invalid base64 credentials from {}",
                client_ip
            );
            return unauthorized_response(&state.config.webdav.realm);
        }
    };

    let cred_str = match String::from_utf8(decoded) {
        Ok(s) => s,
        Err(_) => {
            warn!(
                "WebDAV auth failed: credentials not valid UTF-8 from {}",
                client_ip
            );
            return unauthorized_response(&state.config.webdav.realm);
        }
    };

    let Some((username, password)) = cred_str.split_once(':') else {
        warn!(
            "WebDAV auth failed: credentials missing separator from {}",
            client_ip
        );
        return unauthorized_response(&state.config.webdav.realm);
    };

    let conn = match state.pool.get() {
        Ok(c) => c,
        Err(e) => {
            error!("WebDAV auth failed: database error: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response();
        }
    };

    let user_result: Option<(i64, String, String, i32)> = fetch_one(
        &conn,
        queries::auth::SELECT_USER_BY_USERNAME,
        &[&username],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(4)?,
                row.get::<_, i32>(5)?,
            ))
        },
    )
    .ok()
    .flatten();

    let Some((user_id, db_username, hash, is_active)) = user_result else {
        warn!(
            "WebDAV auth failed: unknown user {} from {}",
            username,
            client_ip
        );
        return unauthorized_response(&state.config.webdav.realm);
    };

    if is_active == 0 || !verify_password(password, &hash) {
        warn!(
            "WebDAV auth failed: invalid credentials for user {} from {}",
            db_username,
            client_ip
        );
        return unauthorized_response(&state.config.webdav.realm);
    }

    request.extensions_mut().insert(WebDAVUser {
        id: user_id,
        username: db_username,
    });

    next.run(request).await
}

pub async fn path_guard_middleware(request: Request<Body>, next: Next) -> Response {
    let webdav_user = request.extensions().get::<WebDAVUser>().cloned();
    let client_ip = client_ip(request.headers());

    match webdav_user {
        Some(_) => next.run(request).await,
        None => {
            warn!(
                "WebDAV path guard denied unauthenticated request from {}",
                client_ip
            );
            (StatusCode::UNAUTHORIZED, "Not authenticated").into_response()
        }
    }
}

fn unauthorized_response(realm: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(
            header::WWW_AUTHENTICATE,
            format!("Basic realm=\"{}\"", realm),
        )],
        "Authentication required",
    )
        .into_response()
}

fn client_ip(headers: &HeaderMap) -> String {
    if let Some(value) = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
    {
        if let Some(ip) = value.split(',').next() {
            let trimmed = ip.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }

    if let Some(value) = headers
        .get("x-real-ip")
        .and_then(|value| value.to_str().ok())
    {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    "unknown".to_string()
}
