use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use momento_common::llm::{
    encode_input_chunk, CancelJobsRequest, CancelJobsResponse, ClientControlMessage, JobManifest,
    ServiceControlMessage, MAX_BINARY_CHUNK_BYTES, WEBSOCKET_PROTOCOL,
};
use tokio::io::AsyncReadExt;
use tokio::sync::{mpsc, oneshot, watch, Mutex, Notify};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue, Message};

use crate::database::DbPool;

const OUTBOUND_MESSAGE_CAPACITY: usize = 256;
const ADMISSION_TIMEOUT: Duration = Duration::from_secs(300);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const WEBSOCKET_PATH: &str = "/api/v1/llm/connect";

type CancellationWaiters =
    Arc<Mutex<HashMap<String, oneshot::Sender<Result<CancelJobsResponse, String>>>>>;

pub struct PreparedSubmissionInput {
    pub sequence: u32,
    pub file: tokio::fs::File,
}

pub enum SubmissionOutcome {
    Acknowledged { status: String },
    Rejected { retryable: bool, error: String },
}

enum SubmissionEvent {
    Ready {
        attempt: u32,
    },
    Acknowledged {
        attempt: u32,
        status: String,
    },
    Rejected {
        attempt: u32,
        retryable: bool,
        error: String,
    },
    ConnectionError(String),
}

#[derive(Clone, Default)]
pub struct TransportHandle {
    submission_wake: Arc<Notify>,
    cancellation_wake: Arc<Notify>,
}

impl TransportHandle {
    pub fn wake_submissions(&self) {
        self.submission_wake.notify_one();
    }

    pub async fn submission_notified(&self) {
        self.submission_wake.notified().await;
    }

    pub fn wake_cancellations(&self) {
        self.cancellation_wake.notify_one();
    }

    pub async fn cancellation_notified(&self) {
        self.cancellation_wake.notified().await;
    }
}

#[derive(Clone)]
pub struct LlmConnection {
    outbound: mpsc::Sender<Message>,
    submissions: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<SubmissionEvent>>>>,
    cancellations: CancellationWaiters,
    closed: watch::Receiver<bool>,
}

impl LlmConnection {
    pub async fn connect(
        server_address: &str,
        client_id: &str,
        api_key: &str,
        pool: DbPool,
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
        let (socket, response) = tokio_tungstenite::connect_async(request)
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
        let (mut socket_writer, mut socket_reader) = socket.split();
        let (outbound, mut outbound_receiver) = mpsc::channel(OUTBOUND_MESSAGE_CAPACITY);
        let submissions = Arc::new(Mutex::new(HashMap::new()));
        let cancellations = Arc::new(Mutex::new(HashMap::new()));
        let (closed_sender, closed) = watch::channel(false);

        let writer_closed = closed_sender.clone();
        let mut writer_shutdown = closed.clone();
        tokio::spawn(async move {
            let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
            loop {
                let message = tokio::select! {
                    changed = writer_shutdown.changed() => {
                        if changed.is_err() || *writer_shutdown.borrow() {
                            break;
                        }
                        continue;
                    }
                    message = outbound_receiver.recv() => match message {
                        Some(message) => message,
                        None => break,
                    },
                    _ = heartbeat.tick() => Message::Ping(Vec::new()),
                };
                if socket_writer.send(message).await.is_err() {
                    break;
                }
            }
            let _ = writer_closed.send(true);
        });

        let reader_outbound = outbound.clone();
        let reader_submissions = Arc::clone(&submissions);
        let reader_cancellations = Arc::clone(&cancellations);
        let reader_closed = closed_sender;
        let mut reader_shutdown = closed.clone();
        tokio::spawn(async move {
            loop {
                let message = tokio::select! {
                    changed = reader_shutdown.changed() => {
                        if changed.is_err() || *reader_shutdown.borrow() {
                            break;
                        }
                        continue;
                    }
                    message = socket_reader.next() => match message {
                        Some(message) => message,
                        None => break,
                    }
                };
                match message {
                    Ok(Message::Text(text)) => {
                        let Ok(message) = serde_json::from_str::<ServiceControlMessage>(&text)
                        else {
                            break;
                        };
                        handle_service_message(
                            message,
                            &reader_submissions,
                            &reader_cancellations,
                            &reader_outbound,
                            &pool,
                        )
                        .await;
                    }
                    Ok(Message::Ping(bytes)) => {
                        if reader_outbound.send(Message::Pong(bytes)).await.is_err() {
                            break;
                        }
                    }
                    Ok(Message::Pong(_)) => {}
                    Ok(Message::Close(_)) | Err(_) => break,
                    Ok(Message::Binary(_)) | Ok(Message::Frame(_)) => break,
                }
            }
            let _ = reader_closed.send(true);
            fail_pending(&reader_submissions, &reader_cancellations).await;
        });

        Ok(Self {
            outbound,
            submissions,
            cancellations,
            closed,
        })
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
        let (events, mut event_receiver) = mpsc::unbounded_channel();
        if self
            .submissions
            .lock()
            .await
            .insert(manifest.job_id.clone(), events)
            .is_some()
        {
            return Err("submission is already active".to_string());
        }
        let outcome = self
            .submit_inner(&manifest, inputs, &mut event_receiver)
            .await;
        self.submissions.lock().await.remove(&manifest.job_id);
        outcome
    }

