use momento_common::llm::{
    decode_input_chunk, decode_result_chunk, decode_result_record, decode_result_record_header,
    decode_result_record_parts, encode_input_chunk, encode_result_chunk, encode_result_record,
    is_valid_client_id, is_valid_job_id, CancelJobsRequest, CancelJobsResponse,
    ClientControlMessage, JobInputDescriptor, JobManifest, ResultRecord, ResultRecordKind,
    ServiceControlMessage, SubmissionDeferredReason, MAX_BINARY_CHUNK_BYTES,
    MAX_CONTROL_MESSAGE_BYTES, MAX_LLM_JOB_ID_BYTES, MAX_LLM_RESULT_RECORD_PAYLOAD_BYTES,
    MAX_LLM_SERVICE_WS_MESSAGE_BYTES, MAX_MOMENTO_WS_MESSAGE_BYTES,
    MAX_NORMALIZED_RESULT_RECORD_BYTES, MAX_WS_WRITE_BUFFER_BYTES, RESULT_RECORD_HEADER_BYTES,
    RESULT_RECORD_KIND_SPECS,
};

#[path = "llm/result_payload.rs"]
mod result_payload;
#[path = "llm/result_stream.rs"]
mod result_stream;

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
    assert!(json["manifest"]["inputs"][0].get("path").is_none());
    assert!(json["manifest"]["inputs"][0].get("storageRoot").is_none());
    assert_eq!(
        serde_json::from_value::<ClientControlMessage>(json).expect("control message"),
        message
    );

    assert_eq!(
        serde_json::to_value(ClientControlMessage::ResultReceiptDeferred {
            job_id: "abcdef12".to_string(),
            attempt: 3,
            retry_after_ms: 1_000,
        })
        .expect("result-receipt-deferred JSON"),
        serde_json::json!({
            "type": "resultReceiptDeferred",
            "jobId": "abcdef12",
            "attempt": 3,
            "retryAfterMs": 1_000
        })
    );

    let ready = ServiceControlMessage::SubmissionReady {
        job_id: "abcdef12".to_string(),
        attempt: 3,
        required_input_sequences: vec![0, 2],
    };
    assert_eq!(
        serde_json::to_value(ready).expect("submission-ready JSON"),
        serde_json::json!({
            "type": "submissionReady",
            "jobId": "abcdef12",
            "attempt": 3,
            "requiredInputSequences": [0, 2]
        })
    );

    let deferred = ServiceControlMessage::SubmissionDeferred {
        job_id: "abcdef12".to_string(),
        attempt: 3,
        reason: SubmissionDeferredReason::QueueCapacity,
        required_bytes: 10,
        available_bytes: 4,
        retry_after_ms: 30_000,
    };
    assert_eq!(
        serde_json::to_value(deferred).expect("submission-deferred JSON"),
        serde_json::json!({
            "type": "submissionDeferred",
            "jobId": "abcdef12",
            "attempt": 3,
            "reason": "queueCapacity",
            "requiredBytes": 10,
            "availableBytes": 4,
            "retryAfterMs": 30_000
        })
    );
}

#[test]
fn job_manifest_enforces_shared_descriptor_and_aggregate_bounds() {
    let descriptor = JobInputDescriptor {
        sequence: 2,
        filename: "input.jpg".to_string(),
        mime_type: "image/jpeg".to_string(),
        byte_size: 32 * 1024 * 1024 * 1024,
        content_hash: "a".repeat(64),
        input_kind: "image".to_string(),
        frame_timestamp_ms: None,
    };
    let manifest = JobManifest {
        job_id: "abcdef12".to_string(),
        media_id: 42,
        task: "ocr".to_string(),
        attempt: 1,
        inputs: vec![descriptor.clone()],
    };
    manifest.validate().expect("maximum-size input");

    let mut invalid = manifest.clone();
    invalid.inputs[0].filename = "directory/input.jpg".to_string();
    assert!(invalid.validate().is_err());
    invalid = manifest.clone();
    invalid.inputs[0].input_kind = "thumbnail".to_string();
    assert!(invalid.validate().is_err());
    invalid = manifest.clone();
    invalid.inputs.push(JobInputDescriptor {
        sequence: 1,
        byte_size: 1,
        ..descriptor
    });
    assert!(invalid.validate().is_err());
    invalid = manifest;
    invalid.inputs[0].byte_size += 1;
    assert!(invalid.validate().is_err());
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
    assert!(encode_input_chunk("ab", 0, &[]).is_err());
    assert!(encode_input_chunk("ab", 0, &vec![0; 64 * 1024 + 1]).is_err());
    assert!(encode_input_chunk("not-hex", 0, b"x").is_err());
    assert!(decode_input_chunk(&[0, 3, b'j', b'o', b'b', 0]).is_err());
}

