use crate::test_utils::{
    create_test_app, create_test_db, create_test_media_with_gps_and_date, create_test_user,
    grant_media_access,
};
use axum::http::header::AUTHORIZATION;
use axum_test::TestServer;
use momento_api::auth::create_access_token;
use momento_api::config::Config;
use momento_api::constants::{OBJECT_DETECTION_PLUGIN_ID, OCR_PLUGIN_ID};
use momento_api::database::{init_database, queries};
use rusqlite::params;
use serde_json::{json, Value};

fn insert_image_text(
    pool: &momento_api::database::DbPool,
    image_id: i64,
    plugin_id: i64,
    text: &str,
) {
    let conn = pool.get().expect("Failed to get database connection");
    conn.execute(
        queries::image_text::INSERT,
        params![image_id, plugin_id, text],
    )
    .expect("Failed to insert image text");
}

fn access_token(user_id: i64) -> String {
    create_access_token(user_id, "testuser", "user", &Config::default())
        .expect("Failed to create test access token")
}

#[tokio::test]
async fn test_search_returns_accessible_image_and_plugin_names() {
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
    insert_image_text(&pool, visible_media, OCR_PLUGIN_ID, "beach sunset");
    insert_image_text(
        &pool,
        visible_media,
        OBJECT_DETECTION_PLUGIN_ID,
        "beach person",
    );
    insert_image_text(&pool, hidden_media, OCR_PLUGIN_ID, "beach hidden");

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
    assert_eq!(
        body["results"][0]["plugins"],
        json!(["OCR", "Object Detection"])
    );

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
    insert_image_text(&pool, media_id, OCR_PLUGIN_ID, "谈判思考的技术");

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
    insert_image_text(&pool, matching_media, OCR_PLUGIN_ID, "mountain lake");
    insert_image_text(&pool, non_matching_media, OCR_PLUGIN_ID, "city street");

    let server = TestServer::new(app).expect("Failed to create test server");
    let token = access_token(user_id);
    let response = server
        .post("/api/v1/media/list")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&json!({
            "groupBy": "day",
            "limit": 1,
            "search": "mountain"
        }))
        .await;
    response.assert_status_ok();

    let body: Value = response.json();
    assert_eq!(body["groups"].as_array().unwrap().len(), 1);
    assert_eq!(body["groups"][0]["media"].as_array().unwrap().len(), 1);
    assert_eq!(body["groups"][0]["media"][0]["id"], json!(matching_media));
    assert_eq!(body["hasMore"], json!(false));

    let no_match_response = server
        .post("/api/v1/media/list")
        .add_header(AUTHORIZATION, format!("Bearer {}", token))
        .json(&json!({ "groupBy": "day", "search": "not-indexed" }))
        .await;
    no_match_response.assert_status_ok();
    let no_match_body: Value = no_match_response.json();
    assert!(no_match_body["groups"].as_array().unwrap().is_empty());
}

#[test]
fn test_image_text_is_removed_when_media_is_deleted() {
    let pool = create_test_db();
    let media_id = create_test_media_with_gps_and_date(
        &pool,
        "deleted.jpg",
        40.7128,
        -74.0060,
        "2024-01-15T10:30:00",
    );
    insert_image_text(&pool, media_id, OCR_PLUGIN_ID, "delete me");

    let conn = pool.get().expect("Failed to get database connection");
    conn.execute("DELETE FROM media WHERE id = ?", [media_id])
        .expect("Failed to delete test media");

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM image_text WHERE image_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("Failed to count image text");
    assert_eq!(count, 0);
}

#[test]
fn test_existing_database_receives_image_text_schema() {
    let pool = create_test_db();
    let conn = pool.get().expect("Failed to get database connection");
    conn.execute("DROP TABLE image_text", [])
        .expect("Failed to remove image text table");

    init_database(&conn).expect("Failed to reinitialize database schema");

    let table_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'image_text'",
            [],
            |row| row.get(0),
        )
        .expect("Failed to check image text table");
    assert_eq!(table_exists, 1);
}
