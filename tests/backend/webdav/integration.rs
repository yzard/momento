use std::sync::Arc;

use axum::http::{header, Method, StatusCode};
use axum_test::{TestServer, TestServerConfig};
use base64::Engine;
use momento_api::{app::create_app, auth::hash_password, config::Config, constants::paths};

use crate::test_utils::{create_test_db, create_test_user, init_test_paths, lock_webdav_test};

#[tokio::test]
async fn test_authenticated_non_default_mount_enforces_limit_and_stages_upload() {
    let _webdav_test_guard = lock_webdav_test().await;
    init_test_paths();
    let pool = create_test_db();
    let username = format!("webdav-{}", uuid::Uuid::new_v4());
    let user_id = create_test_user(&pool, &username, "webdav@example.com");
    let password = "photo-sync-password";
    let password_hash = hash_password(password).expect("password hash");
    pool.get()
        .expect("database")
        .execute(
            "UPDATE users SET hashed_password = ?1 WHERE id = ?2",
            rusqlite::params![password_hash, user_id],
        )
        .expect("update password");

    let user_root = paths().webdav.join(&username);
    let _ = std::fs::remove_dir_all(&user_root);

    let mut config = Config::default();
    config.webdav.mount_path = "/photos".to_string();
    config.webdav.max_upload_bytes = 11;
    config.webdav.max_concurrent_requests = 1;
    let app = create_app(
        Arc::new(config),
        pool,
        Default::default(),
        Arc::new(tokio::sync::Semaphore::new(1)),
    );
    let server =
        TestServer::new_with_config(app, TestServerConfig::builder().http_transport().build())
            .expect("server");

    let options = Method::from_bytes(b"OPTIONS").expect("OPTIONS method");
    server
        .method(options.clone(), "/photos/")
        .await
        .assert_status_unauthorized();

    let credentials =
        base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
    let authorization = format!("Basic {credentials}");
    let options_response = server
        .method(options, "/photos/")
        .add_header(header::AUTHORIZATION, authorization.clone())
        .await;
    options_response.assert_status_ok();
    assert!(options_response.headers().contains_key("dav"));

    server
        .put("/photos/oversized.jpg")
        .add_header(header::AUTHORIZATION, authorization.clone())
        .add_header(header::CONTENT_LENGTH, "12")
        .bytes(axum::body::Bytes::from_static(b"twelve bytes"))
        .await
        .assert_status(StatusCode::PAYLOAD_TOO_LARGE);
    assert!(!user_root.join("oversized.jpg").exists());

    server
        .put("/photos/photo.jpg")
        .add_header(header::AUTHORIZATION, authorization)
        .add_header(header::CONTENT_LENGTH, "11")
        .bytes(axum::body::Bytes::from_static(b"photo bytes"))
        .await
        .assert_status(StatusCode::CREATED);
    assert_eq!(
        std::fs::read(user_root.join("photo.jpg")).expect("staged upload"),
        b"photo bytes"
    );

    std::fs::remove_dir_all(user_root).expect("remove WebDAV test directory");
}
