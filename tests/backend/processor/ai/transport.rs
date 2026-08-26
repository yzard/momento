use std::future::Future;

use futures::{SinkExt, StreamExt};
use momento_api::processor::ai::result::process_received_results;
use momento_api::processor::ai::transport::{
    LlmConnection, PreparedSubmissionInput, SubmissionOutcome, TransportHandle,
};
use momento_api::processor::ai::{cancel_active_jobs, deliver_pending_cancellations};
use momento_common::llm::{
    decode_input_chunk, CancelJobsResponse, ClientControlMessage, JobInputDescriptor, JobManifest,
    JobResult, ServiceControlMessage, MAX_BINARY_CHUNK_BYTES, WEBSOCKET_PROTOCOL,
};
use sha2::{Digest, Sha256};
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use crate::test_utils::{create_test_db, create_test_media};

const CLIENT_ID: &str = "momento-test";
const API_KEY: &str = "test-key";

type ServerSocket = WebSocketStream<TcpStream>;

#[tokio::test]
async fn submission_and_cancellation_wakes_are_independent() {
    let handle = TransportHandle::default();
    handle.wake_submissions();
    tokio::time::timeout(
        std::time::Duration::from_millis(50),
        handle.submission_notified(),
    )
    .await
    .expect("submission wake");
    assert!(tokio::time::timeout(
        std::time::Duration::from_millis(10),
        handle.cancellation_notified(),
    )
    .await
    .is_err());

    handle.wake_cancellations();
    tokio::time::timeout(
        std::time::Duration::from_millis(50),
        handle.cancellation_notified(),
    )
    .await
    .expect("cancellation wake");
}

#[allow(clippy::result_large_err)]
async fn start_server<H, F>(accept_protocol: bool, handler: H) -> (String, JoinHandle<()>)
where
    H: FnOnce(ServerSocket) -> F + Send + 'static,
    F: Future<Output = ()> + Send + 'static,
{
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test listener");
    let address = listener.local_addr().expect("test address");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("client connection");
        let socket = tokio_tungstenite::accept_hdr_async(
            stream,
            move |request: &Request, mut response: Response| {
                assert_eq!(request.uri().path(), "/api/v1/llm/connect");
                assert_eq!(
                    request
                        .headers()
                        .get("x-momento-client-id")
                        .expect("client ID"),
                    CLIENT_ID
                );
                assert_eq!(
                    request.headers().get("x-api-key").expect("API key"),
                    API_KEY
                );
                assert_eq!(
                    request
                        .headers()
                        .get("sec-websocket-protocol")
                        .expect("WebSocket protocol"),
                    WEBSOCKET_PROTOCOL
                );
                if accept_protocol {
                    response.headers_mut().insert(
                        "sec-websocket-protocol",
                        HeaderValue::from_static(WEBSOCKET_PROTOCOL),
                    );
                }
                Ok(response)
            },
        )
        .await
        .expect("WebSocket handshake");
        handler(socket).await;
    });
    (address.to_string(), server)
}

async fn receive_client_control(socket: &mut ServerSocket) -> ClientControlMessage {
    loop {
        let message = socket
            .next()
            .await
            .expect("client message")
            .expect("valid client message");
        match message {
            Message::Text(text) => {
                return serde_json::from_str(&text).expect("client control message")
            }
            Message::Ping(bytes) => socket
                .send(Message::Pong(bytes))
                .await
                .expect("heartbeat response"),
            Message::Pong(_) => {}
            other => panic!("expected client control message, received {other:?}"),
        }
    }
}

async fn send_service_control(socket: &mut ServerSocket, message: ServiceControlMessage) {
    socket
        .send(Message::Text(
            serde_json::to_string(&message).expect("service control message"),
        ))
        .await
        .expect("send service control message");
}

fn pending_count(pool: &momento_api::database::DbPool, table: &str) -> i64 {
    pool.get()
        .expect("connection")
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .expect("pending count")
}

#[tokio::test]
async fn connection_sends_authentication_headers_and_requires_selected_subprotocol() {
    let pool = create_test_db();
    let (server_address, server) = start_server(true, |mut socket| async move {
        socket.close(None).await.expect("close socket");
    })
    .await;
    let connection = LlmConnection::connect(&server_address, CLIENT_ID, API_KEY, pool.clone())
        .await
        .expect("WebSocket connection");
    tokio::time::timeout(std::time::Duration::from_secs(1), connection.closed())
        .await
        .expect("connection close");
    server.await.expect("server task");

    let (server_address, server) = start_server(false, |_socket| async {}).await;
    let error = match LlmConnection::connect(&server_address, CLIENT_ID, API_KEY, pool).await {
        Ok(_) => panic!("missing selected subprotocol must fail"),
        Err(error) => error,
    };
    assert!(
        error.to_ascii_lowercase().contains("subprotocol"),
        "{error}"
    );
    server.await.expect("server task");
}

