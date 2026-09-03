use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    http::{header::SEC_WEBSOCKET_PROTOCOL, HeaderMap},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use futures::{SinkExt, StreamExt};
use momento_common::llm::{
    decode_input_chunk, is_valid_client_id, ClientControlMessage, JobInputDescriptor, JobManifest,
    ServiceControlMessage, SubmissionDeferredReason, MAX_LLM_SERVICE_WS_MESSAGE_BYTES,
    MAX_WS_WRITE_BUFFER_BYTES, QUEUE_CAPACITY_RETRY_AFTER_MS, WEBSOCKET_PROTOCOL,
};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::config::Config;
use crate::error::ServiceError;
use crate::provider::ServiceManager;
use crate::scheduler::{QueueAdmission, QueueManifest, QueueStaging, Scheduler};
use crate::transport::{ConnectionIdentity, ResultDeliveryOutcome, SharedConnectionRegistry};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub manager: Arc<tokio::sync::Mutex<ServiceManager>>,
    pub scheduler: Arc<Scheduler>,
    pub connections: SharedConnectionRegistry,
}

struct InputUpload {
    descriptor: JobInputDescriptor,
    file: tokio::fs::File,
    byte_count: u64,
    hasher: Sha256,
}

struct SubmissionUpload {
    manifest: JobManifest,
    staging: Box<QueueStaging>,
    inputs: HashMap<u32, InputUpload>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/llm/connect", get(connect))
        .route("/health", get(health))
        .route("/ready", get(ready))
        .with_state(state)
}

async fn connect(
    State(state): State<AppState>,
    headers: HeaderMap,
    websocket: WebSocketUpgrade,
) -> Result<Response, ServiceError> {
    validate_api_key(&headers, &state.config.server.api_key)?;
    let client_id = headers
        .get("x-momento-client-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !is_valid_client_id(client_id) {
        return Err(ServiceError::BadRequest(
            "x-momento-client-id must contain 1 to 128 letters, numbers, hyphens, or underscores"
                .to_string(),
        ));
    }
    let supports_protocol = headers
        .get(SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|protocols| {
            protocols
                .split(',')
                .any(|protocol| protocol.trim() == WEBSOCKET_PROTOCOL)
        });
    if !supports_protocol {
        return Err(ServiceError::BadRequest(format!(
            "WebSocket subprotocol {WEBSOCKET_PROTOCOL} is required"
        )));
    }
    let registration = state.connections.register(client_id).await?;
    state.scheduler.wake_result_delivery();
    let client_id = client_id.to_string();
    let failed_connections = Arc::clone(&state.connections);
    let failed_client_id = client_id.clone();
    let generation = registration.generation;
    Ok(websocket
        .write_buffer_size(0)
        .max_write_buffer_size(MAX_WS_WRITE_BUFFER_BYTES)
        .max_message_size(MAX_LLM_SERVICE_WS_MESSAGE_BYTES)
        .max_frame_size(MAX_LLM_SERVICE_WS_MESSAGE_BYTES)
        .protocols([WEBSOCKET_PROTOCOL])
        .on_failed_upgrade(move |_| {
            tokio::spawn(async move {
                failed_connections
                    .unregister(&failed_client_id, generation)
                    .await;
            });
        })
        .on_upgrade(move |socket| {
            run_connection(state, client_id, generation, registration.outbound, socket)
        })
        .into_response())
}

async fn run_connection(
    state: AppState,
    client_id: String,
    generation: u64,
    mut outbound: tokio::sync::mpsc::Receiver<crate::transport::OutboundMessage>,
    socket: WebSocket,
) {
    tracing::info!(client_id, "Momento LLM WebSocket connected");
    let (mut socket_writer, mut socket_reader) = socket.split();
    let mut writer = tokio::spawn(async move {
        while let Some(outbound_message) = outbound.recv().await {
            let sent = socket_writer.send(outbound_message.message).await;
            if let Some(response) = outbound_message.sent {
                let _ = response.send(sent.as_ref().map(|_| ()).map_err(ToString::to_string));
            }
            if sent.is_err() {
                break;
            }
        }
    });
    let mut submissions = HashMap::new();
    loop {
        let message = tokio::select! {
            _ = &mut writer => break,
            message = socket_reader.next() => message,
        };
        let Some(message) = message else {
            break;
        };
        let outcome = match message {
            Ok(Message::Text(text)) => {
                handle_control(&state, &client_id, generation, &mut submissions, &text).await
            }
            Ok(Message::Binary(frame)) => handle_input_chunk(&mut submissions, &frame).await,
            Ok(Message::Ping(bytes)) => state
                .connections
                .send_raw(&client_id, generation, Message::Pong(bytes))
                .await
                .map_err(ServiceError::Internal),
            Ok(Message::Pong(_)) => Ok(()),
            Ok(Message::Close(_)) => break,
            Err(error) => Err(ServiceError::Internal(error.to_string())),
        };
        if let Err(error) = outcome {
            tracing::warn!(client_id, error = %error, "LLM WebSocket protocol failure");
            break;
        }
    }
    drop(submissions);
    state.connections.unregister(&client_id, generation).await;
    if !writer.is_finished() {
        writer.abort();
    }
    tracing::info!(client_id, "Momento LLM WebSocket disconnected");
}

