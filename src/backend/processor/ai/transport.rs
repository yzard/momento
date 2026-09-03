use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use futures::{SinkExt, StreamExt};
use momento_common::llm::result_stream::{
    ResultInputCorrelation, ResultManifest, ResultRecordChunkDecoder, ResultRecordCollector,
    ResultStatus,
};
use momento_common::llm::{
    decode_result_chunk, encode_input_chunk, CancelJobsRequest, CancelJobsResponse,
    ClientControlMessage, JobManifest, ServiceControlMessage, MAX_BINARY_CHUNK_BYTES,
    MAX_MOMENTO_WS_MESSAGE_BYTES, MAX_WS_WRITE_BUFFER_BYTES, QUEUE_CAPACITY_MAX_RETRY_AFTER_MS,
    WEBSOCKET_PROTOCOL,
};
use momento_common::work_signal::WorkSignal;
use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest, http::HeaderValue, protocol::WebSocketConfig, Message,
};

use crate::database::operations::{
    CommitLlmResultReceipt, CreateLlmResultReceipt, CreateLlmResultReceiptOutcome,
    LlmResultReceiptOutcome, LlmResultReceiptPreparation, LlmResultReceiptRejection,
    PrepareLlmResultReceipt, RejectLlmResultReceipt,
};
use crate::executor::{FileIoExecutorHandle, SqliteExecutorHandle};
use crate::io::file::{
    NormalizedStoragePath, PathClaimMode, PathClaimScope, StorageRootId, LLM_RESULT_INBOX_DIRECTORY,
};
use crate::io::journal::{
    FileEntryAction, FileEntryPlan, FileOperationPlan, FilePathClaimPlan,
    JournalSpaceReservationPlan,
};
use crate::io::StorageFileSession;
use crate::runtime::{DurableSourceId, SchedulerAdmissionKind, SchedulerHandle};

const COMMAND_CAPACITY: usize = 256;
const MAX_PENDING_CONTROLS: usize = 16;
const ADMISSION_TIMEOUT: Duration = Duration::from_secs(300);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const HEARTBEAT_EXPIRY: Duration = Duration::from_secs(90);
const WEBSOCKET_PATH: &str = "/api/v1/llm/connect";

pub struct PreparedSubmissionInput {
    pub sequence: u32,
    pub file_io: FileIoExecutorHandle,
    pub session: StorageFileSession,
}

pub enum SubmissionOutcome {
    Acknowledged {
        status: String,
    },
    Deferred {
        retry_after: Duration,
        required_bytes: u64,
        available_bytes: u64,
    },
    Rejected {
        retryable: bool,
        error: String,
    },
}

enum TransportCommand {
    Submit {
        manifest: JobManifest,
        inputs: Vec<PreparedSubmissionInput>,
        reply: oneshot::Sender<Result<SubmissionOutcome, String>>,
    },
    Cancel {
        request_id: String,
        request: CancelJobsRequest,
        reply: oneshot::Sender<Result<CancelJobsResponse, String>>,
    },
}

enum SubmissionPhase {
    StartPending,
    WaitingReady { deadline: Instant },
    Sending { input_index: usize },
    InputFinishedPending { input_index: usize },
    SubmissionFinishedPending,
    WaitingAcknowledgement { deadline: Instant },
}

struct SubmissionSession {
    manifest: JobManifest,
    inputs: Vec<ActiveSubmissionInput>,
    phase: SubmissionPhase,
    reply: oneshot::Sender<Result<SubmissionOutcome, String>>,
}

struct ActiveSubmissionInput {
    sequence: u32,
    file_io: FileIoExecutorHandle,
    session: Option<StorageFileSession>,
}

struct InboundResultSession {
    manifest: ResultManifest,
    job_version: i64,
    decoder: ResultRecordChunkDecoder,
    collector: ResultRecordCollector,
    hasher: Sha256,
    received_bytes: u64,
    journal_group_id: String,
    file_session: Option<StorageFileSession>,
    deadline: Instant,
}

struct TransportState {
    submissions: HashMap<String, SubmissionSession>,
    submission_order: VecDeque<String>,
    cancellations: HashMap<String, oneshot::Sender<Result<CancelJobsResponse, String>>>,
    results: HashMap<String, InboundResultSession>,
    pending_controls: VecDeque<Message>,
}

impl TransportState {
    fn new() -> Self {
        Self {
            submissions: HashMap::new(),
            submission_order: VecDeque::new(),
            cancellations: HashMap::new(),
            results: HashMap::new(),
            pending_controls: VecDeque::new(),
        }
    }
}

#[derive(Clone, Default)]
pub struct TransportHandle {
    submission_work: WorkSignal,
    cancellation_work: WorkSignal,
}

impl TransportHandle {
    pub fn wake_submissions(&self) {
        self.submission_work.notify();
    }

    pub fn submission_work_version(&self) -> u64 {
        self.submission_work.version()
    }

    pub async fn wait_for_submission_work(&self, observed_version: u64) -> u64 {
        self.submission_work.wait_for_change(observed_version).await
    }

    pub fn wake_cancellations(&self) {
        self.cancellation_work.notify();
    }

    pub fn cancellation_work_version(&self) -> u64 {
        self.cancellation_work.version()
    }

    pub async fn wait_for_cancellation_work(&self, observed_version: u64) -> u64 {
        self.cancellation_work
            .wait_for_change(observed_version)
            .await
    }
}

#[derive(Clone)]
pub struct LlmConnection {
    commands: mpsc::Sender<TransportCommand>,
    closed: watch::Receiver<bool>,
}