    async fn submit_inner(
        &self,
        manifest: &JobManifest,
        inputs: Vec<PreparedSubmissionInput>,
        events: &mut mpsc::UnboundedReceiver<SubmissionEvent>,
    ) -> Result<SubmissionOutcome, String> {
        self.send_control(ClientControlMessage::SubmissionStart {
            manifest: manifest.clone(),
        })
        .await?;
        match receive_submission_event(events).await? {
            SubmissionEvent::Ready { attempt } if attempt == manifest.attempt => {}
            SubmissionEvent::Acknowledged { attempt, status } if attempt == manifest.attempt => {
                return Ok(SubmissionOutcome::Acknowledged { status });
            }
            SubmissionEvent::Rejected {
                attempt,
                retryable,
                error,
            } if attempt == manifest.attempt => {
                return Ok(SubmissionOutcome::Rejected { retryable, error });
            }
            SubmissionEvent::ConnectionError(error) => return Err(error),
            _ => return Err("submission response correlation does not match request".to_string()),
        }
        for mut input in inputs {
            let mut buffer = vec![0_u8; MAX_BINARY_CHUNK_BYTES];
            loop {
                let bytes_read = input
                    .file
                    .read(&mut buffer)
                    .await
                    .map_err(|error| error.to_string())?;
                if bytes_read == 0 {
                    break;
                }
                let frame =
                    encode_input_chunk(&manifest.job_id, input.sequence, &buffer[..bytes_read])?;
                self.outbound
                    .send(Message::Binary(frame))
                    .await
                    .map_err(|_| "LLM WebSocket writer closed".to_string())?;
            }
            self.send_control(ClientControlMessage::InputFinished {
                job_id: manifest.job_id.clone(),
                sequence: input.sequence,
            })
            .await?;
        }
        self.send_control(ClientControlMessage::SubmissionFinished {
            job_id: manifest.job_id.clone(),
        })
        .await?;
        match receive_submission_event(events).await? {
            SubmissionEvent::Acknowledged { attempt, status } if attempt == manifest.attempt => {
                Ok(SubmissionOutcome::Acknowledged { status })
            }
            SubmissionEvent::Rejected {
                attempt,
                retryable,
                error,
            } if attempt == manifest.attempt => {
                Ok(SubmissionOutcome::Rejected { retryable, error })
            }
            SubmissionEvent::ConnectionError(error) => Err(error),
            _ => Err("submission response correlation does not match request".to_string()),
        }
    }

    pub async fn cancel(&self, request: CancelJobsRequest) -> Result<CancelJobsResponse, String> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let (sender, receiver) = oneshot::channel();
        self.cancellations
            .lock()
            .await
            .insert(request_id.clone(), sender);
        if let Err(error) = self
            .send_control(ClientControlMessage::CancelJobs {
                request_id: request_id.clone(),
                request,
            })
            .await
        {
            self.cancellations.lock().await.remove(&request_id);
            return Err(error);
        }
        match tokio::time::timeout(ADMISSION_TIMEOUT, receiver).await {
            Ok(Ok(response)) => response,
            Ok(Err(_)) => Err("cancellation acknowledgement channel closed".to_string()),
            Err(_) => {
                self.cancellations.lock().await.remove(&request_id);
                Err("cancellation acknowledgement timed out".to_string())
            }
        }
    }

    async fn send_control(&self, message: ClientControlMessage) -> Result<(), String> {
        let text = serde_json::to_string(&message).map_err(|error| error.to_string())?;
        self.outbound
            .send(Message::Text(text))
            .await
            .map_err(|_| "LLM WebSocket writer closed".to_string())
    }
}

