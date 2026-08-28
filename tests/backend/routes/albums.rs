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

fn insert_aesthetic_score(
    pool: &momento_api::database::DbPool,
    media_id: i64,
    aesthetic_score: f64,
) {
    pool.get()
        .expect("database")
        .execute(
            "INSERT INTO media_aesthetics (media_id, model_type, model_version, aesthetic_score, scenic_score, simplicity_score, landscape_score, technical_quality_score) VALUES (?, 'image_aesthetics', 'test', ?, 0.0, 0.0, 0.0, 0.0)",
            rusqlite::params![media_id, aesthetic_score],
        )
        .expect("aesthetic score");
}

#[tokio::test]
async fn album_create_atomically_associates_accessible_media() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "album-owner", "album-owner@example.com");
    let media_id = create_test_media(&pool, "album-media.jpg");
    grant_media_access(&pool, media_id, user_id);
    let server = TestServer::new(app).expect("Failed to create server");
    let authorization = format!("Bearer {}", access_token(user_id));

    let created = server
        .post("/api/v1/album/create")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({ "name": "Trip", "description": "Summer", "mediaIds": [media_id] }))
        .await;
    created.assert_status_ok();
    let album: Value = created.json();
    let album_id = album["id"].as_i64().expect("album ID");
    assert_eq!(album["media"][0]["id"], json!(media_id));

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
async fn rejected_album_create_does_not_leave_a_partial_album() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "album-limit", "album-limit@example.com");
    let server = TestServer::new(app).expect("server");
    let authorization = format!("Bearer {}", access_token(user_id));

    server
        .post("/api/v1/album/create")
        .add_header(AUTHORIZATION, authorization)
        .json(&json!({
            "name": "Too large",
            "mediaIds": (1..=501).collect::<Vec<_>>()
        }))
        .await
        .assert_status_bad_request();

    let album_count = pool
        .get()
        .expect("database")
        .query_row("SELECT COUNT(*) FROM albums", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("album count");
    assert_eq!(album_count, 0);
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
        .json(&json!({ "name": "Batch", "mediaIds": [] }))
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
        .json(&json!({ "name": "Ordered", "mediaIds": [] }))
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

#[tokio::test]
async fn album_list_selects_four_highest_aesthetic_thumbnails_and_treats_missing_as_zero() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "album-thumbnails", "album-thumbnails@example.com");
    let missing_first = create_test_media(&pool, "missing-first.jpg");
    let highest = create_test_media(&pool, "highest.jpg");
    let middle = create_test_media(&pool, "middle.jpg");
    let lowest_positive = create_test_media(&pool, "lowest-positive.jpg");
    let scored_zero = create_test_media(&pool, "scored-zero.jpg");
    let missing_last = create_test_media(&pool, "missing-last.jpg");
    let media_ids = [
        missing_first,
        highest,
        middle,
        lowest_positive,
        scored_zero,
        missing_last,
    ];
    for media_id in media_ids {
        grant_media_access(&pool, media_id, user_id);
    }
    insert_aesthetic_score(&pool, highest, 0.9);
    insert_aesthetic_score(&pool, middle, 0.6);
    insert_aesthetic_score(&pool, lowest_positive, 0.2);
    insert_aesthetic_score(&pool, scored_zero, 0.0);

    let server = TestServer::new(app).expect("server");
    let authorization = format!("Bearer {}", access_token(user_id));
    let created = server
        .post("/api/v1/album/create")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({ "name": "Best four", "mediaIds": [] }))
        .await;
    let album_id = created.json::<Value>()["id"].as_i64().expect("album ID");
    server
        .post("/api/v1/album/add-media")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({ "albumId": album_id, "mediaIds": media_ids }))
        .await
        .assert_status_ok();

    let response = server
        .post("/api/v1/album/list")
        .add_header(AUTHORIZATION, authorization)
        .await;
    response.assert_status_ok();
    let album = response.json::<Value>()["albums"][0].clone();

    assert_eq!(album["mediaCount"], json!(6));
    assert_eq!(
        album["thumbnailMediaIds"],
        json!([highest, middle, lowest_positive, missing_first]),
    );

    let updated = server
        .post("/api/v1/album/update")
        .add_header(AUTHORIZATION, format!("Bearer {}", access_token(user_id)))
        .json(&json!({ "albumId": album_id, "name": "Still the best four" }))
        .await;
    updated.assert_status_ok();
    assert_eq!(
        updated.json::<Value>()["thumbnailMediaIds"],
        json!([highest, middle, lowest_positive, missing_first]),
    );
}
