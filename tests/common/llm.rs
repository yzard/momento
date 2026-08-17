use momento_common::llm::{
    decode_input_chunk, encode_input_chunk, is_valid_client_id, CancelJobsRequest,
    CancelJobsResponse, ClientControlMessage, JobInputDescriptor, JobManifest,
};

#[test]
fn cancellation_wire_contract_uses_camel_case() {
    let request = CancelJobsRequest {
        all: false,
        tasks: vec!["ocr".to_string()],
        job_ids: vec!["abcdef12".to_string()],
    };
    let response = CancelJobsResponse {
        requested_jobs: 6,
        cancelled_jobs: 1,
        running_jobs: 2,
        missing_jobs: 3,
    };

    assert_eq!(
        serde_json::to_value(request).expect("request JSON"),
        serde_json::json!({"all": false, "tasks": ["ocr"], "jobIds": ["abcdef12"]})
    );
    assert_eq!(
        serde_json::to_value(response).expect("response JSON"),
        serde_json::json!({"requestedJobs": 6, "cancelledJobs": 1, "runningJobs": 2, "missingJobs": 3})
    );
}

#[test]
fn websocket_control_contract_uses_tagged_camel_case() {
    let message = ClientControlMessage::SubmissionStart {
        manifest: JobManifest {
            job_id: "abcdef12".to_string(),
            media_id: 42,
            task: "ocr".to_string(),
            attempt: 3,
            inputs: vec![JobInputDescriptor {
                sequence: 0,
                filename: "input.jpg".to_string(),
                mime_type: "image/jpeg".to_string(),
                byte_size: 3,
                content_hash: "a".repeat(64),
                input_kind: "image".to_string(),
                frame_timestamp_ms: None,
            }],
        },
    };

    let json = serde_json::to_value(&message).expect("control JSON");
    assert_eq!(json["type"], "submissionStart");
    assert_eq!(json["manifest"]["jobId"], "abcdef12");
    assert_eq!(json["manifest"]["inputs"][0]["mimeType"], "image/jpeg");
    assert_eq!(
        serde_json::from_value::<ClientControlMessage>(json).expect("control message"),
        message
    );
}

#[test]
fn binary_input_chunks_round_trip() {
    let frame = encode_input_chunk("abcdef12", 7, b"payload").expect("encoded frame");
    let (job_id, sequence, payload) = decode_input_chunk(&frame).expect("decoded frame");

    assert_eq!(job_id, "abcdef12");
    assert_eq!(sequence, 7);
    assert_eq!(payload, b"payload");
}

#[test]
fn binary_input_chunks_reject_invalid_bounds() {
    assert!(encode_input_chunk("job", 0, &[]).is_err());
    assert!(encode_input_chunk("job", 0, &vec![0; 64 * 1024 + 1]).is_err());
    assert!(decode_input_chunk(&[0, 3, b'j', b'o', b'b', 0]).is_err());
}

#[test]
fn client_ids_use_safe_stable_identifiers() {
    assert!(is_valid_client_id("client_a-2"));
    assert!(!is_valid_client_id(""));
    assert!(!is_valid_client_id("client/a"));
    assert!(!is_valid_client_id(&"a".repeat(129)));
}
