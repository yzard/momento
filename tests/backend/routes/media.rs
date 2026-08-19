use crate::test_utils::{
    create_test_app, create_test_db, create_test_media_with_gps_and_date, create_test_user,
    grant_media_access,
};
use axum::http::header::AUTHORIZATION;
use axum_test::TestServer;
use momento_api::auth::create_access_token;
use momento_api::config::Config;
use momento_api::constants::{IMAGE_TAGGING_MODEL_TYPE, OCR_MODEL_TYPE};
use momento_api::database::{init_database, queries};
use rusqlite::params;
use serde_json::{json, Value};

fn insert_media_text(
    pool: &momento_api::database::DbPool,
    image_id: i64,
    model_type: &str,
    text: &str,
) {
    let conn = pool.get().expect("Failed to get database connection");
    conn.execute(
        queries::media_text::INSERT,
        params![image_id, model_type, "test-version", text],
    )
    .expect("Failed to insert image text");
}

fn insert_classifications(
    pool: &momento_api::database::DbPool,
    media_id: i64,
    is_screenshot: bool,
    is_document: bool,
) {
    let connection = pool.get().expect("database connection");
    connection.execute("INSERT INTO media_screenshot_classifications (media_id, model_type, model_version, is_screenshot, confidence) VALUES (?, 'screenshot_detection', 'test', ?, 0.9)", rusqlite::params![media_id, is_screenshot]).expect("screenshot classification");
    connection.execute("INSERT INTO media_document_classifications (media_id, model_type, model_version, is_document, confidence) VALUES (?, 'document_detection', 'test', ?, 0.9)", rusqlite::params![media_id, is_document]).expect("document classification");
}

#[tokio::test]
async fn media_delete_accepts_media_ids_batch() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "batch-delete", "batch-delete@example.com");
    let first_media_id = create_test_media_with_gps_and_date(
        &pool,
        "delete-one.jpg",
        40.0,
        -74.0,
        "2024-01-15T10:30:00",
    );
    let second_media_id = create_test_media_with_gps_and_date(
        &pool,
        "delete-two.jpg",
        40.0,
        -74.0,
        "2024-01-15T10:31:00",
    );
    grant_media_access(&pool, first_media_id, user_id);
    grant_media_access(&pool, second_media_id, user_id);
    let server = TestServer::new(app).expect("Failed to create test server");

    let response = server
        .post("/api/v1/media/delete")
        .add_header(AUTHORIZATION, format!("Bearer {}", access_token(user_id)))
        .json(&json!({"mediaIds": [first_media_id, second_media_id]}))
        .await;

    response.assert_status_ok();
    let connection = pool.get().expect("Failed to get connection");
    let deleted_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_access WHERE user_id = ? AND deleted_at IS NOT NULL",
            [user_id],
            |row| row.get(0),
        )
        .expect("Failed to count deleted media");
    assert_eq!(deleted_count, 2);
}

fn access_token(user_id: i64) -> String {
    create_access_token(user_id, "testuser", "user", &Config::default(), None)
        .expect("Failed to create test access token")
}

#[tokio::test]
async fn manual_gps_update_recomputes_or_clears_place_fields() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "gps-update", "gps-update@example.com");
    let media_id = create_test_media_with_gps_and_date(
        &pool,
        "gps-update.jpg",
        44.5325,
        -72.7865,
        "2024-01-15T10:30:00",
    );
    grant_media_access(&pool, media_id, user_id);
    pool.get()
        .expect("connection")
        .execute(
            "UPDATE media_metadata SET location_city = 'Stale', location_state = 'Stale', location_country = 'Stale' WHERE media_id = ?",
            [media_id],
        )
        .expect("stale location");
    let server = TestServer::new(app).expect("server");
    let authorization = format!("Bearer {}", access_token(user_id));

    let updated = server
        .post("/api/v1/media/update")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({
            "mediaId": media_id,
            "gpsLatitude": 40.759,
            "gpsLongitude": -73.9859
        }))
        .await;
    updated.assert_status_ok();
    let updated = updated.json::<Value>();
    assert_eq!(updated["locationCity"], "Times Square");
    assert_eq!(updated["locationState"], "New York");
    assert_eq!(updated["locationCountry"], "United States");

    let cleared = server
        .post("/api/v1/media/update")
        .add_header(AUTHORIZATION, authorization)
        .json(&json!({
            "mediaId": media_id,
            "gpsLatitude": 0.0,
            "gpsLongitude": 1.0
        }))
        .await;
    cleared.assert_status_ok();
    let cleared = cleared.json::<Value>();
    assert!(cleared["locationCity"].is_null());
    assert!(cleared["locationState"].is_null());
    assert!(cleared["locationCountry"].is_null());
}

