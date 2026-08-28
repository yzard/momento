use crate::test_utils::{
    create_test_app, create_test_config_manager, create_test_db, create_test_media,
    create_test_user, init_test_paths,
};
use axum::http::header::{AUTHORIZATION, COOKIE, SET_COOKIE};
use axum::http::StatusCode;
use axum_test::TestServer;
use momento_api::app::create_app;
use momento_api::auth::{create_access_token, hash_password};
use momento_api::config::Config;
use serde_json::{json, Value};
use std::sync::Arc;

fn authorization(user_id: i64, username: &str) -> String {
    let token = create_access_token(user_id, username, "user", &Config::default(), None)
        .expect("access token");
    format!("Bearer {token}")
}

fn grant_media(pool: &momento_api::database::DbPool, media_id: i64, user_id: i64, level: i32) {
    pool.get()
        .expect("connection")
        .execute(
            "INSERT INTO media_access (media_id, user_id, access_level) VALUES (?, ?, ?)",
            rusqlite::params![media_id, user_id, level],
        )
        .expect("media access");
}

#[tokio::test]
async fn public_link_creation_requires_owner_access_and_valid_options() {
    let (app, pool) = create_test_app();
    let owner_id = create_test_user(&pool, "share-owner", "share-owner@example.com");
    let viewer_id = create_test_user(&pool, "share-viewer", "share-viewer@example.com");
    let media_id = create_test_media(&pool, "shared.jpg");
    grant_media(&pool, media_id, owner_id, 2);
    grant_media(&pool, media_id, viewer_id, 1);
    let server = TestServer::new(app).expect("server");

    let viewer_response = server
        .post("/api/v1/share/create")
        .add_header(AUTHORIZATION, authorization(viewer_id, "share-viewer"))
        .json(&json!({ "mediaId": media_id }))
        .await;
    viewer_response.assert_status_not_found();

    for payload in [
        json!({ "mediaId": media_id, "password": "  " }),
        json!({ "mediaId": media_id, "expiresInDays": 0 }),
    ] {
        server
            .post("/api/v1/share/create")
            .add_header(AUTHORIZATION, authorization(owner_id, "share-owner"))
            .json(&payload)
            .await
            .assert_status_bad_request();
    }

    let created = server
        .post("/api/v1/share/create")
        .add_header(AUTHORIZATION, authorization(owner_id, "share-owner"))
        .json(&json!({ "mediaId": media_id, "password": "secret", "expiresInDays": 2 }))
        .await;
    created.assert_status_ok();
    let share: Value = created.json();
    assert_eq!(share["mediaId"], media_id);
    assert_eq!(share["hasPassword"], true);
    assert!(share["expiresAt"].as_str().is_some());
}

