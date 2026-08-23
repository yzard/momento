use std::sync::Arc;

use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use axum_test::{TestServer, TestServerConfig};
use base64::Engine;
use momento_api::{app::create_app, auth::hash_password, config::Config, constants::paths};
use tower::ServiceExt;

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
    let readiness_pool = pool.clone();
    let app = create_app(
        Arc::new(config),
        pool,
        Default::default(),
        Arc::new(tokio::sync::Semaphore::new(1)),
        None,
    );
    let direct_app = app.clone();
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
        .add_header(header::AUTHORIZATION, authorization.clone())
        .add_header(header::CONTENT_LENGTH, "11")
        .bytes(axum::body::Bytes::from_static(b"photo bytes"))
        .await
        .assert_status(StatusCode::CREATED);
    assert_eq!(
        std::fs::read(user_root.join("photo.jpg")).expect("staged upload"),
        b"photo bytes"
    );

    let chunked_video = Request::builder()
        .method(Method::PUT)
        .uri("/photos/video.mp4")
        .header(header::AUTHORIZATION, authorization.clone())
        .header(header::TRANSFER_ENCODING, "chunked")
        .body(Body::from("video bytes"))
        .expect("chunked video request");
    let chunked_video_response = direct_app
        .clone()
        .oneshot(chunked_video)
        .await
        .expect("chunked video response");
    assert_eq!(chunked_video_response.status(), StatusCode::CREATED);
    assert_eq!(
        std::fs::read(user_root.join("video.mp4")).expect("staged video"),
        b"video bytes"
    );
    drop(chunked_video_response);
    let ready_files = readiness_pool
        .get()
        .expect("database")
        .prepare("SELECT file_path FROM webdav_ready_files WHERE user_id = ? ORDER BY file_path")
        .expect("ready file query")
        .query_map([user_id], |row| row.get::<_, String>(0))
        .expect("ready file rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("ready files");
    assert_eq!(ready_files, vec!["photo.jpg", "video.mp4"]);

    let oversized_chunked_video = Request::builder()
        .method(Method::PUT)
        .uri("/photos/oversized-video.mp4")
        .header(header::AUTHORIZATION, authorization)
        .header(header::TRANSFER_ENCODING, "chunked")
        .body(Body::from("twelve bytes"))
        .expect("oversized chunked video request");
    let oversized_chunked_response = direct_app
        .oneshot(oversized_chunked_video)
        .await
        .expect("oversized chunked response");
    assert_eq!(
        oversized_chunked_response.status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert!(!user_root.join("oversized-video.mp4").exists());
    let oversized_ready_count: i64 = readiness_pool
        .get()
        .expect("database")
        .query_row(
            "SELECT COUNT(*) FROM webdav_ready_files WHERE user_id = ? AND file_path = 'oversized-video.mp4'",
            [user_id],
            |row| row.get(0),
        )
        .expect("oversized readiness");
    assert_eq!(oversized_ready_count, 0);

    std::fs::remove_dir_all(user_root).expect("remove WebDAV test directory");
}