#[tokio::test]
async fn test_search_returns_accessible_image_and_model_names() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "testuser", "test@example.com");
    let visible_media = create_test_media_with_gps_and_date(
        &pool,
        "visible.jpg",
        40.7128,
        -74.0060,
        "2024-01-15T10:30:00",
    );
    let hidden_media = create_test_media_with_gps_and_date(
        &pool,
        "hidden.jpg",
        40.7128,
        -74.0060,
        "2024-01-15T10:30:00",
    );
    grant_media_access(&pool, visible_media, user_id);
    insert_media_text(&pool, visible_media, OCR_MODEL_TYPE, "beach sunset");
    insert_media_text(
        &pool,
        visible_media,
        IMAGE_TAGGING_MODEL_TYPE,
        "beach person",
    );
    insert_media_text(&pool, hidden_media, OCR_MODEL_TYPE, "beach hidden");

    let server = TestServer::new(app).expect("Failed to create test server");
    let token = access_token(user_id);
    let response = server
        .post("/api/v1/media/search")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&json!({ "search": "beach" }))
        .await;
    response.assert_status_ok();

    let body: Value = response.json();
    assert_eq!(body["results"].as_array().unwrap().len(), 1);
    assert_eq!(body["results"][0]["imageId"], json!(visible_media));
    assert_eq!(body["results"][0]["models"], json!(["Image Tags", "OCR"]));

    let partial_response = server
        .post("/api/v1/media/search")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&json!({ "search": "unset" }))
        .await;
    partial_response.assert_status_ok();
    let partial_body: Value = partial_response.json();
    assert_eq!(partial_body["results"][0]["imageId"], json!(visible_media));
}

#[tokio::test]
async fn test_search_matches_chinese_prefixes() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "testuser", "chinese@example.com");
    let media_id = create_test_media_with_gps_and_date(
        &pool,
        "negotiation.jpg",
        40.7128,
        -74.0060,
        "2024-01-15T10:30:00",
    );
    grant_media_access(&pool, media_id, user_id);
    insert_media_text(&pool, media_id, OCR_MODEL_TYPE, "谈判思考的技术");

    let server = TestServer::new(app).expect("Failed to create test server");
    let token = access_token(user_id);
    let response = server
        .post("/api/v1/media/search")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&json!({ "search": "判" }))
        .await;
    response.assert_status_ok();

    let body: Value = response.json();
    assert_eq!(body["results"][0]["imageId"], json!(media_id));
}

