use std::future::Future;

use futures::{SinkExt, StreamExt};
use momento_api::config::MediaProcessConfig;
use momento_api::database::operations::{CreateLlmResultReceipt, CreateLlmResultReceiptOutcome};
use momento_api::io::file::{NormalizedStoragePath, PathClaimMode, PathClaimScope, StorageRootId};
use momento_api::io::journal::{
    FileEntryAction, FileEntryPlan, FileOperationPlan, FilePathClaimPlan,
    JournalSpaceReservationPlan,
};
use momento_api::processor::ai::result::process_available_results;
use momento_api::processor::ai::transport::{
    LlmConnection, PreparedSubmissionInput, SubmissionOutcome, TransportHandle,
};
use momento_api::processor::ai::{deliver_pending_cancellations, operation::AiFeature};
use momento_common::llm::result_payload::{
    encode_classification, encode_face, encode_image_aesthetics, encode_image_clustering,
    encode_input_started, encode_text, ClassificationPayload, FacePayload, ImageAestheticsPayload,
    ImageClusteringPayload, InputStartedPayload, TextPayload,
};
use momento_common::llm::result_stream::{ResultManifest, ResultStatus, RESULT_RECORDS_ENCODING};
use momento_common::llm::{
    decode_input_chunk, encode_result_chunk, encode_result_record, CancelJobsResponse,
    ClientControlMessage, JobInputDescriptor, JobManifest, ResultRecord, ResultRecordKind,
    ServiceControlMessage, SubmissionDeferredReason, IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS,
    MAX_BINARY_CHUNK_BYTES, WEBSOCKET_PROTOCOL,
};
use sha2::{Digest, Sha256};
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::handshake::server::{
    Callback, ErrorResponse, Request, Response,
};
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use crate::test_utils::{create_test_db, create_test_media};

const CLIENT_ID: &str = "momento-test";
const API_KEY: &str = "test-key";

type ServerSocket = WebSocketStream<TcpStream>;

struct TestHandshake {
    accept_protocol: bool,
}

impl Callback for TestHandshake {
    fn on_request(
        self,
        request: &Request,
        mut response: Response,
    ) -> Result<Response, ErrorResponse> {
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
        if self.accept_protocol {
            response.headers_mut().insert(
                "sec-websocket-protocol",
                HeaderValue::from_static(WEBSOCKET_PROTOCOL),
            );
        }
        Ok(response)
    }
}

#[tokio::test]
async fn submission_and_cancellation_wakes_are_independent() {
    let handle = TransportHandle::default();
    let submission_version = handle.submission_work_version();
    let cancellation_version = handle.cancellation_work_version();
    handle.wake_submissions();
    tokio::time::timeout(
        std::time::Duration::from_millis(50),
        handle.wait_for_submission_work(submission_version),
    )
    .await
    .expect("submission wake");
    assert!(tokio::time::timeout(
        std::time::Duration::from_millis(10),
        handle.wait_for_cancellation_work(cancellation_version),
    )
    .await
    .is_err());

    handle.wake_cancellations();
    tokio::time::timeout(
        std::time::Duration::from_millis(50),
        handle.wait_for_cancellation_work(cancellation_version),
    )
    .await
    .expect("cancellation wake");
}

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
        let socket = tokio_tungstenite::accept_hdr_async(stream, TestHandshake { accept_protocol })
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

fn completed_ocr_result(job_id: &str, media_id: i64, text: &str) -> (ResultManifest, Vec<u8>) {
    let payloads = [
        (
            ResultRecordKind::InputStarted,
            encode_input_started(InputStartedPayload {
                frame_timestamp_ms: None,
            }),
        ),
        (
            ResultRecordKind::OcrText,
            encode_text(&TextPayload {
                text: text.to_string(),
            })
            .expect("OCR text payload"),
        ),
        (ResultRecordKind::InputFinished, Vec::new()),
    ];
    let mut records = Vec::new();
    for (record_sequence, (kind, payload)) in payloads.into_iter().enumerate() {
        records.extend_from_slice(
            &encode_result_record(ResultRecord {
                kind,
                flags: 0,
                record_sequence: record_sequence as u32,
                input_sequence: 0,
                payload: &payload,
            })
            .expect("result record"),
        );
    }
    let manifest = ResultManifest {
        job_id: job_id.to_string(),
        media_id,
        task: "ocr".to_string(),
        attempt: 1,
        status: ResultStatus::Completed,
        model_type: Some("ocr".to_string()),
        model_version: Some("test".to_string()),
        encoding: RESULT_RECORDS_ENCODING.to_string(),
        record_count: 3,
        byte_size: records.len() as u64,
        content_hash: format!("{:x}", Sha256::digest(&records)),
    };
    (manifest, records)
}

fn completed_single_payload_result(
    job_id: &str,
    media_id: i64,
    task: &str,
    kind: ResultRecordKind,
    payload: Vec<u8>,
) -> (ResultManifest, Vec<u8>) {
    let payloads = [
        (
            ResultRecordKind::InputStarted,
            encode_input_started(InputStartedPayload {
                frame_timestamp_ms: None,
            }),
        ),
        (kind, payload),
        (ResultRecordKind::InputFinished, Vec::new()),
    ];
    let mut records = Vec::new();
    for (record_sequence, (kind, payload)) in payloads.into_iter().enumerate() {
        records.extend_from_slice(
            &encode_result_record(ResultRecord {
                kind,
                flags: 0,
                record_sequence: record_sequence as u32,
                input_sequence: 0,
                payload: &payload,
            })
            .expect("typed result record"),
        );
    }
    (
        ResultManifest {
            job_id: job_id.to_string(),
            media_id,
            task: task.to_string(),
            attempt: 1,
            status: ResultStatus::Completed,
            model_type: Some(task.to_string()),
            model_version: Some("test".to_string()),
            encoding: RESULT_RECORDS_ENCODING.to_string(),
            record_count: 3,
            byte_size: records.len() as u64,
            content_hash: format!("{:x}", Sha256::digest(&records)),
        },
        records,
    )
}

fn completed_multi_input_ocr_result(
    job_id: &str,
    media_id: i64,
    input_count: u32,
) -> (ResultManifest, Vec<u8>) {
    let mut records = Vec::new();
    let mut record_sequence = 0_u32;
    for input_sequence in 0..input_count {
        let payloads = [
            (
                ResultRecordKind::InputStarted,
                encode_input_started(InputStartedPayload {
                    frame_timestamp_ms: None,
                }),
            ),
            (
                ResultRecordKind::OcrText,
                encode_text(&TextPayload {
                    text: format!("input-{input_sequence}"),
                })
                .expect("OCR text payload"),
            ),
            (ResultRecordKind::InputFinished, Vec::new()),
        ];
        for (kind, payload) in payloads {
            records.extend_from_slice(
                &encode_result_record(ResultRecord {
                    kind,
                    flags: 0,
                    record_sequence,
                    input_sequence,
                    payload: &payload,
                })
                .expect("multi-input result record"),
            );
            record_sequence += 1;
        }
    }
    let manifest = ResultManifest {
        job_id: job_id.to_string(),
        media_id,
        task: "ocr".to_string(),
        attempt: 1,
        status: ResultStatus::Completed,
        model_type: Some("ocr".to_string()),
        model_version: Some("test".to_string()),
        encoding: RESULT_RECORDS_ENCODING.to_string(),
        record_count: record_sequence,
        byte_size: records.len() as u64,
        content_hash: format!("{:x}", Sha256::digest(&records)),
    };
    (manifest, records)
}

