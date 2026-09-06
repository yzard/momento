use axum::http::StatusCode;
use axum_test::TestServer;
use serde_json::Value;

use crate::test_utils::{
    create_test_app, create_test_media, create_test_user, grant_media_access, test_data_directory,
};

#[tokio::test]
async fn shared_thumbnail_distinguishes_readiness_from_access_and_missing_files() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "public-thumbnail", "public-thumbnail@example.com");
    let media_id = create_test_media(&pool, "pending.jpg");
    let other_id = create_test_media(&pool, "other.jpg");
    grant_media_access(&pool, media_id, user_id);
    pool.get()
        .unwrap()
        .execute(
            "INSERT INTO share_links (user_id, media_id, token) VALUES (?, ?, 'thumbnail-test')",
            rusqlite::params![user_id, media_id],
        )
        .unwrap();
    let server = TestServer::new(app).expect("server");
    let url = format!("/api/v1/public/share/thumbnail-test/thumbnail/{media_id}");
    for has_metadata in [true, false] {
        if !has_metadata {
            pool.get()
                .unwrap()
                .execute("DELETE FROM media_metadata WHERE media_id = ?", [media_id])
                .unwrap();
        }
        let response = server.get(&url).await;
        response.assert_status(StatusCode::CONFLICT);
        response.assert_header("cache-control", "no-store");
        assert_eq!(response.json::<Value>()["code"], "thumbnail_not_ready");
    }
    server
        .get(&format!(
            "/api/v1/public/share/thumbnail-test/thumbnail/{other_id}"
        ))
        .await
        .assert_status(StatusCode::FORBIDDEN);
    server
        .get(&format!(
            "/api/v1/public/share/invalid-token/thumbnail/{media_id}"
        ))
        .await
        .assert_status_not_found();
    pool.get().unwrap().execute(
        "INSERT INTO media_metadata (media_id, thumbnail_path) VALUES (?, '../database.sqlite')", [media_id],
    ).unwrap();
    server.get(&url).await.assert_status_not_found();
    pool.get()
        .unwrap()
        .execute(
            "UPDATE media_metadata SET thumbnail_path = 'public-ready.jpg' WHERE media_id = ?",
            [media_id],
        )
        .unwrap();
    server.get(&url).await.assert_status_not_found();
    std::fs::write(
        test_data_directory(&pool).join("thumbnails/public-ready.jpg"),
        b"thumbnail",
    )
    .unwrap();
    let response = server.get(&url).await;
    response.assert_status_ok();
    assert_eq!(response.as_bytes().as_ref(), b"thumbnail");
}
