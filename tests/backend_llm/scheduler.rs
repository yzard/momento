use async_trait::async_trait;
use llm_service::config::{Config, SchedulerConfig};
use llm_service::provider::{InferenceResponse, InputInferenceResponse, ServiceManager};
use llm_service::result_output::encode_completed_result;
use llm_service::scheduler::QueueInputDescriptor;
use llm_service::scheduler::{QueueAdmission, QueueManifest, Scheduler};
use llm_service::transport::{ResultDeliveryError, ResultDeliveryOutcome, ResultDeliveryTransport};
use momento_common::llm::result_stream::{
    ResultInputCorrelation, ResultManifest, ResultRecordChunkDecoder, ResultRecordCollector,
    ValidatedResultValue,
};
use momento_common::llm::CancelJobsRequest;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tokio::sync::Mutex;

struct MockResultDeliveryTransport {
    failure: Option<String>,
    outcome: ResultDeliveryOutcome,
    connected: AtomicBool,
    deliveries: Mutex<Vec<(String, ResultManifest, Vec<u8>)>>,
}

impl MockResultDeliveryTransport {
    fn acknowledging() -> Arc<Self> {
        Arc::new(Self {
            failure: None,
            outcome: ResultDeliveryOutcome::Received,
            connected: AtomicBool::new(true),
            deliveries: Mutex::new(Vec::new()),
        })
    }

    fn failing(error: &str) -> Arc<Self> {
        Arc::new(Self {
            failure: Some(error.to_string()),
            outcome: ResultDeliveryOutcome::Received,
            connected: AtomicBool::new(true),
            deliveries: Mutex::new(Vec::new()),
        })
    }

    fn returning(outcome: ResultDeliveryOutcome) -> Arc<Self> {
        Arc::new(Self {
            failure: None,
            outcome,
            connected: AtomicBool::new(true),
            deliveries: Mutex::new(Vec::new()),
        })
    }

    fn disconnected() -> Arc<Self> {
        Arc::new(Self {
            failure: None,
            outcome: ResultDeliveryOutcome::Received,
            connected: AtomicBool::new(false),
            deliveries: Mutex::new(Vec::new()),
        })
    }

    fn connect(&self) {
        self.connected.store(true, Ordering::Release);
    }
}

#[async_trait]
impl ResultDeliveryTransport for MockResultDeliveryTransport {
    async fn client_is_connected(&self, _client_id: &str) -> bool {
        self.connected.load(Ordering::Acquire)
    }

    async fn deliver_result(
        &self,
        client_id: &str,
        manifest: &ResultManifest,
        records_path: &Path,
        _acknowledgement_timeout: Duration,
    ) -> Result<ResultDeliveryOutcome, ResultDeliveryError> {
        let records = std::fs::read(records_path).map_err(|error| {
            ResultDeliveryError::attempt_failed(format!("could not read result records: {error}"))
        })?;
        self.deliveries
            .lock()
            .await
            .push((client_id.to_string(), manifest.clone(), records));
        match &self.failure {
            Some(error) => Err(ResultDeliveryError::attempt_failed(error.clone())),
            None => Ok(self.outcome.clone()),
        }
    }
}

fn write_completed_result(job_path: &std::path::Path, manifest: &QueueManifest) {
    let responses = manifest
        .inputs
        .iter()
        .map(|input| InputInferenceResponse {
            sequence: input.sequence,
            frame_timestamp_ms: input.frame_timestamp_ms,
            response: InferenceResponse {
                task: manifest.task.clone(),
                text: "result".to_string(),
                markdown: "result".to_string(),
                provider: "test".to_string(),
                model_type: "ocr".to_string(),
                model_version: "test".to_string(),
                tags: Vec::new(),
                embedding: None,
                embedding_encoding: None,
                embedding_dimensions: None,
                perceptual_hash: None,
                quality_score: None,
                aesthetic_score: None,
                scenic_score: None,
                simplicity_score: None,
                landscape_score: None,
                technical_quality_score: None,
                faces: Vec::new(),
                detected: None,
                confidence: None,
            },
        })
        .collect();
    let output = encode_completed_result(
        &manifest.job_id,
        manifest.media_id,
        &manifest.task,
        manifest.attempt,
        &manifest.inputs,
        responses,
    )
    .expect("encoded result");
    std::fs::write(job_path.join("result-records.bin"), output.records).expect("result records");
    std::fs::write(
        job_path.join("result-manifest.json"),
        serde_json::to_vec(&output.manifest).expect("result manifest JSON"),
    )
    .expect("result manifest");
}

fn write_callback_result(queue_dir: &Path, job_id: &str, client_id: &str) -> PathBuf {
    let job_path = queue_dir.join("callback_pending").join(job_id);
    std::fs::create_dir(&job_path).expect("callback job directory");
    let manifest = QueueManifest {
        client_id: client_id.to_string(),
        job_id: job_id.to_string(),
        media_id: 3,
        task: "ocr".to_string(),
        attempt: 1,
        inputs: vec![result_descriptor()],
    };
    std::fs::write(
        job_path.join("manifest.json"),
        serde_json::to_vec(&manifest).expect("manifest JSON"),
    )
    .expect("manifest");
    write_completed_result(&job_path, &manifest);
    job_path
}