#[tokio::test]
async fn test_timeline_search_filters_before_grouping_and_pagination() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "testuser", "timeline@example.com");
    let matching_media = create_test_media_with_gps_and_date(
        &pool,
        "matching.jpg",
        40.7128,
        -74.0060,
        "2024-01-15T10:30:00",
    );
    let non_matching_media = create_test_media_with_gps_and_date(
        &pool,
        "other.jpg",
        40.7128,
        -74.0060,
        "2024-01-14T10:30:00",
    );
    grant_media_access(&pool, matching_media, user_id);
    grant_media_access(&pool, non_matching_media, user_id);
    insert_media_text(&pool, matching_media, OCR_MODEL_TYPE, "mountain lake");
    insert_media_text(&pool, non_matching_media, OCR_MODEL_TYPE, "city street");

    let server = TestServer::new(app).expect("Failed to create test server");
    let token = access_token(user_id);
    let markers_response = server
        .post("/api/v1/timeline/markers")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&json!({ "search": "mountain" }))
        .await;
    markers_response.assert_status_ok();
    let markers_body: Value = markers_response.json();
    assert_eq!(markers_body["markers"].as_array().unwrap().len(), 1);

    let response = server
        .post("/api/v1/timeline/list")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&json!({
            "groupBy": "day",
            "search": "mountain",
            "anchorDate": "9999-12-31T23:59:59",
            "direction": "older"
        }))
        .await;
    response.assert_status_ok();

    let body: Value = response.json();
    assert_eq!(body["groups"].as_array().unwrap().len(), 1);
    assert_eq!(body["groups"][0]["media"].as_array().unwrap().len(), 1);
    assert_eq!(body["groups"][0]["media"][0]["id"], json!(matching_media));
    assert_eq!(body["hasOlder"], json!(false));

    let no_match_response = server
        .post("/api/v1/timeline/list")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&json!({
            "groupBy": "day",
            "search": "not-indexed",
            "anchorDate": "9999-12-31T23:59:59",
            "direction": "older"
        }))
        .await;
    no_match_response.assert_status_ok();
    let no_match_body: Value = no_match_response.json();
    assert!(no_match_body["groups"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_timeline_media_type_filter() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "testuser", "media-type@example.com");
    let photo_id = create_test_media_with_gps_and_date(
        &pool,
        "photo.jpg",
        40.7128,
        -74.0060,
        "2024-01-15T10:30:00",
    );
    let video_id = create_test_media_with_gps_and_date(
        &pool,
        "video.mp4",
        40.7128,
        -74.0060,
        "2024-01-15T10:31:00",
    );
    let conn = pool.get().expect("Failed to get database connection");
    conn.execute(
        "UPDATE media SET media_type = 'video', mime_type = 'video/mp4' WHERE id = ?",
        [video_id],
    )
    .expect("Failed to update test media type");
    grant_media_access(&pool, photo_id, user_id);
    grant_media_access(&pool, video_id, user_id);

    let server = TestServer::new(app).expect("Failed to create test server");
    let token = access_token(user_id);

    let all_response = server
        .post("/api/v1/timeline/list")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&json!({
            "groupBy": "day",
            "search": "",
            "anchorDate": "9999-12-31T23:59:59",
            "direction": "older"
        }))
        .await;
    all_response.assert_status_ok();
    let all_body: Value = all_response.json();
    assert_eq!(all_body["groups"][0]["media"].as_array().unwrap().len(), 2);

    let photos_response = server
        .post("/api/v1/timeline/list")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&json!({
            "groupBy": "day",
            "search": "",
            "mediaType": "image",
            "anchorDate": "9999-12-31T23:59:59",
            "direction": "older"
        }))
        .await;
    photos_response.assert_status_ok();
    let photos_body: Value = photos_response.json();
    assert_eq!(
        photos_body["groups"][0]["media"].as_array().unwrap().len(),
        1
    );
    assert_eq!(photos_body["groups"][0]["media"][0]["id"], json!(photo_id));

    let videos_response = server
        .post("/api/v1/timeline/list")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&json!({
            "groupBy": "day",
            "search": "",
            "mediaType": "video",
            "anchorDate": "9999-12-31T23:59:59",
            "direction": "older"
        }))
        .await;
    videos_response.assert_status_ok();
    let videos_body: Value = videos_response.json();
    assert_eq!(
        videos_body["groups"][0]["media"].as_array().unwrap().len(),
        1
    );
    assert_eq!(videos_body["groups"][0]["media"][0]["id"], json!(video_id));
}