impl LlmConnection {
    pub async fn connect(
        server_address: &str,
        client_id: &str,
        api_key: &str,
        sqlite: SqliteExecutorHandle,
        file_io: FileIoExecutorHandle,
        scheduler: SchedulerHandle,
    ) -> Result<Self, String> {
        let websocket_url = format!("ws://{server_address}{WEBSOCKET_PATH}");
        let mut request = websocket_url
            .into_client_request()
            .map_err(|error| error.to_string())?;
        request.headers_mut().insert(
            "x-momento-client-id",
            HeaderValue::from_str(client_id).map_err(|error| error.to_string())?,
        );
        request.headers_mut().insert(
            "x-api-key",
            HeaderValue::from_str(api_key).map_err(|error| error.to_string())?,
        );
        request.headers_mut().insert(
            "sec-websocket-protocol",
            HeaderValue::from_static(WEBSOCKET_PROTOCOL),
        );
        let websocket_config = WebSocketConfig {
            write_buffer_size: 0,
            max_write_buffer_size: MAX_WS_WRITE_BUFFER_BYTES,
            max_message_size: Some(MAX_MOMENTO_WS_MESSAGE_BYTES),
            max_frame_size: Some(MAX_MOMENTO_WS_MESSAGE_BYTES),
            ..WebSocketConfig::default()
        };
        let (socket, response) =
            tokio_tungstenite::connect_async_with_config(request, Some(websocket_config), false)
                .await
                .map_err(|error| error.to_string())?;
        if response
            .headers()
            .get("sec-websocket-protocol")
            .and_then(|value| value.to_str().ok())
            != Some(WEBSOCKET_PROTOCOL)
        {
            return Err(
                "llm-service did not accept the required WebSocket subprotocol".to_string(),
            );
        }

        let (commands, command_receiver) = mpsc::channel(COMMAND_CAPACITY);
        let (closed_sender, closed) = watch::channel(false);
        scheduler.spawn_control(run_transport(
            socket,
            command_receiver,
            sqlite,
            file_io,
            scheduler.clone(),
            closed_sender,
        ));
        Ok(Self { commands, closed })
    }

    pub async fn closed(&self) {
        let mut closed = self.closed.clone();
        if *closed.borrow() {
            return;
        }
        while closed.changed().await.is_ok() {
            if *closed.borrow() {
                return;
            }
        }
    }

    pub async fn submit(
        &self,
        manifest: JobManifest,
        inputs: Vec<PreparedSubmissionInput>,
    ) -> Result<SubmissionOutcome, String> {
        manifest.validate()?;
        if manifest.inputs.len() != inputs.len()
            || manifest
                .inputs
                .iter()
                .zip(&inputs)
                .any(|(descriptor, prepared)| descriptor.sequence != prepared.sequence)
        {
            return Err(
                "prepared submission inputs do not match the validated manifest".to_string(),
            );
        }
        let (reply, response) = oneshot::channel();
        self.commands
            .send(TransportCommand::Submit {
                manifest,
                inputs,
                reply,
            })
            .await
            .map_err(|_| "LLM WebSocket transport closed".to_string())?;
        response
            .await
            .map_err(|_| "LLM submission response channel closed".to_string())?
    }

    pub async fn cancel(&self, request: CancelJobsRequest) -> Result<CancelJobsResponse, String> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let (reply, response) = oneshot::channel();
        self.commands
            .send(TransportCommand::Cancel {
                request_id,
                request,
                reply,
            })
            .await
            .map_err(|_| "LLM WebSocket transport closed".to_string())?;
        match tokio::time::timeout(ADMISSION_TIMEOUT, response).await {
            Ok(Ok(response)) => response,
            Ok(Err(_)) => Err("cancellation acknowledgement channel closed".to_string()),
            Err(_) => Err("cancellation acknowledgement timed out".to_string()),
        }
    }
}

