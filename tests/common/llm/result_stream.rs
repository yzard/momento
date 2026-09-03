use momento_common::llm::result_payload::{
    encode_face, encode_failure, encode_input_started, encode_text, FacePayload, FailurePayload,
    InputStartedPayload, TextPayload,
};
use momento_common::llm::result_stream::{
    ResultInputCorrelation, ResultManifest, ResultRecordChunkDecoder, ResultRecordCollector,
    ResultRecordStreamValidator, ResultStatus, ValidatedResultValue, RESULT_RECORDS_ENCODING,
};
use momento_common::llm::{encode_result_record, ResultRecord, ResultRecordKind};

#[test]
fn completed_ocr_stream_preserves_ordered_input_state() {
    let inputs = [
        ResultInputCorrelation {
            sequence: 3,
            frame_timestamp_ms: None,
        },
        ResultInputCorrelation {
            sequence: 7,
            frame_timestamp_ms: Some(1200),
        },
    ];
    let mut validator =
        ResultRecordStreamValidator::new("ocr", ResultStatus::Completed, &inputs, 7, 4096)
            .expect("validator");
    push(
        &mut validator,
        ResultRecordKind::InputStarted,
        0,
        3,
        &encode_input_started(InputStartedPayload {
            frame_timestamp_ms: None,
        }),
    );
    push_text(&mut validator, ResultRecordKind::OcrText, 1, 3, "first");
    push(&mut validator, ResultRecordKind::InputFinished, 2, 3, &[]);
    push(
        &mut validator,
        ResultRecordKind::InputStarted,
        3,
        7,
        &encode_input_started(InputStartedPayload {
            frame_timestamp_ms: Some(1200),
        }),
    );
    push_text(&mut validator, ResultRecordKind::OcrText, 4, 7, "second");
    push_text(
        &mut validator,
        ResultRecordKind::OcrTextContinuation,
        5,
        7,
        " page",
    );
    push(&mut validator, ResultRecordKind::InputFinished, 6, 7, &[]);
    validator.finish().expect("complete stream");
}

#[test]
fn collector_builds_typed_inputs_from_validated_continuations() {
    let inputs = [ResultInputCorrelation {
        sequence: 3,
        frame_timestamp_ms: Some(40),
    }];
    let mut collector = ResultRecordCollector::new("ocr", ResultStatus::Completed, &inputs, 4, 256)
        .expect("collector");
    collect(
        &mut collector,
        ResultRecordKind::InputStarted,
        0,
        3,
        &encode_input_started(InputStartedPayload {
            frame_timestamp_ms: Some(40),
        }),
    );
    collect_text(&mut collector, ResultRecordKind::OcrText, 1, 3, "first");
    collect_text(
        &mut collector,
        ResultRecordKind::OcrTextContinuation,
        2,
        3,
        " second",
    );
    collect(&mut collector, ResultRecordKind::InputFinished, 3, 3, &[]);

    let result = collector.finish().expect("collected result");
    assert_eq!(result.inputs.len(), 1);
    assert_eq!(result.inputs[0].frame_timestamp_ms, Some(40));
    assert_eq!(
        result.inputs[0].value,
        ValidatedResultValue::Ocr("first second".to_string())
    );
}

#[test]
fn face_stream_allows_an_input_with_no_detected_faces() {
    let inputs = [ResultInputCorrelation {
        sequence: 0,
        frame_timestamp_ms: None,
    }];
    let mut validator = ResultRecordStreamValidator::new(
        "face_detection",
        ResultStatus::Completed,
        &inputs,
        2,
        128,
    )
    .expect("validator");
    push(
        &mut validator,
        ResultRecordKind::InputStarted,
        0,
        0,
        &encode_input_started(InputStartedPayload {
            frame_timestamp_ms: None,
        }),
    );
    push(&mut validator, ResultRecordKind::InputFinished, 1, 0, &[]);
    validator.finish().expect("empty face result");
}

