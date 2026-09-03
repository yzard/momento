use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::extract::ws::Message;
use momento_common::llm::result_stream::ResultManifest;
use momento_common::llm::{encode_result_chunk, ServiceControlMessage, MAX_BINARY_CHUNK_BYTES};
use tokio::io::AsyncReadExt;
use tokio::sync::{mpsc, oneshot, Mutex};

use crate::error::ServiceError;

const OUTBOUND_MESSAGE_CAPACITY: usize = 256;

pub struct OutboundMessage {
    pub message: Message,
    pub sent: Option<oneshot::Sender<Result<(), String>>>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct PendingResultKey {
    client_id: String,
    generation: u64,
    job_id: String,
    attempt: u32,
}

#[derive(Clone)]
struct ClientConnection {
    generation: u64,
    outbound: mpsc::Sender<OutboundMessage>,
}

pub struct RegisteredConnection {
    pub generation: u64,
    pub outbound: mpsc::Receiver<OutboundMessage>,
}

#[async_trait]
pub trait ResultDeliveryTransport: Send + Sync {
    async fn client_is_connected(&self, client_id: &str) -> bool;

    async fn deliver_result(
        &self,
        client_id: &str,
        manifest: &ResultManifest,
        records_path: &Path,
        acknowledgement_timeout: Duration,
    ) -> Result<ResultDeliveryOutcome, ResultDeliveryError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultDeliveryOutcome {
    Received,
    Deferred { retry_after_ms: u64 },
    Rejected { error: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultDeliveryError {
    ClientUnavailable { message: String },
    AttemptFailed { message: String },
}

impl ResultDeliveryError {
    pub fn client_unavailable(message: impl Into<String>) -> Self {
        Self::ClientUnavailable {
            message: message.into(),
        }
    }

    pub fn attempt_failed(message: impl Into<String>) -> Self {
        Self::AttemptFailed {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::ClientUnavailable { message } | Self::AttemptFailed { message } => message,
        }
    }
}

enum ResultDeliveryEvent {
    Ready,
    ChunkReady { offset: u64 },
    Received,
    Deferred { retry_after_ms: u64 },
    Rejected { error: String },
    TransportError(String),
}

#[derive(Clone, Copy)]
pub struct ConnectionIdentity<'a> {
    pub client_id: &'a str,
    pub generation: u64,
}

#[derive(Default)]
pub struct ConnectionRegistry {
    next_generation: AtomicU64,
    connections: Mutex<HashMap<String, ClientConnection>>,
    pending_results: Mutex<HashMap<PendingResultKey, mpsc::Sender<ResultDeliveryEvent>>>,
}

impl ConnectionRegistry {
    pub async fn register(&self, client_id: &str) -> Result<RegisteredConnection, ServiceError> {
        let mut connections = self.connections.lock().await;
        if connections.contains_key(client_id) {
            return Err(ServiceError::Conflict(format!(
                "client ID is already connected: {client_id}"
            )));
        }
        let (outbound, receiver) = mpsc::channel(OUTBOUND_MESSAGE_CAPACITY);
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
        connections.insert(
            client_id.to_string(),
            ClientConnection {
                generation,
                outbound,
            },
        );
        Ok(RegisteredConnection {
            generation,
            outbound: receiver,
        })
    }

    pub async fn unregister(&self, client_id: &str, generation: u64) {
        let mut connections = self.connections.lock().await;
        if connections
            .get(client_id)
            .is_none_or(|connection| connection.generation != generation)
        {
            return;
        }
        connections.remove(client_id);
        let mut pending_results = self.pending_results.lock().await;
        let keys = pending_results
            .keys()
            .filter(|key| key.client_id == client_id && key.generation == generation)
            .cloned()
            .collect::<Vec<_>>();
        for key in keys {
            if let Some(response) = pending_results.remove(&key) {
                let _ = response.try_send(ResultDeliveryEvent::TransportError(
                    "client disconnected".to_string(),
                ));
            }
        }
    }

    pub async fn send(
        &self,
        client_id: &str,
        generation: u64,
        message: ServiceControlMessage,
    ) -> Result<(), String> {
        let outbound = self
            .connections
            .lock()
            .await
            .get(client_id)
            .filter(|connection| connection.generation == generation)
            .map(|connection| connection.outbound.clone())
            .ok_or_else(|| format!("client is not connected: {client_id}"))?;
        let text = serde_json::to_string(&message)
            .map_err(|error| format!("failed to serialize service message: {error}"))?;
        outbound
            .send(OutboundMessage {
                message: Message::Text(text),
                sent: None,
            })
            .await
            .map_err(|_| format!("client connection closed: {client_id}"))
    }

    pub async fn send_raw(
        &self,
        client_id: &str,
        generation: u64,
        message: Message,
    ) -> Result<(), String> {
        let outbound = self
            .connections
            .lock()
            .await
            .get(client_id)
            .filter(|connection| connection.generation == generation)
            .map(|connection| connection.outbound.clone())
            .ok_or_else(|| format!("client is not connected: {client_id}"))?;
        outbound
            .send(OutboundMessage {
                message,
                sent: None,
            })
            .await
            .map_err(|_| format!("client connection closed: {client_id}"))
    }

    async fn send_result_delivery_event(
        &self,
        client_id: &str,
        generation: u64,
        job_id: &str,
        attempt: u32,
        event: ResultDeliveryEvent,
        terminal: bool,
    ) -> Result<(), ServiceError> {
        let key = PendingResultKey {
            client_id: client_id.to_string(),
            generation,
            job_id: job_id.to_string(),
            attempt,
        };
        let sender = if terminal {
            self.pending_results.lock().await.remove(&key)
        } else {
            self.pending_results.lock().await.get(&key).cloned()
        };
        let Some(sender) = sender else {
            return Ok(());
        };
        sender.try_send(event).map_err(|error| {
            ServiceError::Conflict(format!("result delivery state is not ready: {error}"))
        })
    }

    pub async fn result_ready(
        &self,
        connection: ConnectionIdentity<'_>,
        job_id: &str,
        attempt: u32,
    ) -> Result<(), ServiceError> {
        self.send_result_delivery_event(
            connection.client_id,
            connection.generation,
            job_id,
            attempt,
            ResultDeliveryEvent::Ready,
            false,
        )
        .await
    }

    pub async fn result_chunk_ready(
        &self,
        connection: ConnectionIdentity<'_>,
        job_id: &str,
        attempt: u32,
        offset: u64,
    ) -> Result<(), ServiceError> {
        self.send_result_delivery_event(
            connection.client_id,
            connection.generation,
            job_id,
            attempt,
            ResultDeliveryEvent::ChunkReady { offset },
            false,
        )
        .await
    }

    pub async fn complete_result_delivery(
        &self,
        connection: ConnectionIdentity<'_>,
        job_id: &str,
        attempt: u32,
        outcome: ResultDeliveryOutcome,
    ) -> Result<(), ServiceError> {
        let event = match outcome {
            ResultDeliveryOutcome::Received => ResultDeliveryEvent::Received,
            ResultDeliveryOutcome::Deferred { retry_after_ms } => {
                ResultDeliveryEvent::Deferred { retry_after_ms }
            }
            ResultDeliveryOutcome::Rejected { error } => ResultDeliveryEvent::Rejected { error },
        };
        self.send_result_delivery_event(
            connection.client_id,
            connection.generation,
            job_id,
            attempt,
            event,
            true,
        )
        .await
    }
}

#[async_trait]
impl ResultDeliveryTransport for ConnectionRegistry {
    async fn client_is_connected(&self, client_id: &str) -> bool {
        self.connections.lock().await.contains_key(client_id)
    }

    async fn deliver_result(
        &self,
        client_id: &str,
        manifest: &ResultManifest,
        records_path: &Path,
        acknowledgement_timeout: Duration,
    ) -> Result<ResultDeliveryOutcome, ResultDeliveryError> {
        let key = PendingResultKey {
            client_id: client_id.to_string(),
            generation: 0,
            job_id: manifest.job_id.clone(),
            attempt: manifest.attempt,
        };
        let connection = self
            .connections
            .lock()
            .await
            .get(client_id)
            .cloned()
            .ok_or_else(|| {
                ResultDeliveryError::client_unavailable(format!(
                    "client is not connected: {client_id}"
                ))
            })?;
        let key = PendingResultKey {
            generation: connection.generation,
            ..key
        };
        let (sender, mut receiver) = mpsc::channel(1);
        let mut pending_results = self.pending_results.lock().await;
        if pending_results.contains_key(&key) {
            return Err(ResultDeliveryError::attempt_failed(
                "result is already being delivered",
            ));
        }
        pending_results.insert(key.clone(), sender);
        drop(pending_results);
        let delivery = deliver_registered_result(
            &connection,
            manifest,
            records_path,
            acknowledgement_timeout,
            &mut receiver,
        )
        .await;
        self.pending_results.lock().await.remove(&key);
        delivery
    }
}

async fn deliver_registered_result(
    connection: &ClientConnection,
    manifest: &ResultManifest,
    records_path: &Path,
    acknowledgement_timeout: Duration,
    receiver: &mut mpsc::Receiver<ResultDeliveryEvent>,
) -> Result<ResultDeliveryOutcome, ResultDeliveryError> {
    let text = serde_json::to_string(&ServiceControlMessage::ResultStart {
        manifest: manifest.clone(),
    })
    .map_err(|error| {
        ResultDeliveryError::attempt_failed(format!("failed to serialize service message: {error}"))
    })?;
    send_confirmed(connection, Message::Text(text)).await?;
    match receive_result_event(receiver, acknowledgement_timeout).await? {
        ResultDeliveryEvent::Ready => {}
        ResultDeliveryEvent::Received => return Ok(ResultDeliveryOutcome::Received),
        ResultDeliveryEvent::Deferred { retry_after_ms } => {
            return Ok(ResultDeliveryOutcome::Deferred { retry_after_ms });
        }
        ResultDeliveryEvent::Rejected { error } => {
            return Ok(ResultDeliveryOutcome::Rejected { error });
        }
        ResultDeliveryEvent::ChunkReady { .. } => {
            return Err(ResultDeliveryError::attempt_failed(
                "received result chunk credit before resultReady",
            ));
        }
        ResultDeliveryEvent::TransportError(error) => {
            return Err(ResultDeliveryError::client_unavailable(error));
        }
    }

    let mut file = tokio::fs::File::open(records_path).await.map_err(|error| {
        ResultDeliveryError::attempt_failed(format!(
            "failed to open durable result records: {error}"
        ))
    })?;
    let mut offset = 0_u64;
    let mut buffer = vec![0_u8; MAX_BINARY_CHUNK_BYTES];
    loop {
        let read = file.read(&mut buffer).await.map_err(|error| {
            ResultDeliveryError::attempt_failed(format!(
                "failed to read durable result records: {error}"
            ))
        })?;
        if read == 0 {
            break;
        }
        let frame = encode_result_chunk(&manifest.job_id, offset, &buffer[..read])
            .map_err(ResultDeliveryError::attempt_failed)?;
        send_confirmed(connection, Message::Binary(frame)).await?;
        offset = offset.checked_add(read as u64).ok_or_else(|| {
            ResultDeliveryError::attempt_failed("result delivery offset overflowed")
        })?;
        match receive_result_event(receiver, acknowledgement_timeout).await? {
            ResultDeliveryEvent::ChunkReady {
                offset: acknowledged,
            } if acknowledged == offset => {}
            ResultDeliveryEvent::Received => return Ok(ResultDeliveryOutcome::Received),
            ResultDeliveryEvent::Deferred { retry_after_ms } => {
                return Ok(ResultDeliveryOutcome::Deferred { retry_after_ms });
            }
            ResultDeliveryEvent::Rejected { error } => {
                return Ok(ResultDeliveryOutcome::Rejected { error });
            }
            ResultDeliveryEvent::TransportError(error) => {
                return Err(ResultDeliveryError::client_unavailable(error));
            }
            _ => {
                return Err(ResultDeliveryError::attempt_failed(
                    "result chunk acknowledgement is invalid",
                ));
            }
        }
    }
    if offset != manifest.byte_size {
        return Err(ResultDeliveryError::attempt_failed(
            "durable result file size changed during delivery",
        ));
    }
    let finished = serde_json::to_string(&ServiceControlMessage::ResultFinished {
        job_id: manifest.job_id.clone(),
        attempt: manifest.attempt,
    })
    .map_err(|error| {
        ResultDeliveryError::attempt_failed(format!("failed to serialize resultFinished: {error}"))
    })?;
    send_confirmed(connection, Message::Text(finished)).await?;
    let outcome = match receive_result_event(receiver, acknowledgement_timeout).await? {
        ResultDeliveryEvent::Received => ResultDeliveryOutcome::Received,
        ResultDeliveryEvent::Deferred { retry_after_ms } => {
            ResultDeliveryOutcome::Deferred { retry_after_ms }
        }
        ResultDeliveryEvent::Rejected { error } => ResultDeliveryOutcome::Rejected { error },
        ResultDeliveryEvent::TransportError(error) => {
            return Err(ResultDeliveryError::client_unavailable(error));
        }
        _ => {
            return Err(ResultDeliveryError::attempt_failed(
                "result terminal receipt is invalid",
            ));
        }
    };
    Ok(outcome)
}

async fn send_confirmed(
    connection: &ClientConnection,
    message: Message,
) -> Result<(), ResultDeliveryError> {
    let (sent, sent_response) = oneshot::channel();
    connection
        .outbound
        .send(OutboundMessage {
            message,
            sent: Some(sent),
        })
        .await
        .map_err(|error| {
            ResultDeliveryError::client_unavailable(format!("client connection closed: {error}"))
        })?;
    sent_response
        .await
        .map_err(|_| {
            ResultDeliveryError::client_unavailable("result send confirmation channel closed")
        })?
        .map_err(ResultDeliveryError::client_unavailable)
}

async fn receive_result_event(
    receiver: &mut mpsc::Receiver<ResultDeliveryEvent>,
    timeout: Duration,
) -> Result<ResultDeliveryEvent, ResultDeliveryError> {
    match tokio::time::timeout(timeout, receiver.recv()).await {
        Ok(Some(event)) => Ok(event),
        Ok(None) => Err(ResultDeliveryError::client_unavailable(
            "result receipt channel closed",
        )),
        Err(_) => Err(ResultDeliveryError::attempt_failed(
            "result receipt timed out",
        )),
    }
}

pub type SharedConnectionRegistry = Arc<ConnectionRegistry>;
