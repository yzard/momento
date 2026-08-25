use crate::test_utils::{create_test_app, create_test_media, create_test_user, grant_media_access};
use axum::http::header::AUTHORIZATION;
use axum_test::TestServer;
use momento_api::auth::create_access_token;
use momento_api::config::Config;
use serde_json::{json, Value};

fn access_token(user_id: i64) -> String {
    create_access_token(user_id, "album-owner", "user", &Config::default(), None)
        .expect("Failed to create access token")
}

#[tokio::test]
async fn album_create_and_add_media_use_the_shared_album_contract() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "album-owner", "album-owner@example.com");
    let media_id = create_test_media(&pool, "album-media.jpg");
    grant_media_access(&pool, media_id, user_id);
    let server = TestServer::new(app).expect("Failed to create server");
    let authorization = format!("Bearer {}", access_token(user_id));

    let created = server
        .post("/api/v1/album/create")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({ "name": "Trip", "description": "Summer" }))
        .await;
    created.assert_status_ok();
    let album: Value = created.json();
    let album_id = album["id"].as_i64().expect("album ID");
    assert_eq!(album["media"], json!([]));

    server
        .post("/api/v1/album/add-media")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({ "albumId": album_id, "mediaIds": [media_id] }))
        .await
        .assert_status_ok();

    let detail = server
        .post("/api/v1/album/get")
        .add_header(AUTHORIZATION, authorization)
        .json(&json!({ "albumId": album_id }))
        .await;
    detail.assert_status_ok();
    let detail: Value = detail.json();
    assert_eq!(detail["media"][0]["id"], json!(media_id));
}

#[tokio::test]
async fn album_add_media_batches_access_checks_and_preserves_accessible_request_order() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "album-batch", "album-batch@example.com");
    let first = create_test_media(&pool, "album-first.jpg");
    let hidden = create_test_media(&pool, "album-hidden.jpg");
    let second = create_test_media(&pool, "album-second.jpg");
    grant_media_access(&pool, first, user_id);
    grant_media_access(&pool, second, user_id);
    let server = TestServer::new(app).expect("server");
    let authorization = format!("Bearer {}", access_token(user_id));
    let created = server
        .post("/api/v1/album/create")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({ "name": "Batch" }))
        .await;
    created.assert_status_ok();
    let album_id = created.json::<Value>()["id"].as_i64().expect("album ID");

    server
        .post("/api/v1/album/add-media")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({
            "albumId": album_id,
            "mediaIds": [second, hidden, 999999, first]
        }))
        .await
        .assert_status_ok();

    let detail = server
        .post("/api/v1/album/get")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({ "albumId": album_id }))
        .await;
    detail.assert_status_ok();
    let media_ids = detail.json::<Value>()["media"]
        .as_array()
        .expect("album media")
        .iter()
        .map(|media| media["id"].as_i64().expect("media ID"))
        .collect::<Vec<_>>();
    assert_eq!(media_ids, vec![second, first]);

    server
        .post("/api/v1/album/add-media")
        .add_header(AUTHORIZATION, authorization)
        .json(&json!({
            "albumId": album_id,
            "mediaIds": (1..=501).collect::<Vec<_>>()
        }))
        .await
        .assert_status_bad_request();
}

#[tokio::test]
async fn album_reorder_requires_and_atomically_applies_a_complete_permutation() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "album-reorder", "album-reorder@example.com");
    let first = create_test_media(&pool, "reorder-first.jpg");
    let second = create_test_media(&pool, "reorder-second.jpg");
    let third = create_test_media(&pool, "reorder-third.jpg");
    for media_id in [first, second, third] {
        grant_media_access(&pool, media_id, user_id);
    }
    let server = TestServer::new(app).expect("server");
    let authorization = format!("Bearer {}", access_token(user_id));
    let created = server
        .post("/api/v1/album/create")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({ "name": "Ordered" }))
        .await;
    let album_id = created.json::<Value>()["id"].as_i64().expect("album ID");
    server
        .post("/api/v1/album/add-media")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({ "albumId": album_id, "mediaIds": [first, second, third] }))
        .await
        .assert_status_ok();

    server
        .post("/api/v1/album/reorder")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({ "albumId": album_id, "mediaIds": [third, first, second] }))
        .await
        .assert_status_ok();

    for invalid_ids in [
        vec![third, first],
        vec![third, first, first],
        vec![third, first, 999999],
    ] {
        server
            .post("/api/v1/album/reorder")
            .add_header(AUTHORIZATION, authorization.clone())
            .json(&json!({ "albumId": album_id, "mediaIds": invalid_ids }))
            .await
            .assert_status_bad_request();
    }

    let detail = server
        .post("/api/v1/album/get")
        .add_header(AUTHORIZATION, authorization)
        .json(&json!({ "albumId": album_id }))
        .await;
    let media_ids = detail.json::<Value>()["media"]
        .as_array()
        .expect("album media")
        .iter()
        .map(|media| media["id"].as_i64().expect("media ID"))
        .collect::<Vec<_>>();
    assert_eq!(media_ids, vec![third, first, second]);

    let positions = pool
        .get()
        .expect("database")
        .prepare("SELECT position FROM album_media WHERE album_id = ? ORDER BY position")
        .expect("position query")
        .query_map([album_id], |row| row.get::<_, i64>(0))
        .expect("positions")
        .collect::<Result<Vec<_>, _>>()
        .expect("position rows");
    assert_eq!(positions, vec![0, 1, 2]);
}