#[tokio::test]
async fn cancellation_outbox_is_retained_until_websocket_acknowledgement() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "transport-cancel.jpg");
    let job_id = "0123456789abcdef0123456789abcdef";
    pool.get()
        .expect("connection")
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status) VALUES (?, ?, 'ocr', 'queued')",
            rusqlite::params![job_id, media_id],
        )
        .expect("queued job");
    assert_eq!(
        cancel_active_jobs(&pool, Some("ocr")).expect("local cancellation"),
        1
    );
    let expected_job_id = job_id.to_string();
    let (server_address, server) = start_server(true, move |mut socket| async move {
        let first_request_id = match receive_client_control(&mut socket).await {
            ClientControlMessage::CancelJobs {
                request_id,
                request,
            } => {
                assert!(!request.all);
                assert_eq!(request.tasks, vec!["ocr"]);
                assert_eq!(request.job_ids, vec![expected_job_id.clone()]);
                request_id
            }
            other => panic!("expected cancellation request, received {other:?}"),
        };
        send_service_control(
            &mut socket,
            ServiceControlMessage::CancellationRejected {
                request_id: first_request_id,
                retryable: true,
                error: "retry cancellation".to_string(),
            },
        )
        .await;
        let second_request_id = match receive_client_control(&mut socket).await {
            ClientControlMessage::CancelJobs {
                request_id,
                request,
            } => {
                assert_eq!(request.job_ids, vec![expected_job_id]);
                request_id
            }
            other => panic!("expected retried cancellation, received {other:?}"),
        };
        send_service_control(
            &mut socket,
            ServiceControlMessage::CancellationAcknowledged {
                request_id: second_request_id,
                response: CancelJobsResponse {
                    requested_jobs: 1,
                    cancelled_jobs: 0,
                    running_jobs: 0,
                    missing_jobs: 1,
                },
            },
        )
        .await;
    })
    .await;
    let connection = LlmConnection::connect(&server_address, CLIENT_ID, API_KEY, pool.clone())
        .await
        .expect("WebSocket connection");

    assert!(deliver_pending_cancellations(&pool, &connection)
        .await
        .is_err());
    assert_eq!(pending_count(&pool, "llm_job_cancellations"), 1);
    assert_eq!(pending_count(&pool, "llm_cancellation_scopes"), 1);

    assert_eq!(
        deliver_pending_cancellations(&pool, &connection)
            .await
            .expect("acknowledged cancellation"),
        1
    );
    assert_eq!(pending_count(&pool, "llm_job_cancellations"), 0);
    assert_eq!(pending_count(&pool, "llm_cancellation_scopes"), 0);
    server.await.expect("server task");
}

#[tokio::test]
async fn websocket_results_are_acknowledged_after_durable_receipt_before_processing() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "transport-result.jpg");
    let rejected_media_id = create_test_media(&pool, "transport-result-rejected.jpg");
    let connection = pool.get().expect("connection");
    connection.execute("INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES ('transport-result-ok', ?, 'ocr', 'submitted', 1)", [media_id]).expect("valid job");
    connection.execute("INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES ('transport-result-bad', ?, 'ocr', 'submitted', 1)", [rejected_media_id]).expect("invalid job");
    drop(connection);
    let (server_address, server) = start_server(true, move |mut socket| async move {
        send_service_control(
            &mut socket,
            ServiceControlMessage::Result {
                result: JobResult {
                    job_id: "transport-result-ok".to_string(),
                    media_id,
                    task: "ocr".to_string(),
                    attempt: 1,
                    status: "completed".to_string(),
                    model_type: Some("ocr".to_string()),
                    model_version: Some("test".to_string()),
                    result: Some(serde_json::json!({ "text": "persisted" })),
                    input_results: None,
                    error: None,
                },
            },
        )
        .await;
        match receive_client_control(&mut socket).await {
            ClientControlMessage::ResultReceived { job_id, attempt } => {
                assert_eq!(job_id, "transport-result-ok");
                assert_eq!(attempt, 1);
            }
            other => panic!("expected result receipt, received {other:?}"),
        }
        send_service_control(
            &mut socket,
            ServiceControlMessage::Result {
                result: JobResult {
                    job_id: "transport-result-bad".to_string(),
                    media_id: rejected_media_id + 10_000,
                    task: "ocr".to_string(),
                    attempt: 1,
                    status: "completed".to_string(),
                    model_type: Some("ocr".to_string()),
                    model_version: Some("test".to_string()),
                    result: Some(serde_json::json!({ "text": "not persisted" })),
                    input_results: None,
                    error: None,
                },
            },
        )
        .await;
        match receive_client_control(&mut socket).await {
            ClientControlMessage::ResultReceived { job_id, attempt } => {
                assert_eq!(job_id, "transport-result-bad");
                assert_eq!(attempt, 1);
            }
            other => panic!("expected result receipt, received {other:?}"),
        }
    })
    .await;
    let _connection = LlmConnection::connect(&server_address, CLIENT_ID, API_KEY, pool.clone())
        .await
        .expect("WebSocket connection");
    server.await.expect("server task");

    let connection = pool.get().expect("connection");
    let received_results: i64 = connection
        .query_row("SELECT COUNT(*) FROM llm_job_results", [], |row| row.get(0))
        .expect("received result count");
    let persisted_before_processing: i64 = connection
        .query_row("SELECT COUNT(*) FROM media_text", [], |row| row.get(0))
        .expect("unprocessed result count");
    assert_eq!(received_results, 1);
    assert_eq!(persisted_before_processing, 0);
    drop(connection);

    assert_eq!(
        process_received_results(&pool, 10).expect("process received result"),
        1
    );
    let connection = pool.get().expect("connection");
    let persisted: String = connection
        .query_row(
            "SELECT string FROM media_text WHERE media_id = ? AND model_type = 'ocr'",
            [media_id],
            |row| row.get(0),
        )
        .expect("persisted result");
    let rejected_status: String = connection
        .query_row(
            "SELECT status FROM llm_jobs WHERE id = 'transport-result-bad'",
            [],
            |row| row.get(0),
        )
        .expect("rejected job status");
    assert_eq!(persisted, "persisted");
    assert_eq!(rejected_status, "failed");
    assert_eq!(pending_count(&pool, "llm_job_results"), 0);
}