#[test]
fn face_stream_rejects_an_aggregate_embedding_heap_over_two_mebibytes() {
    let inputs = [ResultInputCorrelation {
        sequence: 0,
        frame_timestamp_ms: None,
    }];
    let mut validator = ResultRecordStreamValidator::new(
        "face_detection",
        ResultStatus::Completed,
        &inputs,
        1_027,
        3 * 1024 * 1024,
    )
    .expect("validator");
    push(
        &mut validator,
        ResultRecordKind::InputStarted,
        0,
        0,
        &encode_input_started(InputStartedPayload {
            frame_timestamp_ms: None,
        }),
    );
    let face = encode_face(&FacePayload {
        index: 0,
        x: 0.1,
        y: 0.1,
        width: 0.2,
        height: 0.2,
        eye_center_x: 0.2,
        eye_center_y: 0.2,
        confidence: 0.9,
        face_size_score: 0.8,
        frontality_score: 0.7,
        visibility_score: 0.6,
        feature_clarity_score: 0.5,
        embedding: vec![1.0 / (512.0_f32).sqrt(); 512],
    })
    .expect("face payload");
    for sequence in 1..=1_024 {
        push(&mut validator, ResultRecordKind::Face, sequence, 0, &face);
    }
    let error = validator
        .push(ResultRecord {
            kind: ResultRecordKind::Face,
            flags: 0,
            record_sequence: 1_025,
            input_sequence: 0,
            payload: &face,
        })
        .expect_err("aggregate face heap must be bounded");
    assert!(error.contains("aggregate exceeds 2 MiB"), "{error}");
}

#[test]
fn failed_stream_contains_one_unscoped_failure() {
    let inputs = [ResultInputCorrelation {
        sequence: 0,
        frame_timestamp_ms: None,
    }];
    let mut validator =
        ResultRecordStreamValidator::new("ocr", ResultStatus::Failed, &inputs, 1, 64)
            .expect("validator");
    push(
        &mut validator,
        ResultRecordKind::Failure,
        0,
        u32::MAX,
        &encode_failure(&FailurePayload {
            error: "inference failed".to_string(),
        })
        .expect("failure payload"),
    );
    validator.finish().expect("failed stream");
}

#[test]
fn stream_rejects_task_mismatch_and_excess_continuations() {
    let inputs = [ResultInputCorrelation {
        sequence: 0,
        frame_timestamp_ms: None,
    }];
    let mut mismatch =
        ResultRecordStreamValidator::new("ocr", ResultStatus::Completed, &inputs, 3, 64)
            .expect("validator");
    push(
        &mut mismatch,
        ResultRecordKind::InputStarted,
        0,
        0,
        &encode_input_started(InputStartedPayload {
            frame_timestamp_ms: None,
        }),
    );
    let text = encode_text(&TextPayload {
        text: "bad kind".to_string(),
    })
    .expect("text payload");
    assert!(mismatch
        .push(ResultRecord {
            kind: ResultRecordKind::ImageTags,
            flags: 0,
            record_sequence: 1,
            input_sequence: 0,
            payload: &text,
        })
        .is_err());

    let mut continuation =
        ResultRecordStreamValidator::new("ocr", ResultStatus::Completed, &inputs, 8, 4096)
            .expect("validator");
    push(
        &mut continuation,
        ResultRecordKind::InputStarted,
        0,
        0,
        &encode_input_started(InputStartedPayload {
            frame_timestamp_ms: None,
        }),
    );
    push_text(&mut continuation, ResultRecordKind::OcrText, 1, 0, "base");
    for sequence in 2..6 {
        push_text(
            &mut continuation,
            ResultRecordKind::OcrTextContinuation,
            sequence,
            0,
            "part",
        );
    }
    let fifth = encode_text(&TextPayload {
        text: "too many".to_string(),
    })
    .expect("text payload");
    assert!(continuation
        .push(ResultRecord {
            kind: ResultRecordKind::OcrTextContinuation,
            flags: 0,
            record_sequence: 6,
            input_sequence: 0,
            payload: &fifth,
        })
        .is_err());
}

#[test]
fn stream_rejects_invalid_manifest_and_correlation() {
    let unordered = [
        ResultInputCorrelation {
            sequence: 2,
            frame_timestamp_ms: None,
        },
        ResultInputCorrelation {
            sequence: 1,
            frame_timestamp_ms: None,
        },
    ];
    assert!(
        ResultRecordStreamValidator::new("ocr", ResultStatus::Completed, &unordered, 6, 100,)
            .is_err()
    );

    let inputs = [ResultInputCorrelation {
        sequence: 0,
        frame_timestamp_ms: Some(10),
    }];
    let mut validator = ResultRecordStreamValidator::new(
        "face_detection",
        ResultStatus::Completed,
        &inputs,
        3,
        4096,
    )
    .expect("validator");
    let wrong_timestamp = encode_input_started(InputStartedPayload {
        frame_timestamp_ms: Some(11),
    });
    assert!(validator
        .push(ResultRecord {
            kind: ResultRecordKind::InputStarted,
            flags: 0,
            record_sequence: 0,
            input_sequence: 0,
            payload: &wrong_timestamp,
        })
        .is_err());

    let invalid_face = encode_face(&FacePayload {
        index: 0,
        x: 0.9,
        y: 0.1,
        width: 0.2,
        height: 0.2,
        eye_center_x: 0.9,
        eye_center_y: 0.2,
        confidence: 0.9,
        face_size_score: 0.8,
        frontality_score: 0.7,
        visibility_score: 0.6,
        feature_clarity_score: 0.5,
        embedding: vec![1.0 / (512.0_f32).sqrt(); 512],
    });
    assert!(invalid_face.is_err());
}