#[tokio::test]
async fn direct_media_sharing_validates_and_upserts_recipient_access() {
    let (app, pool) = create_test_app();
    let owner_id = create_test_user(&pool, "grant-owner", "grant-owner@example.com");
    let target_id = create_test_user(&pool, "grant-target", "grant-target@example.com");
    let media_id = create_test_media(&pool, "grant.jpg");
    grant_media(&pool, media_id, owner_id, 2);
    let server = TestServer::new(app).expect("server");
    let owner_authorization = authorization(owner_id, "grant-owner");

    for payload in [
        json!({ "mediaId": media_id, "targetUserId": target_id, "accessLevel": 0 }),
        json!({ "mediaId": media_id, "targetUserId": target_id, "accessLevel": 3 }),
        json!({ "mediaId": media_id, "targetUserId": owner_id, "accessLevel": 1 }),
    ] {
        server
            .post("/api/v1/share/media")
            .add_header(AUTHORIZATION, owner_authorization.clone())
            .json(&payload)
            .await
            .assert_status_bad_request();
    }

    server
        .post("/api/v1/share/media")
        .add_header(AUTHORIZATION, owner_authorization.clone())
        .json(&json!({ "mediaId": media_id, "targetUserId": 999999, "accessLevel": 1 }))
        .await
        .assert_status_not_found();

    server
        .post("/api/v1/share/media")
        .add_header(AUTHORIZATION, owner_authorization.clone())
        .json(&json!({ "mediaId": media_id, "targetUserId": target_id, "accessLevel": 1 }))
        .await
        .assert_status_ok();

    pool.get()
        .expect("connection")
        .execute(
            "UPDATE media_access SET deleted_at = datetime('now') WHERE media_id = ? AND user_id = ?",
            rusqlite::params![media_id, target_id],
        )
        .expect("soft delete access");

    server
        .post("/api/v1/share/media")
        .add_header(AUTHORIZATION, owner_authorization)
        .json(&json!({ "mediaId": media_id, "targetUserId": target_id, "accessLevel": 2 }))
        .await
        .assert_status_ok();

    let conn = pool.get().expect("connection");
    let (access_level, deleted_at): (i32, Option<String>) = conn
        .query_row(
            "SELECT access_level, deleted_at FROM media_access WHERE media_id = ? AND user_id = ?",
            rusqlite::params![media_id, target_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("shared access");
    assert_eq!(access_level, 2);
    assert_eq!(deleted_at, None);
}

#[tokio::test]
async fn album_sharing_requires_owner_access_and_updates_recipient_access() {
    let (app, pool) = create_test_app();
    let owner_id = create_test_user(&pool, "album-share-owner", "album-share-owner@example.com");
    let viewer_id = create_test_user(
        &pool,
        "album-share-viewer",
        "album-share-viewer@example.com",
    );
    let target_id = create_test_user(
        &pool,
        "album-share-target",
        "album-share-target@example.com",
    );
    let conn = pool.get().expect("connection");
    conn.execute(
        "INSERT INTO albums (id, user_id, name) VALUES (700, ?, 'Shared')",
        [owner_id],
    )
    .expect("album");
    conn.execute(
        "INSERT INTO album_access (album_id, user_id, access_level) VALUES (700, ?, 2), (700, ?, 1)",
        rusqlite::params![owner_id, viewer_id],
    )
    .expect("album access");
    drop(conn);
    let server = TestServer::new(app).expect("server");

    server
        .post("/api/v1/share/album")
        .add_header(
            AUTHORIZATION,
            authorization(viewer_id, "album-share-viewer"),
        )
        .json(&json!({ "albumId": 700, "targetUserId": target_id, "accessLevel": 1 }))
        .await
        .assert_status_not_found();

    for level in [1, 2] {
        server
            .post("/api/v1/share/album")
            .add_header(AUTHORIZATION, authorization(owner_id, "album-share-owner"))
            .json(&json!({ "albumId": 700, "targetUserId": target_id, "accessLevel": level }))
            .await
            .assert_status_ok();
    }

    let access_level: i32 = pool
        .get()
        .expect("connection")
        .query_row(
            "SELECT access_level FROM album_access WHERE album_id = 700 AND user_id = ?",
            [target_id],
            |row| row.get(0),
        )
        .expect("album access");
    assert_eq!(access_level, 2);
}

#[tokio::test]
async fn password_verification_rejects_expired_links() {
    let (app, pool) = create_test_app();
    let owner_id = create_test_user(&pool, "expired-owner", "expired-owner@example.com");
    let media_id = create_test_media(&pool, "expired.jpg");
    pool.get()
        .expect("connection")
        .execute(
            "INSERT INTO share_links (user_id, media_id, token, expires_at) VALUES (?, ?, 'expired-token', '2020-01-01T00:00:00Z')",
            rusqlite::params![owner_id, media_id],
        )
        .expect("share link");
    let server = TestServer::new(app).expect("server");

    let response = server
        .post("/api/v1/public/share/expired-token/verify")
        .json(&json!({ "password": "anything" }))
        .await;
    assert_eq!(response.status_code(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn public_share_password_verification_is_rate_limited() {
    init_test_paths();
    let pool = create_test_db();
    let owner_id = create_test_user(&pool, "limited-owner", "limited-owner@example.com");
    let media_id = create_test_media(&pool, "limited.jpg");
    let password_hash = hash_password("secret").expect("password hash");
    pool.get()
        .expect("connection")
        .execute(
            "INSERT INTO share_links (user_id, media_id, token, password_hash) VALUES (?1, ?2, 'limited-token', ?3)",
            rusqlite::params![owner_id, media_id, password_hash],
        )
        .expect("share link");
    let mut config = Config::default();
    config.security.password_attempts_per_identity = 2;
    config.security.password_attempts_per_source = 10;
    let app = create_app(
        create_test_config_manager(config),
        pool,
        Default::default(),
        Arc::new(tokio::sync::Semaphore::new(16)),
        None,
    );
    let server = TestServer::new(app).expect("server");

    for _ in 0..2 {
        server
            .post("/api/v1/public/share/limited-token/verify")
            .json(&json!({"password": "wrong"}))
            .await
            .assert_status_ok();
    }
    server
        .post("/api/v1/public/share/limited-token/verify")
        .json(&json!({"password": "wrong"}))
        .await
        .assert_status(StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn password_protected_public_shares_use_a_path_scoped_secure_session_cookie() {
    let (app, pool) = create_test_app();
    let owner_id = create_test_user(&pool, "protected-owner", "protected-owner@example.com");
    let media_id = create_test_media(&pool, "protected.jpg");
    let password_hash = hash_password("secret").expect("password hash");
    pool.get()
        .expect("connection")
        .execute(
            "INSERT INTO share_links (id, user_id, media_id, token, password_hash) VALUES (900, ?, ?, 'protected-token', ?)",
            rusqlite::params![owner_id, media_id, password_hash],
        )
        .expect("share link");
    let server = TestServer::new(app).expect("server");

    server
        .get("/api/v1/public/share/protected-token?password=secret")
        .await
        .assert_status_unauthorized();

    let invalid = server
        .post("/api/v1/public/share/protected-token/verify")
        .json(&json!({"password": "wrong"}))
        .await;
    invalid.assert_status_ok();
    let invalid_body: Value = invalid.json();
    assert_eq!(invalid_body["valid"], false);
    assert!(invalid.headers().get(SET_COOKIE).is_none());

    let verified = server
        .post("/api/v1/public/share/protected-token/verify")
        .json(&json!({"password": "secret"}))
        .await;
    verified.assert_status_ok();
    let set_cookie = verified
        .header(SET_COOKIE)
        .to_str()
        .expect("set-cookie")
        .to_string();
    assert!(set_cookie.contains("Path=/api/v1/public/share/protected-token"));
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("Secure"));
    assert!(set_cookie.contains("SameSite=Strict"));
    let cookie = set_cookie.split(';').next().expect("session cookie");

    let content = server
        .get("/api/v1/public/share/protected-token")
        .add_header(COOKIE, cookie)
        .await;
    content.assert_status_ok();
    content.assert_header("referrer-policy", "no-referrer");
    server
        .get("/api/v1/public/share/protected-token")
        .add_header(COOKIE, "momento_share_session=tampered")
        .await
        .assert_status_unauthorized();

    pool.get()
        .expect("connection")
        .execute(
            "INSERT INTO share_links (id, user_id, media_id, token, password_hash) VALUES (901, ?, ?, 'other-token', ?)",
            rusqlite::params![owner_id, media_id, hash_password("secret").expect("password hash")],
        )
        .expect("other share link");
    server
        .get("/api/v1/public/share/other-token")
        .add_header(COOKIE, cookie)
        .await
        .assert_status_unauthorized();

    pool.get()
        .expect("connection")
        .execute("DELETE FROM share_links WHERE id = 900", [])
        .expect("delete share link");
    server
        .get("/api/v1/public/share/protected-token")
        .add_header(COOKIE, cookie)
        .await
        .assert_status_not_found();
}
