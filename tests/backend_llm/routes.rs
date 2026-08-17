use axum::body::Body;
use axum::http::{Request, StatusCode};
use llm_service::config::Config;
use llm_service::provider::ServiceManager;
use llm_service::routes::{router, AppState};
use llm_service::scheduler::{QueueAdmission, QueueInputDescriptor, QueueManifest, Scheduler};
use momento_common::llm::{CancelJobsRequest, CancelJobsResponse};
use sha2::Digest;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::sync::Mutex;
use tower::ServiceExt;

fn queue_job(scheduler: &Scheduler, job_id: &str) {
    queue_task_job(scheduler, job_id, "ocr");
}

fn queue_task_job(scheduler: &Scheduler, job_id: &str, task: &str) {
    let bytes = b"image".to_vec();
    let descriptor = QueueInputDescriptor {
        sequence: 0,
        filename: "input.jpg".to_string(),
        mime_type: "image/jpeg".to_string(),
        byte_size: bytes.len() as u64,
        content_hash: format!("{:x}", sha2::Sha256::digest(&bytes)),
        input_kind: "image".to_string(),
        frame_timestamp_ms: None,
    };
    scheduler
        .accept(
            QueueManifest {
                job_id: job_id.to_string(),
                media_id: 1,
                task: task.to_string(),
                attempt: 1,
                inputs: vec![descriptor.clone()],
                callback_url: "http://example.test/callback".to_string(),
            },
            vec![(descriptor, bytes)],
        )
        .expect("queued job");
}

#[test]
fn scoped_cancellation_cleans_matching_jobs_in_every_state() {
    let directory = tempdir().expect("queue directory");
    let config = Arc::new(Config::default());
    let scheduler = Scheduler::new(
        directory.path().to_path_buf(),
        config.server.scheduler.clone(),
        config.callback.clone(),
        Arc::new(Mutex::new(ServiceManager::new(config))),
    )
    .expect("scheduler");
    let queued_id = "10000000000000000000000000000001";
    let failed_id = "10000000000000000000000000000002";
    let processing_id = "10000000000000000000000000000003";
    let callback_id = "10000000000000000000000000000004";
    let other_task_id = "10000000000000000000000000000005";
    for job_id in [queued_id, failed_id, processing_id, callback_id] {
        queue_task_job(&scheduler, job_id, "ocr");
    }
    queue_task_job(&scheduler, other_task_id, "image_tagging");
    for (job_id, state) in [
        (failed_id, "failed"),
        (processing_id, "processing"),
        (callback_id, "callback_pending"),
    ] {
        std::fs::rename(
            directory.path().join("queuing").join(job_id),
            directory.path().join(state).join(job_id),
        )
        .expect("queue state transition");
    }

    let response = scheduler
        .cancel_jobs(&CancelJobsRequest {
            all: false,
            tasks: vec!["ocr".to_string()],
            job_ids: Vec::new(),
        })
        .expect("scoped cancellation");

    assert_eq!(response.requested_jobs, 0);
    assert_eq!(response.cancelled_jobs, 3);
    assert_eq!(response.running_jobs, 1);
    assert_eq!(response.missing_jobs, 0);
    assert!(!directory.path().join("queuing").join(queued_id).exists());
    assert!(!directory.path().join("failed").join(failed_id).exists());
    assert!(!directory
        .path()
        .join("callback_pending")
        .join(callback_id)
        .exists());
    assert!(directory
        .path()
        .join("processing")
        .join(processing_id)
        .exists());
    assert!(directory
        .path()
        .join("queuing")
        .join(other_task_id)
        .exists());

    let response = scheduler
        .cancel_jobs(&CancelJobsRequest {
            all: true,
            tasks: Vec::new(),
            job_ids: Vec::new(),
        })
        .expect("all-task cancellation");
    assert_eq!(response.cancelled_jobs, 1);
    assert_eq!(response.running_jobs, 1);
    assert!(!directory
        .path()
        .join("queuing")
        .join(other_task_id)
        .exists());
}