fn result_descriptor() -> QueueInputDescriptor {
    QueueInputDescriptor {
        sequence: 0,
        filename: "result-input.jpg".to_string(),
        mime_type: "image/jpeg".to_string(),
        byte_size: 1,
        content_hash: "0".repeat(64),
        input_kind: "image".to_string(),
        frame_timestamp_ms: None,
    }
}

fn scheduler_with_capacity(queue_dir: &Path, max_queue_bytes: u64) -> Scheduler {
    Scheduler::new(
        queue_dir.to_path_buf(),
        SchedulerConfig {
            max_queue_bytes,
            working_space_reserve_bytes: 1,
            ..SchedulerConfig::default()
        },
        Arc::new(Mutex::new(ServiceManager::new(Arc::new(Config {
            service: Vec::new(),
            ..Config::default()
        })))),
        MockResultDeliveryTransport::acknowledging(),
    )
    .expect("scheduler")
}

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
        Arc::new(Mutex::new(ServiceManager::new(config))),
        MockResultDeliveryTransport::acknowledging(),
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
                client_id: "client-a".to_string(),
                job_id: job_id.to_string(),
                media_id: 1,
                task: "ocr".to_string(),
                attempt: 1,
                inputs: vec![input_descriptor.clone()],
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
fn duplicate_job_id_requires_an_identical_owned_manifest() {
    let directory = tempdir().expect("queue directory");
    let scheduler = Scheduler::new(
        directory.path().to_path_buf(),
        SchedulerConfig::default(),
        Arc::new(Mutex::new(ServiceManager::new(Arc::new(Config {
            service: Vec::new(),
            ..Config::default()
        })))),
        MockResultDeliveryTransport::acknowledging(),
    )
    .expect("scheduler");
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
    let manifest = QueueManifest {
        client_id: "client-a".to_string(),
        job_id: "018f36e77c917cc89f7054252a33eaf0".to_string(),
        media_id: 1,
        task: "ocr".to_string(),
        attempt: 1,
        inputs: vec![descriptor.clone()],
    };
    scheduler
        .accept(manifest.clone(), vec![(descriptor, bytes)])
        .expect("initial admission");

    assert!(matches!(
        scheduler.begin_admission(manifest.clone()),
        Ok(QueueAdmission::Duplicate)
    ));
    let mut mismatched = manifest;
    mismatched.media_id = 2;
    assert!(scheduler.begin_admission(mismatched).is_err());
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
        Arc::new(Mutex::new(ServiceManager::new(Arc::new(Config {
            service: Vec::new(),
            ..Config::default()
        })))),
        MockResultDeliveryTransport::acknowledging(),
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
        client_id: "client-a".to_string(),
        job_id: job_id.to_string(),
        media_id: 1,
        task: "face_detection".to_string(),
        attempt: 1,
        inputs: vec![descriptor.clone()],
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
        Arc::new(Mutex::new(ServiceManager::new(config))),
        MockResultDeliveryTransport::acknowledging(),
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
                client_id: "client-a".to_string(),
                job_id: job_id.to_string(),
                media_id: 1,
                task: "ocr".to_string(),
                attempt: 1,
                inputs: vec![descriptor.clone()],
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
        Arc::new(Mutex::new(ServiceManager::new(config))),
        MockResultDeliveryTransport::acknowledging(),
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
                client_id: "client-a".to_string(),
                job_id: job_id.to_string(),
                media_id: 1,
                task: "ocr".to_string(),
                attempt: 1,
                inputs: vec![first_descriptor.clone(), second_descriptor.clone()],
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
fn queue_acceptance_recognizes_image_aesthetics_with_non_contiguous_sequences() {
    let directory = tempdir().expect("queue directory");
    let scheduler = Scheduler::new(
        directory.path().to_path_buf(),
        SchedulerConfig::default(),
        Arc::new(Mutex::new(ServiceManager::new(Arc::new(Config {
            service: Vec::new(),
            ..Config::default()
        })))),
        MockResultDeliveryTransport::acknowledging(),
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
                client_id: "client-a".to_string(),
                job_id: "0123456789abcdef0123456789abcdef".to_string(),
                media_id: 1,
                task: "image_aesthetics".to_string(),
                attempt: 1,
                inputs: vec![descriptor.clone()],
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
fn queue_admission_and_cancellation_recognize_classifier_tasks() {
    let directory = tempdir().expect("queue directory");
    let scheduler = Scheduler::new(
        directory.path().to_path_buf(),
        SchedulerConfig::default(),
        Arc::new(Mutex::new(ServiceManager::new(Arc::new(Config {
            service: Vec::new(),
            ..Config::default()
        })))),
        MockResultDeliveryTransport::acknowledging(),
    )
    .expect("scheduler");

    for (index, task) in ["screenshot_detection", "document_detection"]
        .into_iter()
        .enumerate()
    {
        let bytes = format!("classifier-{index}").into_bytes();
        let descriptor = QueueInputDescriptor {
            sequence: 0,
            filename: format!("classifier-{index}.jpg"),
            mime_type: "image/jpeg".to_string(),
            byte_size: bytes.len() as u64,
            content_hash: format!("{:x}", Sha256::digest(&bytes)),
            input_kind: "image".to_string(),
            frame_timestamp_ms: None,
        };
        scheduler
            .accept(
                QueueManifest {
                    client_id: "classifier-client".to_string(),
                    job_id: format!("abcdef0000000000000000000000000{index}"),
                    media_id: index as i64 + 1,
                    task: task.to_string(),
                    attempt: 1,
                    inputs: vec![descriptor.clone()],
                },
                vec![(descriptor, bytes)],
            )
            .expect("classifier admission");
    }

    let response = scheduler
        .cancel_jobs(
            "classifier-client",
            &CancelJobsRequest {
                all: false,
                tasks: vec![
                    "screenshot_detection".to_string(),
                    "document_detection".to_string(),
                ],
                job_ids: Vec::new(),
            },
        )
        .expect("classifier cancellation");

    assert_eq!(response.cancelled_jobs, 2);
    assert!(directory
        .path()
        .join("cancelled/classifier-client-abcdef00000000000000000000000000")
        .is_file());
    assert!(directory
        .path()
        .join("cancelled/classifier-client-abcdef00000000000000000000000001")
        .is_file());
}

#[tokio::test]
async fn classifier_job_uses_first_input_as_aggregate_and_preserves_all_input_results() {
    let (_runtime_directory, script_path, start_log) = super::provider::fixture();
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("available port");
        listener.local_addr().expect("local address").port()
    };
    let classifier = super::provider::service(
        "document_detection",
        "ordered_classifier",
        port,
        &script_path,
        &start_log,
    );
    let manager = Arc::new(Mutex::new(super::provider::manager(vec![classifier])));
    let directory = tempdir().expect("queue directory");
    let result_delivery = MockResultDeliveryTransport::acknowledging();
    let scheduler = Arc::new(
        Scheduler::new(
            directory.path().to_path_buf(),
            SchedulerConfig {
                max_in_flight_jobs: 1,
                ..SchedulerConfig::default()
            },
            Arc::clone(&manager),
            result_delivery.clone(),
        )
        .expect("scheduler"),
    );
    let input_bytes = [b"first".to_vec(), b"second".to_vec()];
    let descriptors = input_bytes
        .iter()
        .enumerate()
        .map(|(sequence, bytes)| QueueInputDescriptor {
            sequence: sequence as u32,
            filename: format!("document-{sequence}.jpg"),
            mime_type: "image/jpeg".to_string(),
            byte_size: bytes.len() as u64,
            content_hash: format!("{:x}", Sha256::digest(bytes)),
            input_kind: "image".to_string(),
            frame_timestamp_ms: Some(sequence as i64 * 1000),
        })
        .collect::<Vec<_>>();
    let expected_descriptors = descriptors.clone();
    let scheduler_task = tokio::spawn(Arc::clone(&scheduler).run());
    tokio::task::yield_now().await;
    scheduler
        .accept(
            QueueManifest {
                client_id: "classifier-client".to_string(),
                job_id: "abcdef1234567890abcdef1234567890".to_string(),
                media_id: 9,
                task: "document_detection".to_string(),
                attempt: 1,
                inputs: descriptors.clone(),
            },
            descriptors
                .into_iter()
                .zip(input_bytes.into_iter())
                .collect(),
        )
        .expect("classifier admission");

    for _ in 0..300 {
        if !result_delivery.deliveries.lock().await.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    scheduler_task.abort();
    manager
        .lock()
        .await
        .shutdown()
        .await
        .expect("runtime shutdown");

    let deliveries = result_delivery.deliveries.lock().await;
    let (_, result_manifest, result_records) = deliveries.first().expect("classifier delivery");
    let correlations = expected_descriptors
        .iter()
        .map(|input| ResultInputCorrelation {
            sequence: input.sequence,
            frame_timestamp_ms: input.frame_timestamp_ms,
        })
        .collect::<Vec<_>>();
    let mut collector = ResultRecordCollector::new(
        &result_manifest.task,
        result_manifest.status,
        &correlations,
        result_manifest.record_count,
        result_manifest.byte_size,
    )
    .expect("result collector");
    let mut decoder = ResultRecordChunkDecoder::new();
    decoder
        .push(result_records, |record| {
            collector.push(record.as_borrowed())
        })
        .expect("classifier records");
    decoder.finish().expect("complete classifier records");
    let result = collector.finish().expect("classifier result");
    let ValidatedResultValue::DocumentDetection(first) = &result.inputs[0].value else {
        panic!("expected first document result");
    };
    let ValidatedResultValue::DocumentDetection(second) = &result.inputs[1].value else {
        panic!("expected second document result");
    };
    assert!(!first.detected);
    assert!((first.confidence - 0.2).abs() < 0.000_001);
    assert!(second.detected);
    assert!((second.confidence - 0.9).abs() < 0.000_001);
}

#[test]
fn abandoned_staging_removes_temporary_queue_directory() {
    let directory = tempdir().expect("queue directory");
    let scheduler = Scheduler::new(
        directory.path().to_path_buf(),
        SchedulerConfig::default(),
        Arc::new(Mutex::new(ServiceManager::new(Arc::new(Config {
            service: Vec::new(),
            ..Config::default()
        })))),
        MockResultDeliveryTransport::acknowledging(),
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
            client_id: "client-a".to_string(),
            job_id: job_id.to_string(),
            media_id: 1,
            task: "ocr".to_string(),
            attempt: 1,
            inputs: vec![descriptor],
        })
        .expect("admission");
    assert!(matches!(admission, QueueAdmission::Staging(_)));
    assert!(directory.path().join(format!(".tmp/{job_id}")).is_dir());

    drop(admission);

    assert!(!directory.path().join(format!(".tmp/{job_id}")).exists());
}

#[test]
fn admission_defers_before_creating_staging_when_queue_capacity_is_full() {
    let directory = tempdir().expect("queue directory");
    let scheduler = scheduler_with_capacity(directory.path(), 5);
    let existing_bytes = b"12345".to_vec();
    let existing_descriptor = QueueInputDescriptor {
        sequence: 0,
        filename: "existing.jpg".to_string(),
        mime_type: "image/jpeg".to_string(),
        byte_size: existing_bytes.len() as u64,
        content_hash: format!("{:x}", Sha256::digest(&existing_bytes)),
        input_kind: "image".to_string(),
        frame_timestamp_ms: None,
    };
    scheduler
        .accept(
            QueueManifest {
                client_id: "client-a".to_string(),
                job_id: "0123456789abcdef0123456789abc001".to_string(),
                media_id: 1,
                task: "ocr".to_string(),
                attempt: 1,
                inputs: vec![existing_descriptor.clone()],
            },
            vec![(existing_descriptor.clone(), existing_bytes)],
        )
        .expect("seed queue content");
    let next_job_id = "0123456789abcdef0123456789abc002";
    let next_bytes = b"x";
    let admission = scheduler
        .begin_admission(QueueManifest {
            client_id: "client-a".to_string(),
            job_id: next_job_id.to_string(),
            media_id: 2,
            task: "ocr".to_string(),
            attempt: 1,
            inputs: vec![QueueInputDescriptor {
                sequence: 0,
                filename: "next.jpg".to_string(),
                mime_type: "image/jpeg".to_string(),
                byte_size: 1,
                content_hash: format!("{:x}", Sha256::digest(next_bytes)),
                input_kind: "image".to_string(),
                frame_timestamp_ms: None,
            }],
        })
        .expect("capacity decision");

    assert!(matches!(admission, QueueAdmission::Deferred(_)));
    assert!(!directory.path().join(".tmp").join(next_job_id).exists());

    let cached_job_id = "0123456789abcdef0123456789abc003";
    let cached = scheduler
        .begin_admission(QueueManifest {
            client_id: "client-a".to_string(),
            job_id: cached_job_id.to_string(),
            media_id: 3,
            task: "image_tagging".to_string(),
            attempt: 1,
            inputs: vec![existing_descriptor],
        })
        .expect("cached admission");
    let QueueAdmission::Staging(cached) = cached else {
        panic!("cached content must not consume queue capacity");
    };
    assert!(cached.required_sequences().is_empty());
}

#[test]
fn cached_source_is_not_reused_until_its_first_admission_commits() {
    let directory = tempdir().expect("queue directory");
    let scheduler = scheduler_with_capacity(directory.path(), 10);
    let bytes = b"shared";
    let descriptor = QueueInputDescriptor {
        sequence: 0,
        filename: "shared.jpg".to_string(),
        mime_type: "image/jpeg".to_string(),
        byte_size: bytes.len() as u64,
        content_hash: format!("{:x}", Sha256::digest(bytes)),
        input_kind: "image".to_string(),
        frame_timestamp_ms: None,
    };
    let first_manifest = QueueManifest {
        client_id: "client-a".to_string(),
        job_id: "0123456789abcdef0123456789abc030".to_string(),
        media_id: 1,
        task: "ocr".to_string(),
        attempt: 1,
        inputs: vec![descriptor.clone()],
    };
    let QueueAdmission::Staging(mut first) = scheduler
        .begin_admission(first_manifest)
        .expect("first admission")
    else {
        panic!("first admission must stage");
    };
    first
        .write_input(&descriptor, bytes)
        .expect("publish first input");

    let second_manifest = QueueManifest {
        client_id: "client-a".to_string(),
        job_id: "0123456789abcdef0123456789abc031".to_string(),
        media_id: 2,
        task: "image_tagging".to_string(),
        attempt: 1,
        inputs: vec![descriptor.clone()],
    };
    assert!(matches!(
        scheduler
            .begin_admission(second_manifest.clone())
            .expect("second capacity decision"),
        QueueAdmission::Deferred(_)
    ));

    assert!(first.commit().expect("commit first admission"));
    let QueueAdmission::Staging(second) = scheduler
        .begin_admission(second_manifest)
        .expect("second admission after commit")
    else {
        panic!("committed cached source must be reusable");
    };
    assert!(second.required_sequences().is_empty());
}

#[test]
fn cancelling_a_queued_job_releases_unique_content_capacity() {
    let directory = tempdir().expect("queue directory");
    let scheduler = scheduler_with_capacity(directory.path(), 5);
    let first_job_id = "0123456789abcdef0123456789abc010";
    let first_bytes = b"12345".to_vec();
    let first_descriptor = QueueInputDescriptor {
        sequence: 0,
        filename: "first.jpg".to_string(),
        mime_type: "image/jpeg".to_string(),
        byte_size: first_bytes.len() as u64,
        content_hash: format!("{:x}", Sha256::digest(&first_bytes)),
        input_kind: "image".to_string(),
        frame_timestamp_ms: None,
    };
    scheduler
        .accept(
            QueueManifest {
                client_id: "client-a".to_string(),
                job_id: first_job_id.to_string(),
                media_id: 1,
                task: "ocr".to_string(),
                attempt: 1,
                inputs: vec![first_descriptor.clone()],
            },
            vec![(first_descriptor, first_bytes)],
        )
        .expect("first admission");

    let response = scheduler
        .cancel_jobs(
            "client-a",
            &CancelJobsRequest {
                all: false,
                tasks: vec!["ocr".to_string()],
                job_ids: vec![first_job_id.to_string()],
            },
        )
        .expect("cancel first job");
    assert_eq!(response.cancelled_jobs, 1);

    let second_job_id = "0123456789abcdef0123456789abc011";
    let second_bytes = b"abcde";
    let admission = scheduler
        .begin_admission(QueueManifest {
            client_id: "client-a".to_string(),
            job_id: second_job_id.to_string(),
            media_id: 2,
            task: "ocr".to_string(),
            attempt: 1,
            inputs: vec![QueueInputDescriptor {
                sequence: 0,
                filename: "second.jpg".to_string(),
                mime_type: "image/jpeg".to_string(),
                byte_size: second_bytes.len() as u64,
                content_hash: format!("{:x}", Sha256::digest(second_bytes)),
                input_kind: "image".to_string(),
                frame_timestamp_ms: None,
            }],
        })
        .expect("second capacity decision");
    assert!(matches!(admission, QueueAdmission::Staging(_)));
}

#[tokio::test]
async fn acknowledged_result_releases_unique_content_capacity() {
    let directory = tempdir().expect("queue directory");
    let scheduler = Arc::new(scheduler_with_capacity(directory.path(), 5));
    let first_job_id = "0123456789abcdef0123456789abc020";
    let first_bytes = b"12345".to_vec();
    let first_descriptor = QueueInputDescriptor {
        sequence: 0,
        filename: "first.jpg".to_string(),
        mime_type: "image/jpeg".to_string(),
        byte_size: first_bytes.len() as u64,
        content_hash: format!("{:x}", Sha256::digest(&first_bytes)),
        input_kind: "image".to_string(),
        frame_timestamp_ms: None,
    };
    let first_manifest = QueueManifest {
        client_id: "client-a".to_string(),
        job_id: first_job_id.to_string(),
        media_id: 1,
        task: "ocr".to_string(),
        attempt: 1,
        inputs: vec![first_descriptor.clone()],
    };
    scheduler
        .accept(
            first_manifest.clone(),
            vec![(first_descriptor, first_bytes)],
        )
        .expect("first admission");
    let callback_path = directory.path().join("callback_pending").join(first_job_id);
    std::fs::rename(
        directory.path().join("queuing").join(first_job_id),
        &callback_path,
    )
    .expect("callback transition fixture");
    write_completed_result(&callback_path, &first_manifest);

    let scheduler_task = tokio::spawn(Arc::clone(&scheduler).run());
    for _ in 0..100 {
        if !callback_path.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    scheduler_task.abort();
    assert!(!callback_path.exists(), "acknowledged job was not removed");

    let second_job_id = "0123456789abcdef0123456789abc021";
    let second_bytes = b"abcde";
    let admission = scheduler
        .begin_admission(QueueManifest {
            client_id: "client-a".to_string(),
            job_id: second_job_id.to_string(),
            media_id: 2,
            task: "ocr".to_string(),
            attempt: 1,
            inputs: vec![QueueInputDescriptor {
                sequence: 0,
                filename: "second.jpg".to_string(),
                mime_type: "image/jpeg".to_string(),
                byte_size: second_bytes.len() as u64,
                content_hash: format!("{:x}", Sha256::digest(second_bytes)),
                input_kind: "image".to_string(),
                frame_timestamp_ms: None,
            }],
        })
        .expect("second capacity decision");
    assert!(matches!(admission, QueueAdmission::Staging(_)));
}

#[test]
fn admission_permanently_rejects_a_job_larger_than_the_queue_budget() {
    let directory = tempdir().expect("queue directory");
    let scheduler = scheduler_with_capacity(directory.path(), 4);
    let job_id = "0123456789abcdef0123456789abc004";
    let bytes = b"12345";
    let admission = scheduler
        .begin_admission(QueueManifest {
            client_id: "client-a".to_string(),
            job_id: job_id.to_string(),
            media_id: 1,
            task: "ocr".to_string(),
            attempt: 1,
            inputs: vec![QueueInputDescriptor {
                sequence: 0,
                filename: "large.jpg".to_string(),
                mime_type: "image/jpeg".to_string(),
                byte_size: bytes.len() as u64,
                content_hash: format!("{:x}", Sha256::digest(bytes)),
                input_kind: "image".to_string(),
                frame_timestamp_ms: None,
            }],
        })
        .expect("capacity decision");

    assert!(matches!(admission, QueueAdmission::JobTooLarge(_)));
    assert!(!directory.path().join(".tmp").join(job_id).exists());
}

#[test]
fn admission_allows_streamed_media_descriptors_larger_than_twenty_gibibytes() {
    let directory = tempdir().expect("queue directory");
    let scheduler = Scheduler::new(
        directory.path().to_path_buf(),
        SchedulerConfig {
            max_queue_bytes: 32 * 1024 * 1024 * 1024,
            working_space_reserve_bytes: 1,
            ..SchedulerConfig::default()
        },
        Arc::new(Mutex::new(ServiceManager::new(Arc::new(Config {
            service: Vec::new(),
            ..Config::default()
        })))),
        MockResultDeliveryTransport::acknowledging(),
    )
    .expect("scheduler");
    let job_id = "1123456789abcdef0123456789abcdef";
    let admission = scheduler
        .begin_admission(QueueManifest {
            client_id: "client-a".to_string(),
            job_id: job_id.to_string(),
            media_id: 1,
            task: "ocr".to_string(),
            attempt: 1,
            inputs: vec![QueueInputDescriptor {
                sequence: 0,
                filename: "large-image.tiff".to_string(),
                mime_type: "image/tiff".to_string(),
                byte_size: 20 * 1024 * 1024 * 1024 + 1,
                content_hash: "a".repeat(64),
                input_kind: "image".to_string(),
                frame_timestamp_ms: None,
            }],
        })
        .expect("large streamed input descriptor");

    assert!(matches!(admission, QueueAdmission::Staging(_)));
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
        Arc::new(Mutex::new(ServiceManager::new(Arc::new(Config {
            service: Vec::new(),
            ..Config::default()
        })))),
        MockResultDeliveryTransport::acknowledging(),
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
                    client_id: "client-a".to_string(),
                    job_id: format!("{index:032x}"),
                    media_id: i64::from(index) + 1,
                    task: if index % 2 == 0 {
                        "image_tagging".to_string()
                    } else {
                        "ocr".to_string()
                    },
                    attempt: 1,
                    inputs: vec![descriptor.clone()],
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
        Arc::new(Mutex::new(ServiceManager::new(config))),
        MockResultDeliveryTransport::acknowledging(),
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
                    client_id: "client-a".to_string(),
                    job_id: job_id.to_string(),
                    media_id: 1,
                    task: task.to_string(),
                    attempt: 1,
                    inputs: vec![descriptor.clone()],
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
                client_id: "client-a".to_string(),
                job_id: "00000000000000000000000000000001".to_string(),
                media_id: 1,
                task: "ocr".to_string(),
                attempt: 1,
                inputs: Vec::new(),
            },
        ),
        (
            std::path::PathBuf::from("tagging"),
            QueueManifest {
                client_id: "client-a".to_string(),
                job_id: "00000000000000000000000000000002".to_string(),
                media_id: 2,
                task: "image_tagging".to_string(),
                attempt: 1,
                inputs: Vec::new(),
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
            Arc::clone(&manager),
            MockResultDeliveryTransport::acknowledging(),
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
                    client_id: "client-a".to_string(),
                    job_id: (*job_id).to_string(),
                    media_id: media_id as i64 + 1,
                    task: "face_detection".to_string(),
                    attempt: 1,
                    inputs: vec![descriptor.clone()],
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
    let result_delivery = MockResultDeliveryTransport::acknowledging();
    let scheduler = Arc::new(
        Scheduler::new(
            directory.path().to_path_buf(),
            SchedulerConfig {
                max_in_flight_jobs: 1,
                ..SchedulerConfig::default()
            },
            Arc::clone(&manager),
            result_delivery.clone(),
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
    let job_id = "00000000000000000000000000000000";
    scheduler
        .accept(
            QueueManifest {
                client_id: "client-a".to_string(),
                job_id: job_id.to_string(),
                media_id: 1,
                task: "face_detection".to_string(),
                attempt: 1,
                inputs: vec![descriptor.clone()],
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
        .cancel_jobs(
            "client-a",
            &CancelJobsRequest {
                all: false,
                tasks: vec!["face_detection".to_string()],
                job_ids: Vec::new(),
            },
        )
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
    assert!(directory
        .path()
        .join("cancelled")
        .join(format!("client-a-{job_id}"))
        .exists());
    assert!(result_delivery.deliveries.lock().await.is_empty());
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
        Arc::new(Mutex::new(ServiceManager::new(config))),
        MockResultDeliveryTransport::acknowledging(),
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
                client_id: "client-a".to_string(),
                job_id: processing_job_id.to_string(),
                media_id: 1,
                task: "ocr".to_string(),
                attempt: 1,
                inputs: vec![descriptor.clone()],
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
    std::fs::write(
        directory
            .path()
            .join(format!("processing/{processing_job_id}/result-records.tmp")),
        b"partial result",
    )
    .expect("partial processing result");
    let completed_processing_job_id = "018f36e77c917cc89f7054252a33eaf5";
    let completed_processing_manifest = QueueManifest {
        client_id: "client-a".to_string(),
        job_id: completed_processing_job_id.to_string(),
        media_id: 3,
        task: "ocr".to_string(),
        attempt: 1,
        inputs: vec![descriptor.clone()],
    };
    scheduler
        .accept(
            completed_processing_manifest.clone(),
            vec![(descriptor.clone(), raw_bytes.clone())],
        )
        .expect("accepted completed processing job");
    let completed_processing_path = directory
        .path()
        .join(format!("processing/{completed_processing_job_id}"));
    std::fs::rename(
        directory
            .path()
            .join(format!("queuing/{completed_processing_job_id}")),
        &completed_processing_path,
    )
    .expect("move completed processing job");
    write_completed_result(&completed_processing_path, &completed_processing_manifest);
    std::fs::write(
        completed_processing_path.join("result-manifest.tmp"),
        b"stale temporary",
    )
    .expect("stale result temporary");
    let callback_job_id = "018f36e77c917cc89f7054252a33eaf3";
    let callback_manifest = QueueManifest {
        client_id: "client-a".to_string(),
        job_id: callback_job_id.to_string(),
        media_id: 2,
        task: "ocr".to_string(),
        attempt: 1,
        inputs: vec![descriptor.clone()],
    };
    scheduler
        .accept(callback_manifest.clone(), vec![(descriptor, raw_bytes)])
        .expect("accepted");
    std::fs::rename(
        directory.path().join(format!("queuing/{callback_job_id}")),
        directory
            .path()
            .join(format!("callback_pending/{callback_job_id}")),
    )
    .expect("move callback pending");
    write_completed_result(
        &directory
            .path()
            .join(format!("callback_pending/{callback_job_id}")),
        &callback_manifest,
    );
    drop(scheduler);
    let config = Arc::new(Config {
        service: Vec::new(),
        ..Config::default()
    });
    let _recovered = Scheduler::new(
        directory.path().to_path_buf(),
        SchedulerConfig::default(),
        Arc::new(Mutex::new(ServiceManager::new(config))),
        MockResultDeliveryTransport::acknowledging(),
    )
    .expect("recovered scheduler");
    assert!(directory
        .path()
        .join(format!("queuing/{processing_job_id}"))
        .is_dir());
    assert!(!directory
        .path()
        .join(format!("queuing/{processing_job_id}/result-records.tmp"))
        .exists());
    assert!(directory
        .path()
        .join(format!(
            "callback_pending/{completed_processing_job_id}/result-records.bin"
        ))
        .is_file());
    assert!(!directory
        .path()
        .join(format!(
            "callback_pending/{completed_processing_job_id}/result-manifest.tmp"
        ))
        .exists());
    assert!(directory
        .path()
        .join(format!(
            "callback_pending/{callback_job_id}/result-manifest.json"
        ))
        .is_file());
    assert!(directory
        .path()
        .join(format!(
            "callback_pending/{callback_job_id}/result-records.bin"
        ))
        .is_file());
}

#[tokio::test]
async fn result_delivery_failure_is_recorded_for_retry() {
    let directory = tempdir().expect("queue directory");
    let config = Arc::new(Config {
        service: Vec::new(),
        ..Config::default()
    });
    let result_delivery = MockResultDeliveryTransport::failing("client rejected result");
    let scheduler = Arc::new(
        Scheduler::new(
            directory.path().to_path_buf(),
            SchedulerConfig {
                result_delivery_retry_delay_seconds: 60,
                result_delivery_max_concurrent_deliveries: 4,
                ..SchedulerConfig::default()
            },
            Arc::new(Mutex::new(ServiceManager::new(config))),
            result_delivery.clone(),
        )
        .expect("scheduler"),
    );
    let job_id = "018f36e77c917cc89f7054252a33eaf4";
    let job_path = write_callback_result(directory.path(), job_id, "client-a");

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
    assert_eq!(last_error, "client rejected result");
    let deliveries = result_delivery.deliveries.lock().await;
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].0, "client-a");
    assert_eq!(deliveries[0].1.job_id, job_id);
}

#[tokio::test]
async fn deferred_result_delivery_does_not_consume_a_delivery_attempt() {
    let directory = tempdir().expect("queue directory");
    let config = Arc::new(Config {
        service: Vec::new(),
        ..Config::default()
    });
    let result_delivery = MockResultDeliveryTransport::returning(ResultDeliveryOutcome::Deferred {
        retry_after_ms: 60_000,
    });
    let scheduler = Arc::new(
        Scheduler::new(
            directory.path().to_path_buf(),
            SchedulerConfig {
                result_delivery_max_concurrent_deliveries: 1,
                ..SchedulerConfig::default()
            },
            Arc::new(Mutex::new(ServiceManager::new(config))),
            result_delivery,
        )
        .expect("scheduler"),
    );
    let job_id = "018f36e77c917cc89f7054252a33eaf5";
    let job_path = write_callback_result(directory.path(), job_id, "client-a");

    let scheduler_task = tokio::spawn(Arc::clone(&scheduler).run());
    let callback_state_path = job_path.join("callback.json");
    for _ in 0..50 {
        if callback_state_path.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    scheduler_task.abort();

    let callback_state: serde_json::Value = serde_json::from_slice(
        &std::fs::read(callback_state_path).expect("callback deferred state"),
    )
    .expect("callback state JSON");
    assert_eq!(callback_state["attempts"], 0);
    assert!(job_path.is_dir());
}

#[tokio::test]
async fn disconnected_client_results_resume_when_the_client_reconnects() {
    let directory = tempdir().expect("queue directory");
    let config = Arc::new(Config {
        service: Vec::new(),
        ..Config::default()
    });
    let result_delivery = MockResultDeliveryTransport::disconnected();
    let scheduler = Arc::new(
        Scheduler::new(
            directory.path().to_path_buf(),
            SchedulerConfig {
                result_delivery_max_attempts: 1,
                result_delivery_max_concurrent_deliveries: 1,
                ..SchedulerConfig::default()
            },
            Arc::new(Mutex::new(ServiceManager::new(config))),
            result_delivery.clone(),
        )
        .expect("scheduler"),
    );
    let job_id = "018f36e77c917cc89f7054252a33eaf7";
    let job_path = write_callback_result(directory.path(), job_id, "client-a");

    let scheduler_task = tokio::spawn(Arc::clone(&scheduler).run());
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(job_path.is_dir());
    assert!(!job_path.join("callback.json").exists());
    assert!(result_delivery.deliveries.lock().await.is_empty());

    result_delivery.connect();
    scheduler.wake_result_delivery();
    for _ in 0..100 {
        if !job_path.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    scheduler_task.abort();

    assert!(!job_path.exists());
    assert_eq!(result_delivery.deliveries.lock().await.len(), 1);
}

#[tokio::test]
async fn rejected_result_delivery_is_moved_to_failed_with_evidence() {
    let directory = tempdir().expect("queue directory");
    let config = Arc::new(Config {
        service: Vec::new(),
        ..Config::default()
    });
    let result_delivery = MockResultDeliveryTransport::returning(ResultDeliveryOutcome::Rejected {
        error: "permanent protocol rejection".to_string(),
    });
    let scheduler = Arc::new(
        Scheduler::new(
            directory.path().to_path_buf(),
            SchedulerConfig {
                result_delivery_max_concurrent_deliveries: 1,
                ..SchedulerConfig::default()
            },
            Arc::new(Mutex::new(ServiceManager::new(config))),
            result_delivery,
        )
        .expect("scheduler"),
    );
    let job_id = "018f36e77c917cc89f7054252a33eaf6";
    write_callback_result(directory.path(), job_id, "client-a");

    let scheduler_task = tokio::spawn(Arc::clone(&scheduler).run());
    let failed_path = directory.path().join(format!("failed/{job_id}"));
    for _ in 0..50 {
        if failed_path.is_dir() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    scheduler_task.abort();

    let evidence = std::fs::read_to_string(failed_path.join("failure.json"))
        .expect("durable rejection evidence");
    assert!(evidence.contains("permanent protocol rejection"));
}

#[tokio::test]
async fn result_delivery_window_refills_and_prioritizes_fresh_results() {
    let directory = tempdir().expect("queue directory");
    let config = Arc::new(Config {
        service: Vec::new(),
        ..Config::default()
    });
    let result_delivery = MockResultDeliveryTransport::acknowledging();
    let scheduler = Arc::new(
        Scheduler::new(
            directory.path().to_path_buf(),
            SchedulerConfig {
                result_delivery_max_concurrent_deliveries: 2,
                ..SchedulerConfig::default()
            },
            Arc::new(Mutex::new(ServiceManager::new(config))),
            result_delivery.clone(),
        )
        .expect("scheduler"),
    );
    for index in 0..5 {
        let job_id = format!("{index:032x}");
        let job_path = directory.path().join("callback_pending").join(&job_id);
        std::fs::create_dir(&job_path).expect("callback job directory");
        let manifest = QueueManifest {
            client_id: "client-a".to_string(),
            job_id: job_id.clone(),
            media_id: index + 1,
            task: "ocr".to_string(),
            attempt: 1,
            inputs: vec![result_descriptor()],
        };
        std::fs::write(
            job_path.join("manifest.json"),
            serde_json::to_vec(&manifest).expect("manifest JSON"),
        )
        .expect("manifest");
        write_completed_result(&job_path, &manifest);
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
    let deliveries = result_delivery.deliveries.lock().await;
    assert_eq!(deliveries.len(), 5);
    let stale_position = deliveries
        .iter()
        .position(|(_, result, _)| result.job_id == "00000000000000000000000000000000")
        .expect("stale retry delivery");
    assert!(stale_position >= 2);
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
            SchedulerConfig {
                result_delivery_acknowledgement_timeout_seconds: 1,
                result_delivery_retry_delay_seconds: 1,
                result_delivery_max_attempts: 1,
                result_delivery_max_concurrent_deliveries: 1,
                ..SchedulerConfig::default()
            },
            Arc::new(Mutex::new(ServiceManager::new(config))),
            MockResultDeliveryTransport::failing("delivery must not run"),
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
                client_id: "client-a".to_string(),
                job_id: job_id.to_string(),
                media_id: 1,
                task: "ocr".to_string(),
                attempt: 1,
                inputs: vec![descriptor.clone()],
            },
            vec![(descriptor, bytes)],
        )
        .expect("queued job");
    let processing = directory.path().join("processing").join(job_id);
    std::fs::rename(directory.path().join("queuing").join(job_id), &processing)
        .expect("processing state");
    scheduler
        .cancel_jobs(
            "client-a",
            &CancelJobsRequest {
                all: false,
                tasks: vec!["ocr".to_string()],
                job_ids: vec![job_id.to_string()],
            },
        )
        .expect("running cancellation");
    let callback_pending = directory.path().join("callback_pending").join(job_id);
    std::fs::rename(&processing, &callback_pending).expect("completed processing state");
    std::fs::write(callback_pending.join("result-records.bin"), b"invalid")
        .expect("callback records");
    std::fs::write(callback_pending.join("result-manifest.json"), b"{}")
        .expect("callback manifest");

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
    assert!(directory
        .path()
        .join("cancelled")
        .join(format!("client-a-{job_id}"))
        .exists());
}
