use crate::test_utils::create_test_db;
use momento_api::config::LlmConfig;
use momento_api::database::DbPool;
use momento_api::llm_client::LlmClient;
use std::io::Write;
use tempfile::NamedTempFile;
use wiremock::matchers::method;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn enabled_config(service_url: String) -> LlmConfig {
    LlmConfig {
        enabled: true,
        service_url,
        api_key: "test-key".to_string(),
        timeout_seconds: 10,
        startup_timeout_seconds: 10,
        ready_poll_interval_seconds: 1,
        ready_connection_timeout_seconds: 5,
        max_concurrent_requests: 1,
        object_detection_enabled: false,
        object_detection_endpoint: "/v1/infer".to_string(),
    }
}

fn image_text(pool: &DbPool, media_id: i64) -> String {
    let conn = pool.get().expect("Failed to get database connection");
    conn.query_row(
        "SELECT string FROM image_text WHERE image_id = ? AND plugin_id = ?",
        rusqlite::params![media_id, momento_api::constants::OCR_PLUGIN_ID],
        |row| row.get(0),
    )
    .expect("Failed to query OCR text")
}

fn image_text_for_plugin(pool: &DbPool, media_id: i64, plugin_id: i64) -> String {
    let conn = pool.get().expect("Failed to get database connection");
    conn.query_row(
        "SELECT string FROM image_text WHERE image_id = ? AND plugin_id = ?",
        rusqlite::params![media_id, plugin_id],
        |row| row.get(0),
    )
    .expect("Failed to query plugin text")
}

#[tokio::test]
async fn ocr_client_sends_image_and_stores_text() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "text": "recognized words",
            "markdown": "recognized words",
            "provider": "local"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut image = NamedTempFile::new().expect("Failed to create image fixture");
    image
        .write_all(b"fake image bytes")
        .expect("Failed to write image fixture");

    let pool = create_test_db();
    let client = LlmClient::new(&enabled_config(server.uri())).expect("Failed to create client");
    client
        .ocr_and_store(&pool, 42, image.path())
        .await
        .expect("OCR request should succeed");

    assert_eq!(image_text(&pool, 42), "recognized words");
}

#[tokio::test]
async fn llm_client_waits_for_ready_endpoint_before_processing() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ready"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let client = LlmClient::new(&enabled_config(server.uri())).expect("Failed to create client");
    client
        .wait_until_ready()
        .await
        .expect("LLM client should observe readiness");
}

#[tokio::test]
async fn disabled_ocr_client_does_not_call_service() {
    let pool = create_test_db();
    let config = LlmConfig::default();
    let client = LlmClient::new(&config).expect("Failed to create client");
    let result = client
        .ocr_and_store(&pool, 42, std::path::Path::new("missing.jpg"))
        .await;

    assert!(!result.expect("Disabled OCR should be skipped"));
}

#[tokio::test]
async fn object_detection_hook_stores_returned_text_under_detection_plugin() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/infer"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "text": "person bicycle",
            "markdown": "person bicycle",
            "provider": "configured-detector"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut image = NamedTempFile::new().expect("Failed to create image fixture");
    image
        .write_all(b"fake image bytes")
        .expect("Failed to write image fixture");

    let pool = create_test_db();
    let mut config = enabled_config(server.uri());
    config.object_detection_enabled = true;
    let client = LlmClient::new(&config).expect("Failed to create client");
    client
        .object_detection_and_store(&pool, 43, image.path())
        .await
        .expect("Object detection hook should succeed");

    assert_eq!(
        image_text_for_plugin(
            &pool,
            43,
            momento_api::constants::OBJECT_DETECTION_PLUGIN_ID
        ),
        "person bicycle"
    );
}

#[tokio::test]
async fn empty_ocr_response_does_not_store_text() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "text": "",
            "markdown": "",
            "provider": "local"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut image = NamedTempFile::new().expect("Failed to create image fixture");
    image
        .write_all(b"fake image bytes")
        .expect("Failed to write image fixture");
    let pool = create_test_db();
    let client = LlmClient::new(&enabled_config(server.uri())).expect("Failed to create client");

    assert!(!client
        .ocr_and_store(&pool, 44, image.path())
        .await
        .expect("Empty OCR should be a no-op"));

    let conn = pool.get().expect("Failed to get database connection");
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM image_text WHERE image_id = ? AND plugin_id = ?",
            rusqlite::params![44, momento_api::constants::OCR_PLUGIN_ID],
            |row| row.get(0),
        )
        .expect("Failed to query OCR text");
    assert_eq!(count, 0);
}