async fn handle_control(
    state: &AppState,
    client_id: &str,
    generation: u64,
    submissions: &mut HashMap<String, SubmissionUpload>,
    text: &str,
) -> Result<(), ServiceError> {
    let message = serde_json::from_str::<ClientControlMessage>(text)
        .map_err(|error| ServiceError::BadRequest(error.to_string()))?;
    match message {
        ClientControlMessage::SubmissionStart { manifest } => {
            start_submission(state, client_id, generation, submissions, manifest).await
        }
        ClientControlMessage::InputFinished { job_id, sequence } => {
            let attempt = submissions
                .get(&job_id)
                .map(|submission| submission.manifest.attempt)
                .ok_or_else(|| {
                    ServiceError::Conflict("input has no active submission".to_string())
                })?;
            match finish_input(submissions, &job_id, sequence).await {
                Ok(()) => Ok(()),
                Err(error) => {
                    submissions.remove(&job_id);
                    send_submission_rejection(
                        state,
                        client_id,
                        generation,
                        &job_id,
                        attempt,
                        is_retryable(&error),
                        error.to_string(),
                    )
                    .await
                }
            }
        }
        ClientControlMessage::SubmissionFinished { job_id } => {
            finish_submission(state, client_id, generation, submissions, &job_id).await
        }
        ClientControlMessage::CancelJobs {
            request_id,
            request,
        } => {
            let message = match state.scheduler.cancel_jobs(client_id, &request) {
                Ok(response) => ServiceControlMessage::CancellationAcknowledged {
                    request_id,
                    response,
                },
                Err(error) => ServiceControlMessage::CancellationRejected {
                    request_id,
                    retryable: is_retryable(&error),
                    error: error.to_string(),
                },
            };
            state
                .connections
                .send(client_id, generation, message)
                .await
                .map_err(ServiceError::Internal)
        }
        ClientControlMessage::ResultReady { job_id, attempt } => {
            state
                .connections
                .result_ready(
                    ConnectionIdentity {
                        client_id,
                        generation,
                    },
                    &job_id,
                    attempt,
                )
                .await
        }
        ClientControlMessage::ResultChunkReady {
            job_id,
            attempt,
            offset,
        } => {
            state
                .connections
                .result_chunk_ready(
                    ConnectionIdentity {
                        client_id,
                        generation,
                    },
                    &job_id,
                    attempt,
                    offset,
                )
                .await
        }
        ClientControlMessage::ResultReceived { job_id, attempt } => {
            state
                .connections
                .complete_result_delivery(
                    ConnectionIdentity {
                        client_id,
                        generation,
                    },
                    &job_id,
                    attempt,
                    ResultDeliveryOutcome::Received,
                )
                .await
        }
        ClientControlMessage::ResultReceiptDeferred {
            job_id,
            attempt,
            retry_after_ms,
        } => {
            state
                .connections
                .complete_result_delivery(
                    ConnectionIdentity {
                        client_id,
                        generation,
                    },
                    &job_id,
                    attempt,
                    ResultDeliveryOutcome::Deferred { retry_after_ms },
                )
                .await
        }
        ClientControlMessage::ResultReceiptRejected {
            job_id,
            attempt,
            error,
        } => {
            state
                .connections
                .complete_result_delivery(
                    ConnectionIdentity {
                        client_id,
                        generation,
                    },
                    &job_id,
                    attempt,
                    ResultDeliveryOutcome::Rejected { error },
                )
                .await
        }
    }
}