#[tokio::test]
async fn timeline_classification_filters_allow_overlap_and_enforce_access() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "classifier-user", "classifier-user@example.com");
    let screenshot_id = create_test_media_with_gps_and_date(
        &pool,
        "screenshot.jpg",
        40.0,
        -74.0,
        "2024-01-15T10:30:00",
    );
    let document_id = create_test_media_with_gps_and_date(
        &pool,
        "document.jpg",
        40.0,
        -74.0,
        "2024-01-15T10:31:00",
    );
    let overlap_id = create_test_media_with_gps_and_date(
        &pool,
        "overlap.jpg",
        40.0,
        -74.0,
        "2024-01-15T10:32:00",
    );
    let hidden_id = create_test_media_with_gps_and_date(
        &pool,
        "hidden-classification.jpg",
        40.0,
        -74.0,
        "2024-01-15T10:33:00",
    );
    insert_classifications(&pool, screenshot_id, true, false);
    insert_classifications(&pool, document_id, false, true);
    insert_classifications(&pool, overlap_id, true, true);
    insert_classifications(&pool, hidden_id, true, true);
    for media_id in [screenshot_id, document_id, overlap_id] {
        grant_media_access(&pool, media_id, user_id);
    }
    let server = TestServer::new(app).expect("server");
    let authorization = format!("Bearer {}", access_token(user_id));

    for (classification, expected_ids) in [
        ("screenshot", vec![screenshot_id, overlap_id]),
        ("document", vec![document_id, overlap_id]),
    ] {
        let response = server
            .post("/api/v1/timeline/list")
            .add_header(AUTHORIZATION, authorization.clone())
            .json(&json!({
                "groupBy": "day",
                "search": "",
                "classification": classification,
                "anchorDate": "9999-12-31T23:59:59",
                "direction": "older"
            }))
            .await;
        response.assert_status_ok();
        let body: Value = response.json();
        let mut media_ids = body["groups"][0]["media"]
            .as_array()
            .expect("classified media")
            .iter()
            .map(|media| media["id"].as_i64().expect("media id"))
            .collect::<Vec<_>>();
        media_ids.sort_unstable();
        let mut expected_ids = expected_ids;
        expected_ids.sort_unstable();
        assert_eq!(media_ids, expected_ids);

        let markers = server
            .post("/api/v1/timeline/markers")
            .add_header(AUTHORIZATION, authorization.clone())
            .json(&json!({ "classification": classification, "search": "" }))
            .await;
        markers.assert_status_ok();
        assert_eq!(
            markers.json::<Value>()["markers"].as_array().unwrap().len(),
            1
        );
    }

    server
        .post("/api/v1/timeline/list")
        .add_header(AUTHORIZATION, authorization)
        .json(&json!({
            "groupBy": "day",
            "search": "",
            "classification": "receipt",
            "direction": "older"
        }))
        .await
        .assert_status_bad_request();
}

#[tokio::test]
async fn test_timeline_page_contains_the_complete_group_period() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "testuser", "timeline-period@example.com");
    for index in 0..101 {
        let media_id = create_test_media_with_gps_and_date(
            &pool,
            &format!("same-day-{index}.jpg"),
            40.7128,
            -74.0060,
            "2024-01-15T10:30:00",
        );
        grant_media_access(&pool, media_id, user_id);
    }

    let server = TestServer::new(app).expect("Failed to create test server");
    let token = access_token(user_id);
    let response = server
        .post("/api/v1/timeline/list")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&json!({
            "groupBy": "day",
            "search": "",
            "anchorDate": "2024-01-15T10:30:00",
            "direction": "older"
        }))
        .await;
    response.assert_status_ok();

    let body: Value = response.json();
    assert_eq!(body["groups"].as_array().unwrap().len(), 1);
    assert_eq!(body["groups"][0]["media"].as_array().unwrap().len(), 101);
    assert!(!body["hasOlder"].as_bool().unwrap());
}

#[tokio::test]
async fn test_timeline_markers_respect_media_type() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "testuser", "timeline-range@example.com");
    let photo_id = create_test_media_with_gps_and_date(
        &pool,
        "photo.jpg",
        40.7128,
        -74.0060,
        "2024-01-15T10:30:00",
    );
    let video_id = create_test_media_with_gps_and_date(
        &pool,
        "video.mp4",
        40.7128,
        -74.0060,
        "2023-06-15T10:30:00",
    );
    let conn = pool.get().expect("Failed to get database connection");
    conn.execute(
        "UPDATE media SET media_type = 'video', mime_type = 'video/mp4' WHERE id = ?",
        [video_id],
    )
    .expect("Failed to update test media type");
    grant_media_access(&pool, photo_id, user_id);
    grant_media_access(&pool, video_id, user_id);

    let server = TestServer::new(app).expect("Failed to create test server");
    let token = access_token(user_id);
    let response = server
        .post("/api/v1/timeline/markers")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&json!({ "mediaType": "video", "search": "" }))
        .await;
    response.assert_status_ok();

    let body: Value = response.json();
    assert_eq!(body["markers"][0]["label"], json!("2023-06"));
    assert_eq!(body["markers"].as_array().unwrap().len(), 1);

    let selected_month_response = server
        .post("/api/v1/timeline/list")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&json!({
            "groupBy": "day",
            "search": "",
            "anchorDate": body["markers"][0]["anchorDate"],
            "direction": "older"
        }))
        .await;
    selected_month_response.assert_status_ok();
    let selected_month_body: Value = selected_month_response.json();
    assert_eq!(
        selected_month_body["groups"][0]["media"][0]["id"],
        json!(video_id)
    );
}

