use axum::http::header::AUTHORIZATION;
use axum_test::TestServer;
use momento_api::auth::create_access_token;
use momento_api::config::Config;
use momento_api::constants::paths;
use serde_json::{json, Value};

use crate::test_utils::{create_test_app, create_test_media, create_test_user, grant_media_access};

fn token(user_id: i64) -> String {
    create_access_token(user_id, "places", "user", &Config::default(), None).expect("token")
}

fn set_place(
    pool: &momento_api::database::DbPool,
    media_id: i64,
    city: &str,
    state: Option<&str>,
    country: &str,
) {
    pool.get()
        .expect("connection")
        .execute(
            "UPDATE media_metadata SET location_city = ?, location_state = ?, location_country = ? WHERE media_id = ?",
            rusqlite::params![city, state, country, media_id],
        )
        .expect("place metadata");
}

fn insert_aesthetics(pool: &momento_api::database::DbPool, media_id: i64, score: f64) {
    pool.get()
        .expect("connection")
        .execute(
            "INSERT INTO media_aesthetics (media_id, model_type, model_version, aesthetic_score, scenic_score, simplicity_score, landscape_score, technical_quality_score) VALUES (?, 'image_aesthetics', 'test', ?, ?, ?, ?, ?)",
            rusqlite::params![media_id, score, score, score, score, score],
        )
        .expect("aesthetics");
}

fn set_place_thumbnail(pool: &momento_api::database::DbPool, media_id: i64, bytes: &[u8]) {
    let relative_path = format!("{media_id}/thumbnail.jpg");
    pool.get()
        .expect("connection")
        .execute(
            "UPDATE media_metadata SET thumbnail_path = ? WHERE media_id = ?",
            rusqlite::params![relative_path, media_id],
        )
        .expect("thumbnail path");
    let thumbnail_path = paths().thumbnails_places.join(&relative_path);
    std::fs::create_dir_all(thumbnail_path.parent().expect("thumbnail parent"))
        .expect("thumbnail directory");
    std::fs::write(thumbnail_path, bytes).expect("place thumbnail");
}

#[tokio::test]
async fn places_are_access_filtered_and_preserve_nullable_state_identity() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "place-viewer", "place-viewer@example.com");
    let california_media = create_test_media(&pool, "california.jpg");
    let state_unknown_media = create_test_media(&pool, "state-unknown.jpg");
    let empty_state_media = create_test_media(&pool, "state-empty.jpg");
    let hidden_media = create_test_media(&pool, "hidden.jpg");
    set_place(
        &pool,
        california_media,
        "Springfield",
        Some("California"),
        "United States",
    );
    set_place(
        &pool,
        empty_state_media,
        "Springfield",
        Some(""),
        "United States",
    );
    set_place(
        &pool,
        state_unknown_media,
        "Springfield",
        None,
        "United States",
    );
    set_place(&pool, hidden_media, "Hidden City", None, "United States");
    grant_media_access(&pool, california_media, user_id);
    grant_media_access(&pool, state_unknown_media, user_id);
    grant_media_access(&pool, empty_state_media, user_id);
    let server = TestServer::new(app).expect("server");

    let response = server
        .post("/api/v1/places/list")
        .add_header(AUTHORIZATION, format!("Bearer {}", token(user_id)))
        .json(&json!({"cursor": null, "limit": 1}))
        .await;
    response.assert_status_ok();
    let mut body = response.json::<Value>();
    let mut places = Vec::new();
    loop {
        places.extend(body["places"].as_array().expect("places").iter().cloned());
        if !body["hasMore"].as_bool().expect("hasMore") {
            break;
        }
        let response = server
            .post("/api/v1/places/list")
            .add_header(AUTHORIZATION, format!("Bearer {}", token(user_id)))
            .json(&json!({"cursor": body["nextCursor"], "limit": 1}))
            .await;
        response.assert_status_ok();
        body = response.json::<Value>();
    }
    assert_eq!(places.len(), 3);
    assert!(places.iter().all(|place| place["city"] == "Springfield"));
    assert!(places[0]["state"].is_null());
    assert_eq!(places[1]["state"], "");
    assert_eq!(places[2]["state"], "California");
    let place_ids = places
        .iter()
        .map(|place| place["placeId"].as_str().expect("place ID"))
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(place_ids.len(), 3);
}