#[tokio::test]
async fn submission_has_no_framework_multipart_body_limit() {
    let directory = tempdir().expect("queue directory");
    let config = Arc::new(Config::default());
    let scheduler = Arc::new(
        Scheduler::new(
            directory.path().to_path_buf(),
            config.server.scheduler.clone(),
            config.callback.clone(),
            Arc::new(Mutex::new(ServiceManager::new(Arc::clone(&config)))),
        )
        .expect("scheduler"),
    );
    let app = router(AppState {
        config,
        manager: Arc::new(Mutex::new(ServiceManager::new(Arc::new(Config::default())))),
        scheduler,
    });
    let boundary = "momento-boundary";
    let image = "a".repeat(3 * 1024 * 1024);
    let content_hash = format!("{:x}", sha2::Sha256::digest(image.as_bytes()));
    let manifest = format!(
        r#"{{"jobId":"0123456789abcdef0123456789abcdef","mediaId":1,"task":"image_clustering","attempt":1,"inputs":[{{"sequence":0,"filename":"input.jpg","mimeType":"image/jpeg","byteSize":{},"contentHash":"{}","inputKind":"image","frameTimestampMs":null}}],"callbackUrl":"http://example.test/callback"}}"#,
        image.len(),
        content_hash
    );
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"manifest\"\r\nContent-Type: application/json\r\n\r\n{manifest}\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"input-0\"; filename=\"input.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n{image}\r\n--{boundary}--\r\n"
    );

    let response = app
        .oneshot(
            Request::post("/api/v1/jobs/submit")
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn oversized_streaming_input_is_rejected_and_removes_staging() {
    let directory = tempdir().expect("queue directory");
    let config = Arc::new(Config::default());
    let scheduler = Arc::new(
        Scheduler::new(
            directory.path().to_path_buf(),
            config.server.scheduler.clone(),
            config.callback.clone(),
            Arc::new(Mutex::new(ServiceManager::new(Arc::clone(&config)))),
        )
        .expect("scheduler"),
    );
    let app = router(AppState {
        config,
        manager: Arc::new(Mutex::new(ServiceManager::new(Arc::new(Config::default())))),
        scheduler,
    });
    let boundary = "momento-boundary";
    let declared_input = "abc";
    let submitted_input = "abcdef";
    let job_id = "0123456789abcdef0123456789abcdef";
    let manifest = format!(
        r#"{{"jobId":"{job_id}","mediaId":1,"task":"ocr","attempt":1,"inputs":[{{"sequence":0,"filename":"input.jpg","mimeType":"image/jpeg","byteSize":{},"contentHash":"{:x}","inputKind":"image","frameTimestampMs":null}}],"callbackUrl":"http://example.test/callback"}}"#,
        declared_input.len(),
        sha2::Sha256::digest(declared_input.as_bytes()),
    );
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"manifest\"\r\nContent-Type: application/json\r\n\r\n{manifest}\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"input-0\"; filename=\"input.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n{submitted_input}\r\n--{boundary}--\r\n"
    );

    let response = app
        .oneshot(
            Request::post("/api/v1/jobs/submit")
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(!directory.path().join(format!(".tmp/{job_id}")).exists());
    assert!(!directory.path().join(format!("queuing/{job_id}")).exists());
}

#[tokio::test]
async fn cancellation_removes_non_running_states_and_leaves_processing_jobs() {
    let directory = tempdir().expect("queue directory");
    let mut config = Config::default();
    config.server.api_key = "cancel-key".to_string();
    let config = Arc::new(config);
    let manager = Arc::new(Mutex::new(ServiceManager::new(Arc::clone(&config))));
    let scheduler = Arc::new(
        Scheduler::new(
            directory.path().to_path_buf(),
            config.server.scheduler.clone(),
            config.callback.clone(),
            Arc::clone(&manager),
        )
        .expect("scheduler"),
    );
    let queued_id = "00000000000000000000000000000001";
    let failed_id = "00000000000000000000000000000002";
    let running_id = "00000000000000000000000000000003";
    let missing_id = "00000000000000000000000000000004";
    let callback_id = "00000000000000000000000000000007";
    for job_id in [queued_id, failed_id, running_id, callback_id] {
        queue_job(&scheduler, job_id);
    }
    std::fs::rename(
        directory.path().join("queuing").join(failed_id),
        directory.path().join("failed").join(failed_id),
    )
    .expect("failed state");
    std::fs::rename(
        directory.path().join("queuing").join(running_id),
        directory.path().join("processing").join(running_id),
    )
    .expect("processing state");
    std::fs::rename(
        directory.path().join("queuing").join(callback_id),
        directory.path().join("callback_pending").join(callback_id),
    )
    .expect("callback-pending state");
    let app = router(AppState {
        config,
        manager,
        scheduler,
    });

    let response = app
        .oneshot(
            Request::post("/api/v1/ai/cancel")
                .header("content-type", "application/json")
                .header("x-api-key", "cancel-key")
                .body(Body::from(
                    serde_json::json!({
                        "all": false,
                        "tasks": ["ocr"],
                        "jobIds": [queued_id, failed_id, running_id, callback_id, missing_id]
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let response: CancelJobsResponse = serde_json::from_slice(&body).expect("response JSON");
    assert_eq!(response.cancelled_jobs, 3);
    assert_eq!(response.running_jobs, 1);
    assert_eq!(response.missing_jobs, 1);
    assert!(!directory.path().join("queuing").join(queued_id).exists());
    assert!(!directory.path().join("failed").join(failed_id).exists());
    assert!(!directory
        .path()
        .join("callback_pending")
        .join(callback_id)
        .exists());
    assert!(directory
        .path()
        .join("processing")
        .join(running_id)
        .exists());
    assert!(directory.path().join("cancelled").join(queued_id).exists());
    assert!(directory.path().join("cancelled").join(failed_id).exists());
    assert!(directory.path().join("cancelled").join(missing_id).exists());
    assert!(directory.path().join("cancelled").join(running_id).exists());
}

#[test]
fn cancellation_marker_rejects_late_admission_without_storing_payloads() {
    let directory = tempdir().expect("queue directory");
    let config = Arc::new(Config::default());
    let scheduler = Scheduler::new(
        directory.path().to_path_buf(),
        config.server.scheduler.clone(),
        config.callback.clone(),
        Arc::new(Mutex::new(ServiceManager::new(config))),
    )
    .expect("scheduler");
    let job_id = "00000000000000000000000000000005";

    let response = scheduler
        .cancel_jobs(&CancelJobsRequest {
            all: false,
            tasks: vec!["ocr".to_string()],
            job_ids: vec![job_id.to_string()],
        })
        .expect("cancellation marker");
    queue_job(&scheduler, job_id);

    assert_eq!(response.missing_jobs, 1);
    assert!(directory.path().join("cancelled").join(job_id).exists());
    assert!(!directory.path().join("queuing").join(job_id).exists());
    assert!(!directory.path().join(".tmp").join(job_id).exists());
}

#[test]
fn cancellation_during_staging_prevents_the_atomic_queue_commit() {
    let directory = tempdir().expect("queue directory");
    let config = Arc::new(Config::default());
    let scheduler = Scheduler::new(
        directory.path().to_path_buf(),
        config.server.scheduler.clone(),
        config.callback.clone(),
        Arc::new(Mutex::new(ServiceManager::new(config))),
    )
    .expect("scheduler");
    let job_id = "00000000000000000000000000000006";
    let bytes = b"image".to_vec();
    let descriptor = QueueInputDescriptor {
        sequence: 0,
        filename: "input.jpg".to_string(),
        mime_type: "image/jpeg".to_string(),
        byte_size: bytes.len() as u64,
        content_hash: format!("{:x}", sha2::Sha256::digest(&bytes)),
        input_kind: "image".to_string(),
        frame_timestamp_ms: None,
    };
    let QueueAdmission::Staging(mut staging) = scheduler
        .begin_admission(QueueManifest {
            job_id: job_id.to_string(),
            media_id: 1,
            task: "ocr".to_string(),
            attempt: 1,
            inputs: vec![descriptor.clone()],
            callback_url: "http://example.test/callback".to_string(),
        })
        .expect("staging admission")
    else {
        panic!("expected staging admission");
    };
    staging
        .write_input(&descriptor, &bytes)
        .expect("staged input");

    scheduler
        .cancel_jobs(&CancelJobsRequest {
            all: false,
            tasks: vec!["ocr".to_string()],
            job_ids: Vec::new(),
        })
        .expect("cancellation");
    let committed = staging.commit().expect("cancelled commit");

    assert!(!committed);
    assert!(!directory.path().join("queuing").join(job_id).exists());
    assert!(!directory.path().join(".tmp").join(job_id).exists());
}

#[test]
fn concurrent_cancellation_retries_create_one_idempotent_marker() {
    let directory = tempdir().expect("queue directory");
    let config = Arc::new(Config::default());
    let scheduler = Arc::new(
        Scheduler::new(
            directory.path().to_path_buf(),
            config.server.scheduler.clone(),
            config.callback.clone(),
            Arc::new(Mutex::new(ServiceManager::new(config))),
        )
        .expect("scheduler"),
    );
    let job_id = "00000000000000000000000000000008".to_string();
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let threads = (0..8)
        .map(|_| {
            let scheduler = Arc::clone(&scheduler);
            let job_id = job_id.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                scheduler.cancel_jobs(&CancelJobsRequest {
                    all: false,
                    tasks: vec!["ocr".to_string()],
                    job_ids: vec![job_id],
                })
            })
        })
        .collect::<Vec<_>>();

    for thread in threads {
        thread
            .join()
            .expect("cancellation thread")
            .expect("idempotent cancellation");
    }

    assert!(directory.path().join("cancelled").join(job_id).exists());
}