async fn start_submission(
    state: &AppState,
    client_id: &str,
    generation: u64,
    submissions: &mut HashMap<String, SubmissionUpload>,
    manifest: JobManifest,
) -> Result<(), ServiceError> {
    if submissions.contains_key(&manifest.job_id) {
        return send_submission_rejection(
            state,
            client_id,
            generation,
            &manifest.job_id,
            manifest.attempt,
            false,
            "submission is already being uploaded".to_string(),
        )
        .await;
    }
    let queue_manifest = QueueManifest {
        client_id: client_id.to_string(),
        job_id: manifest.job_id.clone(),
        media_id: manifest.media_id,
        task: manifest.task.clone(),
        attempt: manifest.attempt,
        inputs: manifest.inputs.clone(),
    };
    match state.scheduler.begin_admission(queue_manifest) {
        Ok(QueueAdmission::Staging(staging)) => {
            let job_id = manifest.job_id.clone();
            let attempt = manifest.attempt;
            let required_input_sequences = staging.required_sequences();
            submissions.insert(
                job_id.clone(),
                SubmissionUpload {
                    manifest,
                    staging,
                    inputs: HashMap::new(),
                },
            );
            state
                .connections
                .send(
                    client_id,
                    generation,
                    ServiceControlMessage::SubmissionReady {
                        job_id,
                        attempt,
                        required_input_sequences,
                    },
                )
                .await
                .map_err(ServiceError::Internal)
        }
        Ok(QueueAdmission::Duplicate) => {
            send_submission_acknowledgement(state, client_id, generation, &manifest, "queued").await
        }
        Ok(QueueAdmission::Cancelled) => {
            send_submission_acknowledgement(state, client_id, generation, &manifest, "cancelled")
                .await
        }
        Ok(QueueAdmission::Deferred(status)) => {
            tracing::warn!(
                job_id = manifest.job_id,
                required_bytes = status.required_bytes,
                available_bytes = status.available_bytes,
                used_bytes = status.used_bytes,
                reserved_bytes = status.reserved_bytes,
                max_queue_bytes = status.max_queue_bytes,
                filesystem_available_bytes = status.filesystem_available_bytes,
                working_space_reserve_bytes = status.working_space_reserve_bytes,
                "LLM submission deferred because queue capacity is unavailable"
            );
            state
                .connections
                .send(
                    client_id,
                    generation,
                    ServiceControlMessage::SubmissionDeferred {
                        job_id: manifest.job_id,
                        attempt: manifest.attempt,
                        reason: SubmissionDeferredReason::QueueCapacity,
                        required_bytes: status.required_bytes,
                        available_bytes: status.available_bytes,
                        retry_after_ms: QUEUE_CAPACITY_RETRY_AFTER_MS,
                    },
                )
                .await
                .map_err(ServiceError::Internal)
        }
        Ok(QueueAdmission::JobTooLarge(status)) => {
            send_submission_rejection(
                state,
                client_id,
                generation,
                &manifest.job_id,
                manifest.attempt,
                false,
                format!(
                    "job requires {} uncached input bytes but llm-service max_queue_bytes is {}",
                    status.required_bytes, status.max_queue_bytes
                ),
            )
            .await
        }
        Err(error) => {
            send_submission_rejection(
                state,
                client_id,
                generation,
                &manifest.job_id,
                manifest.attempt,
                is_retryable(&error),
                error.to_string(),
            )
            .await
        }
    }
}

async fn handle_input_chunk(
    submissions: &mut HashMap<String, SubmissionUpload>,
    frame: &[u8],
) -> Result<(), ServiceError> {
    let (job_id, sequence, bytes) = decode_input_chunk(frame).map_err(ServiceError::BadRequest)?;
    let submission = submissions
        .get_mut(job_id)
        .ok_or_else(|| ServiceError::Conflict("input has no active submission".to_string()))?;
    if !submission.inputs.contains_key(&sequence) {
        let descriptor = submission
            .manifest
            .inputs
            .iter()
            .find(|descriptor| descriptor.sequence == sequence)
            .cloned()
            .ok_or_else(|| {
                ServiceError::BadRequest("input has no manifest descriptor".to_string())
            })?;
        let input_path = submission.staging.input_path(&descriptor)?;
        let file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(input_path)
            .await
            .map_err(|error| ServiceError::Internal(error.to_string()))?;
        submission.inputs.insert(
            sequence,
            InputUpload {
                descriptor,
                file,
                byte_count: 0,
                hasher: Sha256::new(),
            },
        );
    }
    let input = submission
        .inputs
        .get_mut(&sequence)
        .expect("input upload was inserted");
    let chunk_size = u64::try_from(bytes.len())
        .map_err(|_| ServiceError::BadRequest("input chunk is too large".to_string()))?;
    input.byte_count = input
        .byte_count
        .checked_add(chunk_size)
        .ok_or_else(|| ServiceError::BadRequest("input exceeds descriptor size".to_string()))?;
    if input.byte_count > input.descriptor.byte_size {
        return Err(ServiceError::BadRequest(
            "input exceeds descriptor size".to_string(),
        ));
    }
    input
        .file
        .write_all(bytes)
        .await
        .map_err(|error| ServiceError::Internal(error.to_string()))?;
    input.hasher.update(bytes);
    Ok(())
}