#[test]
fn client_ids_use_safe_stable_identifiers() {
    assert!(is_valid_client_id("client_a-2"));
    assert!(!is_valid_client_id(""));
    assert!(!is_valid_client_id("client/a"));
    assert!(!is_valid_client_id(&"a".repeat(129)));
}

#[test]
fn job_ids_use_the_shared_bounded_hexadecimal_contract() {
    assert!(is_valid_job_id("abcdef12"));
    assert!(is_valid_job_id(&"a".repeat(MAX_LLM_JOB_ID_BYTES)));
    assert!(!is_valid_job_id(""));
    assert!(!is_valid_job_id("job-id"));
    assert!(!is_valid_job_id(&"a".repeat(MAX_LLM_JOB_ID_BYTES + 1)));
}

#[test]
fn result_chunk_v1_matches_the_exact_golden_vector() {
    let encoded = encode_result_chunk("ab12", 5, b"xyz").expect("encoded result chunk");
    assert_eq!(
        encoded,
        [
            b'M', b'R', b'C', b'H', 1, 0, 4, 0, 5, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, b'a', b'b',
            b'1', b'2', b'x', b'y', b'z',
        ]
    );
    let decoded = decode_result_chunk(&encoded).expect("decoded result chunk");
    assert_eq!(decoded.job_id, "ab12");
    assert_eq!(decoded.offset, 5);
    assert_eq!(decoded.payload, b"xyz");
}

#[test]
fn result_chunk_v1_accepts_its_exact_job_and_payload_bounds() {
    let job_id = "a".repeat(MAX_LLM_JOB_ID_BYTES);
    let payload = vec![7_u8; MAX_BINARY_CHUNK_BYTES];
    let encoded = encode_result_chunk(&job_id, u64::MAX, &payload).expect("maximum result chunk");
    let decoded = decode_result_chunk(&encoded).expect("maximum decoded result chunk");

    assert_eq!(decoded.job_id, job_id);
    assert_eq!(decoded.offset, u64::MAX);
    assert_eq!(decoded.payload, payload);
}

#[test]
fn result_chunk_v1_rejects_invalid_framing_and_bounds() {
    assert!(encode_result_chunk("", 0, b"x").is_err());
    assert!(encode_result_chunk("not-hex", 0, b"x").is_err());
    assert!(encode_result_chunk(&"a".repeat(MAX_LLM_JOB_ID_BYTES + 1), 0, b"x").is_err());
    assert!(encode_result_chunk("ab", 0, &[]).is_err());
    assert!(encode_result_chunk("ab", 0, &vec![0; MAX_BINARY_CHUNK_BYTES + 1]).is_err());

    let valid = encode_result_chunk("ab", 0, b"x").expect("valid result chunk");
    for index in 0..valid.len() {
        assert!(decode_result_chunk(&valid[..index]).is_err());
    }
    let mut invalid = valid.clone();
    invalid[0] = b'X';
    assert!(decode_result_chunk(&invalid).is_err());
    let mut invalid = valid.clone();
    invalid[4] = 2;
    assert!(decode_result_chunk(&invalid).is_err());
    let mut invalid = valid.clone();
    invalid[5] = 1;
    assert!(decode_result_chunk(&invalid).is_err());
    let mut invalid = valid.clone();
    invalid.extend_from_slice(b"trailing");
    assert!(decode_result_chunk(&invalid).is_err());
}

#[test]
fn websocket_allocation_bounds_are_source_owned_and_directional() {
    assert_eq!(MAX_MOMENTO_WS_MESSAGE_BYTES, 128 * 1024);
    assert_eq!(
        MAX_LLM_SERVICE_WS_MESSAGE_BYTES,
        MAX_CONTROL_MESSAGE_BYTES + 1024
    );
    assert_eq!(MAX_WS_WRITE_BUFFER_BYTES, 256 * 1024);
}

