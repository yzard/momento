use axum::http::header::{AUTHORIZATION, COOKIE, SET_COOKIE};
use axum_test::TestServer;
use base64::Engine;
use momento_api::{
    app::create_app,
    auth::{hash_password, hash_refresh_token, prepare_admin_password_reset, verify_password},
    config::Config,
};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::test_utils::{
    create_test_config_manager, create_test_db, create_test_user, init_test_paths,
};

fn basic_credentials(username: &str, password: &str) -> String {
    let encoded =
        base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
    format!("Basic {encoded}")
}

fn create_admin_fixture() -> (momento_api::database::DbPool, i64) {
    let pool = create_test_db();
    let admin_id = create_test_user(&pool, "stored-admin", "stored-admin@example.com");
    let password_hash = hash_password("stored-password").expect("password hash");
    pool.get()
        .expect("database")
        .execute(
            "UPDATE users SET role = 'admin', hashed_password = ?1, must_change_password = 0 WHERE id = ?2",
            rusqlite::params![password_hash, admin_id],
        )
        .expect("configure administrator");
    (pool, admin_id)
}

fn create_server(pool: momento_api::database::DbPool, reset_user_id: Option<i64>) -> TestServer {
    create_server_with_config(pool, reset_user_id, Config::default())
}

fn create_server_with_config(
    pool: momento_api::database::DbPool,
    reset_user_id: Option<i64>,
    config: Config,
) -> TestServer {
    init_test_paths();
    let config_manager = create_test_config_manager(config);
    let app = create_app(
        config_manager,
        pool,
        Default::default(),
        Arc::new(tokio::sync::Semaphore::new(16)),
        reset_user_id,
    );
    TestServer::new(app).expect("server")
}

#[tokio::test]
async fn token_and_browser_password_logins_share_the_same_rate_limit() {
    let pool = create_test_db();
    let mut config = Config::default();
    config.security.password_attempts_per_identity = 2;
    config.security.password_attempts_per_source = 10;
    let server = create_server_with_config(pool, None, config);

    server
        .post("/api/v1/user/authenticate")
        .add_header(AUTHORIZATION, basic_credentials("missing-user", "wrong"))
        .await
        .assert_status_unauthorized();
    server
        .post("/api/v1/user/session/create")
        .add_header(AUTHORIZATION, basic_credentials("missing-user", "wrong"))
        .await
        .assert_status_unauthorized();
    let limited = server
        .post("/api/v1/user/authenticate")
        .add_header(AUTHORIZATION, basic_credentials("missing-user", "wrong"))
        .await;
    limited.assert_status(axum::http::StatusCode::TOO_MANY_REQUESTS);
    assert!(limited
        .headers()
        .contains_key(axum::http::header::RETRY_AFTER));
}

