// Submission error response bodies are persisted by the processor so operators can
// diagnose LLM queue rejections without reading the service logs.

use crate::test_utils::{create_test_db, create_test_media};
use momento_api::config::Config;
use momento_api::processor::ai::{
    cancel_active_jobs, deliver_pending_cancellations, verify_prepared_input,
};
use sha2::{Digest, Sha256};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn prepared_input_verification_streams_size_and_hash_validation() {
    let directory = tempfile::TempDir::new().expect("temporary directory");
    let path = directory.path().join("prepared.jpg");
    let bytes = vec![42_u8; 256 * 1024];
    std::fs::write(&path, &bytes).expect("prepared input");
    let content_hash = format!("{:x}", Sha256::digest(&bytes));

    verify_prepared_input(&path, bytes.len() as u64, &content_hash)
        .await
        .expect("matching descriptor");
    assert!(
        verify_prepared_input(&path, bytes.len() as u64 - 1, &content_hash)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn cancellation_outbox_retries_exact_job_ids_until_acknowledged() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "cancel.jpg");
    let job_id = "0123456789abcdef0123456789abcdef";
    pool.get()
        .expect("database connection")
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status) VALUES (?, ?, 'ocr', 'queued')",
            rusqlite::params![job_id, media_id],
        )
        .expect("queued job");
    let cancelled = cancel_active_jobs(&pool, Some("ocr")).expect("local cancellation");
    let llm_service = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/ai/cancel"))
        .and(header("x-api-key", "llm-key"))
        .and(body_json(serde_json::json!({
            "all": false,
            "tasks": ["ocr"],
            "jobIds": [job_id]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "requestedJobs": 1,
            "cancelledJobs": 0,
            "runningJobs": 0,
            "missingJobs": 1
        })))
        .expect(1)
        .mount(&llm_service)
        .await;
    let mut config = Config::default();
    config.llm.enabled = true;
    config.llm.service_url = llm_service.uri();
    config.llm.api_key = "llm-key".to_string();

    let delivered = deliver_pending_cancellations(&config, &pool)
        .await
        .expect("cancellation delivery");

    assert_eq!(cancelled, 1);
    assert_eq!(delivered, 1);
    let pending: i64 = pool
        .get()
        .expect("database connection")
        .query_row("SELECT COUNT(*) FROM llm_job_cancellations", [], |row| {
            row.get(0)
        })
        .expect("pending cancellation count");
    assert_eq!(pending, 0);

    let failed_job_id = "1123456789abcdef0123456789abcdef";
    pool.get()
        .expect("database connection")
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status) VALUES (?, ?, 'ocr', 'queued')",
            rusqlite::params![failed_job_id, media_id],
        )
        .expect("second queued job");
    cancel_active_jobs(&pool, Some("ocr")).expect("second local cancellation");
    Mock::given(method("POST"))
        .and(path("/api/v1/ai/cancel"))
        .and(body_json(serde_json::json!({
            "all": false,
            "tasks": ["ocr"],
            "jobIds": [failed_job_id]
        })))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&llm_service)
        .await;

    assert!(deliver_pending_cancellations(&config, &pool).await.is_err());
    let pending: i64 = pool
        .get()
        .expect("database connection")
        .query_row("SELECT COUNT(*) FROM llm_job_cancellations", [], |row| {
            row.get(0)
        })
        .expect("retained cancellation count");
    assert_eq!(pending, 1);
}

#[tokio::test]
async fn all_task_cancellation_is_delivered_without_local_job_ids() {
    let pool = create_test_db();
    cancel_active_jobs(&pool, None).expect("queue all-task cancellation");
    let llm_service = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/ai/cancel"))
        .and(header("x-api-key", "llm-key"))
        .and(body_json(serde_json::json!({
            "all": true,
            "tasks": [],
            "jobIds": []
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "requestedJobs": 0,
            "cancelledJobs": 2,
            "runningJobs": 1,
            "missingJobs": 0
        })))
        .expect(1)
        .mount(&llm_service)
        .await;
    let mut config = Config::default();
    config.llm.enabled = true;
    config.llm.service_url = llm_service.uri();
    config.llm.api_key = "llm-key".to_string();

    let delivered = deliver_pending_cancellations(&config, &pool)
        .await
        .expect("all-task cancellation delivery");

    assert_eq!(delivered, 0);
    let pending_scopes: i64 = pool
        .get()
        .expect("database connection")
        .query_row("SELECT COUNT(*) FROM llm_cancellation_scopes", [], |row| {
            row.get(0)
        })
        .expect("pending scope count");
    assert_eq!(pending_scopes, 0);
}

#[test]
fn cancelling_a_submitting_job_preserves_its_in_flight_attempt() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "submitting-cancel.jpg");
    pool.get()
        .expect("database connection")
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES ('2123456789abcdef0123456789abcdef', ?, 'ocr', 'submitting', 4)",
            [media_id],
        )
        .expect("submitting job");

    cancel_active_jobs(&pool, Some("ocr")).expect("local cancellation");

    let (status, attempts): (String, i64) = pool
        .get()
        .expect("database connection")
        .query_row(
            "SELECT status, attempts FROM llm_jobs WHERE id = '2123456789abcdef0123456789abcdef'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("cancelled job");
    assert_eq!(status, "cancelled");
    assert_eq!(attempts, 5);
}