#[tokio::test]
async fn place_cover_uses_hybrid_score_and_detail_paginates() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "place-cover", "place-cover@example.com");
    let iconic_media = create_test_media(&pool, "iconic.jpg");
    let cluttered_media = create_test_media(&pool, "cluttered.jpg");
    for media_id in [iconic_media, cluttered_media] {
        set_place(&pool, media_id, "Paris", None, "France");
        grant_media_access(&pool, media_id, user_id);
    }
    insert_aesthetics(&pool, iconic_media, 0.8);
    insert_aesthetics(&pool, cluttered_media, 0.9);
    set_place_thumbnail(&pool, iconic_media, &[1, 2, 3]);
    set_place_thumbnail(&pool, cluttered_media, &[4, 5, 6]);
    let connection = pool.get().expect("connection");
    connection
        .execute(
            "INSERT INTO media_text (media_id, model_type, model_version, string) VALUES (?, 'ocr', 'test', ?)",
            rusqlite::params![cluttered_media, "text".repeat(100)],
        )
        .expect("ocr clutter");
    connection.execute("INSERT INTO media_faces (media_id, input_sequence, face_index, x, y, width, height, confidence, quality, frontality, embedding, crop_path) VALUES (?, 0, 0, 0.1, 0.1, 0.6, 0.6, 1, 1, 1, X'00000000', 'faces/cluttered.jpg')", [cluttered_media]).expect("dominant face");
    drop(connection);
    let server = TestServer::new(app).expect("server");
    let authorization = format!("Bearer {}", token(user_id));

    let list = server
        .post("/api/v1/places/list")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({"cursor": null, "limit": 1}))
        .await;
    list.assert_status_ok();
    let list = list.json::<Value>();
    assert!(list["places"][0].get("representativeMediaId").is_none());
    assert_eq!(list["places"][0]["mediaCount"], 2);
    let place_id = list["places"][0]["placeId"].as_str().expect("place ID");

    let initial_thumbnail = server
        .post("/api/v1/places/thumbnail")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({"placeId": place_id}))
        .await;
    initial_thumbnail.assert_status_ok();
    assert_eq!(
        initial_thumbnail.json::<Value>()["thumbnail"],
        "data:image/jpeg;base64,AQID"
    );

    let new_best_media = create_test_media(&pool, "new-best.jpg");
    set_place(&pool, new_best_media, "Paris", None, "France");
    grant_media_access(&pool, new_best_media, user_id);
    insert_aesthetics(&pool, new_best_media, 1.0);
    set_place_thumbnail(&pool, new_best_media, &[7, 8, 9]);
    let updated_thumbnail = server
        .post("/api/v1/places/thumbnail")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({"placeId": place_id}))
        .await;
    updated_thumbnail.assert_status_ok();
    assert_eq!(
        updated_thumbnail.json::<Value>()["thumbnail"],
        "data:image/jpeg;base64,BwgJ"
    );

    pool.get()
        .expect("connection")
        .execute(
            "UPDATE media_access SET deleted_at = CURRENT_TIMESTAMP WHERE media_id = ? AND user_id = ?",
            rusqlite::params![new_best_media, user_id],
        )
        .expect("remove access");
    let thumbnail_after_delete = server
        .post("/api/v1/places/thumbnail")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({"placeId": place_id}))
        .await;
    thumbnail_after_delete.assert_status_ok();
    assert_eq!(
        thumbnail_after_delete.json::<Value>()["thumbnail"],
        "data:image/jpeg;base64,AQID"
    );

    let first_page = server
        .post("/api/v1/places/get")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({"placeId": place_id, "cursor": null, "limit": 1}))
        .await;
    first_page.assert_status_ok();
    let first_page = first_page.json::<Value>();
    assert!(first_page["hasMore"].as_bool().expect("hasMore"));
    assert_eq!(first_page["media"].as_array().expect("media").len(), 1);

    let second_page = server
        .post("/api/v1/places/get")
        .add_header(AUTHORIZATION, authorization)
        .json(&json!({
            "placeId": place_id,
            "cursor": first_page["nextCursor"],
            "limit": 1
        }))
        .await;
    second_page.assert_status_ok();
    assert!(!second_page.json::<Value>()["hasMore"]
        .as_bool()
        .expect("hasMore"));
}

#[tokio::test]
async fn invalid_place_identifiers_and_limits_are_rejected() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "place-invalid", "place-invalid@example.com");
    let server = TestServer::new(app).expect("server");
    let authorization = format!("Bearer {}", token(user_id));

    server
        .post("/api/v1/places/list")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({"cursor": null, "limit": 0}))
        .await
        .assert_status_bad_request();
    server
        .post("/api/v1/places/get")
        .add_header(AUTHORIZATION, authorization)
        .json(&json!({"placeId": "not-valid", "cursor": null, "limit": 10}))
        .await
        .assert_status_bad_request();
}