#[tokio::test]
async fn buffered_json_routes_enforce_the_configured_body_limit() {
    init_test_paths();
    let pool = create_test_db();
    let mut config = Config::default();
    config.server.api_request_body_max_bytes = 16;
    config.server.request_log_body_max_bytes = 8;
    let config_manager = create_test_config_manager(config);
    let app = create_app(
        config_manager,
        pool,
        Default::default(),
        Arc::new(tokio::sync::Semaphore::new(16)),
        None,
    );
    let server = TestServer::new(app).expect("server");

    server
        .post("/api/v1/user/refresh")
        .json(&json!({"refreshToken": "request larger than sixteen bytes"}))
        .await
        .assert_status(axum::http::StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn temporary_admin_credentials_apply_only_to_the_reset_process() {
    let (pool, admin_id) = create_admin_fixture();
    prepare_admin_password_reset(&pool, admin_id).expect("prepare reset");
    let reset_server = create_server(pool.clone(), Some(admin_id));

    let reset_login = reset_server
        .post("/api/v1/user/authenticate")
        .add_header(AUTHORIZATION, basic_credentials("admin", "admin"))
        .await;
    reset_login.assert_status_ok();
    let access_token = reset_login.json::<Value>()["accessToken"]
        .as_str()
        .expect("access token")
        .to_string();
    let temporary_refresh_token = reset_login.json::<Value>()["refreshToken"]
        .as_str()
        .expect("refresh token")
        .to_string();
    let current_user = reset_server
        .post("/api/v1/user/get")
        .add_header(AUTHORIZATION, format!("Bearer {access_token}"))
        .await;
    current_user.assert_status_ok();
    let current_user = current_user.json::<Value>();
    assert_eq!(current_user["username"], "stored-admin");
    assert_eq!(current_user["role"], "admin");
    assert_eq!(current_user["mustChangePassword"], true);
    reset_server
        .post("/api/v1/user/list")
        .add_header(AUTHORIZATION, format!("Bearer {access_token}"))
        .await
        .assert_status_forbidden();
    reset_server
        .post("/api/v1/user/refresh")
        .json(&json!({"refreshToken": temporary_refresh_token}))
        .await
        .assert_status_unauthorized();

    let later_reset_server = create_server(pool.clone(), Some(admin_id));
    later_reset_server
        .post("/api/v1/user/get")
        .add_header(AUTHORIZATION, format!("Bearer {access_token}"))
        .await
        .assert_status_unauthorized();

    let restarted_server = create_server(pool, None);
    restarted_server
        .post("/api/v1/user/get")
        .add_header(AUTHORIZATION, format!("Bearer {access_token}"))
        .await
        .assert_status_unauthorized();
    restarted_server
        .post("/api/v1/user/authenticate")
        .add_header(AUTHORIZATION, basic_credentials("admin", "admin"))
        .await
        .assert_status_unauthorized();
    let stored_login = restarted_server
        .post("/api/v1/user/authenticate")
        .add_header(
            AUTHORIZATION,
            basic_credentials("stored-admin", "stored-password"),
        )
        .await;
    stored_login.assert_status_ok();
    let stored_access_token = stored_login.json::<Value>()["accessToken"]
        .as_str()
        .expect("stored access token")
        .to_string();
    let restarted_user = restarted_server
        .post("/api/v1/user/get")
        .add_header(AUTHORIZATION, format!("Bearer {stored_access_token}"))
        .await;
    restarted_user.assert_status_ok();
    assert_eq!(restarted_user.json::<Value>()["mustChangePassword"], false);
}

#[tokio::test]
async fn temporary_admin_password_can_be_replaced_without_the_stored_password() {
    let (pool, admin_id) = create_admin_fixture();
    prepare_admin_password_reset(&pool, admin_id).expect("prepare reset");
    let server = create_server(pool.clone(), Some(admin_id));
    let login = server
        .post("/api/v1/user/authenticate")
        .add_header(AUTHORIZATION, basic_credentials("admin", "admin"))
        .await;
    login.assert_status_ok();
    let access_token = login.json::<Value>()["accessToken"]
        .as_str()
        .expect("access token")
        .to_string();

    server
        .post("/api/v1/user/change-password")
        .add_header(AUTHORIZATION, format!("Bearer {access_token}"))
        .json(&json!({
            "currentPassword": "admin",
            "newPassword": "new-admin-password"
        }))
        .await
        .assert_status_ok();

    let connection = pool.get().expect("database");
    let (password_hash, must_change_password): (String, i32) = connection
        .query_row(
            "SELECT hashed_password, must_change_password FROM users WHERE id = ?1",
            [admin_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("administrator");
    assert!(verify_password("new-admin-password", &password_hash));
    assert_eq!(must_change_password, 0);
    drop(connection);

    server
        .post("/api/v1/user/authenticate")
        .add_header(AUTHORIZATION, basic_credentials("admin", "admin"))
        .await
        .assert_status_unauthorized();
}

#[tokio::test]
async fn refresh_rejects_expired_tokens_and_rotates_a_token_once() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "refresh-user", "refresh@example.com");
    pool.get()
        .expect("database")
        .execute(
            "UPDATE users SET hashed_password = ? WHERE id = ?",
            rusqlite::params![
                hash_password("refresh-password").expect("password hash"),
                user_id
            ],
        )
        .expect("password");
    let server = create_server(pool.clone(), None);

    let login = server
        .post("/api/v1/user/authenticate")
        .add_header(
            AUTHORIZATION,
            basic_credentials("refresh-user", "refresh-password"),
        )
        .await;
    login.assert_status_ok();
    let refresh_token = login.json::<Value>()["refreshToken"]
        .as_str()
        .expect("refresh token")
        .to_string();
    let first_refresh = server
        .post("/api/v1/user/refresh")
        .json(&json!({"refreshToken": refresh_token}))
        .await;
    first_refresh.assert_status_ok();
    server
        .post("/api/v1/user/refresh")
        .json(&json!({"refreshToken": refresh_token}))
        .await
        .assert_status_unauthorized();

    let expired_token = "expired-refresh-token";
    pool.get()
        .expect("database")
        .execute(
            "INSERT INTO refresh_tokens (token_hash, user_id, expires_at) VALUES (?, ?, ?)",
            rusqlite::params![
                hash_refresh_token(expired_token),
                user_id,
                "2000-01-01T00:00:00+00:00"
            ],
        )
        .expect("expired token");
    server
        .post("/api/v1/user/refresh")
        .json(&json!({"refreshToken": expired_token}))
        .await
        .assert_status_unauthorized();
}

#[tokio::test]
async fn browser_authentication_uses_http_only_cookies_without_returning_tokens() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "browser-user", "browser@example.com");
    pool.get()
        .expect("database")
        .execute(
            "UPDATE users SET hashed_password = ?, must_change_password = 0 WHERE id = ?",
            rusqlite::params![
                hash_password("browser-password").expect("password hash"),
                user_id
            ],
        )
        .expect("password");
    let server = create_server(pool.clone(), None);

    let login = server
        .post("/api/v1/user/session/create")
        .add_header(
            AUTHORIZATION,
            basic_credentials("browser-user", "browser-password"),
        )
        .await;
    login.assert_status_ok();
    assert!(login.json::<Value>().get("accessToken").is_none());
    let set_cookies = login
        .headers()
        .get_all(SET_COOKIE)
        .iter()
        .map(|value| value.to_str().expect("set-cookie").to_string())
        .collect::<Vec<_>>();
    assert_eq!(set_cookies.len(), 2);
    assert!(set_cookies.iter().all(|cookie| cookie.contains("HttpOnly")));
    assert!(set_cookies.iter().all(|cookie| cookie.contains("Secure")));
    assert!(set_cookies
        .iter()
        .all(|cookie| cookie.contains("SameSite=Strict")));

    let access_cookie = set_cookies
        .iter()
        .find(|cookie| cookie.starts_with("momento_access_token="))
        .expect("access cookie")
        .split(';')
        .next()
        .expect("access cookie pair");
    let current_user = server
        .post("/api/v1/user/get")
        .add_header(COOKIE, access_cookie)
        .await;
    current_user.assert_status_ok();
    assert_eq!(current_user.json::<Value>()["username"], "browser-user");
}