async fn finish_input(
    submissions: &mut HashMap<String, SubmissionUpload>,
    job_id: &str,
    sequence: u32,
) -> Result<(), ServiceError> {
    let submission = submissions
        .get_mut(job_id)
        .ok_or_else(|| ServiceError::Conflict("input has no active submission".to_string()))?;
    let input = submission.inputs.remove(&sequence).ok_or_else(|| {
        ServiceError::BadRequest("input finished before bytes were supplied".to_string())
    })?;
    input
        .file
        .sync_all()
        .await
        .map_err(|error| ServiceError::Internal(error.to_string()))?;
    submission
        .staging
        .verify_input(&input.descriptor, input.byte_count, input.hasher.finalize())
}

async fn finish_submission(
    state: &AppState,
    client_id: &str,
    generation: u64,
    submissions: &mut HashMap<String, SubmissionUpload>,
    job_id: &str,
) -> Result<(), ServiceError> {
    let submission = submissions
        .remove(job_id)
        .ok_or_else(|| ServiceError::Conflict("submission is not active".to_string()))?;
    if !submission.inputs.is_empty() {
        return send_submission_rejection(
            state,
            client_id,
            generation,
            &submission.manifest.job_id,
            submission.manifest.attempt,
            false,
            "submission has unfinished inputs".to_string(),
        )
        .await;
    }
    match submission.staging.commit() {
        Ok(true) => {
            send_submission_acknowledgement(
                state,
                client_id,
                generation,
                &submission.manifest,
                "queued",
            )
            .await
        }
        Ok(false) => {
            send_submission_acknowledgement(
                state,
                client_id,
                generation,
                &submission.manifest,
                "cancelled",
            )
            .await
        }
        Err(error) => {
            send_submission_rejection(
                state,
                client_id,
                generation,
                &submission.manifest.job_id,
                submission.manifest.attempt,
                is_retryable(&error),
                error.to_string(),
            )
            .await
        }
    }
}

async fn send_submission_acknowledgement(
    state: &AppState,
    client_id: &str,
    generation: u64,
    manifest: &JobManifest,
    status: &str,
) -> Result<(), ServiceError> {
    state
        .connections
        .send(
            client_id,
            generation,
            ServiceControlMessage::SubmissionAcknowledged {
                job_id: manifest.job_id.clone(),
                attempt: manifest.attempt,
                status: status.to_string(),
            },
        )
        .await
        .map_err(ServiceError::Internal)
}

async fn send_submission_rejection(
    state: &AppState,
    client_id: &str,
    generation: u64,
    job_id: &str,
    attempt: u32,
    retryable: bool,
    error: String,
) -> Result<(), ServiceError> {
    state
        .connections
        .send(
            client_id,
            generation,
            ServiceControlMessage::SubmissionRejected {
                job_id: job_id.to_string(),
                attempt,
                retryable,
                error,
            },
        )
        .await
        .map_err(ServiceError::Internal)
}

fn is_retryable(error: &ServiceError) -> bool {
    matches!(
        error,
        ServiceError::Internal(_) | ServiceError::Upstream(_) | ServiceError::RuntimeUnavailable(_)
    )
}

fn validate_api_key(headers: &HeaderMap, configured_key: &str) -> Result<(), ServiceError> {
    let provided_key = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if provided_key == configured_key {
        return Ok(());
    }
    Err(ServiceError::Authentication)
}

#[derive(serde::Serialize)]
struct HealthResponse {
    status: &'static str,
    provider: &'static str,
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy",
        provider: state.manager.lock().await.active_name(),
    })
}

async fn ready(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ready",
        provider: state.manager.lock().await.active_name(),
    })
}

pub async fn serve(
    listener: tokio::net::TcpListener,
    state: AppState,
) -> Result<(), std::io::Error> {
    let manager = Arc::clone(&state.manager);
    let server_result = axum::serve(listener, router(state)).await;
    if let Err(error) = manager.lock().await.shutdown().await {
        tracing::error!("Failed to stop active LLM runtime: {error}");
    }
    server_result
}