async fn send_streamed_result(
    socket: &mut ServerSocket,
    manifest: ResultManifest,
    records: Vec<u8>,
) {
    send_service_control(
        socket,
        ServiceControlMessage::ResultStart {
            manifest: manifest.clone(),
        },
    )
    .await;
    match receive_client_control(socket).await {
        ClientControlMessage::ResultReady { job_id, attempt } => {
            assert_eq!(job_id, manifest.job_id);
            assert_eq!(attempt, manifest.attempt);
        }
        other => panic!("expected result readiness, received {other:?}"),
    }
    let frame = encode_result_chunk(&manifest.job_id, 0, &records).expect("result frame");
    socket
        .send(Message::Binary(frame))
        .await
        .expect("send result frame");
    match receive_client_control(socket).await {
        ClientControlMessage::ResultChunkReady {
            job_id,
            attempt,
            offset,
        } => {
            assert_eq!(job_id, manifest.job_id);
            assert_eq!(attempt, manifest.attempt);
            assert_eq!(offset, records.len() as u64);
        }
        other => panic!("expected result chunk credit, received {other:?}"),
    }
    send_service_control(
        socket,
        ServiceControlMessage::ResultFinished {
            job_id: manifest.job_id,
            attempt: manifest.attempt,
        },
    )
    .await;
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
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    let connection = LlmConnection::connect(
        &server_address,
        CLIENT_ID,
        API_KEY,
        executors.sqlite.clone(),
        executors.file_io.clone(),
        executors.scheduler.clone(),
    )
    .await
    .expect("WebSocket connection");
    tokio::time::timeout(std::time::Duration::from_secs(1), connection.closed())
        .await
        .expect("connection close");
    server.await.expect("server task");

    let (server_address, server) = start_server(false, |_socket| async {}).await;
    let error = match LlmConnection::connect(
        &server_address,
        CLIENT_ID,
        API_KEY,
        executors.sqlite.clone(),
        executors.file_io.clone(),
        executors.scheduler.clone(),
    )
    .await
    {
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
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    let media_id = create_test_media(&pool, "transport-cancel.jpg");
    let job_id = "0123456789abcdef0123456789abcdef";
    pool.get()
        .expect("connection")
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status) VALUES (?, ?, 'ocr', 'queued')",
            rusqlite::params![job_id, media_id],
        )
        .expect("queued job");
    let cancellation = executors
        .sqlite
        .cancel_ai_feature_request(AiFeature::Ocr)
        .await
        .expect("local cancellation");
    assert_eq!(cancellation.affected_jobs, 1);
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
    let connection = LlmConnection::connect(
        &server_address,
        CLIENT_ID,
        API_KEY,
        executors.sqlite.clone(),
        executors.file_io.clone(),
        executors.scheduler.clone(),
    )
    .await
    .expect("WebSocket connection");

    assert!(
        deliver_pending_cancellations(&executors.sqlite, &connection)
            .await
            .is_err()
    );
    assert_eq!(pending_count(&pool, "llm_job_cancellations"), 1);
    assert_eq!(pending_count(&pool, "llm_cancellation_scopes"), 1);

    assert_eq!(
        deliver_pending_cancellations(&executors.sqlite, &connection)
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
    const VALID_JOB_ID: &str = "aa111111111111111111111111111111";
    const CORRELATION_JOB_ID: &str = "bb222222222222222222222222222222";
    let pool = create_test_db();
    let (executors, data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let media_id = create_test_media(&pool, "transport-result.jpg");
    let rejected_media_id = create_test_media(&pool, "transport-result-rejected.jpg");
    let connection = pool.get().expect("connection");
    connection.execute("INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES (?, ?, 'ocr', 'submitted', 1)", rusqlite::params![VALID_JOB_ID, media_id]).expect("valid job");
    connection.execute("INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES (?, ?, 'ocr', 'submitted', 1)", rusqlite::params![CORRELATION_JOB_ID, rejected_media_id]).expect("invalid job");
    for job_id in [VALID_JOB_ID, CORRELATION_JOB_ID] {
        connection.execute("INSERT INTO llm_job_inputs (job_id, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, 0, 'image', 'originals', 'test.jpg', 'test.jpg', 'image/jpeg', 1, ?)", rusqlite::params![job_id, "0".repeat(64)]).expect("job input");
    }
    drop(connection);
    let (server_address, server) = start_server(true, move |mut socket| async move {
        let (manifest, records) = completed_ocr_result(VALID_JOB_ID, media_id, "persisted");
        send_streamed_result(&mut socket, manifest, records).await;
        match receive_client_control(&mut socket).await {
            ClientControlMessage::ResultReceived { job_id, attempt } => {
                assert_eq!(job_id, VALID_JOB_ID);
                assert_eq!(attempt, 1);
            }
            other => panic!("expected result receipt, received {other:?}"),
        }
        let (manifest, _records) = completed_ocr_result(
            CORRELATION_JOB_ID,
            rejected_media_id + 10_000,
            "not persisted",
        );
        send_service_control(&mut socket, ServiceControlMessage::ResultStart { manifest }).await;
        match receive_client_control(&mut socket).await {
            ClientControlMessage::ResultReceiptRejected {
                job_id, attempt, ..
            } => {
                assert_eq!(job_id, CORRELATION_JOB_ID);
                assert_eq!(attempt, 1);
            }
            other => panic!("expected result rejection, received {other:?}"),
        }
    })
    .await;
    let _connection = LlmConnection::connect(
        &server_address,
        CLIENT_ID,
        API_KEY,
        executors.sqlite.clone(),
        executors.file_io.clone(),
        executors.scheduler.clone(),
    )
    .await
    .expect("WebSocket connection");
    server.await.expect("server task");

    let connection = pool.get().expect("connection");
    let persisted_before_processing: i64 = connection
        .query_row("SELECT COUNT(*) FROM media_text", [], |row| row.get(0))
        .expect("unprocessed result count");
    let (
        receipt_state,
        group_state,
        inbox_path,
        sqlite_reservation_state,
        sqlite_reserved_bytes,
        receipt_byte_size,
    ): (
        String,
        String,
        String,
        String,
        u64,
        u64,
    ) = connection
        .query_row(
            "SELECT r.state, g.state, r.inbox_path, s.state, s.reserved_peak_additional_bytes, r.byte_size FROM llm_result_receipts AS r JOIN file_operation_groups AS g ON g.id = r.journal_group_id JOIN data_dir_space_reservations AS s ON s.id = r.sqlite_reservation_id WHERE r.job_id = ?",
            [VALID_JOB_ID],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
        )
        .expect("durable result receipt");
    assert_eq!(persisted_before_processing, 0);
    assert_eq!(receipt_state, "received");
    assert_eq!(group_state, "completed");
    assert_eq!(sqlite_reservation_state, "active");
    let expected_sqlite_reservation =
        momento_api::database::result_footprint::SqliteFootprintRegistry::new(4096)
            .expect("SQLite footprint registry")
            .result("ocr", 3, receipt_byte_size)
            .expect("OCR SQLite footprint")
            .construction_max_growth_bytes;
    assert_eq!(sqlite_reserved_bytes, expected_sqlite_reservation);
    let inbox_file = data_directory.join("journal").join(inbox_path);
    assert!(inbox_file.is_file());
    let interrupted_claim = "00000000-0000-0000-0000-000000000099";
    connection
        .execute(
            "UPDATE llm_result_receipts SET state = 'processing', claim_token = ? WHERE job_id = ?",
            rusqlite::params![interrupted_claim, VALID_JOB_ID],
        )
        .expect("simulate interrupted result claim");
    drop(connection);

    let recovery = executors
        .sqlite
        .recover_llm_result_state_durable()
        .await
        .expect("recover interrupted result claim");
    assert_eq!(recovery.claims_recovered, 1);
    assert_eq!(recovery.replayable_receipts_retired, 0);
    assert_eq!(recovery.orphaned_reservations_retired, 0);
    assert!(!recovery.has_more);
    assert!(!executors
        .sqlite
        .release_llm_result_claim_durable(VALID_JOB_ID.to_string(), interrupted_claim.to_string(),)
        .await
        .expect("stale result claim release"));
    let recovered_claim: (String, Option<String>, i64, i64) = pool
        .get()
        .expect("recovery connection")
        .query_row(
            "SELECT state, claim_token, next_record_sequence, next_byte_offset FROM llm_result_receipts WHERE job_id = ?",
            [VALID_JOB_ID],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("recovered result claim state");
    assert_eq!(
        recovered_claim,
        ("received".to_string(), None, 0, 0),
        "startup recovery must retain the durable staging cursor while clearing ownership"
    );
    let active_claim = "00000000-0000-0000-0000-000000000100";
    pool.get()
        .expect("active claim connection")
        .execute(
            "UPDATE llm_result_receipts SET state = 'processing', claim_token = ? WHERE job_id = ?",
            rusqlite::params![active_claim, VALID_JOB_ID],
        )
        .expect("install active result claim");
    assert_eq!(
        process_available_results(&executors, MediaProcessConfig::default())
            .await
            .expect("exclude an already claimed result"),
        0
    );
    assert!(executors
        .sqlite
        .release_llm_result_claim_durable(VALID_JOB_ID.to_string(), active_claim.to_string())
        .await
        .expect("release exact result claim"));

    assert_eq!(
        process_available_results(&executors, MediaProcessConfig::default())
            .await
            .expect("process received result"),
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
            "SELECT status FROM llm_jobs WHERE id = ?",
            [CORRELATION_JOB_ID],
            |row| row.get(0),
        )
        .expect("rejected job status");
    assert_eq!(persisted, "persisted");
    assert_eq!(rejected_status, "failed");
    let cleanup: (String, String, Option<String>) = connection
        .query_row(
            "SELECT r.state, g.state, g.product_target FROM llm_result_receipts AS r JOIN file_operation_groups AS g ON g.id = r.journal_group_id WHERE r.job_id = ?",
            [VALID_JOB_ID],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("queued result cleanup");
    assert_eq!(
        cleanup,
        (
            "file_cleanup_pending".to_string(),
            "cleanup_pending".to_string(),
            None
        )
    );
    let staging_progress: (i64, i64, i64) = connection
        .query_row(
            "SELECT r.next_record_sequence, r.next_byte_offset, (SELECT COUNT(*) FROM llm_result_staging WHERE job_id = r.job_id) FROM llm_result_receipts AS r WHERE r.job_id = ?",
            [VALID_JOB_ID],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("staged result cleanup progress");
    assert_eq!(staging_progress.0, 3);
    assert!(staging_progress.1 > 0);
    assert_eq!(staging_progress.2, 0);
    let sqlite_capacity_after_persistence: (String, u64, u64, u64) = connection
        .query_row(
            "SELECT owner_kind, reserved_peak_additional_bytes, newly_allocated_blocks, version FROM data_dir_space_reservations WHERE id = (SELECT sqlite_reservation_id FROM llm_result_receipts WHERE job_id = ?)",
            [VALID_JOB_ID],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("result SQLite cleanup reservation");
    let cleanup_reservation =
        momento_api::database::result_footprint::SqliteFootprintRegistry::new(4096)
            .expect("SQLite footprint registry")
            .result_cleanup_recovery_max_growth_bytes;
    assert_eq!(sqlite_capacity_after_persistence.0, "llm_result_cleanup");
    assert_eq!(
        sqlite_capacity_after_persistence.1 - sqlite_capacity_after_persistence.2,
        cleanup_reservation,
        "terminal result persistence must return all construction capacity except one cleanup batch"
    );
    assert!(sqlite_capacity_after_persistence.3 > 1);
    drop(connection);
    momento_api::io::recovery::recover_generic_file_operations(&executors)
        .await
        .expect("clean durable result inbox");
    process_available_results(&executors, MediaProcessConfig::default())
        .await
        .expect("finalize durable result cleanup");
    assert!(!inbox_file.exists());
    let connection = pool.get().expect("connection after cleanup");
    let terminal: (String, String, String, String) = connection
        .query_row(
            "SELECT r.state, g.state, journal_space.state, sqlite_space.state FROM llm_result_receipts AS r JOIN file_operation_groups AS g ON g.id = r.journal_group_id JOIN data_dir_space_reservations AS journal_space ON journal_space.journal_group_id = g.id JOIN data_dir_space_reservations AS sqlite_space ON sqlite_space.id = r.sqlite_reservation_id WHERE r.job_id = ?",
            [VALID_JOB_ID],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("cleaned result receipt");
    assert_eq!(
        terminal,
        (
            "cleaned".to_string(),
            "cleaned".to_string(),
            "released".to_string(),
            "released".to_string(),
        )
    );
}

#[tokio::test]
async fn interrupted_result_receive_restarts_with_a_new_journal_group() {
    const JOB_ID: &str = "ab111111111111111111111111111111";
    let pool = create_test_db();
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    let media_id = create_test_media(&pool, "transport-result-retry.jpg");
    let connection = pool.get().expect("connection");
    connection
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES (?, ?, 'ocr', 'submitted', 1)",
            rusqlite::params![JOB_ID, media_id],
        )
        .expect("submitted job");
    connection
        .execute(
            "INSERT INTO llm_job_inputs (job_id, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, 0, 'image', 'originals', 'test.jpg', 'test.jpg', 'image/jpeg', 1, ?)",
            rusqlite::params![JOB_ID, "0".repeat(64)],
        )
        .expect("job input");
    drop(connection);

    let (first_address, first_server) = start_server(true, move |mut socket| async move {
        let (manifest, _) = completed_ocr_result(JOB_ID, media_id, "retry");
        send_service_control(&mut socket, ServiceControlMessage::ResultStart { manifest }).await;
        assert!(matches!(
            receive_client_control(&mut socket).await,
            ClientControlMessage::ResultReady { .. }
        ));
        socket.close(None).await.expect("interrupt result receive");
    })
    .await;
    let first_connection = LlmConnection::connect(
        &first_address,
        CLIENT_ID,
        API_KEY,
        executors.sqlite.clone(),
        executors.file_io.clone(),
        executors.scheduler.clone(),
    )
    .await
    .expect("first WebSocket connection");
    first_server.await.expect("first server task");
    first_connection.closed().await;
    momento_api::io::recovery::recover_generic_file_operations(&executors)
        .await
        .expect("roll back interrupted result receive");

    let interrupted_state: (i64, i64) = pool
        .get()
        .expect("interrupted state connection")
        .query_row(
            "SELECT (SELECT COUNT(*) FROM llm_result_receipts WHERE job_id = ?), (SELECT COUNT(*) FROM file_operation_groups WHERE owner_kind = 'llm_result' AND owner_id = ? AND state = 'rolled_back')",
            rusqlite::params![JOB_ID, JOB_ID],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("interrupted result state");
    assert_eq!(interrupted_state, (0, 1));
    let connection = pool.get().expect("legacy receipt connection");
    connection
        .execute(
            "UPDATE file_operation_groups SET state = 'cleaned', completion_outcome = 'discarded' WHERE owner_kind = 'llm_result' AND owner_id = ?",
            [JOB_ID],
        )
        .expect("legacy discarded result group");
    connection
        .execute(
            "INSERT INTO data_dir_space_reservations (id, class, owner_kind, owner_id, filesystem_id, reserved_peak_additional_bytes, state) VALUES ('legacy-result-sqlite', 'sqlite', 'llm_result', ?, 'test', 4096, 'released')",
            [JOB_ID],
        )
        .expect("legacy result reservation");
    connection
        .execute(
            "INSERT INTO llm_result_receipts (job_id, attempt, job_version, media_id, task, result_status, model_type, model_version, encoding, record_count, byte_size, content_hash, journal_group_id, sqlite_reservation_id, inbox_path, receive_token, state, result_product_version) VALUES (?, 1, 1, ?, 'ocr', 'completed', 'ocr', 'test', 'momento-result-records-v1', 1, 24, ?, (SELECT id FROM file_operation_groups WHERE owner_kind = 'llm_result' AND owner_id = ?), 'legacy-result-sqlite', 'legacy.records', '00000000-0000-0000-0000-000000000112', 'discarded', 1)",
            rusqlite::params![JOB_ID, media_id, "0".repeat(64), JOB_ID],
        )
        .expect("legacy discarded result receipt");
    drop(connection);
    let startup_recovery = executors
        .sqlite
        .recover_llm_result_state_durable()
        .await
        .expect("retire legacy discarded result receipt");
    assert_eq!(startup_recovery.claims_recovered, 0);
    assert_eq!(startup_recovery.replayable_receipts_retired, 1);
    assert_eq!(startup_recovery.orphaned_reservations_retired, 0);
    assert!(!startup_recovery.has_more);

    pool.get()
        .expect("legacy orphan connection")
        .execute(
            "INSERT INTO data_dir_space_reservations (id, class, owner_kind, owner_id, filesystem_id, reserved_peak_additional_bytes, state) VALUES ('legacy-orphaned-result-sqlite', 'sqlite', 'llm_result', ?, 'test', 4096, 'active')",
            [JOB_ID],
        )
        .expect("orphaned active result reservation");
    let orphan_recovery = executors
        .sqlite
        .recover_llm_result_state_durable()
        .await
        .expect("retire legacy orphaned result reservation");
    assert_eq!(orphan_recovery.replayable_receipts_retired, 0);
    assert_eq!(orphan_recovery.orphaned_reservations_retired, 1);

    let (second_address, second_server) = start_server(true, move |mut socket| async move {
        let (manifest, records) = completed_ocr_result(JOB_ID, media_id, "retry");
        send_streamed_result(&mut socket, manifest, records).await;
        assert!(matches!(
            receive_client_control(&mut socket).await,
            ClientControlMessage::ResultReceived { .. }
        ));
    })
    .await;
    let _second_connection = LlmConnection::connect(
        &second_address,
        CLIENT_ID,
        API_KEY,
        executors.sqlite.clone(),
        executors.file_io.clone(),
        executors.scheduler.clone(),
    )
    .await
    .expect("second WebSocket connection");
    second_server.await.expect("second server task");

    let resumed_state: (String, i64, i64) = pool
        .get()
        .expect("resumed state connection")
        .query_row(
            "SELECT state, (SELECT COUNT(*) FROM file_operation_groups WHERE owner_kind = 'llm_result' AND owner_id = ?), (SELECT COUNT(DISTINCT id) FROM file_operation_groups WHERE owner_kind = 'llm_result' AND owner_id = ?) FROM llm_result_receipts WHERE job_id = ?",
            rusqlite::params![JOB_ID, JOB_ID, JOB_ID],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("resumed result state");
    assert_eq!(resumed_state, ("received".to_string(), 2, 2));
}

#[tokio::test]
async fn streamed_typed_results_persist_without_the_legacy_json_inbox() {
    const CLUSTER_JOB_ID: &str = "c1111111111111111111111111111111";
    const AESTHETICS_JOB_ID: &str = "c2222222222222222222222222222222";
    const SCREENSHOT_JOB_ID: &str = "c3333333333333333333333333333333";
    let pool = create_test_db();
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    let jobs = [
        (
            CLUSTER_JOB_ID,
            create_test_media(&pool, "typed-clustering.jpg"),
            "image_clustering",
        ),
        (
            AESTHETICS_JOB_ID,
            create_test_media(&pool, "typed-aesthetics.jpg"),
            "image_aesthetics",
        ),
        (
            SCREENSHOT_JOB_ID,
            create_test_media(&pool, "typed-screenshot.jpg"),
            "screenshot_detection",
        ),
    ];
    let connection = pool.get().expect("connection");
    let deduplicate_run_id = connection
        .query_row(
            "INSERT INTO media_similarity_runs (trigger, status) VALUES ('manual', 'running') RETURNING id",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("typed clustering run");
    for (job_id, media_id, task) in jobs {
        connection
            .execute(
                "INSERT INTO llm_jobs (id, media_id, deduplicate_run_id, task, status, attempts) VALUES (?, ?, ?, ?, 'submitted', 1)",
                rusqlite::params![
                    job_id,
                    media_id,
                    (task == "image_clustering").then_some(deduplicate_run_id),
                    task
                ],
            )
            .expect("typed result job");
        connection
            .execute(
                "INSERT INTO llm_job_inputs (job_id, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, 0, 'image', 'originals', 'typed.jpg', 'typed.jpg', 'image/jpeg', 1, ?)",
                rusqlite::params![job_id, "0".repeat(64)],
            )
            .expect("typed result input");
    }
    drop(connection);

    let (server_address, server) = start_server(true, move |mut socket| async move {
        let embedding = vec![
            1.0_f32 / (IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS as f32).sqrt();
            IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS
        ];
        let results = [
            completed_single_payload_result(
                CLUSTER_JOB_ID,
                jobs[0].1,
                "image_clustering",
                ResultRecordKind::ImageClustering,
                encode_image_clustering(&ImageClusteringPayload {
                    embedding,
                    perceptual_hash: 0x1234_5678_90ab_cdef,
                    quality_score: 0.75,
                })
                .expect("clustering payload"),
            ),
            completed_single_payload_result(
                AESTHETICS_JOB_ID,
                jobs[1].1,
                "image_aesthetics",
                ResultRecordKind::ImageAesthetics,
                encode_image_aesthetics(ImageAestheticsPayload {
                    aesthetic: 0.9,
                    scenic: 0.8,
                    simplicity: 0.7,
                    landscape: 0.6,
                    technical_quality: 0.5,
                })
                .expect("aesthetics payload"),
            ),
            completed_single_payload_result(
                SCREENSHOT_JOB_ID,
                jobs[2].1,
                "screenshot_detection",
                ResultRecordKind::ScreenshotDetection,
                encode_classification(ClassificationPayload {
                    detected: true,
                    confidence: 0.95,
                })
                .expect("classification payload"),
            ),
        ];
        for (manifest, records) in results {
            let job_id = manifest.job_id.clone();
            send_streamed_result(&mut socket, manifest, records).await;
            match receive_client_control(&mut socket).await {
                ClientControlMessage::ResultReceived {
                    job_id: received_job_id,
                    attempt: 1,
                } => assert_eq!(received_job_id, job_id),
                other => panic!("expected durable typed result receipt, received {other:?}"),
            }
        }
    })
    .await;
    let _connection = LlmConnection::connect(
        &server_address,
        CLIENT_ID,
        API_KEY,
        executors.sqlite.clone(),
        executors.file_io.clone(),
        executors.scheduler.clone(),
    )
    .await
    .expect("WebSocket connection");
    server.await.expect("server task");

    assert_eq!(
        process_available_results(&executors, MediaProcessConfig::default())
            .await
            .expect("persist typed streamed results"),
        3
    );
    let connection = pool.get().expect("persistence connection");
    let state: (i64, i64, i64) = connection
        .query_row(
            "SELECT (SELECT COUNT(*) FROM media_similarity_index WHERE media_id = ?), (SELECT COUNT(*) FROM media_aesthetics WHERE media_id = ?), (SELECT COUNT(*) FROM media_screenshot_classifications WHERE media_id = ?)",
            rusqlite::params![jobs[0].1, jobs[1].1, jobs[2].1],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("typed result state");
    assert_eq!(state, (1, 1, 1));
}

#[tokio::test]
async fn streamed_face_result_switches_one_versioned_artifact_group() {
    const JOB_ID: &str = "c4444444444444444444444444444444";
    let pool = create_test_db();
    let (executors, data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let media_id = create_test_media(&pool, "typed-face.jpg");
    let mut image_bytes = Vec::new();
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        320,
        240,
        image::Rgb([80, 120, 160]),
    ))
    .write_to(
        &mut std::io::Cursor::new(&mut image_bytes),
        image::ImageFormat::Jpeg,
    )
    .expect("face test JPEG");
    let input_relative = format!("typed-face-{media_id}.jpg");
    std::fs::write(
        data_directory.join("originals").join(&input_relative),
        &image_bytes,
    )
    .expect("face test original");
    let input_hash = format!("{:x}", Sha256::digest(&image_bytes));
    let connection = pool.get().expect("connection");
    let run_id = connection
        .query_row(
            "INSERT INTO face_grouping_runs (status) VALUES ('running') RETURNING id",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("face grouping run");
    connection
        .execute(
            "INSERT INTO llm_jobs (id, media_id, face_grouping_run_id, task, status, attempts) VALUES (?, ?, ?, 'face_detection', 'submitted', 1)",
            rusqlite::params![JOB_ID, media_id, run_id],
        )
        .expect("face job");
    connection
        .execute(
            "INSERT INTO llm_job_inputs (job_id, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, 0, 'image', 'originals', ?, 'typed-face.jpg', 'image/jpeg', ?, ?)",
            rusqlite::params![JOB_ID, input_relative, image_bytes.len(), input_hash],
        )
        .expect("face input");
    drop(connection);

    let (server_address, server) = start_server(true, move |mut socket| async move {
        let embedding = vec![1.0_f32 / 512.0_f32.sqrt(); 512];
        let (manifest, records) = completed_single_payload_result(
            JOB_ID,
            media_id,
            "face_detection",
            ResultRecordKind::Face,
            encode_face(&FacePayload {
                index: 0,
                x: 0.25,
                y: 0.2,
                width: 0.4,
                height: 0.5,
                eye_center_x: 0.45,
                eye_center_y: 0.38,
                confidence: 0.95,
                face_size_score: 0.8,
                frontality_score: 0.9,
                visibility_score: 0.9,
                feature_clarity_score: 0.85,
                embedding,
            })
            .expect("face payload"),
        );
        send_streamed_result(&mut socket, manifest, records).await;
        assert!(matches!(
            receive_client_control(&mut socket).await,
            ClientControlMessage::ResultReceived { attempt: 1, .. }
        ));
    })
    .await;
    let _connection = LlmConnection::connect(
        &server_address,
        CLIENT_ID,
        API_KEY,
        executors.sqlite.clone(),
        executors.file_io.clone(),
        executors.scheduler.clone(),
    )
    .await
    .expect("WebSocket connection");
    server.await.expect("server task");
    assert_eq!(
        process_available_results(&executors, MediaProcessConfig::default())
            .await
            .expect("persist streamed face result"),
        1
    );
    let connection = pool.get().expect("persistence connection");
    let (crop_path, group_state, product_target, product_version): (
        String,
        String,
        Option<String>,
        i64,
    ) = connection
        .query_row(
            "SELECT f.crop_path, g.state, g.product_target, g.product_version FROM media_faces AS f JOIN file_operation_groups AS g ON g.owner_id = ? AND g.kind = 'llm_result_artifacts' WHERE f.media_id = ?",
            rusqlite::params![JOB_ID, media_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("versioned face product");
    assert!(crop_path.contains(&format!("/{JOB_ID}/v1/")));
    assert_eq!(group_state, "cleanup_pending");
    assert_eq!(product_target, None);
    assert_eq!(product_version, 1);
    let published_crop = data_directory.join("previews").join(crop_path);
    assert!(published_crop.is_file());
    drop(connection);

    momento_api::io::recovery::recover_generic_file_operations(&executors)
        .await
        .expect("clean versioned face artifact group");
    let connection = pool.get().expect("connection after artifact cleanup");
    let cleaned_state = connection
        .query_row(
            "SELECT state FROM file_operation_groups WHERE owner_id = ? AND kind = 'llm_result_artifacts'",
            [JOB_ID],
            |row| row.get::<_, String>(0),
        )
        .expect("cleaned face artifact group");
    assert_eq!(cleaned_state, "cleaned");
    assert!(published_crop.is_file());
}

#[tokio::test]
async fn result_worker_stages_and_cleans_more_than_one_record_page() {
    const JOB_ID: &str = "ee555555555555555555555555555555";
    const INPUT_COUNT: u32 = 100;
    let pool = create_test_db();
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    let media_id = create_test_media(&pool, "transport-paged-result.jpg");
    let connection = pool.get().expect("connection");
    connection
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES (?, ?, 'ocr', 'submitted', 1)",
            rusqlite::params![JOB_ID, media_id],
        )
        .expect("paged result job");
    for sequence in 0..INPUT_COUNT {
        connection
            .execute(
                "INSERT INTO llm_job_inputs (job_id, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, ?, 'image', 'originals', ?, ?, 'image/jpeg', 1, ?)",
                rusqlite::params![
                    JOB_ID,
                    sequence,
                    format!("input-{sequence}.jpg"),
                    format!("input-{sequence}.jpg"),
                    "0".repeat(64),
                ],
            )
            .expect("paged result input");
    }
    drop(connection);

    let (server_address, server) = start_server(true, move |mut socket| async move {
        let (manifest, records) = completed_multi_input_ocr_result(JOB_ID, media_id, INPUT_COUNT);
        send_streamed_result(&mut socket, manifest, records).await;
        assert!(matches!(
            receive_client_control(&mut socket).await,
            ClientControlMessage::ResultReceived { .. }
        ));
    })
    .await;
    let _connection = LlmConnection::connect(
        &server_address,
        CLIENT_ID,
        API_KEY,
        executors.sqlite.clone(),
        executors.file_io.clone(),
        executors.scheduler.clone(),
    )
    .await
    .expect("WebSocket connection");
    server.await.expect("server task");

    assert_eq!(
        process_available_results(&executors, MediaProcessConfig::default())
            .await
            .expect("process paged result"),
        1
    );
    let connection = pool.get().expect("connection");
    let state: (String, i64, i64, i64) = connection
        .query_row(
            "SELECT j.status, r.next_record_sequence, (SELECT COUNT(*) FROM llm_result_staging WHERE job_id = r.job_id), (SELECT COUNT(*) FROM media_text_inputs WHERE media_id = j.media_id AND model_type = 'ocr') FROM llm_jobs AS j JOIN llm_result_receipts AS r ON r.job_id = j.id WHERE j.id = ?",
            [JOB_ID],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("paged result state");
    assert_eq!(state, ("completed".to_string(), 300, 0, 100));
}

#[tokio::test]
async fn invalid_streamed_result_is_failed_before_permanent_rejection() {
    const JOB_ID: &str = "cc333333333333333333333333333333";
    let pool = create_test_db();
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    let media_id = create_test_media(&pool, "transport-invalid-result.jpg");
    let connection = pool.get().expect("connection");
    connection
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES (?, ?, 'ocr', 'submitted', 1)",
            rusqlite::params![JOB_ID, media_id],
        )
        .expect("active job");
    connection
        .execute(
            "INSERT INTO llm_job_inputs (job_id, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, 0, 'image', 'originals', 'test.jpg', 'test.jpg', 'image/jpeg', 1, ?)",
            rusqlite::params![JOB_ID, "0".repeat(64)],
        )
        .expect("job input");
    drop(connection);

    let (server_address, server) = start_server(true, move |mut socket| async move {
        let (manifest, mut records) = completed_ocr_result(JOB_ID, media_id, "invalid");
        send_service_control(
            &mut socket,
            ServiceControlMessage::ResultStart {
                manifest: manifest.clone(),
            },
        )
        .await;
        assert!(matches!(
            receive_client_control(&mut socket).await,
            ClientControlMessage::ResultReady { .. }
        ));
        let last = records.last_mut().expect("encoded result byte");
        *last ^= 0xff;
        socket
            .send(Message::Binary(
                encode_result_chunk(&manifest.job_id, 0, &records).expect("result frame"),
            ))
            .await
            .expect("send invalid result frame");
        match receive_client_control(&mut socket).await {
            ClientControlMessage::ResultReceiptRejected {
                job_id, attempt, ..
            } => {
                assert_eq!(job_id, JOB_ID);
                assert_eq!(attempt, 1);
            }
            other => panic!("expected permanent result rejection, received {other:?}"),
        }
    })
    .await;
    let _connection = LlmConnection::connect(
        &server_address,
        CLIENT_ID,
        API_KEY,
        executors.sqlite.clone(),
        executors.file_io.clone(),
        executors.scheduler.clone(),
    )
    .await
    .expect("WebSocket connection");
    server.await.expect("server task");

    let connection = pool.get().expect("connection");
    let (status, error): (String, String) = connection
        .query_row(
            "SELECT status, last_error FROM llm_jobs WHERE id = ?",
            [JOB_ID],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("rejected job state");
    assert_eq!(status, "failed");
    assert!(error.contains("CRC32C"), "{error}");
    let cleanup: (String, String, Option<String>) = connection
        .query_row(
            "SELECT r.state, g.state, g.product_target FROM llm_result_receipts AS r JOIN file_operation_groups AS g ON g.id = r.journal_group_id WHERE r.job_id = ?",
            [JOB_ID],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("invalid result rollback");
    assert_eq!(cleanup.0, "discarded");
    assert_eq!(cleanup.1, "rollback_pending");
    assert_eq!(cleanup.2, None);
}

#[tokio::test]
async fn cancellation_version_wins_over_a_late_invalid_result_frame() {
    const JOB_ID: &str = "dd444444444444444444444444444444";
    let pool = create_test_db();
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    let media_id = create_test_media(&pool, "transport-cancelled-result.jpg");
    let connection = pool.get().expect("connection");
    connection
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES (?, ?, 'ocr', 'submitted', 1)",
            rusqlite::params![JOB_ID, media_id],
        )
        .expect("active job");
    connection
        .execute(
            "INSERT INTO llm_job_inputs (job_id, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, 0, 'image', 'originals', 'test.jpg', 'test.jpg', 'image/jpeg', 1, ?)",
            rusqlite::params![JOB_ID, "0".repeat(64)],
        )
        .expect("job input");
    drop(connection);

    let (ready_sender, ready_receiver) = tokio::sync::oneshot::channel();
    let (continue_sender, continue_receiver) = tokio::sync::oneshot::channel();
    let (server_address, server) = start_server(true, move |mut socket| async move {
        let (manifest, mut records) = completed_ocr_result(JOB_ID, media_id, "cancelled");
        send_service_control(
            &mut socket,
            ServiceControlMessage::ResultStart {
                manifest: manifest.clone(),
            },
        )
        .await;
        assert!(matches!(
            receive_client_control(&mut socket).await,
            ClientControlMessage::ResultReady { .. }
        ));
        ready_sender.send(()).expect("signal ready");
        continue_receiver
            .await
            .expect("continue after cancellation");

        *records.last_mut().expect("encoded result byte") ^= 0xff;
        socket
            .send(Message::Binary(
                encode_result_chunk(&manifest.job_id, 0, &records).expect("result frame"),
            ))
            .await
            .expect("send invalid result frame");
        match receive_client_control(&mut socket).await {
            ClientControlMessage::ResultReceived { job_id, attempt } => {
                assert_eq!(job_id, JOB_ID);
                assert_eq!(attempt, 1);
            }
            other => panic!("expected stale result acknowledgement, received {other:?}"),
        }
    })
    .await;
    let _connection = LlmConnection::connect(
        &server_address,
        CLIENT_ID,
        API_KEY,
        executors.sqlite.clone(),
        executors.file_io.clone(),
        executors.scheduler.clone(),
    )
    .await
    .expect("WebSocket connection");
    ready_receiver.await.expect("result readiness");
    let cancellation = executors
        .sqlite
        .cancel_ai_feature_request(AiFeature::Ocr)
        .await
        .expect("cancel active result job");
    assert_eq!(cancellation.affected_jobs, 1);
    continue_sender.send(()).expect("continue result stream");
    server.await.expect("server task");

    let (status, state_version): (String, i64) = pool
        .get()
        .expect("connection")
        .query_row(
            "SELECT status, state_version FROM llm_jobs WHERE id = ?",
            [JOB_ID],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("cancelled job state");
    assert_eq!(status, "cancelled");
    assert_eq!(state_version, 2);
}

#[tokio::test]
async fn submission_streams_prepared_input_in_bounded_binary_chunks() {
    let pool = create_test_db();
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    let relative_path = "transport-tests/input.jpg";
    let input_path = crate::test_utils::test_data_directory(&pool)
        .join("imports")
        .join(relative_path);
    std::fs::create_dir_all(input_path.parent().expect("input parent")).expect("input directory");
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
                required_input_sequences: vec![0],
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
    let connection = LlmConnection::connect(
        &server_address,
        CLIENT_ID,
        API_KEY,
        executors.sqlite.clone(),
        executors.file_io.clone(),
        executors.scheduler.clone(),
    )
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
                file_io: executors.file_io.clone(),
                session: executors
                    .file_io
                    .open_storage_read_session_durable(
                        StorageRootId::Imports,
                        NormalizedStoragePath::parse(relative_path).expect("input path"),
                    )
                    .await
                    .expect("submission input")
                    .0,
            }],
        )
        .await
        .expect("submission outcome");
    match outcome {
        SubmissionOutcome::Acknowledged { status } => assert_eq!(status, "queued"),
        SubmissionOutcome::Deferred { .. } => panic!("submission was unexpectedly deferred"),
        SubmissionOutcome::Rejected { error, .. } => panic!("submission rejected: {error}"),
    }
    server.await.expect("server task");
}

#[tokio::test]
async fn queue_capacity_deferral_returns_without_streaming_input_bytes() {
    let pool = create_test_db();
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    let relative_path = "transport-tests/deferred-input.jpg";
    let input_path = crate::test_utils::test_data_directory(&pool)
        .join("imports")
        .join(relative_path);
    std::fs::create_dir_all(input_path.parent().expect("input parent")).expect("input directory");
    let input_bytes = b"must not be transmitted";
    std::fs::write(&input_path, input_bytes).expect("prepared input");
    let job_id = "1234567890abcdef1234567890abcdef";
    let (server_address, server) = start_server(true, move |mut socket| async move {
        let manifest = match receive_client_control(&mut socket).await {
            ClientControlMessage::SubmissionStart { manifest } => manifest,
            other => panic!("expected submission start, received {other:?}"),
        };
        send_service_control(
            &mut socket,
            ServiceControlMessage::SubmissionDeferred {
                job_id: manifest.job_id,
                attempt: manifest.attempt,
                reason: SubmissionDeferredReason::QueueCapacity,
                required_bytes: input_bytes.len() as u64,
                available_bytes: 0,
                retry_after_ms: 30_000,
            },
        )
        .await;
    })
    .await;
    let connection = LlmConnection::connect(
        &server_address,
        CLIENT_ID,
        API_KEY,
        executors.sqlite.clone(),
        executors.file_io.clone(),
        executors.scheduler.clone(),
    )
    .await
    .expect("WebSocket connection");
    let manifest = JobManifest {
        job_id: job_id.to_string(),
        media_id: 42,
        task: "ocr".to_string(),
        attempt: 3,
        inputs: vec![JobInputDescriptor {
            sequence: 0,
            filename: "deferred-input.jpg".to_string(),
            mime_type: "image/jpeg".to_string(),
            byte_size: input_bytes.len() as u64,
            content_hash: format!("{:x}", Sha256::digest(input_bytes)),
            input_kind: "image".to_string(),
            frame_timestamp_ms: None,
        }],
    };

    let outcome = connection
        .submit(
            manifest,
            vec![PreparedSubmissionInput {
                sequence: 0,
                file_io: executors.file_io.clone(),
                session: executors
                    .file_io
                    .open_storage_read_session_durable(
                        StorageRootId::Imports,
                        NormalizedStoragePath::parse(relative_path).expect("input path"),
                    )
                    .await
                    .expect("submission input")
                    .0,
            }],
        )
        .await
        .expect("submission outcome");
    assert!(matches!(
        outcome,
        SubmissionOutcome::Deferred {
            retry_after,
            required_bytes,
            available_bytes: 0,
        } if retry_after == std::time::Duration::from_secs(30)
            && required_bytes == input_bytes.len() as u64
    ));
    server.await.expect("server task");
}

#[tokio::test]
async fn submission_skips_inputs_already_cached_by_the_llm_service() {
    let pool = create_test_db();
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    let relative_path = "transport-tests/cached-input.jpg";
    let input_path = crate::test_utils::test_data_directory(&pool)
        .join("imports")
        .join(relative_path);
    std::fs::create_dir_all(input_path.parent().expect("input parent")).expect("input directory");
    let input_bytes = b"already cached";
    std::fs::write(&input_path, input_bytes).expect("prepared input");
    let job_id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let (server_address, server) = start_server(true, move |mut socket| async move {
        let manifest = match receive_client_control(&mut socket).await {
            ClientControlMessage::SubmissionStart { manifest } => manifest,
            other => panic!("expected submission start, received {other:?}"),
        };
        send_service_control(
            &mut socket,
            ServiceControlMessage::SubmissionReady {
                job_id: manifest.job_id.clone(),
                attempt: manifest.attempt,
                required_input_sequences: Vec::new(),
            },
        )
        .await;
        let message = receive_client_control(&mut socket).await;
        assert_eq!(
            message,
            ClientControlMessage::SubmissionFinished {
                job_id: manifest.job_id.clone(),
            }
        );
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
    let connection = LlmConnection::connect(
        &server_address,
        CLIENT_ID,
        API_KEY,
        executors.sqlite.clone(),
        executors.file_io.clone(),
        executors.scheduler.clone(),
    )
    .await
    .expect("WebSocket connection");
    let outcome = connection
        .submit(
            JobManifest {
                job_id: job_id.to_string(),
                media_id: 42,
                task: "image_tagging".to_string(),
                attempt: 1,
                inputs: vec![JobInputDescriptor {
                    sequence: 0,
                    filename: "cached-input.jpg".to_string(),
                    mime_type: "image/jpeg".to_string(),
                    byte_size: input_bytes.len() as u64,
                    content_hash: format!("{:x}", Sha256::digest(input_bytes)),
                    input_kind: "image".to_string(),
                    frame_timestamp_ms: None,
                }],
            },
            vec![PreparedSubmissionInput {
                sequence: 0,
                file_io: executors.file_io.clone(),
                session: executors
                    .file_io
                    .open_storage_read_session_durable(
                        StorageRootId::Imports,
                        NormalizedStoragePath::parse(relative_path).expect("input path"),
                    )
                    .await
                    .expect("submission input")
                    .0,
            }],
        )
        .await
        .expect("submission outcome");
    assert!(matches!(
        outcome,
        SubmissionOutcome::Acknowledged { ref status } if status == "queued"
    ));
    server.await.expect("server task");
}

#[tokio::test]
async fn concurrent_submissions_complete_without_blocking_each_other() {
    let pool = create_test_db();
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    let data_directory = crate::test_utils::test_data_directory(&pool).join("imports/round-robin");
    std::fs::create_dir_all(&data_directory).expect("round-robin directory");
    let bytes = vec![7_u8; MAX_BINARY_CHUNK_BYTES + 1];
    std::fs::write(data_directory.join("first.jpg"), &bytes).expect("first input");
    std::fs::write(data_directory.join("second.jpg"), &bytes).expect("second input");

    let (server_address, server) = start_server(true, move |mut socket| async move {
        let mut manifests = Vec::new();
        for _ in 0..2 {
            let manifest = match receive_client_control(&mut socket).await {
                ClientControlMessage::SubmissionStart { manifest } => manifest,
                other => panic!("expected submission start, received {other:?}"),
            };
            send_service_control(
                &mut socket,
                ServiceControlMessage::SubmissionReady {
                    job_id: manifest.job_id.clone(),
                    attempt: manifest.attempt,
                    required_input_sequences: vec![0],
                },
            )
            .await;
            manifests.push(manifest);
        }
        let mut binary_order = Vec::new();
        let mut finished = 0;
        while finished < 2 {
            let message = socket
                .next()
                .await
                .expect("round-robin message")
                .expect("valid round-robin message");
            match message {
                Message::Binary(frame) => {
                    let (job_id, _, payload) = decode_input_chunk(&frame).expect("input chunk");
                    assert!(payload.len() <= MAX_BINARY_CHUNK_BYTES);
                    binary_order.push(job_id.to_string());
                }
                Message::Text(text) => {
                    let message: ClientControlMessage =
                        serde_json::from_str(&text).expect("control message");
                    if let ClientControlMessage::SubmissionFinished { job_id } = message {
                        let manifest = manifests
                            .iter()
                            .find(|manifest| manifest.job_id == job_id)
                            .expect("finished manifest");
                        send_service_control(
                            &mut socket,
                            ServiceControlMessage::SubmissionAcknowledged {
                                job_id,
                                attempt: manifest.attempt,
                                status: "queued".to_string(),
                            },
                        )
                        .await;
                        finished += 1;
                    }
                }
                Message::Ping(bytes) => socket
                    .send(Message::Pong(bytes))
                    .await
                    .expect("heartbeat response"),
                Message::Pong(_) => {}
                other => panic!("unexpected round-robin message {other:?}"),
            }
        }
        assert_eq!(binary_order.len(), 4);
        for manifest in manifests {
            assert_eq!(
                binary_order
                    .iter()
                    .filter(|job_id| *job_id == &manifest.job_id)
                    .count(),
                2,
                "each admitted submission must stream every chunk"
            );
        }
    })
    .await;
    let connection = LlmConnection::connect(
        &server_address,
        CLIENT_ID,
        API_KEY,
        executors.sqlite.clone(),
        executors.file_io.clone(),
        executors.scheduler.clone(),
    )
    .await
    .expect("WebSocket connection");

    let prepare = |job_id: &str, filename: &str| {
        let relative_path = format!("round-robin/{filename}");
        let manifest = JobManifest {
            job_id: job_id.to_string(),
            media_id: 42,
            task: "ocr".to_string(),
            attempt: 1,
            inputs: vec![JobInputDescriptor {
                sequence: 0,
                filename: filename.to_string(),
                mime_type: "image/jpeg".to_string(),
                byte_size: bytes.len() as u64,
                content_hash: format!("{:x}", Sha256::digest(&bytes)),
                input_kind: "image".to_string(),
                frame_timestamp_ms: None,
            }],
        };
        (manifest, relative_path)
    };
    let (first_manifest, first_path) = prepare("1111111111111111", "first.jpg");
    let (second_manifest, second_path) = prepare("2222222222222222", "second.jpg");
    let first_input = PreparedSubmissionInput {
        sequence: 0,
        file_io: executors.file_io.clone(),
        session: executors
            .file_io
            .open_storage_read_session_durable(
                StorageRootId::Imports,
                NormalizedStoragePath::parse(&first_path).expect("first path"),
            )
            .await
            .expect("first session")
            .0,
    };
    let second_input = PreparedSubmissionInput {
        sequence: 0,
        file_io: executors.file_io.clone(),
        session: executors
            .file_io
            .open_storage_read_session_durable(
                StorageRootId::Imports,
                NormalizedStoragePath::parse(&second_path).expect("second path"),
            )
            .await
            .expect("second session")
            .0,
    };
    let (first, second) = tokio::join!(
        connection.submit(first_manifest, vec![first_input]),
        connection.submit(second_manifest, vec![second_input])
    );
    assert!(matches!(first, Ok(SubmissionOutcome::Acknowledged { .. })));
    assert!(matches!(second, Ok(SubmissionOutcome::Acknowledged { .. })));
    server.await.expect("server task");
}

#[tokio::test]
async fn journal_cleanup_may_finish_before_result_staging_cleanup() {
    const JOB_ID: &str = "ee111111111111111111111111111111";
    const GROUP_ID: &str = "result-cleanup-order-group";
    let pool = create_test_db();
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    let media_id = create_test_media(&pool, "cleanup-order.jpg");
    let connection = pool.get().expect("cleanup-order connection");
    connection
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES (?, ?, 'ocr', 'submitted', 1)",
            rusqlite::params![JOB_ID, media_id],
        )
        .expect("active cleanup-order job");
    drop(connection);
    let temporary_path =
        NormalizedStoragePath::parse("cleanup-order.tmp").expect("temporary result path");
    let destination_path =
        NormalizedStoragePath::parse("cleanup-order.records").expect("result path");
    let journal_token = executors
        .file_io
        .reserve_journal_space(GROUP_ID.to_string(), 4096)
        .expect("cleanup-order Journal reservation")
        .into_result()
        .expect("cleanup-order Journal capacity");
    let journal_plan = FileOperationPlan {
        group_id: GROUP_ID.to_string(),
        kind: "llm_result_receive".to_string(),
        owner_kind: "llm_result".to_string(),
        owner_id: JOB_ID.to_string(),
        claim_token: None,
        product_target: Some("llm_result_inbox".to_string()),
        product_version: Some(1),
        entries: vec![FileEntryPlan {
            action: FileEntryAction::Publish,
            storage_root: StorageRootId::Journal,
            source_path: None,
            temporary_path: Some(temporary_path.clone()),
            destination_path: Some(destination_path.clone()),
            tombstone_path: None,
            expected_size: Some(24),
            expected_sha256: Some([0; 32]),
            expected_version: None,
        }],
        claims: vec![
            FilePathClaimPlan {
                storage_root: StorageRootId::Journal,
                path: temporary_path,
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Exact,
                role: "temporary".to_string(),
                expected_version: None,
            },
            FilePathClaimPlan {
                storage_root: StorageRootId::Journal,
                path: destination_path,
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Exact,
                role: "destination".to_string(),
                expected_version: None,
            },
        ],
        space_reservation: Some(
            JournalSpaceReservationPlan::new(journal_token)
                .expect("cleanup-order Journal reservation plan"),
        ),
    };
    assert_eq!(
        executors
            .sqlite
            .create_llm_result_receipt_durable(CreateLlmResultReceipt {
                job_id: JOB_ID.to_string(),
                attempt: 1,
                expected_job_version: 1,
                media_id,
                task: "ocr".to_string(),
                result_status: "completed".to_string(),
                model_type: Some("ocr".to_string()),
                model_version: Some("test".to_string()),
                encoding: "momento-result-records-v1".to_string(),
                record_count: 1,
                byte_size: 24,
                content_hash: "0".repeat(64),
                journal_group_id: GROUP_ID.to_string(),
                inbox_path: "cleanup-order.records".to_string(),
                receive_token: "00000000-0000-0000-0000-000000000111".to_string(),
                journal_plan,
            })
            .await
            .expect("create cleanup-order receipt"),
        CreateLlmResultReceiptOutcome::Created
    );
    let connection = pool.get().expect("advance cleanup-order state");
    connection
        .execute(
            "UPDATE file_operation_groups SET state = 'cleanup_pending', product_target = NULL, completion_outcome = 'published', version = 2 WHERE id = ?",
            [GROUP_ID],
        )
        .expect("advance cleanup-order group");
    connection
        .execute(
            "UPDATE file_operation_entries SET state = 'committed' WHERE group_id = ?",
            [GROUP_ID],
        )
        .expect("advance cleanup-order entry");
    connection
        .execute(
            "UPDATE llm_result_receipts SET state = 'cleanup_pending' WHERE job_id = ?",
            [JOB_ID],
        )
        .expect("advance cleanup-order receipt");
    connection
        .execute(
            "UPDATE llm_jobs SET status = 'completed' WHERE id = ?",
            [JOB_ID],
        )
        .expect("complete cleanup-order job");
    connection
        .execute(
            "INSERT INTO llm_result_staging (job_id, attempt, record_sequence, kind, byte_offset, encoded_size, normalized_payload) VALUES (?, 1, 0, 'ocr_text', 0, 24, X'')",
            [JOB_ID],
        )
        .expect("cleanup-order staging");
    drop(connection);

    let checkpoint = executors
        .sqlite
        .record_file_entry_cleaned_durable(GROUP_ID.to_string(), 2, 0)
        .await
        .expect("finish Journal cleanup first")
        .expect("owned Journal cleanup");
    assert!(checkpoint.phase_complete);
    let before_staging_cleanup: (String, String, String) = pool
        .get()
        .expect("cleanup-order state")
        .query_row(
            "SELECT r.state, g.state, s.state FROM llm_result_receipts AS r JOIN file_operation_groups AS g ON g.id = r.journal_group_id JOIN data_dir_space_reservations AS s ON s.id = r.sqlite_reservation_id WHERE r.job_id = ?",
            [JOB_ID],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("cleanup-order intermediate state");
    assert_eq!(
        before_staging_cleanup,
        (
            "cleanup_pending".to_string(),
            "cleaned".to_string(),
            "active".to_string(),
        )
    );

    let cleanup = executors
        .sqlite
        .cleanup_llm_result_staging_page_durable(JOB_ID.to_string(), 256)
        .await
        .expect("cleanup staging after Journal");
    assert!(cleanup.complete);
    assert!(executors
        .sqlite
        .finalize_llm_result_cleanup_durable(JOB_ID.to_string())
        .await
        .expect("finalize cleanup reservation"));
    let terminal: (String, String, i64) = pool
        .get()
        .expect("cleanup-order terminal state")
        .query_row(
            "SELECT r.state, s.state, (SELECT COUNT(*) FROM llm_result_staging WHERE job_id = r.job_id) FROM llm_result_receipts AS r JOIN data_dir_space_reservations AS s ON s.id = r.sqlite_reservation_id WHERE r.job_id = ?",
            [JOB_ID],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("cleanup-order terminal receipt");
    assert_eq!(terminal, ("cleaned".to_string(), "released".to_string(), 0));
}