#[tokio::test]
async fn browser_refresh_rotates_cookie_and_logout_clears_both_cookies() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "cookie-user", "cookie@example.com");
    pool.get()
        .expect("database")
        .execute(
            "UPDATE users SET hashed_password = ?, must_change_password = 0 WHERE id = ?",
            rusqlite::params![
                hash_password("cookie-password").expect("password hash"),
                user_id
            ],
        )
        .expect("password");
    let server = create_server(pool.clone(), None);

    let login = server
        .post("/api/v1/user/session/create")
        .add_header(
            AUTHORIZATION,
            basic_credentials("cookie-user", "cookie-password"),
        )
        .await;
    let refresh_cookie = login
        .headers()
        .get_all(SET_COOKIE)
        .iter()
        .find_map(|value| {
            let cookie = value.to_str().ok()?;
            cookie
                .starts_with("momento_refresh_token=")
                .then(|| cookie.split(';').next().expect("cookie pair").to_string())
        })
        .expect("refresh cookie");

    let refreshed = server
        .post("/api/v1/user/session/refresh")
        .add_header(COOKIE, &refresh_cookie)
        .await;
    refreshed.assert_status_ok();
    assert_eq!(refreshed.headers().get_all(SET_COOKIE).iter().count(), 2);
    server
        .post("/api/v1/user/session/refresh")
        .add_header(COOKIE, refresh_cookie)
        .await
        .assert_status_unauthorized();

    let rotated_refresh_cookie = refreshed
        .headers()
        .get_all(SET_COOKIE)
        .iter()
        .find_map(|value| {
            let cookie = value.to_str().ok()?;
            cookie
                .starts_with("momento_refresh_token=")
                .then(|| cookie.split(';').next().expect("cookie pair").to_string())
        })
        .expect("rotated refresh cookie");
    let logout = server
        .post("/api/v1/user/session/delete")
        .add_header(COOKIE, rotated_refresh_cookie)
        .await;
    logout.assert_status_ok();
    let cleared_cookies = logout
        .headers()
        .get_all(SET_COOKIE)
        .iter()
        .map(|value| value.to_str().expect("clear cookie"))
        .collect::<Vec<_>>();
    assert_eq!(cleared_cookies.len(), 2);
    assert!(cleared_cookies
        .iter()
        .all(|cookie| cookie.contains("Max-Age=0")));
    let token_count: i64 = pool
        .get()
        .expect("database")
        .query_row(
            "SELECT COUNT(*) FROM refresh_tokens WHERE user_id = ?1",
            [user_id],
            |row| row.get(0),
        )
        .expect("refresh-token count");
    assert_eq!(token_count, 0);
}
