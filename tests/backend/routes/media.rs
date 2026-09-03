use crate::test_utils::{
    create_test_app, create_test_db, create_test_media_with_gps_and_date, create_test_user,
    grant_media_access, test_data_directory,
};
use axum::http::{
    header::{
        AUTHORIZATION, CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, ETAG,
        LAST_MODIFIED, RANGE,
    },
    StatusCode,
};
use axum_test::TestServer;
use momento_api::auth::create_access_token;
use momento_api::config::Config;
use momento_api::constants::{IMAGE_TAGGING_MODEL_TYPE, OCR_MODEL_TYPE};
use momento_api::database::queries;
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
async fn individual_media_endpoints_require_access_and_honor_original_cache_and_ranges() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "binary-media", "binary-media@example.com");
    let other_user_id = create_test_user(&pool, "hidden-media", "hidden-media@example.com");
    let media_id = create_test_media_with_gps_and_date(
        &pool,
        "binary-media.jpg",
        40.0,
        -74.0,
        "2024-01-15T10:30:00",
    );
    grant_media_access(&pool, media_id, user_id);
    let relative_path = format!("route-tests/{media_id}.jpg");
    let thumbnail_path = format!("route-tests/{media_id}.jpg");
    let data_directory = test_data_directory(&pool);
    let originals = data_directory.join("originals").join(&relative_path);
    let thumbnail = data_directory.join("thumbnails").join(&thumbnail_path);
    let tiny_thumbnail = data_directory.join("thumbnails_tiny").join(&thumbnail_path);
    std::fs::create_dir_all(originals.parent().expect("original parent"))
        .expect("original directory");
    std::fs::create_dir_all(thumbnail.parent().expect("thumbnail parent"))
        .expect("thumbnail directory");
    std::fs::create_dir_all(tiny_thumbnail.parent().expect("tiny thumbnail parent"))
        .expect("tiny thumbnail directory");
    std::fs::write(&originals, b"abcdef").expect("original bytes");
    std::fs::write(&thumbnail, b"normal").expect("thumbnail bytes");
    std::fs::write(&tiny_thumbnail, b"tiny").expect("tiny thumbnail bytes");
    let connection = pool.get().expect("database");
    connection
        .execute(
            "UPDATE media SET file_path = ? WHERE id = ?",
            rusqlite::params![relative_path, media_id],
        )
        .expect("media path");
    connection
        .execute(
            "UPDATE media_metadata SET thumbnail_path = ? WHERE media_id = ?",
            rusqlite::params![thumbnail_path, media_id],
        )
        .expect("thumbnail path");
    drop(connection);
    let server = TestServer::new(app).expect("server");
    let authorization = format!("Bearer {}", access_token(user_id));

    server
        .get(&format!(
            "/api/v1/media/{media_id}/original?token={}",
            access_token(user_id)
        ))
        .await
        .assert_status_unauthorized();

    let ticket_response = server
        .post("/api/v1/media/access-ticket")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({"mediaId": media_id, "resource": "original"}))
        .await;
    ticket_response.assert_status_ok();
    ticket_response.assert_header(CACHE_CONTROL, "no-store");
    let ticket_body: Value = ticket_response.json();
    let ticket_url = ticket_body["url"].as_str().expect("ticket URL");
    assert!(ticket_body["expiresAt"].as_str().is_some());
    let ticket_range = server.get(ticket_url).add_header(RANGE, "bytes=1-3").await;
    ticket_range.assert_status(StatusCode::PARTIAL_CONTENT);
    ticket_range.assert_header(CONTENT_RANGE, "bytes 1-3/6");
    ticket_range.assert_header("referrer-policy", "no-referrer");
    server
        .get(ticket_url)
        .add_header(AUTHORIZATION, "Basic cached-browser-credential")
        .await
        .assert_status_ok();

    let (ticket_prefix, ticket) = ticket_url.split_once("?ticket=").expect("ticket query");
    server
        .get(&format!("{ticket_prefix}?ticket={ticket}tampered"))
        .await
        .assert_status_unauthorized();
    server
        .get(&format!("{ticket_prefix}?ticket={ticket}tampered"))
        .add_header(AUTHORIZATION, authorization.clone())
        .await
        .assert_status_unauthorized();
    server
        .get(&format!(
            "/api/v1/media/{}/original?ticket={ticket}",
            media_id + 10_000
        ))
        .await
        .assert_status(StatusCode::FORBIDDEN);
    server
        .get(&format!(
            "/api/v1/media/{media_id}/thumbnail?ticket={ticket}"
        ))
        .await
        .assert_status_unauthorized();

    server
        .get(&format!("/api/v1/media/{media_id}/thumbnail"))
        .add_header(AUTHORIZATION, authorization.clone())
        .await
        .assert_status_ok();
    server
        .get(&format!("/api/v1/media/{media_id}/thumbnail/tiny"))
        .add_header(AUTHORIZATION, authorization.clone())
        .await
        .assert_status_ok();
    server
        .get(&format!("/api/v1/media/{media_id}/preview"))
        .add_header(AUTHORIZATION, authorization.clone())
        .await
        .assert_status_ok();
    server
        .get(&format!("/api/v1/media/{media_id}/original"))
        .add_header(
            AUTHORIZATION,
            format!("Bearer {}", access_token(other_user_id)),
        )
        .await
        .assert_status_not_found();

    let original = server
        .get(&format!("/api/v1/media/{media_id}/original"))
        .add_header(AUTHORIZATION, authorization.clone())
        .await;
    original.assert_status_ok();
    original.assert_header(CACHE_CONTROL, "private");
    original.assert_header(CONTENT_DISPOSITION, "inline; filename=\"binary-media.jpg\"");
    let etag = original.header(ETAG).to_str().expect("etag").to_string();
    let last_modified = original
        .header(LAST_MODIFIED)
        .to_str()
        .expect("last modified")
        .to_string();
    server
        .get(&format!("/api/v1/media/{media_id}/original"))
        .add_header(AUTHORIZATION, authorization.clone())
        .add_header("if-none-match", &etag)
        .await
        .assert_status(StatusCode::NOT_MODIFIED);
    let range = server
        .get(&format!("/api/v1/media/{media_id}/original"))
        .add_header(AUTHORIZATION, authorization.clone())
        .add_header(RANGE, "bytes=1-3")
        .await;
    range.assert_status(StatusCode::PARTIAL_CONTENT);
    range.assert_header(CONTENT_RANGE, "bytes 1-3/6");
    let matching_if_range = server
        .get(&format!("/api/v1/media/{media_id}/original"))
        .add_header(AUTHORIZATION, authorization.clone())
        .add_header(RANGE, "bytes=1-3")
        .add_header("if-range", etag.clone())
        .await;
    matching_if_range.assert_status_ok();
    matching_if_range.assert_header(CONTENT_LENGTH, "6");
    let stale_if_range = server
        .get(&format!("/api/v1/media/{media_id}/original"))
        .add_header(AUTHORIZATION, authorization.clone())
        .add_header(RANGE, "bytes=1-3")
        .add_header("if-range", "W/\"stale\"")
        .await;
    stale_if_range.assert_status_ok();
    stale_if_range.assert_header(CONTENT_LENGTH, "6");
    let date_if_range = server
        .get(&format!("/api/v1/media/{media_id}/original"))
        .add_header(AUTHORIZATION, authorization.clone())
        .add_header(RANGE, "bytes=1-3")
        .add_header("if-range", "Tue, 15 Nov 1994 08:12:31 GMT")
        .await;
    date_if_range.assert_status_ok();
    let matching_date_if_range = server
        .get(&format!("/api/v1/media/{media_id}/original"))
        .add_header(AUTHORIZATION, authorization.clone())
        .add_header(RANGE, "bytes=1-3")
        .add_header("if-range", last_modified)
        .await;
    matching_date_if_range.assert_status(StatusCode::PARTIAL_CONTENT);
    let open_range = server
        .get(&format!("/api/v1/media/{media_id}/original"))
        .add_header(AUTHORIZATION, authorization.clone())
        .add_header(RANGE, "bytes=4-")
        .await;
    open_range.assert_status(StatusCode::PARTIAL_CONTENT);
    open_range.assert_header(CONTENT_RANGE, "bytes 4-5/6");
    let suffix_range = server
        .get(&format!("/api/v1/media/{media_id}/original"))
        .add_header(AUTHORIZATION, authorization.clone())
        .add_header(RANGE, "bytes=-2")
        .await;
    suffix_range.assert_status(StatusCode::PARTIAL_CONTENT);
    suffix_range.assert_header(CONTENT_RANGE, "bytes 4-5/6");
    let invalid_range = server
        .get(&format!("/api/v1/media/{media_id}/original"))
        .add_header(AUTHORIZATION, authorization.clone())
        .add_header(RANGE, "bytes=0-1,3-4")
        .await;
    invalid_range.assert_status(StatusCode::RANGE_NOT_SATISFIABLE);
    invalid_range.assert_header(CONTENT_RANGE, "bytes */6");
    pool.get()
        .expect("database")
        .execute(
            "UPDATE media_access SET deleted_at = datetime('now') WHERE media_id = ? AND user_id = ?",
            rusqlite::params![media_id, user_id],
        )
        .expect("revoke media access");
    server.get(ticket_url).await.assert_status_not_found();
    pool.get()
        .expect("database")
        .execute(
            "UPDATE media_access SET deleted_at = NULL WHERE media_id = ? AND user_id = ?",
            rusqlite::params![media_id, user_id],
        )
        .expect("restore media access");
    std::fs::write(&originals, b"").expect("empty original bytes");
    let empty_range = server
        .get(&format!("/api/v1/media/{media_id}/original"))
        .add_header(AUTHORIZATION, authorization)
        .add_header(RANGE, "bytes=0-0")
        .await;
    empty_range.assert_status(StatusCode::RANGE_NOT_SATISFIABLE);
    empty_range.assert_header(CONTENT_RANGE, "bytes */0");
}

