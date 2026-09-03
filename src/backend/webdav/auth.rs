use axum::{
    body::Body,
    extract::{ConnectInfo, State},
    http::{header, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use base64::Engine;
use std::net::SocketAddr;
use tracing::{error, warn};

use crate::auth::AppState;
use crate::database::operations::UserAuthIdentifier;

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
    let config = state.config.current();
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok());
    let peer_address = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|peer| peer.0);
    let client_ip = state
        .authentication_protection
        .client_source(request.headers(), peer_address);

    let Some(auth_value) = auth_header else {
        warn!(
            "WebDAV auth failed: missing Authorization header from {}",
            client_ip
        );
        return unauthorized_response(&config.webdav.realm);
    };

    let Some(credentials) = auth_value.strip_prefix("Basic ") else {
        warn!(
            "WebDAV auth failed: unsupported auth scheme from {}",
            client_ip
        );
        return unauthorized_response(&config.webdav.realm);
    };

    let decoded = match base64::engine::general_purpose::STANDARD.decode(credentials) {
        Ok(d) => d,
        Err(_) => {
            warn!(
                "WebDAV auth failed: invalid base64 credentials from {}",
                client_ip
            );
            return unauthorized_response(&config.webdav.realm);
        }
    };

    let cred_str = match String::from_utf8(decoded) {
        Ok(s) => s,
        Err(_) => {
            warn!(
                "WebDAV auth failed: credentials not valid UTF-8 from {}",
                client_ip
            );
            return unauthorized_response(&config.webdav.realm);
        }
    };

    let Some((username, password)) = cred_str.split_once(':') else {
        warn!(
            "WebDAV auth failed: credentials missing separator from {}",
            client_ip
        );
        return unauthorized_response(&config.webdav.realm);
    };
    if let Err(error) = state
        .authentication_protection
        .begin_password_attempt(&client_ip, username)
        .await
    {
        return error.into_response();
    }

    let user_result = match state
        .executors
        .sqlite
        .load_user_for_authentication_request(UserAuthIdentifier::Username(username.to_string()))
        .await
    {
        Ok(user) => user,
        Err(error) => {
            error!("WebDAV auth failed: database error: {}", error);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response();
        }
    };
    let verified = match state
        .authentication_protection
        .verify_password(
            password,
            user_result
                .as_ref()
                .map(|user| user.hashed_password.as_str()),
        )
        .await
    {
        Ok(verified) => verified,
        Err(error) => return error.into_response(),
    };

    let Some(user) = user_result else {
        warn!(
            "WebDAV auth failed: unknown user {} from {}",
            username, client_ip
        );
        return unauthorized_response(&config.webdav.realm);
    };

    if !user.is_active || !verified {
        warn!(
            "WebDAV auth failed: invalid credentials for user {} from {}",
            user.username, client_ip
        );
        return unauthorized_response(&config.webdav.realm);
    }
    if let Err(error) = state
        .authentication_protection
        .record_password_success(&client_ip, username)
        .await
    {
        return error.into_response();
    }

    request.extensions_mut().insert(WebDAVUser {
        id: user.id,
        username: user.username,
    });

    next.run(request).await
}

pub async fn path_guard_middleware(request: Request<Body>, next: Next) -> Response {
    let webdav_user = request.extensions().get::<WebDAVUser>().cloned();
    let client_ip = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map_or_else(|| "unknown".to_string(), |peer| peer.ip().to_string());

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