#[tokio::test]
async fn submission_streams_prepared_input_in_bounded_binary_chunks() {
    let pool = create_test_db();
    let directory = tempfile::TempDir::new().expect("input directory");
    let input_path = directory.path().join("input.jpg");
    let input_bytes = vec![42_u8; (MAX_BINARY_CHUNK_BYTES * 2) + 17];
    std::fs::write(&input_path, &input_bytes).expect("prepared input");
    let expected_bytes = input_bytes.clone();
    let job_id = "abcdef0123456789abcdef0123456789";
    let expected_job_id = job_id.to_string();
    let (server_address, server) = start_server(true, move |mut socket| async move {
        let manifest = match receive_client_control(&mut socket).await {
            ClientControlMessage::SubmissionStart { manifest } => manifest,
            other => panic!("expected submission start, received {other:?}"),
        };
        assert_eq!(manifest.job_id, expected_job_id);
        assert_eq!(manifest.attempt, 3);
        send_service_control(
            &mut socket,
            ServiceControlMessage::SubmissionReady {
                job_id: manifest.job_id.clone(),
                attempt: manifest.attempt,
            },
        )
        .await;
        let mut received = Vec::new();
        let mut chunks = 0;
        loop {
            let message = socket
                .next()
                .await
                .expect("submission message")
                .expect("valid submission message");
            match message {
                Message::Binary(frame) => {
                    let (frame_job_id, sequence, payload) =
                        decode_input_chunk(&frame).expect("input chunk");
                    assert_eq!(frame_job_id, manifest.job_id);
                    assert_eq!(sequence, 0);
                    assert!(payload.len() <= MAX_BINARY_CHUNK_BYTES);
                    received.extend_from_slice(payload);
                    chunks += 1;
                }
                Message::Text(text) => {
                    let message: ClientControlMessage =
                        serde_json::from_str(&text).expect("submission control message");
                    match message {
                        ClientControlMessage::InputFinished { job_id, sequence } => {
                            assert_eq!(job_id, manifest.job_id);
                            assert_eq!(sequence, 0);
                        }
                        ClientControlMessage::SubmissionFinished { job_id } => {
                            assert_eq!(job_id, manifest.job_id);
                            break;
                        }
                        other => panic!("unexpected submission control message {other:?}"),
                    }
                }
                Message::Ping(bytes) => socket
                    .send(Message::Pong(bytes))
                    .await
                    .expect("heartbeat response"),
                Message::Pong(_) => {}
                other => panic!("unexpected submission message {other:?}"),
            }
        }
        assert_eq!(chunks, 3);
        assert_eq!(received, expected_bytes);
        send_service_control(
            &mut socket,
            ServiceControlMessage::SubmissionAcknowledged {
                job_id: manifest.job_id,
                attempt: manifest.attempt,
                status: "queued".to_string(),
            },
        )
        .await;
    })
    .await;
    let connection = LlmConnection::connect(&server_address, CLIENT_ID, API_KEY, pool)
        .await
        .expect("WebSocket connection");
    let manifest = JobManifest {
        job_id: job_id.to_string(),
        media_id: 42,
        task: "ocr".to_string(),
        attempt: 3,
        inputs: vec![JobInputDescriptor {
            sequence: 0,
            filename: "input.jpg".to_string(),
            mime_type: "image/jpeg".to_string(),
            byte_size: input_bytes.len() as u64,
            content_hash: format!("{:x}", Sha256::digest(&input_bytes)),
            input_kind: "image".to_string(),
            frame_timestamp_ms: None,
        }],
    };

    let outcome = connection
        .submit(
            manifest,
            vec![PreparedSubmissionInput {
                sequence: 0,
                file: tokio::fs::File::open(input_path)
                    .await
                    .expect("submission input"),
            }],
        )
        .await
        .expect("submission outcome");
    match outcome {
        SubmissionOutcome::Acknowledged { status } => assert_eq!(status, "queued"),
        SubmissionOutcome::Rejected { error, .. } => panic!("submission rejected: {error}"),
    }
    server.await.expect("server task");
}
