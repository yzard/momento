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
        image_tagging_enabled: false,
        image_tagging_endpoint: "/v1/infer".to_string(),
    }
}

fn image_text(pool: &DbPool, media_id: i64) -> String {
    let conn = pool.get().expect("Failed to get database connection");
    conn.query_row(
        "SELECT string FROM image_text WHERE image_id = ? AND model_type = ?",
        rusqlite::params![media_id, momento_api::constants::OCR_MODEL_TYPE],
        |row| row.get(0),
    )
    .expect("Failed to query OCR text")
}

fn image_text_for_model(pool: &DbPool, media_id: i64, model_type: &str) -> String {
    let conn = pool.get().expect("Failed to get database connection");
    conn.query_row(
        "SELECT string FROM image_text WHERE image_id = ? AND model_type = ?",
        rusqlite::params![media_id, model_type],
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
            "provider": "local",
            "modelType": "ocr",
            "modelVersion": "test-ocr"
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
async fn image_tagging_hook_stores_returned_tags_under_tagging_model() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/infer"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "text": "person\nbicycle",
            "markdown": "person\nbicycle",
            "tags": ["person", "bicycle"],
            "provider": "ram++",
            "modelType": "image_tagging",
            "modelVersion": "ram++"
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
    config.image_tagging_enabled = true;
    let client = LlmClient::new(&config).expect("Failed to create client");
    client
        .image_tagging_and_store(&pool, 43, image.path())
        .await
        .expect("Image tagging hook should succeed");

    assert_eq!(
        image_text_for_model(&pool, 43, momento_api::constants::IMAGE_TAGGING_MODEL_TYPE),
        "person\nbicycle"
    );
}

#[tokio::test]
async fn empty_ocr_response_is_stored_as_completed_text() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "text": "",
            "markdown": "",
            "provider": "local",
            "modelType": "ocr",
            "modelVersion": "test-ocr"
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

    assert!(client
        .ocr_and_store(&pool, 44, image.path())
        .await
        .expect("Empty OCR should be stored"));

    let conn = pool.get().expect("Failed to get database connection");
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM image_text WHERE image_id = ? AND model_type = ?",
            rusqlite::params![44, momento_api::constants::OCR_MODEL_TYPE],
            |row| row.get(0),
        )
        .expect("Failed to query OCR text");
    assert_eq!(count, 1);
    let text: String = conn
        .query_row(
            "SELECT string FROM image_text WHERE image_id = ? AND model_type = ?",
            rusqlite::params![44, momento_api::constants::OCR_MODEL_TYPE],
            |row| row.get(0),
        )
        .expect("Failed to query empty OCR text");
    assert!(text.is_empty());
}