async fn receive_submission_event(
    events: &mut mpsc::UnboundedReceiver<SubmissionEvent>,
) -> Result<SubmissionEvent, String> {
    match tokio::time::timeout(ADMISSION_TIMEOUT, events.recv()).await {
        Ok(Some(event)) => Ok(event),
        Ok(None) => Err("submission connection closed".to_string()),
        Err(_) => Err("submission acknowledgement timed out".to_string()),
    }
}

async fn handle_service_message(
    message: ServiceControlMessage,
    submissions: &Mutex<HashMap<String, mpsc::UnboundedSender<SubmissionEvent>>>,
    cancellations: &CancellationWaiters,
    outbound: &mpsc::Sender<Message>,
    pool: &DbPool,
) {
    match message {
        ServiceControlMessage::SubmissionReady { job_id, attempt } => {
            send_submission_event(submissions, &job_id, SubmissionEvent::Ready { attempt }).await;
        }
        ServiceControlMessage::SubmissionAcknowledged {
            job_id,
            attempt,
            status,
        } => {
            if status == "queued" {
                let submission_pool = pool.clone();
                let submission_job_id = job_id.clone();
                let persisted = tokio::task::spawn_blocking(move || {
                    submission_pool
                        .get()
                        .map_err(|error| error.to_string())?
                        .execute(
                            crate::database::queries::ai_jobs::MARK_SUBMITTED,
                            rusqlite::params![submission_job_id, attempt],
                        )
                        .map_err(|error| error.to_string())
                })
                .await;
                if let Err(error) = persisted
                    .map_err(|error| error.to_string())
                    .and_then(|persisted| persisted)
                {
                    send_submission_event(
                        submissions,
                        &job_id,
                        SubmissionEvent::ConnectionError(error),
                    )
                    .await;
                    return;
                }
            }
            send_submission_event(
                submissions,
                &job_id,
                SubmissionEvent::Acknowledged { attempt, status },
            )
            .await;
        }
        ServiceControlMessage::SubmissionRejected {
            job_id,
            attempt,
            retryable,
            error,
        } => {
            send_submission_event(
                submissions,
                &job_id,
                SubmissionEvent::Rejected {
                    attempt,
                    retryable,
                    error,
                },
            )
            .await;
        }
        ServiceControlMessage::CancellationAcknowledged {
            request_id,
            response,
        } => {
            if let Some(sender) = cancellations.lock().await.remove(&request_id) {
                let _ = sender.send(Ok(response));
            }
        }
        ServiceControlMessage::CancellationRejected {
            request_id, error, ..
        } => {
            if let Some(sender) = cancellations.lock().await.remove(&request_id) {
                let _ = sender.send(Err(error));
            }
        }
        ServiceControlMessage::Result { result } => {
            let job_id = result.job_id.clone();
            let attempt = result.attempt;
            let result_pool = pool.clone();
            let result_outbound = outbound.clone();
            tokio::spawn(async move {
                let received = tokio::task::spawn_blocking(move || {
                    super::result::receive_result(&result_pool, result)
                })
                .await;
                let message = match received {
                    Ok(Ok(())) => ClientControlMessage::ResultReceived { job_id, attempt },
                    Ok(Err(error)) => ClientControlMessage::ResultReceiptRejected {
                        job_id,
                        attempt,
                        error: error.to_string(),
                    },
                    Err(error) => ClientControlMessage::ResultReceiptRejected {
                        job_id,
                        attempt,
                        error: error.to_string(),
                    },
                };
                if let Ok(text) = serde_json::to_string(&message) {
                    let _ = result_outbound.send(Message::Text(text)).await;
                }
            });
        }
    }
}

async fn send_submission_event(
    submissions: &Mutex<HashMap<String, mpsc::UnboundedSender<SubmissionEvent>>>,
    job_id: &str,
    event: SubmissionEvent,
) {
    if let Some(sender) = submissions.lock().await.get(job_id) {
        let _ = sender.send(event);
    }
}

async fn fail_pending(
    submissions: &Mutex<HashMap<String, mpsc::UnboundedSender<SubmissionEvent>>>,
    cancellations: &CancellationWaiters,
) {
    submissions.lock().await.clear();
    let pending = std::mem::take(&mut *cancellations.lock().await);
    for (_, sender) in pending {
        let _ = sender.send(Err("LLM WebSocket disconnected".to_string()));
    }
}
