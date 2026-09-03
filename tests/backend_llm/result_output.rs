use base64::{engine::general_purpose::STANDARD, Engine};
use llm_service::provider::{
    FaceDetection, InferenceResponse, InputInferenceResponse, NormalizedBoundingBox,
    NormalizedPoint,
};
use llm_service::result_output::{
    encode_completed_result, encode_failed_result, DurableResultOutput,
};
use momento_common::llm::result_stream::{
    ResultInputCorrelation, ResultRecordChunkDecoder, ResultRecordCollector, ResultStatus,
    ValidatedResultStream, ValidatedResultValue,
};
use momento_common::llm::{JobInputDescriptor, IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS};

fn input(sequence: u32) -> JobInputDescriptor {
    JobInputDescriptor {
        sequence,
        filename: format!("input-{sequence}.jpg"),
        mime_type: "image/jpeg".to_string(),
        byte_size: 1,
        content_hash: "0".repeat(64),
        input_kind: "image".to_string(),
        frame_timestamp_ms: Some(i64::from(sequence) * 10),
    }
}

fn response(task: &str, sequence: u32) -> InputInferenceResponse {
    InputInferenceResponse {
        sequence,
        frame_timestamp_ms: Some(i64::from(sequence) * 10),
        response: InferenceResponse {
            task: task.to_string(),
            text: String::new(),
            markdown: String::new(),
            provider: "test".to_string(),
            model_type: task.to_string(),
            model_version: "test-v1".to_string(),
            tags: Vec::new(),
            embedding: None,
            embedding_encoding: None,
            embedding_dimensions: None,
            perceptual_hash: None,
            quality_score: None,
            aesthetic_score: None,
            scenic_score: None,
            simplicity_score: None,
            landscape_score: None,
            technical_quality_score: None,
            faces: Vec::new(),
            detected: None,
            confidence: None,
        },
    }
}

fn collect_result(
    output: &DurableResultOutput,
    inputs: &[JobInputDescriptor],
) -> ValidatedResultStream {
    let correlations = inputs
        .iter()
        .map(|input| ResultInputCorrelation {
            sequence: input.sequence,
            frame_timestamp_ms: input.frame_timestamp_ms,
        })
        .collect::<Vec<_>>();
    let mut collector = ResultRecordCollector::new(
        &output.manifest.task,
        output.manifest.status,
        &correlations,
        output.manifest.record_count,
        output.manifest.byte_size,
    )
    .expect("result collector");
    let mut decoder = ResultRecordChunkDecoder::new();
    decoder
        .push(&output.records, |record| {
            collector.push(record.as_borrowed())
        })
        .expect("result records");
    decoder.finish().expect("complete record stream");
    collector.finish().expect("collected result")
}

#[test]
fn ocr_output_uses_bounded_continuation_records_and_round_trips() {
    let inputs = vec![input(7)];
    let text = "x".repeat(1024 * 1024);
    let mut inference = response("ocr", 7);
    inference.response.text = text.clone();
    let output = encode_completed_result(
        "0123456789abcdef0123456789abcdef",
        41,
        "ocr",
        2,
        &inputs,
        vec![inference],
    )
    .expect("encoded OCR result");

    assert_eq!(output.manifest.status, ResultStatus::Completed);
    assert_eq!(output.manifest.record_count, 4);
    let decoded = collect_result(&output, &inputs);
    assert_eq!(decoded.inputs[0].value, ValidatedResultValue::Ocr(text));
}

#[test]
fn failed_output_is_one_durable_failure_record() {
    let inputs = vec![input(0)];
    let output = encode_failed_result(
        "abcdefabcdefabcdefabcdefabcdefab",
        9,
        "image_tagging",
        1,
        &inputs,
        "model rejected input".to_string(),
    )
    .expect("encoded failed result");

    assert_eq!(output.manifest.status, ResultStatus::Failed);
    assert_eq!(output.manifest.record_count, 1);
    let decoded = collect_result(&output, &inputs);
    assert_eq!(decoded.status, ResultStatus::Failed);
    assert_eq!(decoded.failure.as_deref(), Some("model rejected input"));
    assert!(decoded.inputs.is_empty());
}

