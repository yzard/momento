use std::path::Path;
use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use llm_service::config::{Config, SchedulerConfig};
use llm_service::provider::ServiceManager;
use llm_service::routes::{router, AppState};
use llm_service::scheduler::{QueueInputDescriptor, QueueManifest, Scheduler};
use llm_service::transport::ConnectionRegistry;
use momento_common::llm::{
    encode_input_chunk, CancelJobsRequest, ClientControlMessage, JobManifest,
    ServiceControlMessage, SubmissionDeferredReason, QUEUE_CAPACITY_RETRY_AFTER_MS,
    WEBSOCKET_PROTOCOL,
};
use sha2::{Digest, Sha256};
use tempfile::tempdir;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderValue, StatusCode};
use tokio_tungstenite::tungstenite::{Error as WebSocketError, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type ClientSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn start_server(
    queue_dir: &Path,
    scheduler_configuration: SchedulerConfig,
) -> (String, Arc<Scheduler>, JoinHandle<()>) {
    let mut config = Config::default();
    config.server.api_key = "test-key".to_string();
    config.scheduler = scheduler_configuration;
    let config = Arc::new(config);
    let manager = Arc::new(Mutex::new(ServiceManager::new(Arc::clone(&config))));
    let connections = Arc::new(ConnectionRegistry::default());
    let scheduler = Arc::new(
        Scheduler::new(
            queue_dir.to_path_buf(),
            config.scheduler.clone(),
            Arc::clone(&manager),
            connections.clone(),
        )
        .expect("scheduler"),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test listener");
    let address = listener.local_addr().expect("test address");
    let app = router(AppState {
        config,
        manager,
        scheduler: Arc::clone(&scheduler),
        connections,
    });
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("test server");
    });
    (
        format!("ws://{address}/api/v1/llm/connect"),
        scheduler,
        server,
    )
}

fn connection_request(url: &str, api_key: &str, client_id: &str) -> axum::http::Request<()> {
    let mut request = url.into_client_request().expect("WebSocket request");
    request.headers_mut().insert(
        "x-api-key",
        HeaderValue::from_str(api_key).expect("API key"),
    );
    request.headers_mut().insert(
        "x-momento-client-id",
        HeaderValue::from_str(client_id).expect("client ID"),
    );
    request.headers_mut().insert(
        "sec-websocket-protocol",
        HeaderValue::from_static(WEBSOCKET_PROTOCOL),
    );
    request
}

async fn connect(url: &str, client_id: &str) -> ClientSocket {
    let (socket, response) =
        tokio_tungstenite::connect_async(connection_request(url, "test-key", client_id))
            .await
            .expect("WebSocket connection");
    assert_eq!(
        response
            .headers()
            .get("sec-websocket-protocol")
            .expect("selected subprotocol"),
        WEBSOCKET_PROTOCOL
    );
    socket
}

async fn receive_control(socket: &mut ClientSocket) -> ServiceControlMessage {
    let message = socket
        .next()
        .await
        .expect("service response")
        .expect("valid WebSocket response");
    let Message::Text(text) = message else {
        panic!("expected text control response");
    };
    serde_json::from_str(&text).expect("service control response")
}

async fn send_control(socket: &mut ClientSocket, message: ClientControlMessage) {
    socket
        .send(Message::Text(
            serde_json::to_string(&message).expect("client control message"),
        ))
        .await
        .expect("send client control message");
}

fn input_descriptor(bytes: &[u8]) -> QueueInputDescriptor {
    QueueInputDescriptor {
        sequence: 0,
        filename: "input.jpg".to_string(),
        mime_type: "image/jpeg".to_string(),
        byte_size: bytes.len() as u64,
        content_hash: format!("{:x}", Sha256::digest(bytes)),
        input_kind: "image".to_string(),
        frame_timestamp_ms: None,
    }
}

fn queue_job(scheduler: &Scheduler, client_id: &str, job_id: &str) {
    let bytes = b"image".to_vec();
    let descriptor = input_descriptor(&bytes);
    scheduler
        .accept(
            QueueManifest {
                client_id: client_id.to_string(),
                job_id: job_id.to_string(),
                media_id: 1,
                task: "ocr".to_string(),
                attempt: 1,
                inputs: vec![descriptor.clone()],
            },
            vec![(descriptor, bytes)],
        )
        .expect("queued job");
}

#[tokio::test]
async fn websocket_requires_api_key_client_id_and_protocol() {
    let directory = tempdir().expect("queue directory");
    let (url, _scheduler, server) =
        start_server(directory.path(), SchedulerConfig::default()).await;

    let error = tokio_tungstenite::connect_async(connection_request(&url, "wrong-key", "client-a"))
        .await
        .expect_err("invalid API key must fail");
    let WebSocketError::Http(response) = error else {
        panic!("expected HTTP authentication rejection");
    };
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let mut missing_client = connection_request(&url, "test-key", "client-a");
    missing_client.headers_mut().remove("x-momento-client-id");
    let error = tokio_tungstenite::connect_async(missing_client)
        .await
        .expect_err("missing client ID must fail");
    let WebSocketError::Http(response) = error else {
        panic!("expected HTTP client ID rejection");
    };
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let mut missing_protocol = connection_request(&url, "test-key", "client-a");
    missing_protocol
        .headers_mut()
        .remove("sec-websocket-protocol");
    let error = tokio_tungstenite::connect_async(missing_protocol)
        .await
        .expect_err("missing subprotocol must fail");
    let WebSocketError::Http(response) = error else {
        panic!("expected HTTP subprotocol rejection");
    };
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    server.abort();
}