#[tokio::test]
async fn test_timeline_marker_query_stays_within_selected_month() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "testuser", "timeline-marker@example.com");
    let december_id = create_test_media_with_gps_and_date(
        &pool,
        "december.jpg",
        40.7128,
        -74.0060,
        "2010-12-24T10:30:00",
    );
    let early_december_id = create_test_media_with_gps_and_date(
        &pool,
        "early-december.jpg",
        40.7128,
        -74.0060,
        "2010-12-01T10:30:00",
    );
    let old_id = create_test_media_with_gps_and_date(
        &pool,
        "old.jpg",
        40.7128,
        -74.0060,
        "2005-10-24T10:30:00",
    );
    grant_media_access(&pool, december_id, user_id);
    grant_media_access(&pool, early_december_id, user_id);
    grant_media_access(&pool, old_id, user_id);

    let server = TestServer::new(app).expect("Failed to create test server");
    let token = access_token(user_id);
    let markers_response = server
        .post("/api/v1/timeline/markers")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&json!({ "search": "" }))
        .await;
    markers_response.assert_status_ok();

    let markers_body: Value = markers_response.json();
    let marker = markers_body["markers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|marker| marker["label"] == "2010-12")
        .expect("December 2010 marker missing");

    let response = server
        .post("/api/v1/timeline/list")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&json!({
            "groupBy": "day",
            "search": "",
            "anchorDate": marker["anchorDate"],
            "direction": "older",
        }))
        .await;
    response.assert_status_ok();

    let body: Value = response.json();
    assert_eq!(body["groups"].as_array().unwrap().len(), 1);
    assert_eq!(body["groups"][0]["media"][0]["id"], json!(december_id));
    assert_ne!(body["groups"][0]["media"][0]["id"], json!(old_id));

    let next_response = server
        .post("/api/v1/timeline/list")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&json!({
            "groupBy": "day",
            "search": "",
            "cursor": body["nextCursor"],
            "direction": "older"
        }))
        .await;
    next_response.assert_status_ok();
    let next_body: Value = next_response.json();
    assert_eq!(
        next_body["groups"][0]["media"][0]["id"],
        json!(early_december_id)
    );
    assert_ne!(next_body["groups"][0]["media"][0]["id"], json!(old_id));
}

#[tokio::test]
async fn test_timeline_reverse_pagination_does_not_repeat_media() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "testuser", "timeline-pagination@example.com");
    let newest_id = create_test_media_with_gps_and_date(
        &pool,
        "newest.jpg",
        40.7128,
        -74.0060,
        "2024-01-15T10:30:00",
    );
    let middle_id = create_test_media_with_gps_and_date(
        &pool,
        "middle.jpg",
        40.7128,
        -74.0060,
        "2010-01-15T10:30:00",
    );
    let oldest_id = create_test_media_with_gps_and_date(
        &pool,
        "oldest.jpg",
        40.7128,
        -74.0060,
        "2005-01-15T10:30:00",
    );
    grant_media_access(&pool, newest_id, user_id);
    grant_media_access(&pool, middle_id, user_id);
    grant_media_access(&pool, oldest_id, user_id);

    let server = TestServer::new(app).expect("Failed to create test server");
    let token = access_token(user_id);
    let request = |cursor: Option<&Value>| {
        let mut body = json!({
            "groupBy": "day",
            "search": "",
            "anchorDate": "9999-12-31T23:59:59",
            "direction": "older",
        });
        if let Some(cursor) = cursor {
            body["cursor"] = cursor.clone();
        }
        body
    };

    let first_response = server
        .post("/api/v1/timeline/list")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&request(None))
        .await;
    first_response.assert_status_ok();
    let first_body: Value = first_response.json();
    assert_eq!(first_body["groups"][0]["media"][0]["id"], json!(newest_id));

    let second_response = server
        .post("/api/v1/timeline/list")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&request(Some(&first_body["nextCursor"])))
        .await;
    second_response.assert_status_ok();
    let second_body: Value = second_response.json();
    assert_eq!(second_body["groups"][0]["media"][0]["id"], json!(middle_id));

    let third_response = server
        .post("/api/v1/timeline/list")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&request(Some(&second_body["nextCursor"])))
        .await;
    third_response.assert_status_ok();
    let third_body: Value = third_response.json();
    assert_eq!(third_body["groups"][0]["media"][0]["id"], json!(oldest_id));
    assert!(!third_body["hasOlder"].as_bool().unwrap());
}