#[test]
fn result_record_v1_round_trips_the_exact_header_fields() {
    let encoded = encode_result_record(ResultRecord {
        kind: ResultRecordKind::OcrText,
        flags: 0x0201,
        record_sequence: 7,
        input_sequence: 9,
        payload: b"text",
    })
    .expect("encoded result record");
    assert_eq!(encoded.len(), 28);
    assert_eq!(
        &encoded[0..20],
        &[28, 0, 0, 0, 1, 3, 1, 2, 7, 0, 0, 0, 9, 0, 0, 0, 4, 0, 0, 0]
    );
    assert_eq!(&encoded[20..24], &[220, 167, 65, 217]);
    let decoded = decode_result_record(&encoded).expect("decoded result record");
    assert_eq!(decoded.kind, ResultRecordKind::OcrText);
    assert_eq!(decoded.flags, 0x0201);
    assert_eq!(decoded.record_sequence, 7);
    assert_eq!(decoded.input_sequence, 9);
    assert_eq!(decoded.payload, b"text");
}

#[test]
fn result_record_header_can_be_validated_before_payload_allocation() {
    let encoded = encode_result_record(ResultRecord {
        kind: ResultRecordKind::OcrText,
        flags: 0,
        record_sequence: 4,
        input_sequence: 7,
        payload: b"bounded text",
    })
    .expect("encoded record");
    let header = decode_result_record_header(&encoded[..RESULT_RECORD_HEADER_BYTES])
        .expect("decoded header");
    assert_eq!(header.total_length, encoded.len());
    assert_eq!(header.payload_length, b"bounded text".len());
    assert_eq!(header.kind, ResultRecordKind::OcrText);
    assert_eq!(header.record_sequence, 4);
    assert_eq!(header.input_sequence, 7);
    let decoded = decode_result_record_parts(
        &encoded[..RESULT_RECORD_HEADER_BYTES],
        &encoded[RESULT_RECORD_HEADER_BYTES..],
    )
    .expect("decoded parts");
    assert_eq!(decoded.payload, b"bounded text");

    assert!(decode_result_record_header(&encoded[..23]).is_err());
    assert!(decode_result_record_parts(
        &encoded[..RESULT_RECORD_HEADER_BYTES],
        &encoded[RESULT_RECORD_HEADER_BYTES..encoded.len() - 1],
    )
    .is_err());
}

#[test]
fn result_record_v1_accepts_empty_and_maximum_payloads() {
    for payload in [Vec::new(), vec![7_u8; MAX_LLM_RESULT_RECORD_PAYLOAD_BYTES]] {
        let encoded = encode_result_record(ResultRecord {
            kind: ResultRecordKind::OcrText,
            flags: 0,
            record_sequence: u32::MAX,
            input_sequence: u32::MAX,
            payload: &payload,
        })
        .expect("boundary result record");
        assert_eq!(decode_result_record(&encoded).unwrap().payload, payload);
    }
}

#[test]
fn result_record_v1_rejects_unknown_corrupt_and_oversized_records() {
    assert!(encode_result_record(ResultRecord {
        kind: ResultRecordKind::Face,
        flags: 0,
        record_sequence: 0,
        input_sequence: 0,
        payload: &vec![0; MAX_LLM_RESULT_RECORD_PAYLOAD_BYTES + 1],
    })
    .is_err());
    let valid = encode_result_record(ResultRecord {
        kind: ResultRecordKind::Failure,
        flags: 0,
        record_sequence: 0,
        input_sequence: 0,
        payload: b"failure",
    })
    .expect("valid result record");
    for index in 0..24 {
        assert!(decode_result_record(&valid[..index]).is_err());
    }
    let mut invalid = valid.clone();
    invalid[4] = 2;
    assert!(decode_result_record(&invalid).is_err());
    let mut invalid = valid.clone();
    invalid[5] = 255;
    assert!(decode_result_record(&invalid).is_err());
    let mut invalid = valid.clone();
    invalid[24] ^= 1;
    assert!(decode_result_record(&invalid).is_err());
    let mut invalid = valid;
    invalid.extend_from_slice(b"trailing");
    assert!(decode_result_record(&invalid).is_err());
}

#[test]
fn result_record_kind_table_is_closed_ordered_and_bounded() {
    assert_eq!(RESULT_RECORD_KIND_SPECS.len(), 12);
    for (index, spec) in RESULT_RECORD_KIND_SPECS.iter().enumerate() {
        assert_eq!(spec.kind as usize, index + 1);
        assert!(RESULT_RECORD_HEADER_BYTES + spec.maximum_encoded_payload_bytes <= 1024 * 1024);
        assert!(spec.maximum_normalized_heap_bytes <= MAX_NORMALIZED_RESULT_RECORD_BYTES);
        assert_eq!(spec.kind.spec(), spec);
    }
}