#[test]
fn clustering_output_preserves_exact_float32_and_hash_fields() {
    let inputs = vec![input(3)];
    let embedding = (0..IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS)
        .map(|index| index as f32 / IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS as f32)
        .collect::<Vec<_>>();
    let mut embedding_bytes = Vec::new();
    for value in &embedding {
        embedding_bytes.extend_from_slice(&value.to_le_bytes());
    }
    let mut inference = response("image_clustering", 3);
    inference.response.embedding = Some(STANDARD.encode(embedding_bytes));
    inference.response.embedding_encoding = Some("float32_le".to_string());
    inference.response.embedding_dimensions = Some(IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS);
    inference.response.perceptual_hash = Some("0123456789abcdef".to_string());
    inference.response.quality_score = Some(0.75);
    let output = encode_completed_result(
        "11111111111111111111111111111111",
        10,
        "image_clustering",
        1,
        &inputs,
        vec![inference],
    )
    .expect("encoded clustering result");

    let decoded = collect_result(&output, &inputs);
    let ValidatedResultValue::ImageClustering(result) = &decoded.inputs[0].value else {
        panic!("expected clustering result");
    };
    assert_eq!(result.perceptual_hash, 0x0123_4567_89ab_cdef);
    assert_eq!(result.quality_score, 0.75);
}

#[test]
fn face_output_supports_multiple_faces_and_an_empty_input() {
    let inputs = vec![input(0), input(1)];
    let mut first = response("face_detection", 0);
    first.response.faces.push(FaceDetection {
        index: 0,
        bounding_box: NormalizedBoundingBox {
            x: 0.1,
            y: 0.2,
            width: 0.3,
            height: 0.4,
        },
        eye_center: NormalizedPoint { x: 0.2, y: 0.3 },
        confidence: 0.9,
        face_size_score: 0.8,
        frontality_score: 0.7,
        visibility_score: 0.6,
        feature_clarity_score: 0.5,
        embedding: STANDARD.encode(
            std::iter::once(1.0_f32)
                .chain(std::iter::repeat_n(0.0, 511))
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>(),
        ),
        embedding_encoding: "float32_le".to_string(),
        embedding_dimensions: 512,
    });
    let second = response("face_detection", 1);
    let output = encode_completed_result(
        "22222222222222222222222222222222",
        11,
        "face_detection",
        1,
        &inputs,
        vec![first, second],
    )
    .expect("encoded face result");

    let decoded = collect_result(&output, &inputs);
    let ValidatedResultValue::Faces(first_faces) = &decoded.inputs[0].value else {
        panic!("expected first face result");
    };
    let ValidatedResultValue::Faces(second_faces) = &decoded.inputs[1].value else {
        panic!("expected second face result");
    };
    assert_eq!(first_faces.len(), 1);
    assert!(second_faces.is_empty());
}

#[test]
fn face_output_stops_before_accumulating_more_than_two_mebibytes() {
    let inputs = vec![input(0)];
    let mut inference = response("face_detection", 0);
    let embedding = STANDARD.encode(
        std::iter::once(1.0_f32)
            .chain(std::iter::repeat_n(0.0, 511))
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    inference.response.faces = (0..1_025)
        .map(|index| FaceDetection {
            index,
            bounding_box: NormalizedBoundingBox {
                x: 0.1,
                y: 0.2,
                width: 0.3,
                height: 0.4,
            },
            eye_center: NormalizedPoint { x: 0.2, y: 0.3 },
            confidence: 0.9,
            face_size_score: 0.8,
            frontality_score: 0.7,
            visibility_score: 0.6,
            feature_clarity_score: 0.5,
            embedding: embedding.clone(),
            embedding_encoding: "float32_le".to_string(),
            embedding_dimensions: 512,
        })
        .collect();
    let error = encode_completed_result(
        "33333333333333333333333333333333",
        12,
        "face_detection",
        1,
        &inputs,
        vec![inference],
    )
    .expect_err("aggregate result heap must be bounded");
    assert!(error.contains("aggregate exceeds 2 MiB"), "{error}");
}
