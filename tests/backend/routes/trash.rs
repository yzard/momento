use crate::test_utils::{
    create_test_app, create_test_media_with_gps_and_date, create_test_user, grant_media_access,
};
use axum::http::header::AUTHORIZATION;
use axum_test::TestServer;
use momento_api::auth::create_access_token;
use momento_api::config::Config;
use serde_json::{json, Value};

fn access_token(user_id: i64) -> String {
    create_access_token(user_id, "testuser", "user", &Config::default(), None)
        .expect("access token")
}

#[tokio::test]
async fn trash_thumbnail_requires_matching_deleted_media_access() {
    let (app, pool) = create_test_app();
    let owner_id = create_test_user(&pool, "trash-owner", "trash-owner@example.com");
    let other_id = create_test_user(&pool, "trash-other", "trash-other@example.com");
    let media_id = create_test_media_with_gps_and_date(
        &pool,
        "trash-thumbnail.jpg",
        40.0,
        -74.0,
        "2026-08-23T10:30:00",
    );
    grant_media_access(&pool, media_id, owner_id);
    let relative_path = format!("trash-route-tests/{media_id}.jpg");
    let thumbnail = momento_api::constants::paths()
        .thumbnails_tiny
        .join(&relative_path);
    std::fs::create_dir_all(thumbnail.parent().expect("thumbnail parent"))
        .expect("thumbnail directory");
    std::fs::write(&thumbnail, b"tiny").expect("thumbnail bytes");
    pool.get()
        .expect("database")
        .execute(
            "UPDATE media_metadata SET thumbnail_path = ? WHERE media_id = ?",
            rusqlite::params![relative_path, media_id],
        )
        .expect("thumbnail path");
    let server = TestServer::new(app).expect("server");
    let owner_authorization = format!("Bearer {}", access_token(owner_id));

    server
        .post("/api/v1/media/delete")
        .add_header(AUTHORIZATION, owner_authorization.clone())
        .json(&json!({"mediaIds": [media_id]}))
        .await
        .assert_status_ok();
    let batch = server
        .post("/api/v1/trash/thumbnails/get")
        .add_header(AUTHORIZATION, owner_authorization.clone())
        .json(&json!({"mediaIds": [media_id, 999999], "size": "tiny"}))
        .await;
    batch.assert_status_ok();
    let body: Value = batch.json();
    assert_eq!(
        body["thumbnails"][media_id.to_string()],
        "data:image/jpeg;base64,dGlueQ=="
    );
    assert_eq!(body["thumbnails"].as_object().expect("thumbnails").len(), 1);

    server
        .get(&format!("/api/v1/trash/{media_id}/thumbnail/tiny"))
        .add_header(AUTHORIZATION, owner_authorization)
        .await
        .assert_status_ok();
    server
        .get(&format!("/api/v1/trash/{media_id}/thumbnail/tiny"))
        .add_header(AUTHORIZATION, format!("Bearer {}", access_token(other_id)))
        .await
        .assert_status_not_found();
    server
        .get(&format!("/api/v1/media/{media_id}/thumbnail/tiny"))
        .add_header(AUTHORIZATION, format!("Bearer {}", access_token(owner_id)))
        .await
        .assert_status_not_found();
}
