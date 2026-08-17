use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::extract::ws::Message;
use momento_common::llm::{JobResult, ServiceControlMessage};
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
    async fn deliver_result(
        &self,
        client_id: &str,
        result: &JobResult,
        acknowledgement_timeout: Duration,
    ) -> Result<(), String>;
}

#[derive(Default)]
pub struct ConnectionRegistry {
    next_generation: AtomicU64,
    connections: Mutex<HashMap<String, ClientConnection>>,
    pending_results: Mutex<HashMap<PendingResultKey, oneshot::Sender<Result<(), String>>>>,
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
                let _ = response.send(Err("client disconnected".to_string()));
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

    pub async fn acknowledge_result(
        &self,
        client_id: &str,
        generation: u64,
        job_id: &str,
        attempt: u32,
        response: Result<(), String>,
    ) -> Result<(), ServiceError> {
        let key = PendingResultKey {
            client_id: client_id.to_string(),
            generation,
            job_id: job_id.to_string(),
            attempt,
        };
        let sender = self.pending_results.lock().await.remove(&key);
        let Some(sender) = sender else {
            return Ok(());
        };
        let _ = sender.send(response);
        Ok(())
    }
}

#[async_trait]
impl ResultDeliveryTransport for ConnectionRegistry {
    async fn deliver_result(
        &self,
        client_id: &str,
        result: &JobResult,
        acknowledgement_timeout: Duration,
    ) -> Result<(), String> {
        let key = PendingResultKey {
            client_id: client_id.to_string(),
            generation: 0,
            job_id: result.job_id.clone(),
            attempt: result.attempt,
        };
        let connection = self
            .connections
            .lock()
            .await
            .get(client_id)
            .cloned()
            .ok_or_else(|| format!("client is not connected: {client_id}"))?;
        let key = PendingResultKey {
            generation: connection.generation,
            ..key
        };
        let (sender, receiver) = oneshot::channel();
        let mut pending_results = self.pending_results.lock().await;
        if pending_results.contains_key(&key) {
            return Err("result is already being delivered".to_string());
        }
        pending_results.insert(key.clone(), sender);
        drop(pending_results);
        let text = serde_json::to_string(&ServiceControlMessage::Result {
            result: result.clone(),
        })
        .map_err(|error| format!("failed to serialize service message: {error}"))?;
        let (sent, sent_response) = oneshot::channel();
        if let Err(error) = connection
            .outbound
            .send(OutboundMessage {
                message: Message::Text(text),
                sent: Some(sent),
            })
            .await
        {
            self.pending_results.lock().await.remove(&key);
            return Err(format!("client connection closed: {client_id}: {error}"));
        }
        match sent_response.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                self.pending_results.lock().await.remove(&key);
                return Err(error);
            }
            Err(_) => {
                self.pending_results.lock().await.remove(&key);
                return Err("result send confirmation channel closed".to_string());
            }
        }
        match tokio::time::timeout(acknowledgement_timeout, receiver).await {
            Ok(Ok(response)) => response,
            Ok(Err(_)) => Err("result acknowledgement channel closed".to_string()),
            Err(_) => {
                self.pending_results.lock().await.remove(&key);
                Err("result acknowledgement timed out".to_string())
            }
        }
    }
}

pub type SharedConnectionRegistry = Arc<ConnectionRegistry>;
