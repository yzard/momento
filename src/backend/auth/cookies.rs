use axum::http::{header::COOKIE, HeaderMap, HeaderValue};

use crate::error::{AppError, AppResult};

pub const ACCESS_TOKEN_COOKIE_NAME: &str = "momento_access_token";
pub const REFRESH_TOKEN_COOKIE_NAME: &str = "momento_refresh_token";

const ACCESS_TOKEN_COOKIE_PATH: &str = "/api/v1";
const REFRESH_TOKEN_COOKIE_PATH: &str = "/api/v1/user/session";

pub fn access_token_cookie(headers: &HeaderMap) -> Option<String> {
    cookie_value(headers, ACCESS_TOKEN_COOKIE_NAME)
}

pub fn refresh_token_cookie(headers: &HeaderMap) -> Option<String> {
    cookie_value(headers, REFRESH_TOKEN_COOKIE_NAME)
}

pub fn create_access_token_cookie(token: &str, maximum_age_seconds: i64) -> AppResult<HeaderValue> {
    create_session_cookie(
        ACCESS_TOKEN_COOKIE_NAME,
        token,
        ACCESS_TOKEN_COOKIE_PATH,
        maximum_age_seconds,
    )
}

pub fn create_refresh_token_cookie(
    token: &str,
    maximum_age_seconds: i64,
) -> AppResult<HeaderValue> {
    create_session_cookie(
        REFRESH_TOKEN_COOKIE_NAME,
        token,
        REFRESH_TOKEN_COOKIE_PATH,
        maximum_age_seconds,
    )
}

pub fn clear_access_token_cookie() -> HeaderValue {
    clear_session_cookie(ACCESS_TOKEN_COOKIE_NAME, ACCESS_TOKEN_COOKIE_PATH)
}

pub fn clear_refresh_token_cookie() -> HeaderValue {
    clear_session_cookie(REFRESH_TOKEN_COOKIE_NAME, REFRESH_TOKEN_COOKIE_PATH)
}

fn cookie_value(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|cookies| cookies.to_str().ok())
        .flat_map(|cookies| cookies.split(';'))
        .filter_map(|cookie| cookie.trim().split_once('='))
        .find_map(|(name, value)| (name == cookie_name).then(|| value.to_string()))
}

fn create_session_cookie(
    name: &str,
    value: &str,
    path: &str,
    maximum_age_seconds: i64,
) -> AppResult<HeaderValue> {
    let cookie = format!(
        "{name}={value}; Path={path}; Max-Age={maximum_age_seconds}; HttpOnly; Secure; SameSite=Strict"
    );
    HeaderValue::from_str(&cookie)
        .map_err(|_| AppError::Internal("Failed to create authentication cookie".to_string()))
}

fn clear_session_cookie(name: &str, path: &str) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{name}=; Path={path}; Max-Age=0; HttpOnly; Secure; SameSite=Strict"
    ))
    .expect("static authentication cookie attributes must be valid")
}
