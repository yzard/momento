use crate::test_utils::{
    create_test_app, create_test_media_with_gps_and_date, create_test_user, grant_media_access,
    test_data_directory,
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
    let thumbnail = test_data_directory(&pool)
        .join("thumbnails_tiny")
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
    server
        .post("/api/v1/trash/thumbnails/get")
        .add_header(AUTHORIZATION, owner_authorization.clone())
        .json(&json!({"mediaIds": [media_id, 999999], "size": "tiny"}))
        .await
        .assert_status_not_found();

    let response = server
        .get(&format!("/api/v1/trash/{media_id}/thumbnail/tiny"))
        .add_header(AUTHORIZATION, owner_authorization)
        .await;
    response.assert_status_ok();
    assert_eq!(response.as_bytes().as_ref(), b"tiny");
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

#[tokio::test]
async fn trash_list_and_restore_use_bounded_owned_database_results() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "trash-restore", "trash-restore@example.com");
    let media_id = create_test_media_with_gps_and_date(
        &pool,
        "trash-restore.jpg",
        40.0,
        -74.0,
        "2026-08-23T10:30:00",
    );
    grant_media_access(&pool, media_id, user_id);
    let server = TestServer::new(app).expect("server");
    let authorization = format!("Bearer {}", access_token(user_id));

    server
        .post("/api/v1/media/delete")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({"mediaIds": [media_id]}))
        .await
        .assert_status_ok();
    let listed = server
        .post("/api/v1/trash/list")
        .add_header(AUTHORIZATION, authorization.clone())
        .await;
    listed.assert_status_ok();
    let listed: Value = listed.json();
    assert_eq!(listed["totalCount"], 1);
    assert_eq!(listed["items"][0]["id"], media_id);

    let restored = server
        .post("/api/v1/trash/restore")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({"mediaIds": [media_id, media_id]}))
        .await;
    restored.assert_status_ok();
    assert_eq!(restored.json::<Value>()["affectedCount"], 1);

    let oversized = (1..=501).collect::<Vec<_>>();
    server
        .post("/api/v1/trash/restore")
        .add_header(AUTHORIZATION, authorization)
        .json(&json!({"mediaIds": oversized}))
        .await
        .assert_status_bad_request();
}
