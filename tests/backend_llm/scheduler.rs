use llm_service::config::{CallbackConfig, Config, SchedulerConfig};
use llm_service::provider::ServiceManager;
use llm_service::scheduler::QueueInputDescriptor;
use llm_service::scheduler::{QueueAdmission, QueueManifest, Scheduler};
use momento_common::llm::CancelJobsRequest;
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
fn unavailable_runtime_is_durably_requeued_until_attempts_are_exhausted() {
    let directory = tempdir().expect("queue directory");
    let scheduler_config = SchedulerConfig {
        runtime_max_attempts: 2,
        ..SchedulerConfig::default()
    };
    let scheduler = Scheduler::new(
        directory.path().to_path_buf(),
        scheduler_config,
        CallbackConfig::default(),
        Arc::new(Mutex::new(ServiceManager::new(Arc::new(Config {
            service: Vec::new(),
            ..Config::default()
        })))),
    )
    .expect("scheduler");
    let job_id = "018f36e77c917cc89f7054252a33eaaa";
    let bytes = b"input".to_vec();
    let descriptor = QueueInputDescriptor {
        sequence: 0,
        filename: "input.jpg".to_string(),
        mime_type: "image/jpeg".to_string(),
        byte_size: bytes.len() as u64,
        content_hash: format!("{:x}", Sha256::digest(&bytes)),
        input_kind: "image".to_string(),
        frame_timestamp_ms: None,
    };
    let manifest = QueueManifest {
        job_id: job_id.to_string(),
        media_id: 1,
        task: "face_detection".to_string(),
        attempt: 1,
        inputs: vec![descriptor.clone()],
        callback_url: "http://example.test/callback".to_string(),
    };
    scheduler
        .accept(manifest.clone(), vec![(descriptor, bytes)])
        .expect("accepted");
    let queued_path = directory.path().join("queuing").join(job_id);
    let processing_path = directory.path().join("processing").join(job_id);
    std::fs::rename(&queued_path, &processing_path).expect("claim");

    assert!(scheduler
        .requeue_runtime_failure(&processing_path, &manifest, "connection reset")
        .expect("first retry"));
    assert!(queued_path.join("runtime.json").is_file());
    std::fs::rename(&queued_path, &processing_path).expect("reclaim");
    assert!(!scheduler
        .requeue_runtime_failure(&processing_path, &manifest, "connection reset")
        .expect("retry exhaustion"));
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
fn queue_acceptance_accepts_non_contiguous_input_sequences() {
    let directory = tempdir().expect("queue directory");
    let scheduler = Scheduler::new(
        directory.path().to_path_buf(),
        SchedulerConfig::default(),
        CallbackConfig::default(),
        Arc::new(Mutex::new(ServiceManager::new(Arc::new(Config {
            service: Vec::new(),
            ..Config::default()
        })))),
    )
    .expect("scheduler");
    let bytes = b"input".to_vec();
    let descriptor = QueueInputDescriptor {
        sequence: 3,
        filename: "frame.jpg".to_string(),
        mime_type: "image/jpeg".to_string(),
        byte_size: bytes.len() as u64,
        content_hash: format!("{:x}", Sha256::digest(&bytes)),
        input_kind: "video_frame".to_string(),
        frame_timestamp_ms: Some(3000),
    };

    scheduler
        .accept(
            QueueManifest {
                job_id: "0123456789abcdef0123456789abcdef".to_string(),
                media_id: 1,
                task: "image_clustering".to_string(),
                attempt: 1,
                inputs: vec![descriptor.clone()],
                callback_url: "http://example.test/callback".to_string(),
            },
            vec![(descriptor, bytes)],
        )
        .expect("sparse sequence should be accepted");

    assert!(directory
        .path()
        .join("queuing/0123456789abcdef0123456789abcdef/input-3")
        .is_file());
}

#[test]
fn abandoned_staging_removes_temporary_queue_directory() {
    let directory = tempdir().expect("queue directory");
    let scheduler = Scheduler::new(
        directory.path().to_path_buf(),
        SchedulerConfig::default(),
        CallbackConfig::default(),
        Arc::new(Mutex::new(ServiceManager::new(Arc::new(Config {
            service: Vec::new(),
            ..Config::default()
        })))),
    )
    .expect("scheduler");
    let job_id = "0123456789abcdef0123456789abcdef";
    let descriptor = QueueInputDescriptor {
        sequence: 0,
        filename: "input.jpg".to_string(),
        mime_type: "image/jpeg".to_string(),
        byte_size: 3,
        content_hash: format!("{:x}", Sha256::digest(b"abc")),
        input_kind: "image".to_string(),
        frame_timestamp_ms: None,
    };

    let admission = scheduler
        .begin_admission(QueueManifest {
            job_id: job_id.to_string(),
            media_id: 1,
            task: "ocr".to_string(),
            attempt: 1,
            inputs: vec![descriptor],
            callback_url: "http://example.test/callback".to_string(),
        })
        .expect("admission");
    assert!(matches!(admission, QueueAdmission::Staging(_)));
    assert!(directory.path().join(format!(".tmp/{job_id}")).is_dir());

    drop(admission);

    assert!(!directory.path().join(format!(".tmp/{job_id}")).exists());
}

#[test]
fn queue_selection_is_task_aware_and_bounded() {
    let directory = tempdir().expect("queue directory");
    let scheduler = Scheduler::new(
        directory.path().to_path_buf(),
        SchedulerConfig {
            max_in_flight_jobs: 3,
            ..SchedulerConfig::default()
        },
        CallbackConfig::default(),
        Arc::new(Mutex::new(ServiceManager::new(Arc::new(Config {
            service: Vec::new(),
            ..Config::default()
        })))),
    )
    .expect("scheduler");
    let bytes = b"input".to_vec();
    let descriptor = QueueInputDescriptor {
        sequence: 0,
        filename: "input.jpg".to_string(),
        mime_type: "image/jpeg".to_string(),
        byte_size: bytes.len() as u64,
        content_hash: format!("{:x}", Sha256::digest(&bytes)),
        input_kind: "image".to_string(),
        frame_timestamp_ms: None,
    };
    for index in 0..128_u32 {
        scheduler
            .accept(
                QueueManifest {
                    job_id: format!("{index:032x}"),
                    media_id: i64::from(index),
                    task: if index % 2 == 0 {
                        "image_tagging".to_string()
                    } else {
                        "ocr".to_string()
                    },
                    attempt: 1,
                    inputs: vec![descriptor.clone()],
                    callback_url: "http://example.test/callback".to_string(),
                },
                vec![(descriptor.clone(), bytes.clone())],
            )
            .expect("accepted");
    }

    let first_task = scheduler.select_queued_jobs(None);
    assert_eq!(first_task.len(), 3);
    assert!(first_task
        .iter()
        .all(|(_, manifest)| manifest.task == "image_tagging"));
    assert_eq!(
        first_task
            .iter()
            .map(|(_, manifest)| manifest.job_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "00000000000000000000000000000000",
            "00000000000000000000000000000002",
            "00000000000000000000000000000004",
        ]
    );

    let warm_task = scheduler.select_queued_jobs(Some("ocr"));
    assert_eq!(warm_task.len(), 3);
    assert!(warm_task.iter().all(|(_, manifest)| manifest.task == "ocr"));
    assert_eq!(
        warm_task
            .iter()
            .map(|(_, manifest)| manifest.job_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "00000000000000000000000000000001",
            "00000000000000000000000000000003",
            "00000000000000000000000000000005",
        ]
    );
}

#[test]
fn scheduler_prioritizes_a_single_task_before_switching_runtimes() {
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
    let bytes = b"input".to_vec();
    let descriptor = QueueInputDescriptor {
        sequence: 0,
        filename: "input.jpg".to_string(),
        mime_type: "image/jpeg".to_string(),
        byte_size: bytes.len() as u64,
        content_hash: format!("{:x}", Sha256::digest(&bytes)),
        input_kind: "image".to_string(),
        frame_timestamp_ms: None,
    };
    for (job_id, task) in [
        ("00000000000000000000000000000002", "image_tagging"),
        ("00000000000000000000000000000001", "ocr"),
    ] {
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
                vec![(descriptor.clone(), bytes.clone())],
            )
            .expect("accepted");
    }

    let queued = std::fs::read_dir(directory.path().join("queuing"))
        .expect("queue directory")
        .flatten()
        .map(|entry| {
            let manifest: QueueManifest = serde_json::from_slice(
                &std::fs::read(entry.path().join("manifest.json")).expect("manifest"),
            )
            .expect("valid manifest");
            (entry.path(), manifest)
        })
        .collect::<Vec<_>>();
    let mut queued = queued;
    queued.sort_by(|left, right| left.1.job_id.cmp(&right.1.job_id));
    assert_eq!(queued[0].1.task, "ocr");
}