#[tokio::test]
async fn converted_preview_uses_the_atomically_persisted_generation_path() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(
        &pool,
        "preview-generation",
        "preview-generation@example.com",
    );
    let media_id = create_test_media_with_gps_and_date(
        &pool,
        "preview-generation.heic",
        40.0,
        -74.0,
        "2024-01-15T10:30:00",
    );
    grant_media_access(&pool, media_id, user_id);
    let data_directory = test_data_directory(&pool);
    let legacy_path = format!("media/{media_id}/preview.jpg");
    let generation_path = format!("media/{media_id}/v3/preview.jpg");
    for path in [&legacy_path, &generation_path] {
        let absolute = data_directory.join("previews").join(path);
        std::fs::create_dir_all(absolute.parent().expect("preview parent"))
            .expect("preview directory");
        std::fs::write(absolute, path.as_bytes()).expect("preview fixture");
    }
    let connection = pool.get().expect("database");
    connection
        .execute(
            "UPDATE media SET mime_type = 'image/heic' WHERE id = ?",
            [media_id],
        )
        .expect("HEIC media");
    drop(connection);
    let server = TestServer::new(app).expect("server");
    let authorization = format!("Bearer {}", access_token(user_id));

    server
        .get(&format!("/api/v1/media/{media_id}/preview"))
        .add_header(AUTHORIZATION, authorization.clone())
        .await
        .assert_status_not_found();

    pool.get()
        .expect("database")
        .execute(
            "UPDATE media_metadata SET preview_path = ?, artifact_version = 3 WHERE media_id = ?",
            rusqlite::params![generation_path, media_id],
        )
        .expect("published preview generation");
    let response = server
        .get(&format!("/api/v1/media/{media_id}/preview"))
        .add_header(AUTHORIZATION, authorization)
        .await;
    response.assert_status_ok();
    assert_eq!(response.as_bytes(), generation_path.as_bytes());
}