#[tokio::test]
async fn websocket_submission_streams_and_durably_admits_input() {
    let directory = tempdir().expect("queue directory");
    let (url, _scheduler, server) =
        start_server(directory.path(), SchedulerConfig::default()).await;
    let mut socket = connect(&url, "client-a").await;
    let job_id = "0123456789abcdef0123456789abcdef";
    let bytes = b"raw image bytes";
    let descriptor = input_descriptor(bytes);

    send_control(
        &mut socket,
        ClientControlMessage::SubmissionStart {
            manifest: JobManifest {
                job_id: job_id.to_string(),
                media_id: 42,
                task: "ocr".to_string(),
                attempt: 3,
                inputs: vec![descriptor.clone()],
            },
        },
    )
    .await;
    assert_eq!(
        receive_control(&mut socket).await,
        ServiceControlMessage::SubmissionReady {
            job_id: job_id.to_string(),
            attempt: 3,
            required_input_sequences: vec![0],
        }
    );
    socket
        .send(Message::Binary(
            encode_input_chunk(job_id, 0, bytes).expect("input frame"),
        ))
        .await
        .expect("send input frame");
    send_control(
        &mut socket,
        ClientControlMessage::InputFinished {
            job_id: job_id.to_string(),
            sequence: 0,
        },
    )
    .await;
    send_control(
        &mut socket,
        ClientControlMessage::SubmissionFinished {
            job_id: job_id.to_string(),
        },
    )
    .await;
    assert_eq!(
        receive_control(&mut socket).await,
        ServiceControlMessage::SubmissionAcknowledged {
            job_id: job_id.to_string(),
            attempt: 3,
            status: "queued".to_string(),
        }
    );

    let queued = directory.path().join("queuing").join(job_id);
    assert_eq!(
        std::fs::read(queued.join("input-0")).expect("queued input"),
        bytes
    );
    let manifest: QueueManifest = serde_json::from_slice(
        &std::fs::read(queued.join("manifest.json")).expect("queued manifest"),
    )
    .expect("queue manifest");
    assert_eq!(manifest.client_id, "client-a");
    assert_eq!(manifest.inputs, vec![descriptor]);

    socket.close(None).await.expect("close WebSocket");
    server.abort();
}

#[tokio::test]
async fn websocket_submission_reuses_cached_content_without_retransmitting_bytes() {
    let directory = tempdir().expect("queue directory");
    let (url, scheduler, server) = start_server(directory.path(), SchedulerConfig::default()).await;
    let bytes = b"shared raw image bytes";
    let descriptor = input_descriptor(bytes);
    scheduler
        .accept(
            QueueManifest {
                client_id: "client-a".to_string(),
                job_id: "11111111111111111111111111111111".to_string(),
                media_id: 41,
                task: "ocr".to_string(),
                attempt: 1,
                inputs: vec![descriptor.clone()],
            },
            vec![(descriptor.clone(), bytes.to_vec())],
        )
        .expect("seed cached input");

    let mut socket = connect(&url, "client-a").await;
    let job_id = "22222222222222222222222222222222";
    send_control(
        &mut socket,
        ClientControlMessage::SubmissionStart {
            manifest: JobManifest {
                job_id: job_id.to_string(),
                media_id: 42,
                task: "image_tagging".to_string(),
                attempt: 1,
                inputs: vec![descriptor],
            },
        },
    )
    .await;
    assert_eq!(
        receive_control(&mut socket).await,
        ServiceControlMessage::SubmissionReady {
            job_id: job_id.to_string(),
            attempt: 1,
            required_input_sequences: Vec::new(),
        }
    );
    send_control(
        &mut socket,
        ClientControlMessage::SubmissionFinished {
            job_id: job_id.to_string(),
        },
    )
    .await;
    assert_eq!(
        receive_control(&mut socket).await,
        ServiceControlMessage::SubmissionAcknowledged {
            job_id: job_id.to_string(),
            attempt: 1,
            status: "queued".to_string(),
        }
    );
    assert_eq!(
        std::fs::read(
            directory
                .path()
                .join("queuing")
                .join(job_id)
                .join("input-0")
        )
        .expect("linked cached input"),
        bytes
    );

    socket.close(None).await.expect("close WebSocket");
    server.abort();
}