async fn run_transport<Stream>(
    socket: tokio_tungstenite::WebSocketStream<Stream>,
    mut commands: mpsc::Receiver<TransportCommand>,
    sqlite: SqliteExecutorHandle,
    file_io: FileIoExecutorHandle,
    scheduler: SchedulerHandle,
    closed: watch::Sender<bool>,
) where
    Stream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (mut sink, mut source) = socket.split();
    let mut state = TransportState::new();
    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_peer_activity = Instant::now();
    let mut failure = None;

    loop {
        expire_submission_waits(&mut state.submissions, &mut state.submission_order);
        expire_result_receipts(&mut state.results, &sqlite, &file_io, &scheduler).await;
        let can_accept_command = state.pending_controls.len() < MAX_PENDING_CONTROLS;
        let has_outbound =
            !state.pending_controls.is_empty() || has_sendable_submission(&state.submissions);
        tokio::select! {
            biased;
            inbound = source.next() => {
                match inbound {
                    Some(Ok(message)) => {
                        last_peer_activity = Instant::now();
                        if let Err(error) = handle_inbound_message(
                            message,
                            &mut state,
                            &sqlite,
                            &file_io,
                            &scheduler,
                        ).await {
                            failure = Some(error);
                            break;
                        }
                    }
                    Some(Err(error)) => {
                        failure = Some(format!("LLM WebSocket read failed: {error}"));
                        break;
                    }
                    None => {
                        failure = Some("LLM WebSocket peer closed the connection".to_string());
                        break;
                    }
                }
            }
            command = commands.recv(), if can_accept_command => {
                let Some(command) = command else {
                    break;
                };
                accept_transport_command(
                    command,
                    &mut state,
                );
            }
            _ = heartbeat.tick() => {
                if last_peer_activity.elapsed() >= HEARTBEAT_EXPIRY {
                    failure = Some("LLM WebSocket heartbeat expired".to_string());
                    break;
                }
                if state.pending_controls.len() < MAX_PENDING_CONTROLS {
                    state.pending_controls.push_back(Message::Ping(Vec::new()));
                }
            }
            () = std::future::ready(()), if has_outbound => {
                let next = if let Some(control) = state.pending_controls.pop_front() {
                    Ok(Some(control))
                } else {
                    next_submission_message(
                        &mut state.submissions,
                        &mut state.submission_order,
                    ).await
                };
                match next {
                    Ok(Some(message)) => {
                        if let Err(error) = sink.send(message).await {
                            failure = Some(format!("LLM WebSocket write failed: {error}"));
                            break;
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                }
            }
        }
    }

    let error = failure.unwrap_or_else(|| "LLM WebSocket transport stopped".to_string());
    for (_, session) in state.submissions {
        let _ = session.reply.send(Err(error.clone()));
    }
    for (_, reply) in state.cancellations {
        let _ = reply.send(Err(error.clone()));
    }
    for (_, mut session) in state.results {
        if let Some(file_session) = session.file_session.take() {
            let _ = file_io.abort_storage_session_durable(file_session).await;
        }
        handoff_result_journal_rollback(
            &session.journal_group_id,
            1,
            &sqlite,
            &file_io,
            &scheduler,
        )
        .await;
    }
    let _ = closed.send(true);
}

fn accept_transport_command(command: TransportCommand, state: &mut TransportState) {
    match command {
        TransportCommand::Submit {
            manifest,
            inputs,
            reply,
        } => {
            if inputs.is_empty() || manifest.inputs.len() != inputs.len() {
                let _ = reply.send(Err(
                    "submission input sessions do not match the manifest".to_string()
                ));
                return;
            }
            let job_id = manifest.job_id.clone();
            if state.submissions.contains_key(&job_id) {
                let _ = reply.send(Err("submission is already active".to_string()));
                return;
            }
            state.submissions.insert(
                job_id.clone(),
                SubmissionSession {
                    manifest,
                    inputs: inputs
                        .into_iter()
                        .map(|input| ActiveSubmissionInput {
                            sequence: input.sequence,
                            file_io: input.file_io,
                            session: Some(input.session),
                        })
                        .collect(),
                    phase: SubmissionPhase::StartPending,
                    reply,
                },
            );
            state.submission_order.push_back(job_id);
        }
        TransportCommand::Cancel {
            request_id,
            request,
            reply,
        } => {
            let message = ClientControlMessage::CancelJobs {
                request_id: request_id.clone(),
                request,
            };
            match control_message(message) {
                Ok(message) => {
                    state.cancellations.insert(request_id, reply);
                    state.pending_controls.push_back(message);
                }
                Err(error) => {
                    let _ = reply.send(Err(error));
                }
            }
        }
    }
}

fn has_sendable_submission(submissions: &HashMap<String, SubmissionSession>) -> bool {
    submissions.values().any(|session| {
        matches!(
            session.phase,
            SubmissionPhase::StartPending
                | SubmissionPhase::Sending { .. }
                | SubmissionPhase::InputFinishedPending { .. }
                | SubmissionPhase::SubmissionFinishedPending
        )
    })
}

async fn next_submission_message(
    submissions: &mut HashMap<String, SubmissionSession>,
    submission_order: &mut VecDeque<String>,
) -> Result<Option<Message>, String> {
    let attempts = submission_order.len();
    for _ in 0..attempts {
        let Some(job_id) = submission_order.pop_front() else {
            return Ok(None);
        };
        let Some(session) = submissions.get_mut(&job_id) else {
            continue;
        };
        let message = match session.phase {
            SubmissionPhase::StartPending => {
                session.phase = SubmissionPhase::WaitingReady {
                    deadline: Instant::now() + ADMISSION_TIMEOUT,
                };
                Some(control_message(ClientControlMessage::SubmissionStart {
                    manifest: session.manifest.clone(),
                })?)
            }
            SubmissionPhase::Sending { input_index } => {
                let input = session
                    .inputs
                    .get_mut(input_index)
                    .ok_or_else(|| "submission input index is invalid".to_string())?;
                let file_session = input
                    .session
                    .take()
                    .ok_or_else(|| "submission input file session is unavailable".to_string())?;
                let (file_session, bytes) = input
                    .file_io
                    .read_storage_session_durable(file_session, MAX_BINARY_CHUNK_BYTES)
                    .await
                    .map_err(|error| error.to_string())?;
                input.session = Some(file_session);
                if bytes.is_empty() {
                    let file_session = input.session.take().ok_or_else(|| {
                        "submission input file session is unavailable".to_string()
                    })?;
                    input
                        .file_io
                        .close_storage_session_durable(file_session)
                        .await
                        .map_err(|error| error.to_string())?;
                    session.phase = SubmissionPhase::InputFinishedPending { input_index };
                    None
                } else {
                    Some(Message::Binary(encode_input_chunk(
                        &job_id,
                        input.sequence,
                        &bytes,
                    )?))
                }
            }
            SubmissionPhase::InputFinishedPending { input_index } => {
                let sequence = session.inputs[input_index].sequence;
                let next_input = input_index + 1;
                session.phase = if next_input < session.inputs.len() {
                    SubmissionPhase::Sending {
                        input_index: next_input,
                    }
                } else {
                    SubmissionPhase::SubmissionFinishedPending
                };
                Some(control_message(ClientControlMessage::InputFinished {
                    job_id: job_id.clone(),
                    sequence,
                })?)
            }
            SubmissionPhase::SubmissionFinishedPending => {
                session.phase = SubmissionPhase::WaitingAcknowledgement {
                    deadline: Instant::now() + ADMISSION_TIMEOUT,
                };
                Some(control_message(ClientControlMessage::SubmissionFinished {
                    job_id: job_id.clone(),
                })?)
            }
            SubmissionPhase::WaitingReady { .. }
            | SubmissionPhase::WaitingAcknowledgement { .. } => None,
        };
        submission_order.push_back(job_id);
        if message.is_some() {
            return Ok(message);
        }
    }
    Ok(None)
}

async fn handle_inbound_message(
    message: Message,
    state: &mut TransportState,
    sqlite: &SqliteExecutorHandle,
    file_io: &FileIoExecutorHandle,
    scheduler: &SchedulerHandle,
) -> Result<(), String> {
    match message {
        Message::Text(text) => {
            let message = serde_json::from_str::<ServiceControlMessage>(&text)
                .map_err(|error| format!("invalid LLM control message: {error}"))?;
            handle_service_message(message, state, sqlite, file_io, scheduler).await
        }
        Message::Ping(bytes) => {
            if state.pending_controls.len() == MAX_PENDING_CONTROLS {
                return Err("urgent LLM control capacity is exhausted".to_string());
            }
            state.pending_controls.push_front(Message::Pong(bytes));
            Ok(())
        }
        Message::Pong(_) => Ok(()),
        Message::Close(_) => Err("LLM WebSocket peer closed the connection".to_string()),
        Message::Binary(bytes) => {
            handle_result_chunk(
                bytes,
                &mut state.results,
                &mut state.pending_controls,
                sqlite,
                file_io,
                scheduler,
            )
            .await
        }
        Message::Frame(_) => Err("unexpected raw LLM WebSocket frame".to_string()),
    }
}

async fn handle_service_message(
    message: ServiceControlMessage,
    state: &mut TransportState,
    sqlite: &SqliteExecutorHandle,
    file_io: &FileIoExecutorHandle,
    scheduler: &SchedulerHandle,
) -> Result<(), String> {
    match message {
        ServiceControlMessage::SubmissionReady {
            job_id,
            attempt,
            required_input_sequences,
        } => {
            let session = state
                .submissions
                .get_mut(&job_id)
                .ok_or_else(|| "submissionReady references an inactive job".to_string())?;
            if session.manifest.attempt != attempt
                || !matches!(session.phase, SubmissionPhase::WaitingReady { .. })
            {
                return Err("submissionReady correlation or phase is invalid".to_string());
            }
            let required_sequence_count = required_input_sequences.len();
            let required_sequences = required_input_sequences
                .into_iter()
                .collect::<std::collections::HashSet<_>>();
            if required_sequences.len() != required_sequence_count
                || required_sequences.iter().any(|sequence| {
                    !session
                        .inputs
                        .iter()
                        .any(|input| input.sequence == *sequence)
                })
            {
                return Err("submissionReady required input sequences are invalid".to_string());
            }
            let mut required_inputs = Vec::with_capacity(required_sequences.len());
            for mut input in session.inputs.drain(..) {
                if required_sequences.contains(&input.sequence) {
                    required_inputs.push(input);
                    continue;
                }
                let file_session = input
                    .session
                    .take()
                    .ok_or_else(|| "submission input file session is unavailable".to_string())?;
                input
                    .file_io
                    .close_storage_session_durable(file_session)
                    .await
                    .map_err(|error| error.to_string())?;
            }
            session.inputs = required_inputs;
            session.phase = if session.inputs.is_empty() {
                SubmissionPhase::SubmissionFinishedPending
            } else {
                SubmissionPhase::Sending { input_index: 0 }
            };
        }
        ServiceControlMessage::SubmissionAcknowledged {
            job_id,
            attempt,
            status,
        } => {
            let session = state
                .submissions
                .remove(&job_id)
                .ok_or_else(|| "submissionAcknowledged references an inactive job".to_string())?;
            state.submission_order.retain(|active| active != &job_id);
            if session.manifest.attempt != attempt
                || !matches!(
                    session.phase,
                    SubmissionPhase::WaitingReady { .. }
                        | SubmissionPhase::WaitingAcknowledgement { .. }
                )
            {
                let _ = session.reply.send(Err(
                    "submissionAcknowledged correlation or phase is invalid".to_string(),
                ));
            } else {
                let _ = session
                    .reply
                    .send(Ok(SubmissionOutcome::Acknowledged { status }));
            }
        }
        ServiceControlMessage::SubmissionDeferred {
            job_id,
            attempt,
            reason: momento_common::llm::SubmissionDeferredReason::QueueCapacity,
            required_bytes,
            available_bytes,
            retry_after_ms,
        } => {
            let session = state
                .submissions
                .remove(&job_id)
                .ok_or_else(|| "submissionDeferred references an inactive job".to_string())?;
            state.submission_order.retain(|active| active != &job_id);
            if session.manifest.attempt != attempt
                || !matches!(session.phase, SubmissionPhase::WaitingReady { .. })
                || required_bytes == 0
                || retry_after_ms == 0
                || retry_after_ms > QUEUE_CAPACITY_MAX_RETRY_AFTER_MS
            {
                let _ = session.reply.send(Err(
                    "submissionDeferred correlation or fields are invalid".to_string(),
                ));
            } else {
                let _ = session.reply.send(Ok(SubmissionOutcome::Deferred {
                    retry_after: Duration::from_millis(retry_after_ms),
                    required_bytes,
                    available_bytes,
                }));
            }
        }
        ServiceControlMessage::SubmissionRejected {
            job_id,
            attempt,
            retryable,
            error,
        } => {
            let session = state
                .submissions
                .remove(&job_id)
                .ok_or_else(|| "submissionRejected references an inactive job".to_string())?;
            state.submission_order.retain(|active| active != &job_id);
            if session.manifest.attempt != attempt {
                let _ = session
                    .reply
                    .send(Err("submissionRejected correlation is invalid".to_string()));
            } else {
                let _ = session
                    .reply
                    .send(Ok(SubmissionOutcome::Rejected { retryable, error }));
            }
        }
        ServiceControlMessage::CancellationAcknowledged {
            request_id,
            response,
        } => {
            let reply = state.cancellations.remove(&request_id).ok_or_else(|| {
                "cancellation acknowledgement references an inactive request".to_string()
            })?;
            let _ = reply.send(Ok(response));
        }
        ServiceControlMessage::CancellationRejected {
            request_id, error, ..
        } => {
            let reply = state.cancellations.remove(&request_id).ok_or_else(|| {
                "cancellation rejection references an inactive request".to_string()
            })?;
            let _ = reply.send(Err(error));
        }
        ServiceControlMessage::ResultStart { manifest } => {
            start_result_receipt(
                manifest,
                &mut state.results,
                &mut state.pending_controls,
                sqlite,
                file_io,
                scheduler,
            )
            .await?;
        }
        ServiceControlMessage::ResultFinished { job_id, attempt } => {
            finish_result_receipt(
                &job_id,
                attempt,
                &mut state.results,
                &mut state.pending_controls,
                sqlite,
                file_io,
                scheduler,
            )
            .await?;
        }
    }
    Ok(())
}

async fn start_result_receipt(
    manifest: ResultManifest,
    results: &mut HashMap<String, InboundResultSession>,
    pending_controls: &mut VecDeque<Message>,
    sqlite: &SqliteExecutorHandle,
    file_io: &FileIoExecutorHandle,
    scheduler: &SchedulerHandle,
) -> Result<(), String> {
    let job_id = manifest.job_id.clone();
    let attempt = manifest.attempt;
    if let Err(error) = manifest.validate() {
        if momento_common::llm::is_valid_job_id(&job_id) && attempt > 0 {
            return finish_invalid_result(
                job_id,
                attempt,
                None,
                error,
                ResultFailureContext {
                    pending_controls,
                    sqlite,
                    journal: None,
                    file_io,
                    scheduler,
                },
            )
            .await;
        }
        return queue_result_control(
            pending_controls,
            ClientControlMessage::ResultReceiptRejected {
                job_id,
                attempt,
                error,
            },
        );
    }
    if results.contains_key(&job_id) {
        return queue_result_control(
            pending_controls,
            ClientControlMessage::ResultReceiptDeferred {
                job_id,
                attempt,
                retry_after_ms: 1_000,
            },
        );
    }
    let (job_version, prepared_inputs) = match sqlite
        .prepare_llm_result_receipt_durable(PrepareLlmResultReceipt {
            job_id: job_id.clone(),
            media_id: manifest.media_id,
            task: manifest.task.clone(),
            attempt,
        })
        .await
    {
        Ok(LlmResultReceiptPreparation::Ready {
            job_version,
            inputs,
        }) => (job_version, inputs),
        Ok(LlmResultReceiptPreparation::Ignored) => {
            tracing::warn!(job_id, "discarding result for an unknown or terminal job");
            return queue_result_control(
                pending_controls,
                ClientControlMessage::ResultReceived { job_id, attempt },
            );
        }
        Ok(LlmResultReceiptPreparation::CorrelationFailed) => {
            return queue_result_control(
                pending_controls,
                ClientControlMessage::ResultReceiptRejected {
                    job_id,
                    attempt,
                    error: "result manifest does not match the active Momento job".to_string(),
                },
            );
        }
        Err(error) => {
            return queue_result_control(
                pending_controls,
                ClientControlMessage::ResultReceiptDeferred {
                    job_id,
                    attempt,
                    retry_after_ms: 1_000,
                },
            )
            .map_err(|queue_error| format!("{queue_error}; SQLite error: {error}"));
        }
    };
    if prepared_inputs.is_empty() {
        return finish_invalid_result(
            job_id,
            attempt,
            Some(job_version),
            "active result job has no retained input correlation".to_string(),
            ResultFailureContext {
                pending_controls,
                sqlite,
                journal: None,
                file_io,
                scheduler,
            },
        )
        .await;
    }
    let correlations = prepared_inputs
        .into_iter()
        .map(|input| {
            u32::try_from(input.sequence)
                .map(|sequence| ResultInputCorrelation {
                    sequence,
                    frame_timestamp_ms: input.frame_timestamp_ms,
                })
                .map_err(|_| "prepared result input sequence is outside u32".to_string())
        })
        .collect::<Result<Vec<_>, _>>();
    let correlations = match correlations {
        Ok(correlations) => correlations,
        Err(error) => {
            return finish_invalid_result(
                job_id,
                attempt,
                Some(job_version),
                error,
                ResultFailureContext {
                    pending_controls,
                    sqlite,
                    journal: None,
                    file_io,
                    scheduler,
                },
            )
            .await;
        }
    };
    let collector = match ResultRecordCollector::new(
        &manifest.task,
        manifest.status,
        &correlations,
        manifest.record_count,
        manifest.byte_size,
    ) {
        Ok(collector) => collector,
        Err(error) => {
            return finish_invalid_result(
                job_id,
                attempt,
                Some(job_version),
                error,
                ResultFailureContext {
                    pending_controls,
                    sqlite,
                    journal: None,
                    file_io,
                    scheduler,
                },
            )
            .await;
        }
    };
    let (journal_group_id, file_session) =
        match prepare_result_journal(&manifest, job_version, sqlite, file_io, scheduler).await {
            Ok(Some(prepared)) => prepared,
            Ok(None) => {
                return queue_result_control(
                    pending_controls,
                    ClientControlMessage::ResultReceived { job_id, attempt },
                );
            }
            Err(PrepareResultJournalError::Transient(error)) => {
                tracing::warn!(job_id, error, "deferring LLM result Journal admission");
                return queue_result_control(
                    pending_controls,
                    ClientControlMessage::ResultReceiptDeferred {
                        job_id,
                        attempt,
                        retry_after_ms: 1_000,
                    },
                );
            }
            Err(PrepareResultJournalError::Permanent(error)) => {
                return finish_invalid_result(
                    job_id,
                    attempt,
                    Some(job_version),
                    error,
                    ResultFailureContext {
                        pending_controls,
                        sqlite,
                        journal: None,
                        file_io,
                        scheduler,
                    },
                )
                .await;
            }
        };
    results.insert(
        job_id.clone(),
        InboundResultSession {
            manifest,
            job_version,
            decoder: ResultRecordChunkDecoder::new(),
            collector,
            hasher: Sha256::new(),
            received_bytes: 0,
            journal_group_id,
            file_session: Some(file_session),
            deadline: Instant::now() + ADMISSION_TIMEOUT,
        },
    );
    queue_result_control(
        pending_controls,
        ClientControlMessage::ResultReady { job_id, attempt },
    )
}

async fn prepare_result_journal(
    manifest: &ResultManifest,
    job_version: i64,
    sqlite: &SqliteExecutorHandle,
    file_io: &FileIoExecutorHandle,
    scheduler: &SchedulerHandle,
) -> Result<Option<(String, StorageFileSession)>, PrepareResultJournalError> {
    let identity = format!(
        "{:x}",
        Sha256::digest(format!("{}:{}", manifest.job_id, manifest.attempt).as_bytes())
    );
    let receive_token = uuid::Uuid::new_v4().to_string();
    let group_id = format!("llm-result-{identity}-{receive_token}");
    let temporary_path = NormalizedStoragePath::parse(&format!(
        "{LLM_RESULT_INBOX_DIRECTORY}/.llm-result-{identity}-{receive_token}.tmp"
    ))
    .map_err(|error| PrepareResultJournalError::Permanent(error.to_string()))?;
    let inbox_path = NormalizedStoragePath::parse(&format!(
        "{LLM_RESULT_INBOX_DIRECTORY}/llm-result-{identity}.records"
    ))
    .map_err(|error| PrepareResultJournalError::Permanent(error.to_string()))?;
    let reservation_bytes = manifest.byte_size.checked_add(16 * 1024).ok_or_else(|| {
        PrepareResultJournalError::Permanent("result Journal reservation overflowed".to_string())
    })?;
    let reservation = match file_io.reserve_journal_space(group_id.clone(), reservation_bytes) {
        Ok(crate::io::space_budget::SpaceAdmission::Fits(reservation)) => reservation,
        Ok(crate::io::space_budget::SpaceAdmission::TemporarilyUnavailable {
            required_bytes,
            available_bytes,
        }) => {
            return Err(PrepareResultJournalError::Transient(format!(
                "result Journal needs {required_bytes} bytes but only {available_bytes} are available"
            )));
        }
        Ok(crate::io::space_budget::SpaceAdmission::ExceedsHardLimit {
            required_bytes,
            class_limit_bytes,
        }) => {
            return Err(PrepareResultJournalError::Permanent(format!(
                "result Journal size {required_bytes} exceeds its hard limit {class_limit_bytes}"
            )));
        }
        Err(error) => return Err(PrepareResultJournalError::Transient(error.to_string())),
    };
    let write_claim = |path: NormalizedStoragePath, role: &str| FilePathClaimPlan {
        storage_root: StorageRootId::Journal,
        path,
        mode: PathClaimMode::Write,
        scope: PathClaimScope::Exact,
        role: role.to_string(),
        expected_version: None,
    };
    let plan = FileOperationPlan {
        group_id: group_id.clone(),
        kind: "llm_result_receive".to_string(),
        owner_kind: "llm_result".to_string(),
        owner_id: manifest.job_id.clone(),
        claim_token: None,
        product_target: Some("llm_result_inbox".to_string()),
        product_version: Some(1),
        entries: vec![FileEntryPlan {
            action: FileEntryAction::Publish,
            storage_root: StorageRootId::Journal,
            source_path: None,
            temporary_path: Some(temporary_path.clone()),
            destination_path: Some(inbox_path.clone()),
            tombstone_path: None,
            expected_size: Some(manifest.byte_size),
            expected_sha256: None,
            expected_version: None,
        }],
        claims: vec![
            write_claim(temporary_path.clone(), "result_receive_temporary"),
            write_claim(inbox_path.clone(), "result_inbox"),
        ],
        space_reservation: Some(
            JournalSpaceReservationPlan::new(reservation)
                .map_err(|error| PrepareResultJournalError::Permanent(error.to_string()))?,
        ),
    };
    let result_status = match manifest.status {
        ResultStatus::Completed => "completed",
        ResultStatus::Failed => "failed",
    };
    let outcome = sqlite
        .create_llm_result_receipt_durable(CreateLlmResultReceipt {
            job_id: manifest.job_id.clone(),
            attempt: manifest.attempt,
            expected_job_version: job_version,
            media_id: manifest.media_id,
            task: manifest.task.clone(),
            result_status: result_status.to_string(),
            model_type: manifest.model_type.clone(),
            model_version: manifest.model_version.clone(),
            encoding: manifest.encoding.clone(),
            record_count: manifest.record_count,
            byte_size: manifest.byte_size,
            content_hash: manifest.content_hash.to_ascii_lowercase(),
            journal_group_id: group_id.clone(),
            inbox_path: inbox_path.relative_path().to_string(),
            receive_token,
            journal_plan: plan,
        })
        .await
        .map_err(|error| match error.kind {
            crate::executor::ExecutorErrorKind::DatabasePermanent
            | crate::executor::ExecutorErrorKind::InvalidInput
            | crate::executor::ExecutorErrorKind::BadRequest => {
                PrepareResultJournalError::Permanent(error.to_string())
            }
            _ => PrepareResultJournalError::Transient(error.to_string()),
        })?;
    match outcome {
        CreateLlmResultReceiptOutcome::Created => {}
        CreateLlmResultReceiptOutcome::Deferred
        | CreateLlmResultReceiptOutcome::Changed
        | CreateLlmResultReceiptOutcome::PathConflict => {
            return Err(PrepareResultJournalError::Transient(format!(
                "result Journal admission returned {outcome:?}"
            )));
        }
    }
    match file_io
        .open_storage_write_session_durable(StorageRootId::Journal, temporary_path, 0)
        .await
    {
        Ok(session) => Ok(Some((group_id, session))),
        Err(error) => {
            handoff_result_journal_rollback(&group_id, 1, sqlite, file_io, scheduler).await;
            Err(PrepareResultJournalError::Transient(error.to_string()))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PrepareResultJournalError {
    Transient(String),
    Permanent(String),
}

async fn handle_result_chunk(
    bytes: Vec<u8>,
    results: &mut HashMap<String, InboundResultSession>,
    pending_controls: &mut VecDeque<Message>,
    sqlite: &SqliteExecutorHandle,
    file_io: &FileIoExecutorHandle,
    scheduler: &SchedulerHandle,
) -> Result<(), String> {
    let chunk = decode_result_chunk(&bytes)?;
    let job_id = chunk.job_id.to_string();
    let session = results
        .get_mut(&job_id)
        .ok_or_else(|| "result chunk references an inactive receipt".to_string())?;
    let attempted_end = session
        .received_bytes
        .checked_add(chunk.payload.len() as u64)
        .ok_or_else(|| "result receipt byte count overflowed".to_string())?;
    if chunk.offset != session.received_bytes || attempted_end > session.manifest.byte_size {
        let session = results
            .remove(&job_id)
            .ok_or_else(|| "result receipt disappeared".to_string())?;
        return finish_invalid_result(
            job_id,
            session.manifest.attempt,
            Some(session.job_version),
            "result chunk offset or size is invalid".to_string(),
            ResultFailureContext {
                pending_controls,
                sqlite,
                journal: Some((session.journal_group_id, session.file_session)),
                file_io,
                scheduler,
            },
        )
        .await;
    }
    let decoded = {
        let InboundResultSession {
            decoder, collector, ..
        } = session;
        decoder.push(chunk.payload, |record| collector.push(record.as_borrowed()))
    };
    if let Err(error) = decoded {
        let session = results
            .remove(&job_id)
            .ok_or_else(|| "result receipt disappeared".to_string())?;
        return finish_invalid_result(
            job_id,
            session.manifest.attempt,
            Some(session.job_version),
            error,
            ResultFailureContext {
                pending_controls,
                sqlite,
                journal: Some((session.journal_group_id, session.file_session)),
                file_io,
                scheduler,
            },
        )
        .await;
    }
    session.hasher.update(chunk.payload);
    let file_session = session
        .file_session
        .take()
        .ok_or_else(|| "result Journal session is unavailable".to_string())?;
    let write = file_io
        .write_storage_session_durable(file_session, chunk.payload.to_vec())
        .await;
    let (returned_session, written) = match write {
        Ok(written) => written,
        Err(error) => {
            let session = results
                .remove(&job_id)
                .ok_or_else(|| "result receipt disappeared".to_string())?;
            handoff_result_journal_rollback(
                &session.journal_group_id,
                1,
                sqlite,
                file_io,
                scheduler,
            )
            .await;
            tracing::warn!(job_id, error = %error, "deferring failed result Journal write");
            return queue_result_control(
                pending_controls,
                ClientControlMessage::ResultReceiptDeferred {
                    job_id,
                    attempt: session.manifest.attempt,
                    retry_after_ms: 1_000,
                },
            );
        }
    };
    if written != chunk.payload.len() {
        let session = results
            .remove(&job_id)
            .ok_or_else(|| "result receipt disappeared".to_string())?;
        let _ = file_io
            .abort_storage_session_durable(returned_session)
            .await;
        return finish_invalid_result(
            job_id,
            session.manifest.attempt,
            Some(session.job_version),
            "result Journal write was incomplete".to_string(),
            ResultFailureContext {
                pending_controls,
                sqlite,
                journal: Some((session.journal_group_id, session.file_session)),
                file_io,
                scheduler,
            },
        )
        .await;
    }
    session.file_session = Some(returned_session);
    session.received_bytes = attempted_end;
    session.deadline = Instant::now() + ADMISSION_TIMEOUT;
    queue_result_control(
        pending_controls,
        ClientControlMessage::ResultChunkReady {
            job_id,
            attempt: session.manifest.attempt,
            offset: attempted_end,
        },
    )
}

async fn finish_result_receipt(
    job_id: &str,
    attempt: u32,
    results: &mut HashMap<String, InboundResultSession>,
    pending_controls: &mut VecDeque<Message>,
    sqlite: &SqliteExecutorHandle,
    file_io: &FileIoExecutorHandle,
    scheduler: &SchedulerHandle,
) -> Result<(), String> {
    let Some(mut session) = results.remove(job_id) else {
        return queue_result_control(
            pending_controls,
            ClientControlMessage::ResultReceiptDeferred {
                job_id: job_id.to_string(),
                attempt,
                retry_after_ms: 1_000,
            },
        );
    };
    let expected_hash = session.manifest.content_hash.clone();
    if session.manifest.attempt != attempt
        || session.received_bytes != session.manifest.byte_size
        || format!("{:x}", session.hasher.clone().finalize()) != expected_hash
    {
        return finish_invalid_result(
            job_id.to_string(),
            session.manifest.attempt,
            Some(session.job_version),
            "result completion does not match its manifest".to_string(),
            ResultFailureContext {
                pending_controls,
                sqlite,
                journal: Some((session.journal_group_id, session.file_session)),
                file_io,
                scheduler,
            },
        )
        .await;
    }
    if let Err(error) = session.decoder.finish() {
        return finish_invalid_result(
            job_id.to_string(),
            attempt,
            Some(session.job_version),
            error,
            ResultFailureContext {
                pending_controls,
                sqlite,
                journal: Some((session.journal_group_id, session.file_session)),
                file_io,
                scheduler,
            },
        )
        .await;
    }
    let collected = match session.collector.finish() {
        Ok(collected) => collected,
        Err(error) => {
            return finish_invalid_result(
                job_id.to_string(),
                attempt,
                Some(session.job_version),
                error,
                ResultFailureContext {
                    pending_controls,
                    sqlite,
                    journal: Some((session.journal_group_id, session.file_session)),
                    file_io,
                    scheduler,
                },
            )
            .await;
        }
    };
    let file_session = session
        .file_session
        .take()
        .ok_or_else(|| "result Journal session is unavailable".to_string())?;
    if let Err(error) = file_io.commit_storage_session_durable(file_session).await {
        handoff_result_journal_rollback(&session.journal_group_id, 1, sqlite, file_io, scheduler)
            .await;
        tracing::warn!(job_id, error = %error, "deferring result Journal sync failure");
        return queue_result_control(
            pending_controls,
            ClientControlMessage::ResultReceiptDeferred {
                job_id: job_id.to_string(),
                attempt,
                retry_after_ms: 1_000,
            },
        );
    }
    let ticket = file_io
        .reserve_journal_mutation(&session.journal_group_id, 2)
        .map_err(|error| error.to_string())?;
    let grant = sqlite
        .begin_file_operation_publication_durable(&ticket, 1)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "result Journal publication changed before it began".to_string())?;
    let mut lease = ticket.acquire(grant).map_err(|error| error.to_string())?;
    if let Err(error) = file_io.apply_next_journal_entry_durable(&mut lease).await {
        drop(lease);
        handoff_result_journal_rollback(&session.journal_group_id, 2, sqlite, file_io, scheduler)
            .await;
        tracing::warn!(job_id, error = %error, "deferring result Journal publication failure");
        return queue_result_control(
            pending_controls,
            ClientControlMessage::ResultReceiptDeferred {
                job_id: job_id.to_string(),
                attempt,
                retry_after_ms: 1_000,
            },
        );
    }
    let checkpoint = sqlite
        .record_file_entry_published_durable(session.journal_group_id.clone(), 2, 0)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "result Journal publication checkpoint changed".to_string())?;
    drop(lease);
    if !checkpoint.phase_complete {
        return Err("result Journal publication did not complete".to_string());
    }
    drop(collected);
    let received = match scheduler
        .acquire_durable(
            DurableSourceId::LlmResult,
            SchedulerAdmissionKind::ExistingClaimCompletion,
        )
        .await
    {
        Ok(_admission) => sqlite
            .commit_llm_result_receipt_durable(CommitLlmResultReceipt {
                job_id: job_id.to_string(),
                attempt,
                expected_job_version: session.job_version,
                journal_group_id: session.journal_group_id.clone(),
                expected_group_version: checkpoint.version,
            })
            .await
            .map_err(|error| error.to_string()),
        Err(error) => Err(error),
    };
    let receipt = match received {
        Ok(LlmResultReceiptOutcome::Received) => {
            scheduler.wake_llm_results();
            ClientControlMessage::ResultReceived {
                job_id: job_id.to_string(),
                attempt,
            }
        }
        Ok(LlmResultReceiptOutcome::Ignored) => {
            handoff_result_journal_rollback(
                &session.journal_group_id,
                checkpoint.version,
                sqlite,
                file_io,
                scheduler,
            )
            .await;
            ClientControlMessage::ResultReceived {
                job_id: job_id.to_string(),
                attempt,
            }
        }
        Ok(LlmResultReceiptOutcome::CorrelationFailed | LlmResultReceiptOutcome::Changed) => {
            ClientControlMessage::ResultReceiptDeferred {
                job_id: job_id.to_string(),
                attempt,
                retry_after_ms: 1_000,
            }
        }
        Err(error) => {
            tracing::warn!(job_id, error, "deferring LLM result receipt");
            ClientControlMessage::ResultReceiptDeferred {
                job_id: job_id.to_string(),
                attempt,
                retry_after_ms: 1_000,
            }
        }
    };
    queue_result_control(pending_controls, receipt)
}

struct ResultFailureContext<'a> {
    pending_controls: &'a mut VecDeque<Message>,
    sqlite: &'a SqliteExecutorHandle,
    journal: Option<(String, Option<StorageFileSession>)>,
    file_io: &'a FileIoExecutorHandle,
    scheduler: &'a SchedulerHandle,
}

async fn finish_invalid_result(
    job_id: String,
    attempt: u32,
    expected_job_version: Option<i64>,
    error: String,
    context: ResultFailureContext<'_>,
) -> Result<(), String> {
    let journal_group_id = if let Some((journal_group_id, file_session)) = context.journal {
        if let Some(file_session) = file_session {
            let _ = context
                .file_io
                .abort_storage_session_durable(file_session)
                .await;
        }
        Some(journal_group_id)
    } else {
        None
    };
    let rejection = context
        .sqlite
        .reject_llm_result_receipt_durable(RejectLlmResultReceipt {
            job_id: job_id.clone(),
            attempt,
            expected_job_version,
            error: error.clone(),
        })
        .await;
    if let Some(journal_group_id) = journal_group_id {
        handoff_result_journal_rollback(
            &journal_group_id,
            1,
            context.sqlite,
            context.file_io,
            context.scheduler,
        )
        .await;
    }
    let control = match rejection {
        Ok(LlmResultReceiptRejection::Failed) => ClientControlMessage::ResultReceiptRejected {
            job_id,
            attempt,
            error,
        },
        Ok(LlmResultReceiptRejection::Discarded) => {
            ClientControlMessage::ResultReceived { job_id, attempt }
        }
        Err(database_error) => {
            tracing::warn!(job_id, error = %database_error, "deferring invalid LLM result receipt");
            ClientControlMessage::ResultReceiptDeferred {
                job_id,
                attempt,
                retry_after_ms: 1_000,
            }
        }
    };
    queue_result_control(context.pending_controls, control)
}

async fn handoff_result_journal_rollback(
    group_id: &str,
    expected_version: i64,
    sqlite: &SqliteExecutorHandle,
    file_io: &FileIoExecutorHandle,
    scheduler: &SchedulerHandle,
) {
    if let Err(error) = crate::io::recovery::cancel_generic_file_operation_with_components(
        sqlite,
        file_io,
        scheduler,
        group_id.to_string(),
        expected_version,
    )
    .await
    {
        tracing::error!(group_id, error = %error, "failed to hand result Journal rollback to recovery");
    }
}

fn queue_result_control(
    pending_controls: &mut VecDeque<Message>,
    message: ClientControlMessage,
) -> Result<(), String> {
    if pending_controls.len() == MAX_PENDING_CONTROLS {
        return Err("urgent LLM result-receipt capacity is exhausted".to_string());
    }
    pending_controls.push_front(control_message(message)?);
    Ok(())
}

fn expire_submission_waits(
    submissions: &mut HashMap<String, SubmissionSession>,
    submission_order: &mut VecDeque<String>,
) {
    let now = Instant::now();
    let expired = submissions
        .iter()
        .filter_map(|(job_id, session)| {
            let deadline = match session.phase {
                SubmissionPhase::WaitingReady { deadline }
                | SubmissionPhase::WaitingAcknowledgement { deadline } => deadline,
                _ => return None,
            };
            (deadline <= now).then(|| job_id.clone())
        })
        .collect::<Vec<_>>();
    for job_id in expired {
        if let Some(session) = submissions.remove(&job_id) {
            let _ = session
                .reply
                .send(Err("submission acknowledgement timed out".to_string()));
        }
        submission_order.retain(|active| active != &job_id);
    }
}

async fn expire_result_receipts(
    results: &mut HashMap<String, InboundResultSession>,
    sqlite: &SqliteExecutorHandle,
    file_io: &FileIoExecutorHandle,
    scheduler: &SchedulerHandle,
) {
    let now = Instant::now();
    let expired = results
        .iter()
        .filter(|(_, session)| session.deadline <= now)
        .map(|(job_id, _)| job_id.clone())
        .collect::<Vec<_>>();
    for job_id in expired {
        if let Some(mut session) = results.remove(&job_id) {
            tracing::warn!(job_id, "expired incomplete LLM result receipt");
            if let Some(file_session) = session.file_session.take() {
                let _ = file_io.abort_storage_session_durable(file_session).await;
            }
            handoff_result_journal_rollback(
                &session.journal_group_id,
                1,
                sqlite,
                file_io,
                scheduler,
            )
            .await;
        }
    }
}

fn control_message(message: ClientControlMessage) -> Result<Message, String> {
    serde_json::to_string(&message)
        .map(Message::Text)
        .map_err(|error| error.to_string())
}