#[test]
fn scheduler_prefers_queued_jobs_for_the_warm_runtime() {
    let queued = vec![
        (
            std::path::PathBuf::from("ocr"),
            QueueManifest {
                job_id: "00000000000000000000000000000001".to_string(),
                media_id: 1,
                task: "ocr".to_string(),
                attempt: 1,
                inputs: Vec::new(),
                callback_url: "http://example.test/callback".to_string(),
            },
        ),
        (
            std::path::PathBuf::from("tagging"),
            QueueManifest {
                job_id: "00000000000000000000000000000002".to_string(),
                media_id: 2,
                task: "image_tagging".to_string(),
                attempt: 1,
                inputs: Vec::new(),
                callback_url: "http://example.test/callback".to_string(),
            },
        ),
    ];

    let task = llm_service::scheduler::select_task(&queued, Some("image_tagging"));

    assert_eq!(task, Some("image_tagging"));
}

#[tokio::test]
async fn scheduler_refills_a_completed_slot_before_a_slow_job_finishes() {
    let (_runtime_directory, script_path, start_log) = super::provider::fixture();
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("available port");
        listener.local_addr().expect("local address").port()
    };
    let face_detection = super::provider::service(
        "face_detection",
        "rolling_face_detection",
        port,
        &script_path,
        &start_log,
    );
    let manager = Arc::new(Mutex::new(super::provider::manager(vec![face_detection])));
    let directory = tempdir().expect("queue directory");
    let scheduler = Arc::new(
        Scheduler::new(
            directory.path().to_path_buf(),
            SchedulerConfig {
                max_in_flight_jobs: 2,
                ..SchedulerConfig::default()
            },
            CallbackConfig::default(),
            Arc::clone(&manager),
        )
        .expect("scheduler"),
    );
    let bytes = b"image".to_vec();
    let descriptor = QueueInputDescriptor {
        sequence: 0,
        filename: "input.jpg".to_string(),
        mime_type: "image/jpeg".to_string(),
        byte_size: bytes.len() as u64,
        content_hash: format!("{:x}", Sha256::digest(&bytes)),
        input_kind: "image".to_string(),
        frame_timestamp_ms: None,
    };
    let job_ids = [
        "00000000000000000000000000000000",
        "00000000000000000000000000000001",
        "00000000000000000000000000000002",
    ];
    for (media_id, job_id) in job_ids.iter().enumerate() {
        scheduler
            .accept(
                QueueManifest {
                    job_id: (*job_id).to_string(),
                    media_id: media_id as i64,
                    task: "face_detection".to_string(),
                    attempt: 1,
                    inputs: vec![descriptor.clone()],
                    callback_url: "http://127.0.0.1:9/callback".to_string(),
                },
                vec![(descriptor.clone(), bytes.clone())],
            )
            .expect("accepted");
    }

    let scheduler_task = tokio::spawn(Arc::clone(&scheduler).run());
    let slow_finish = format!("finish:{}", job_ids[0]);
    for _ in 0..300 {
        let events = std::fs::read_to_string(&start_log).unwrap_or_default();
        if events.lines().any(|event| event == slow_finish) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    scheduler_task.abort();
    manager
        .lock()
        .await
        .shutdown()
        .await
        .expect("runtime shutdown");

    let events = std::fs::read_to_string(start_log).expect("runtime events");
    let events = events.lines().collect::<Vec<_>>();
    let replacement_start = events
        .iter()
        .position(|event| **event == format!("start:{}", job_ids[2]))
        .expect("replacement job start");
    let slow_finish = events
        .iter()
        .position(|event| **event == format!("finish:{}", job_ids[0]))
        .expect("slow job finish");
    assert!(replacement_start < slow_finish);
}