#[tokio::test]
async fn test_timeline_jump_can_page_newer_media() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "testuser", "timeline-direction@example.com");
    let newer_id = create_test_media_with_gps_and_date(
        &pool,
        "newer.jpg",
        40.7128,
        -74.0060,
        "2024-02-15T10:30:00",
    );
    let newest_id = create_test_media_with_gps_and_date(
        &pool,
        "newest.jpg",
        40.7128,
        -74.0060,
        "2024-03-15T10:30:00",
    );
    let older_id = create_test_media_with_gps_and_date(
        &pool,
        "older.jpg",
        40.7128,
        -74.0060,
        "2024-01-15T10:30:00",
    );
    grant_media_access(&pool, newer_id, user_id);
    grant_media_access(&pool, newest_id, user_id);
    grant_media_access(&pool, older_id, user_id);

    let server = TestServer::new(app).expect("Failed to create test server");
    let token = access_token(user_id);
    let older_response = server
        .post("/api/v1/timeline/list")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&json!({
            "groupBy": "day",
            "search": "",
            "cursor": format!("2024-02-15T10:30:00_{}", newer_id),
            "direction": "older",
        }))
        .await;
    older_response.assert_status_ok();
    let older_body: Value = older_response.json();
    assert_eq!(older_body["groups"][0]["media"][0]["id"], json!(older_id));

    let newer_response = server
        .post("/api/v1/timeline/list")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&json!({
            "groupBy": "day",
            "search": "",
            "cursor": older_body["previousCursor"],
            "direction": "newer",
        }))
        .await;
    newer_response.assert_status_ok();
    let newer_body: Value = newer_response.json();
    assert_eq!(newer_body["groups"][0]["media"][0]["id"], json!(newer_id));

    let newest_response = server
        .post("/api/v1/timeline/list")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&json!({
            "groupBy": "day",
            "search": "",
            "cursor": newer_body["previousCursor"],
            "direction": "newer",
        }))
        .await;
    newest_response.assert_status_ok();
    let newest_body: Value = newest_response.json();
    assert_eq!(newest_body["groups"][0]["media"][0]["id"], json!(newest_id));
    assert!(!newest_body["hasNewer"].as_bool().unwrap());
}

#[test]
fn test_media_text_is_removed_when_media_is_deleted() {
    let pool = create_test_db();
    let media_id = create_test_media_with_gps_and_date(
        &pool,
        "deleted.jpg",
        40.7128,
        -74.0060,
        "2024-01-15T10:30:00",
    );
    insert_media_text(&pool, media_id, OCR_MODEL_TYPE, "delete me");

    let conn = pool.get().expect("Failed to get database connection");
    conn.execute("DELETE FROM media WHERE id = ?", [media_id])
        .expect("Failed to delete test media");

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM media_text WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("Failed to count image text");
    assert_eq!(count, 0);
}

#[test]
fn test_existing_database_receives_media_text_schema() {
    let pool = create_test_db();
    let conn = pool.get().expect("Failed to get database connection");
    conn.execute("DROP TABLE media_text", [])
        .expect("Failed to remove media text table");

    init_database(&conn).expect("Failed to reinitialize database schema");

    let table_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'media_text'",
            [],
            |row| row.get(0),
        )
        .expect("Failed to check media text table");
    assert_eq!(table_exists, 1);
}
