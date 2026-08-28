use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::Message;
use llm_service::transport::{ConnectionRegistry, ResultDeliveryTransport};
use momento_common::llm::{JobResult, ServiceControlMessage};

fn completed_result() -> JobResult {
    JobResult {
        job_id: "018f36e77c917cc89f7054252a33eaf0".to_string(),
        media_id: 1,
        task: "ocr".to_string(),
        attempt: 1,
        status: "completed".to_string(),
        model_type: Some("ocr".to_string()),
        model_version: Some("test".to_string()),
        result: Some(serde_json::json!({"text": "result"})),
        input_results: None,
        error: None,
    }
}

#[tokio::test]
async fn result_delivery_waits_for_matching_momento_receipt() {
    let registry = Arc::new(ConnectionRegistry::default());
    let mut connection = registry.register("client_a").await.expect("connection");
    let delivery_registry = Arc::clone(&registry);
    let result = completed_result();
    let delivery = tokio::spawn(async move {
        delivery_registry
            .deliver_result("client_a", &result, Duration::from_secs(1))
            .await
    });
    let outbound = connection.outbound.recv().await.expect("result message");
    let Message::Text(message) = outbound.message else {
        panic!("expected result text frame");
    };
    outbound
        .sent
        .expect("result send confirmation")
        .send(Ok(()))
        .expect("send confirmation receiver");
    let message = serde_json::from_str::<ServiceControlMessage>(&message).expect("result control");
    assert!(matches!(message, ServiceControlMessage::Result { .. }));
    registry
        .complete_result_delivery(
            "client_a",
            connection.generation,
            "018f36e77c917cc89f7054252a33eaf0",
            1,
            Ok(()),
        )
        .await
        .expect("Momento receipt");

    assert!(delivery.await.expect("delivery task").is_ok());
}

#[tokio::test]
async fn stale_unregister_cannot_remove_a_reconnected_client() {
    let registry = ConnectionRegistry::default();
    let old = registry.register("client_a").await.expect("old connection");
    registry.unregister("client_a", old.generation).await;
    let mut current = registry
        .register("client_a")
        .await
        .expect("current connection");

    registry.unregister("client_a", old.generation).await;
    registry
        .send(
            "client_a",
            current.generation,
            ServiceControlMessage::SubmissionReady {
                job_id: "job".to_string(),
                attempt: 1,
            },
        )
        .await
        .expect("current connection remains registered");
    assert!(current.outbound.recv().await.is_some());
}