#[tokio::test]
async fn cancelled_processing_job_finishes_without_callback_delivery() {
    let (_runtime_directory, script_path, start_log) = super::provider::fixture();
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("available port");
        listener.local_addr().expect("local address").port()
    };
    let face_detection = super::provider::service(
        "face_detection",
        "cancelled_face_detection",
        port,
        &script_path,
        &start_log,
    );
    let manager = Arc::new(Mutex::new(super::provider::manager(vec![face_detection])));
    let directory = tempdir().expect("queue directory");
    let scheduler = Arc::new(
        Scheduler::new(
            directory.path().to_path_buf(),
            SchedulerConfig {
                max_in_flight_jobs: 1,
                ..SchedulerConfig::default()
            },
            CallbackConfig::default(),
            Arc::clone(&manager),
        )
        .expect("scheduler"),
    );
    let callback = MockServer::start().await;
    let bytes = b"image".to_vec();
    let descriptor = QueueInputDescriptor {
        sequence: 0,
        filename: "input.jpg".to_string(),
        mime_type: "image/jpeg".to_string(),
        byte_size: bytes.len() as u64,
        content_hash: format!("{:x}", Sha256::digest(&bytes)),
        input_kind: "image".to_string(),
        frame_timestamp_ms: None,
    };
    let job_id = "00000000000000000000000000000000";
    scheduler
        .accept(
            QueueManifest {
                job_id: job_id.to_string(),
                media_id: 1,
                task: "face_detection".to_string(),
                attempt: 1,
                inputs: vec![descriptor.clone()],
                callback_url: callback.uri(),
            },
            vec![(descriptor, bytes)],
        )
        .expect("accepted");

    let scheduler_task = tokio::spawn(Arc::clone(&scheduler).run());
    let processing = directory.path().join("processing").join(job_id);
    for _ in 0..300 {
        if processing.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(processing.exists(), "job did not enter processing");
    let response = scheduler
        .cancel_jobs(&CancelJobsRequest {
            all: false,
            tasks: vec!["face_detection".to_string()],
            job_ids: Vec::new(),
        })
        .expect("processing cancellation");
    assert_eq!(response.running_jobs, 1);
    for _ in 0..300 {
        if !processing.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    scheduler_task.abort();
    manager
        .lock()
        .await
        .shutdown()
        .await
        .expect("runtime shutdown");

    assert!(!processing.exists());
    assert!(!directory
        .path()
        .join("callback_pending")
        .join(job_id)
        .exists());
    assert!(directory.path().join("cancelled").join(job_id).exists());
    assert!(callback
        .received_requests()
        .await
        .expect("callback requests")
        .is_empty());
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
                idle_shutdown_seconds: 60,
                max_in_flight_jobs: 64,
                runtime_max_attempts: 3,
            },
            CallbackConfig {
                request_timeout_seconds: 5,
                retry_delay_seconds: 60,
                max_attempts: 10,
                max_concurrent_deliveries: 4,
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

#[tokio::test]
async fn callback_window_refills_until_all_fresh_results_are_delivered() {
    let callback_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/callback"))
        .respond_with(ResponseTemplate::new(200))
        .expect(5)
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
                ..SchedulerConfig::default()
            },
            CallbackConfig {
                request_timeout_seconds: 5,
                retry_delay_seconds: 60,
                max_attempts: 10,
                max_concurrent_deliveries: 2,
                key: "callback-key".to_string(),
            },
            Arc::new(Mutex::new(ServiceManager::new(config))),
        )
        .expect("scheduler"),
    );
    for index in 0..5 {
        let job_id = format!("{index:032x}");
        let job_path = directory.path().join("callback_pending").join(&job_id);
        std::fs::create_dir(&job_path).expect("callback job directory");
        std::fs::write(
            job_path.join("manifest.json"),
            serde_json::to_vec(&QueueManifest {
                job_id: job_id.clone(),
                media_id: index,
                task: "ocr".to_string(),
                attempt: 1,
                inputs: Vec::new(),
                callback_url: format!("{}/callback", callback_server.uri()),
            })
            .expect("manifest JSON"),
        )
        .expect("manifest");
        std::fs::write(
            job_path.join("result.json"),
            serde_json::json!({"jobId": job_id}).to_string(),
        )
        .expect("result");
        if index == 0 {
            std::fs::write(
                job_path.join("callback.json"),
                serde_json::json!({"attempts": 1, "next_attempt_at": 0}).to_string(),
            )
            .expect("retry state");
        }
    }

    let scheduler_task = tokio::spawn(Arc::clone(&scheduler).run());
    for _ in 0..100 {
        if std::fs::read_dir(directory.path().join("callback_pending"))
            .expect("callback directory")
            .next()
            .is_none()
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    scheduler_task.abort();

    assert!(std::fs::read_dir(directory.path().join("callback_pending"))
        .expect("callback directory")
        .next()
        .is_none());
    let requests = callback_server
        .received_requests()
        .await
        .expect("callback requests");
    let stale_position = requests
        .iter()
        .position(|request| {
            serde_json::from_slice::<serde_json::Value>(&request.body)
                .is_ok_and(|body| body["jobId"] == "00000000000000000000000000000000")
        })
        .expect("stale retry request");
    assert_eq!(stale_position, 4);
}

