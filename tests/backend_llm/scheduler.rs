use llm_service::config::{CallbackConfig, Config, SchedulerConfig};
use llm_service::provider::ServiceManager;
use llm_service::scheduler::QueueInputDescriptor;
use llm_service::scheduler::{QueueManifest, Scheduler};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tempfile::tempdir;
use tokio::sync::Mutex;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn queue_acceptance_persists_raw_bytes_under_queuing() {
    let directory = tempdir().expect("queue directory");
    let config = Arc::new(Config {
        service: Vec::new(),
        ..Config::default()
    });
    let scheduler = Scheduler::new(
        directory.path().to_path_buf(),
        SchedulerConfig::default(),
        CallbackConfig::default(),
        Arc::new(Mutex::new(ServiceManager::new(config))),
    )
    .expect("scheduler");
    let job_id = "018f36e77c917cc89f7054252a33eaf0";
    let raw_bytes = b"raw bytes".to_vec();
    let input_descriptor = QueueInputDescriptor {
        sequence: 0,
        filename: "input.jpg".to_string(),
        mime_type: "image/jpeg".to_string(),
        byte_size: raw_bytes.len() as u64,
        content_hash: format!("{:x}", Sha256::digest(&raw_bytes)),
        input_kind: "image".to_string(),
        frame_timestamp_ms: None,
    };
    scheduler
        .accept(
            QueueManifest {
                job_id: job_id.to_string(),
                media_id: 1,
                task: "ocr".to_string(),
                attempt: 1,
                inputs: vec![input_descriptor.clone()],
                callback_url: "http://example.test/callback".to_string(),
            },
            vec![(input_descriptor, raw_bytes.clone())],
        )
        .expect("accepted");
    assert_eq!(
        std::fs::read(directory.path().join(format!("queuing/{job_id}/input-0")))
            .expect("raw queue bytes"),
        raw_bytes
    );
    assert!(!directory.path().join("completed").exists());
}

#[test]
fn queue_acceptance_supports_momento_hexadecimal_job_ids() {
    let directory = tempdir().expect("queue directory");
    let config = Arc::new(Config {
        service: Vec::new(),
        ..Config::default()
    });
    let scheduler = Scheduler::new(
        directory.path().to_path_buf(),
        SchedulerConfig::default(),
        CallbackConfig::default(),
        Arc::new(Mutex::new(ServiceManager::new(config))),
    )
    .expect("scheduler");
    let job_id = "e3713ac42cf629be1d8041ffb13c2d66";
    let bytes = b"raw bytes".to_vec();
    let descriptor = QueueInputDescriptor {
        sequence: 0,
        filename: "input.jpg".to_string(),
        mime_type: "image/jpeg".to_string(),
        byte_size: bytes.len() as u64,
        content_hash: format!("{:x}", Sha256::digest(&bytes)),
        input_kind: "image".to_string(),
        frame_timestamp_ms: None,
    };
    scheduler
        .accept(
            QueueManifest {
                job_id: job_id.to_string(),
                media_id: 1,
                task: "ocr".to_string(),
                attempt: 1,
                inputs: vec![descriptor.clone()],
                callback_url: "http://example.test/callback".to_string(),
            },
            vec![(descriptor, bytes)],
        )
        .expect("accepted");
    assert!(directory.path().join(format!("queuing/{job_id}")).is_dir());
}

#[test]
fn queue_acceptance_preserves_ordered_frame_inputs() {
    let directory = tempdir().expect("queue directory");
    let config = Arc::new(Config {
        service: Vec::new(),
        ..Config::default()
    });
    let scheduler = Scheduler::new(
        directory.path().to_path_buf(),
        SchedulerConfig::default(),
        CallbackConfig::default(),
        Arc::new(Mutex::new(ServiceManager::new(config))),
    )
    .expect("scheduler");
    let job_id = "018f36e77c917cc89f7054252a33eaf1";
    let first_bytes = b"first frame".to_vec();
    let second_bytes = b"second frame".to_vec();
    let first_descriptor = QueueInputDescriptor {
        sequence: 0,
        filename: "frame-0.jpg".to_string(),
        mime_type: "image/jpeg".to_string(),
        byte_size: first_bytes.len() as u64,
        content_hash: format!("{:x}", Sha256::digest(&first_bytes)),
        input_kind: "video_frame".to_string(),
        frame_timestamp_ms: Some(0),
    };
    let second_descriptor = QueueInputDescriptor {
        sequence: 1,
        filename: "frame-1.jpg".to_string(),
        mime_type: "image/jpeg".to_string(),
        byte_size: second_bytes.len() as u64,
        content_hash: format!("{:x}", Sha256::digest(&second_bytes)),
        input_kind: "video_frame".to_string(),
        frame_timestamp_ms: Some(1_000),
    };
    scheduler
        .accept(
            QueueManifest {
                job_id: job_id.to_string(),
                media_id: 1,
                task: "ocr".to_string(),
                attempt: 1,
                inputs: vec![first_descriptor.clone(), second_descriptor.clone()],
                callback_url: "http://example.test/callback".to_string(),
            },
            vec![
                (first_descriptor, first_bytes.clone()),
                (second_descriptor, second_bytes.clone()),
            ],
        )
        .expect("accepted");
    assert_eq!(
        std::fs::read(directory.path().join(format!("queuing/{job_id}/input-0")))
            .expect("first frame"),
        first_bytes
    );
    assert_eq!(
        std::fs::read(directory.path().join(format!("queuing/{job_id}/input-1")))
            .expect("second frame"),
        second_bytes
    );
}