#[tokio::test]
async fn websocket_defers_capacity_without_requesting_or_staging_input_bytes() {
    let directory = tempdir().expect("queue directory");
    let (url, scheduler, server) = start_server(
        directory.path(),
        SchedulerConfig {
            max_queue_bytes: 5,
            working_space_reserve_bytes: 1,
            ..SchedulerConfig::default()
        },
    )
    .await;
    queue_job(&scheduler, "client-a", "00000000000000000000000000000010");
    let mut socket = connect(&url, "client-a").await;
    let job_id = "00000000000000000000000000000011";
    let descriptor = input_descriptor(b"x");

    send_control(
        &mut socket,
        ClientControlMessage::SubmissionStart {
            manifest: JobManifest {
                job_id: job_id.to_string(),
                media_id: 2,
                task: "ocr".to_string(),
                attempt: 1,
                inputs: vec![descriptor],
            },
        },
    )
    .await;

    assert_eq!(
        receive_control(&mut socket).await,
        ServiceControlMessage::SubmissionDeferred {
            job_id: job_id.to_string(),
            attempt: 1,
            reason: SubmissionDeferredReason::QueueCapacity,
            required_bytes: 1,
            available_bytes: 0,
            retry_after_ms: QUEUE_CAPACITY_RETRY_AFTER_MS,
        }
    );
    assert!(!directory.path().join(".tmp").join(job_id).exists());

    socket.close(None).await.expect("close WebSocket");
    server.abort();
}

#[tokio::test]
async fn websocket_permanently_rejects_a_job_larger_than_the_queue_budget() {
    let directory = tempdir().expect("queue directory");
    let (url, _scheduler, server) = start_server(
        directory.path(),
        SchedulerConfig {
            max_queue_bytes: 4,
            working_space_reserve_bytes: 1,
            ..SchedulerConfig::default()
        },
    )
    .await;
    let mut socket = connect(&url, "client-a").await;
    let job_id = "00000000000000000000000000000012";

    send_control(
        &mut socket,
        ClientControlMessage::SubmissionStart {
            manifest: JobManifest {
                job_id: job_id.to_string(),
                media_id: 2,
                task: "ocr".to_string(),
                attempt: 1,
                inputs: vec![input_descriptor(b"12345")],
            },
        },
    )
    .await;

    let ServiceControlMessage::SubmissionRejected {
        job_id: rejected_job_id,
        attempt,
        retryable,
        error,
    } = receive_control(&mut socket).await
    else {
        panic!("oversized job must be rejected");
    };
    assert_eq!(rejected_job_id, job_id);
    assert_eq!(attempt, 1);
    assert!(!retryable);
    assert!(error.contains("max_queue_bytes"));
    assert!(!directory.path().join(".tmp").join(job_id).exists());

    socket.close(None).await.expect("close WebSocket");
    server.abort();
}

#[tokio::test]
async fn websocket_rejects_a_duplicate_connected_client() {
    let directory = tempdir().expect("queue directory");
    let (url, _scheduler, server) =
        start_server(directory.path(), SchedulerConfig::default()).await;
    let mut first = connect(&url, "client-a").await;

    let error = tokio_tungstenite::connect_async(connection_request(&url, "test-key", "client-a"))
        .await
        .expect_err("duplicate client must fail");
    let WebSocketError::Http(response) = error else {
        panic!("expected HTTP duplicate-client rejection");
    };
    assert_eq!(response.status(), StatusCode::CONFLICT);

    first.close(None).await.expect("close WebSocket");
    server.abort();
}

#[tokio::test]
async fn websocket_cancellation_is_scoped_to_the_authenticated_client() {
    let directory = tempdir().expect("queue directory");
    let (url, scheduler, server) = start_server(directory.path(), SchedulerConfig::default()).await;
    let client_job_id = "00000000000000000000000000000001";
    let other_job_id = "00000000000000000000000000000002";
    queue_job(&scheduler, "client-a", client_job_id);
    queue_job(&scheduler, "client-b", other_job_id);
    let mut socket = connect(&url, "client-a").await;

    send_control(
        &mut socket,
        ClientControlMessage::CancelJobs {
            request_id: "cancel-1".to_string(),
            request: CancelJobsRequest {
                all: false,
                tasks: vec!["ocr".to_string()],
                job_ids: Vec::new(),
            },
        },
    )
    .await;
    let ServiceControlMessage::CancellationAcknowledged {
        request_id,
        response,
    } = receive_control(&mut socket).await
    else {
        panic!("expected cancellation acknowledgement");
    };
    assert_eq!(request_id, "cancel-1");
    assert_eq!(response.cancelled_jobs, 1);
    assert_eq!(response.running_jobs, 0);
    assert_eq!(response.missing_jobs, 0);
    assert!(!directory
        .path()
        .join("queuing")
        .join(client_job_id)
        .exists());
    assert!(directory.path().join("queuing").join(other_job_id).exists());
    assert!(directory
        .path()
        .join("cancelled")
        .join(format!("client-a-{client_job_id}"))
        .exists());

    socket.close(None).await.expect("close WebSocket");
    server.abort();
}
