use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::Message;
use llm_service::result_output::encode_failed_result;
use llm_service::transport::{
    ConnectionIdentity, ConnectionRegistry, ResultDeliveryError, ResultDeliveryOutcome,
    ResultDeliveryTransport,
};
use momento_common::llm::result_stream::ResultManifest;
use momento_common::llm::{decode_result_chunk, JobInputDescriptor, ServiceControlMessage};
use tempfile::tempdir;

fn failed_result(job_id: &str) -> (ResultManifest, Vec<u8>) {
    let output = encode_failed_result(
        job_id,
        1,
        "ocr",
        1,
        &[JobInputDescriptor {
            sequence: 0,
            filename: "input.jpg".to_string(),
            mime_type: "image/jpeg".to_string(),
            byte_size: 1,
            content_hash: "0".repeat(64),
            input_kind: "image".to_string(),
            frame_timestamp_ms: None,
        }],
        "inference failed".to_string(),
    )
    .expect("failed result");
    (output.manifest, output.records)
}

#[tokio::test]
async fn result_delivery_waits_for_matching_momento_receipt() {
    let registry = Arc::new(ConnectionRegistry::default());
    let mut connection = registry.register("client_a").await.expect("connection");
    let delivery_registry = Arc::clone(&registry);
    let directory = tempdir().expect("result directory");
    let (manifest, records) = failed_result("018f36e77c917cc89f7054252a33eaf0");
    let records_path = directory.path().join("result-records.bin");
    std::fs::write(&records_path, &records).expect("result records");
    let expected_records = records;
    let delivery = tokio::spawn(async move {
        delivery_registry
            .deliver_result("client_a", &manifest, &records_path, Duration::from_secs(1))
            .await
    });
    let outbound = connection.outbound.recv().await.expect("result start");
    let Message::Text(message) = outbound.message else {
        panic!("expected result start text frame");
    };
    outbound
        .sent
        .expect("result send confirmation")
        .send(Ok(()))
        .expect("send confirmation receiver");
    let message = serde_json::from_str::<ServiceControlMessage>(&message).expect("result control");
    assert!(matches!(message, ServiceControlMessage::ResultStart { .. }));
    registry
        .result_ready(
            ConnectionIdentity {
                client_id: "client_a",
                generation: connection.generation,
            },
            "018f36e77c917cc89f7054252a33eaf0",
            1,
        )
        .await
        .expect("Momento result readiness");

    let outbound = connection.outbound.recv().await.expect("result chunk");
    let Message::Binary(message) = outbound.message else {
        panic!("expected result binary frame");
    };
    outbound
        .sent
        .expect("chunk send confirmation")
        .send(Ok(()))
        .expect("chunk confirmation receiver");
    let chunk = decode_result_chunk(&message).expect("result chunk");
    assert_eq!(chunk.offset, 0);
    assert_eq!(chunk.payload, expected_records);
    registry
        .result_chunk_ready(
            ConnectionIdentity {
                client_id: "client_a",
                generation: connection.generation,
            },
            chunk.job_id,
            1,
            chunk.payload.len() as u64,
        )
        .await
        .expect("Momento chunk credit");

    let outbound = connection.outbound.recv().await.expect("result finish");
    let Message::Text(message) = outbound.message else {
        panic!("expected result finish text frame");
    };
    outbound
        .sent
        .expect("finish send confirmation")
        .send(Ok(()))
        .expect("finish confirmation receiver");
    let message = serde_json::from_str::<ServiceControlMessage>(&message).expect("result finish");
    assert!(matches!(
        message,
        ServiceControlMessage::ResultFinished { .. }
    ));
    registry
        .complete_result_delivery(
            ConnectionIdentity {
                client_id: "client_a",
                generation: connection.generation,
            },
            "018f36e77c917cc89f7054252a33eaf0",
            1,
            ResultDeliveryOutcome::Received,
        )
        .await
        .expect("Momento receipt");

    assert_eq!(
        delivery.await.expect("delivery task").expect("delivery"),
        ResultDeliveryOutcome::Received
    );
}

#[tokio::test]
async fn disconnected_client_is_reported_as_temporarily_unavailable() {
    let registry = ConnectionRegistry::default();
    let directory = tempdir().expect("result directory");
    let (manifest, records) = failed_result("018f36e77c917cc89f7054252a33eaf1");
    let records_path = directory.path().join("result-records.bin");
    std::fs::write(&records_path, records).expect("result records");

    assert!(!registry.client_is_connected("client_a").await);
    let error = registry
        .deliver_result("client_a", &manifest, &records_path, Duration::from_secs(1))
        .await
        .expect_err("disconnected client must not start a delivery attempt");

    assert!(matches!(
        error,
        ResultDeliveryError::ClientUnavailable { .. }
    ));
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
                required_input_sequences: vec![0],
            },
        )
        .await
        .expect("current connection remains registered");
    assert!(current.outbound.recv().await.is_some());
}