#[tokio::test]
async fn individual_media_endpoints_reject_traversal_in_stored_paths() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "unsafe-paths", "unsafe-paths@example.com");
    let media_id = create_test_media_with_gps_and_date(
        &pool,
        "unsafe-paths.jpg",
        40.0,
        -74.0,
        "2024-01-15T10:30:00",
    );
    grant_media_access(&pool, media_id, user_id);
    let connection = pool.get().expect("database");
    connection
        .execute(
            "UPDATE media SET file_path = '../database.sqlite' WHERE id = ?",
            [media_id],
        )
        .expect("unsafe original path");
    connection
        .execute(
            "UPDATE media_metadata SET thumbnail_path = '../database.sqlite' WHERE media_id = ?",
            [media_id],
        )
        .expect("unsafe thumbnail path");
    drop(connection);
    let server = TestServer::new(app).expect("server");
    let authorization = format!("Bearer {}", access_token(user_id));

    server
        .get(&format!("/api/v1/media/{media_id}/original"))
        .add_header(AUTHORIZATION, authorization.clone())
        .await
        .assert_status_not_found();
    server
        .get(&format!("/api/v1/media/{media_id}/thumbnail"))
        .add_header(AUTHORIZATION, authorization)
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn thumbnail_endpoints_require_a_persisted_thumbnail_reference() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "missing-thumbnail", "missing-thumbnail@example.com");
    let media_id = create_test_media_with_gps_and_date(
        &pool,
        "derived.jpg",
        40.0,
        -74.0,
        "2024-01-15T10:30:00",
    );
    grant_media_access(&pool, media_id, user_id);
    pool.get()
        .expect("database")
        .execute(
            "UPDATE media SET file_path = 'legacy/derived.jpg' WHERE id = ?",
            [media_id],
        )
        .expect("legacy original path");
    let guessed_thumbnail = test_data_directory(&pool)
        .join("thumbnails")
        .join("legacy/derived.jpg");
    std::fs::create_dir_all(guessed_thumbnail.parent().expect("thumbnail parent"))
        .expect("thumbnail directory");
    std::fs::write(guessed_thumbnail, b"must-not-be-served").expect("legacy thumbnail");

    let server = TestServer::new(app).expect("server");
    let authorization = format!("Bearer {}", access_token(user_id));
    server
        .get(&format!("/api/v1/media/{media_id}/thumbnail"))
        .add_header(AUTHORIZATION, authorization.clone())
        .await
        .assert_status_not_found();

    server
        .post("/api/v1/thumbnail/get")
        .add_header(AUTHORIZATION, authorization)
        .json(&json!({"mediaIds": [media_id]}))
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn removed_media_asset_batch_endpoints_are_not_routable() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "batch-assets", "batch-assets@example.com");
    let visible_id = create_test_media_with_gps_and_date(
        &pool,
        "visible-asset.jpg",
        40.0,
        -74.0,
        "2024-01-15T10:30:00",
    );
    grant_media_access(&pool, visible_id, user_id);
    let server = TestServer::new(app).expect("server");
    let authorization = format!("Bearer {}", access_token(user_id));

    for (path, body) in [
        ("/api/v1/thumbnail/get", json!({"mediaIds": [visible_id]})),
        ("/api/v1/preview/get", json!({"ids": [visible_id]})),
    ] {
        let response = server
            .post(path)
            .add_header(AUTHORIZATION, authorization.clone())
            .json(&body)
            .await;
        response.assert_status_not_found();
    }

    let too_many_ids = (1..=501).collect::<Vec<_>>();
    server
        .post("/api/v1/thumbnail/get")
        .add_header(AUTHORIZATION, authorization)
        .json(&json!({"mediaIds": too_many_ids}))
        .await
        .assert_status_not_found();
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
    insert_media_text(
        &pool,
        matching_media,
        IMAGE_TAGGING_MODEL_TYPE,
        "mountain reflection",
    );
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
            "limit": 100,
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
            "limit": 100,
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
async fn timeline_requires_a_bounded_nonzero_limit() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "timeline-limit", "timeline-limit@example.com");
    let server = TestServer::new(app).expect("server");
    let authorization = format!("Bearer {}", access_token(user_id));

    server
        .post("/api/v1/timeline/list")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({
            "groupBy": "day",
            "search": "",
            "anchorDate": "9999-12-31T23:59:59",
            "direction": "older"
        }))
        .await
        .assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    server
        .post("/api/v1/timeline/list")
        .add_header(AUTHORIZATION, authorization)
        .json(&json!({
            "limit": 501,
            "groupBy": "day",
            "search": "",
            "anchorDate": "9999-12-31T23:59:59",
            "direction": "older"
        }))
        .await
        .assert_status_bad_request();
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
            "limit": 100,
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
            "limit": 100,
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
            "limit": 100,
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
                "limit": 100,
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
            "limit": 100,
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
            "limit": 100,
            "search": "",
            "anchorDate": "2024-01-15T10:30:00",
            "direction": "older"
        }))
        .await;
    response.assert_status_ok();

    let body: Value = response.json();
    assert_eq!(body["groups"].as_array().unwrap().len(), 1);
    assert_eq!(body["groups"][0]["media"].as_array().unwrap().len(), 100);
    assert!(body["hasOlder"].as_bool().unwrap());

    let first_page_ids = body["groups"][0]["media"]
        .as_array()
        .expect("first page media")
        .iter()
        .map(|media| media["id"].as_i64().expect("media ID"))
        .collect::<std::collections::HashSet<_>>();
    let next_cursor = body["nextCursor"]
        .as_str()
        .expect("next cursor")
        .to_string();
    let second_page = server
        .post("/api/v1/timeline/list")
        .add_header(AUTHORIZATION, format!("Bearer {token}"))
        .json(&json!({
            "groupBy": "day",
            "limit": 100,
            "search": "",
            "cursor": next_cursor,
            "direction": "older"
        }))
        .await;
    second_page.assert_status_ok();
    let second_page: Value = second_page.json();
    let second_page_ids = second_page["groups"][0]["media"]
        .as_array()
        .expect("second page media")
        .iter()
        .map(|media| media["id"].as_i64().expect("media ID"))
        .collect::<Vec<_>>();
    assert_eq!(second_page_ids.len(), 1);
    assert!(!first_page_ids.contains(&second_page_ids[0]));
    assert!(!second_page["hasOlder"].as_bool().unwrap());
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
            "limit": 100,
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
async fn test_timeline_marker_anchor_fills_page_across_periods() {
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
            "limit": 2,
            "search": "",
            "anchorDate": marker["anchorDate"],
            "direction": "older",
        }))
        .await;
    response.assert_status_ok();

    let body: Value = response.json();
    assert_eq!(body["groups"].as_array().unwrap().len(), 2);
    assert_eq!(body["groups"][0]["media"][0]["id"], json!(december_id));
    assert_eq!(
        body["groups"][1]["media"][0]["id"],
        json!(early_december_id)
    );
    assert!(body["hasOlder"].as_bool().unwrap());

    let next_response = server
        .post("/api/v1/timeline/list")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&json!({
            "groupBy": "day",
            "limit": 2,
            "search": "",
            "cursor": body["nextCursor"],
            "direction": "older"
        }))
        .await;
    next_response.assert_status_ok();
    let next_body: Value = next_response.json();
    assert_eq!(next_body["groups"][0]["media"][0]["id"], json!(old_id));
    assert!(!next_body["hasOlder"].as_bool().unwrap());
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
            "limit": 1,
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
    let latest_id = create_test_media_with_gps_and_date(
        &pool,
        "latest.jpg",
        40.7128,
        -74.0060,
        "2024-04-15T10:30:00",
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
    grant_media_access(&pool, latest_id, user_id);
    grant_media_access(&pool, older_id, user_id);

    let server = TestServer::new(app).expect("Failed to create test server");
    let token = access_token(user_id);
    let older_response = server
        .post("/api/v1/timeline/list")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&json!({
            "groupBy": "day",
            "limit": 1,
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
            "limit": 2,
            "search": "",
            "cursor": older_body["previousCursor"],
            "direction": "newer",
        }))
        .await;
    newer_response.assert_status_ok();
    let newer_body: Value = newer_response.json();
    assert_eq!(newer_body["groups"].as_array().unwrap().len(), 2);
    assert_eq!(newer_body["groups"][0]["media"][0]["id"], json!(newest_id));
    assert_eq!(newer_body["groups"][1]["media"][0]["id"], json!(newer_id));
    assert!(newer_body["hasNewer"].as_bool().unwrap());

    let newest_response = server
        .post("/api/v1/timeline/list")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&json!({
            "groupBy": "day",
            "limit": 1,
            "search": "",
            "cursor": newer_body["previousCursor"],
            "direction": "newer",
        }))
        .await;
    newest_response.assert_status_ok();
    let newest_body: Value = newest_response.json();
    assert_eq!(newest_body["groups"][0]["media"][0]["id"], json!(latest_id));
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