#[tokio::test]
async fn cancelled_processing_failure_is_deleted_instead_of_retained_as_failed() {
    let directory = tempdir().expect("queue directory");
    let config = Arc::new(Config {
        service: Vec::new(),
        ..Config::default()
    });
    let scheduler = Arc::new(
        Scheduler::new(
            directory.path().to_path_buf(),
            SchedulerConfig::default(),
            CallbackConfig {
                request_timeout_seconds: 1,
                retry_delay_seconds: 1,
                max_attempts: 1,
                max_concurrent_deliveries: 1,
                key: "callback-key".to_string(),
            },
            Arc::new(Mutex::new(ServiceManager::new(config))),
        )
        .expect("scheduler"),
    );
    let job_id = "00000000000000000000000000000009";
    let bytes = b"image".to_vec();
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
                callback_url: "http://127.0.0.1:9/callback".to_string(),
            },
            vec![(descriptor, bytes)],
        )
        .expect("queued job");
    let processing = directory.path().join("processing").join(job_id);
    std::fs::rename(directory.path().join("queuing").join(job_id), &processing)
        .expect("processing state");
    scheduler
        .cancel_jobs(&CancelJobsRequest {
            all: false,
            tasks: vec!["ocr".to_string()],
            job_ids: vec![job_id.to_string()],
        })
        .expect("running cancellation");
    let callback_pending = directory.path().join("callback_pending").join(job_id);
    std::fs::rename(&processing, &callback_pending).expect("completed processing state");
    std::fs::write(callback_pending.join("result.json"), "{}").expect("callback result");

    let scheduler_task = tokio::spawn(Arc::clone(&scheduler).run());
    for _ in 0..100 {
        if !callback_pending.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    scheduler_task.abort();

    assert!(!callback_pending.exists());
    assert!(!directory.path().join("failed").join(job_id).exists());
    assert!(directory.path().join("cancelled").join(job_id).exists());
}