#[test]
fn scheduler_recovers_processing_jobs_and_retains_callback_results() {
    let directory = tempdir().expect("queue directory");
    let config = Arc::new(Config {
        service: Vec::new(),
        ..Config::default()
    });
    let scheduler = Scheduler::new(
        directory.path().to_path_buf(),
        SchedulerConfig::default(),
        CallbackConfig::default(),
        Arc::new(Mutex::new(ServiceManager::new(config))),
    )
    .expect("scheduler");
    let raw_bytes = b"recovery input".to_vec();
    let descriptor = QueueInputDescriptor {
        sequence: 0,
        filename: "recovery.jpg".to_string(),
        mime_type: "image/jpeg".to_string(),
        byte_size: raw_bytes.len() as u64,
        content_hash: format!("{:x}", Sha256::digest(&raw_bytes)),
        input_kind: "image".to_string(),
        frame_timestamp_ms: None,
    };
    let processing_job_id = "018f36e77c917cc89f7054252a33eaf2";
    scheduler
        .accept(
            QueueManifest {
                job_id: processing_job_id.to_string(),
                media_id: 1,
                task: "ocr".to_string(),
                attempt: 1,
                inputs: vec![descriptor.clone()],
                callback_url: "http://example.test/callback".to_string(),
            },
            vec![(descriptor.clone(), raw_bytes.clone())],
        )
        .expect("accepted");
    std::fs::rename(
        directory
            .path()
            .join(format!("queuing/{processing_job_id}")),
        directory
            .path()
            .join(format!("processing/{processing_job_id}")),
    )
    .expect("move processing");
    let callback_job_id = "018f36e77c917cc89f7054252a33eaf3";
    scheduler
        .accept(
            QueueManifest {
                job_id: callback_job_id.to_string(),
                media_id: 2,
                task: "ocr".to_string(),
                attempt: 1,
                inputs: vec![descriptor.clone()],
                callback_url: "http://example.test/callback".to_string(),
            },
            vec![(descriptor, raw_bytes)],
        )
        .expect("accepted");
    std::fs::rename(
        directory.path().join(format!("queuing/{callback_job_id}")),
        directory
            .path()
            .join(format!("callback_pending/{callback_job_id}")),
    )
    .expect("move callback pending");
    std::fs::write(
        directory
            .path()
            .join(format!("callback_pending/{callback_job_id}/result.json")),
        "{}",
    )
    .expect("callback result");
    drop(scheduler);
    let config = Arc::new(Config {
        service: Vec::new(),
        ..Config::default()
    });
    let _recovered = Scheduler::new(
        directory.path().to_path_buf(),
        SchedulerConfig::default(),
        CallbackConfig::default(),
        Arc::new(Mutex::new(ServiceManager::new(config))),
    )
    .expect("recovered scheduler");
    assert!(directory
        .path()
        .join(format!("queuing/{processing_job_id}"))
        .is_dir());
    assert!(directory
        .path()
        .join(format!("callback_pending/{callback_job_id}/result.json"))
        .is_file());
}

#[tokio::test]
async fn callback_failure_retains_http_status_and_response_body() {
    let callback_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/callback"))
        .respond_with(
            ResponseTemplate::new(500)
                .set_body_json(serde_json::json!({"detail": "Database error"})),
        )
        .mount(&callback_server)
        .await;
    let directory = tempdir().expect("queue directory");
    let config = Arc::new(Config {
        service: Vec::new(),
        ..Config::default()
    });
    let scheduler = Arc::new(
        Scheduler::new(
            directory.path().to_path_buf(),
            SchedulerConfig {
                poll_interval_seconds: 60,
                active_batch_size: 1,
            },
            CallbackConfig {
                request_timeout_seconds: 5,
                retry_delay_seconds: 60,
                max_attempts: 10,
                key: "callback-key".to_string(),
            },
            Arc::new(Mutex::new(ServiceManager::new(config))),
        )
        .expect("scheduler"),
    );
    let job_id = "018f36e77c917cc89f7054252a33eaf4";
    let job_path = directory.path().join(format!("callback_pending/{job_id}"));
    std::fs::create_dir(&job_path).expect("callback job directory");
    std::fs::write(
        job_path.join("manifest.json"),
        serde_json::to_vec(&QueueManifest {
            job_id: job_id.to_string(),
            media_id: 3,
            task: "image_clustering".to_string(),
            attempt: 1,
            inputs: Vec::new(),
            callback_url: format!("{}/callback", callback_server.uri()),
        })
        .expect("manifest JSON"),
    )
    .expect("manifest");
    std::fs::write(job_path.join("result.json"), "{}").expect("result");

    let scheduler_task = tokio::spawn(Arc::clone(&scheduler).run());
    let callback_state_path = job_path.join("callback.json");
    for _ in 0..50 {
        if callback_state_path.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    scheduler_task.abort();

    let callback_state: serde_json::Value = serde_json::from_slice(
        &std::fs::read(callback_state_path).expect("callback failure state"),
    )
    .expect("callback state JSON");
    let last_error = callback_state["last_error"]
        .as_str()
        .expect("last callback error");
    assert!(last_error.contains("500 Internal Server Error"));
    assert!(last_error.contains("Database error"));
}