#[test]
fn durable_result_manifest_has_bounded_correlation_and_model_metadata() {
    let manifest = ResultManifest {
        job_id: "abcdef12".to_string(),
        media_id: 42,
        task: "ocr".to_string(),
        attempt: 3,
        status: ResultStatus::Completed,
        model_type: Some("ocr".to_string()),
        model_version: Some("unlimited_ocr".to_string()),
        encoding: RESULT_RECORDS_ENCODING.to_string(),
        record_count: 3,
        byte_size: 128,
        content_hash: "a".repeat(64),
    };
    manifest.validate().expect("valid result manifest");
    let json = serde_json::to_value(&manifest).expect("manifest JSON");
    assert_eq!(json["status"], "completed");
    assert_eq!(json["modelVersion"], "unlimited_ocr");

    let mut invalid = manifest.clone();
    invalid.byte_size = 71;
    assert!(invalid.validate().is_err());
    invalid = manifest;
    invalid.content_hash = "not-a-hash".to_string();
    assert!(invalid.validate().is_err());
}

#[test]
fn chunk_decoder_handles_split_headers_payloads_and_multiple_records() {
    let records = [
        ResultRecord {
            kind: ResultRecordKind::OcrText,
            flags: 0,
            record_sequence: 0,
            input_sequence: 2,
            payload: b"first payload",
        },
        ResultRecord {
            kind: ResultRecordKind::InputFinished,
            flags: 0,
            record_sequence: 1,
            input_sequence: 2,
            payload: b"",
        },
    ];
    let encoded = records
        .into_iter()
        .flat_map(|record| encode_result_record(record).expect("encoded record"))
        .collect::<Vec<_>>();
    let mut decoder = ResultRecordChunkDecoder::new();
    let mut decoded = Vec::new();
    for byte in encoded.chunks(1) {
        decoder
            .push(byte, |record| {
                decoded.push(record);
                Ok(())
            })
            .expect("stream chunk");
    }
    decoder.finish().expect("complete record stream");
    assert_eq!(decoded.len(), 2);
    assert_eq!(decoded[0].payload, b"first payload");
    assert!(decoded[1].payload.is_empty());

    let mut partial = ResultRecordChunkDecoder::new();
    partial.push(&encoded[..7], |_| Ok(())).expect("prefix");
    assert!(partial.finish().is_err());
}

fn push(
    validator: &mut ResultRecordStreamValidator,
    kind: ResultRecordKind,
    record_sequence: u32,
    input_sequence: u32,
    payload: &[u8],
) {
    validator
        .push(ResultRecord {
            kind,
            flags: 0,
            record_sequence,
            input_sequence,
            payload,
        })
        .expect("accepted record");
}

fn push_text(
    validator: &mut ResultRecordStreamValidator,
    kind: ResultRecordKind,
    record_sequence: u32,
    input_sequence: u32,
    text: &str,
) {
    let payload = encode_text(&TextPayload {
        text: text.to_string(),
    })
    .expect("text payload");
    push(validator, kind, record_sequence, input_sequence, &payload);
}

fn collect(
    collector: &mut ResultRecordCollector,
    kind: ResultRecordKind,
    record_sequence: u32,
    input_sequence: u32,
    payload: &[u8],
) {
    collector
        .push(ResultRecord {
            kind,
            flags: 0,
            record_sequence,
            input_sequence,
            payload,
        })
        .expect("valid collected record");
}

fn collect_text(
    collector: &mut ResultRecordCollector,
    kind: ResultRecordKind,
    record_sequence: u32,
    input_sequence: u32,
    text: &str,
) {
    let payload = encode_text(&TextPayload {
        text: text.to_string(),
    })
    .expect("text payload");
    collect(collector, kind, record_sequence, input_sequence, &payload);
}
