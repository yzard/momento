use momento_common::llm::result_payload::{
    decode_payload, encode_classification, encode_face, encode_failure, encode_image_aesthetics,
    encode_image_clustering, encode_input_started, encode_tags, encode_text, ClassificationPayload,
    DecodedResultPayload, FacePayload, FailurePayload, ImageAestheticsPayload,
    ImageClusteringPayload, InputStartedPayload, TagsPayload, TextPayload,
};
use momento_common::llm::{ResultRecordKind, IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS};

#[test]
fn every_result_payload_family_round_trips() {
    let cases = [
        (
            ResultRecordKind::Failure,
            encode_failure(&FailurePayload {
                error: "model failed".to_string(),
            })
            .expect("failure payload"),
        ),
        (
            ResultRecordKind::InputStarted,
            encode_input_started(InputStartedPayload {
                frame_timestamp_ms: Some(42),
            }),
        ),
        (
            ResultRecordKind::OcrText,
            encode_text(&TextPayload {
                text: "hello 世界".to_string(),
            })
            .expect("text payload"),
        ),
        (
            ResultRecordKind::ImageTags,
            encode_tags(&TagsPayload {
                tags: vec!["cat".to_string(), "night sky".to_string()],
            })
            .expect("tags payload"),
        ),
        (
            ResultRecordKind::ImageClustering,
            encode_image_clustering(&ImageClusteringPayload {
                embedding: vec![0.25; IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS],
                perceptual_hash: 0x0102_0304_0506_0708,
                quality_score: 0.75,
            })
            .expect("clustering payload"),
        ),
        (
            ResultRecordKind::ImageAesthetics,
            encode_image_aesthetics(ImageAestheticsPayload {
                aesthetic: 0.1,
                scenic: 0.2,
                simplicity: 0.3,
                landscape: 0.4,
                technical_quality: 0.5,
            })
            .expect("aesthetics payload"),
        ),
        (
            ResultRecordKind::Face,
            encode_face(&sample_face()).expect("face payload"),
        ),
        (
            ResultRecordKind::ScreenshotDetection,
            encode_classification(ClassificationPayload {
                detected: true,
                confidence: 0.875,
            })
            .expect("classification payload"),
        ),
        (ResultRecordKind::InputFinished, Vec::new()),
    ];

    for (kind, encoded) in cases {
        decode_payload(kind, &encoded).expect("decoded payload");
    }
}

#[test]
fn result_payloads_have_stable_little_endian_layouts() {
    assert_eq!(
        encode_input_started(InputStartedPayload {
            frame_timestamp_ms: Some(0x0102_0304_0506_0708),
        }),
        [1, 0, 0, 0, 0, 0, 0, 0, 8, 7, 6, 5, 4, 3, 2, 1,]
    );
    assert_eq!(
        encode_classification(ClassificationPayload {
            detected: true,
            confidence: 0.5,
        })
        .expect("classification payload"),
        [1, 0, 0, 0, 0, 0, 0, 63]
    );
}

#[test]
fn malformed_payloads_fail_before_unbounded_allocation() {
    assert!(decode_payload(ResultRecordKind::InputStarted, &[1; 15]).is_err());
    assert!(decode_payload(ResultRecordKind::InputFinished, &[0]).is_err());
    assert!(decode_payload(ResultRecordKind::ImageTags, &u32::MAX.to_le_bytes()).is_err());
    assert!(decode_payload(ResultRecordKind::Face, &[0; 16]).is_err());
    assert!(encode_failure(&FailurePayload {
        error: "x".repeat(4093),
    })
    .is_err());
    assert!(encode_classification(ClassificationPayload {
        detected: false,
        confidence: f32::NAN,
    })
    .is_err());
    let mut invalid_face = sample_face();
    invalid_face.embedding[0] = f32::INFINITY;
    assert!(encode_face(&invalid_face).is_err());
}

#[test]
fn decoded_payload_variant_matches_record_kind() {
    let encoded = encode_text(&TextPayload {
        text: "page one".to_string(),
    })
    .expect("text payload");
    assert_eq!(
        decode_payload(ResultRecordKind::OcrTextContinuation, &encoded)
            .expect("continuation payload"),
        DecodedResultPayload::OcrTextContinuation(TextPayload {
            text: "page one".to_string(),
        })
    );
}

fn sample_face() -> FacePayload {
    FacePayload {
        index: 3,
        x: 0.1,
        y: 0.2,
        width: 0.3,
        height: 0.4,
        eye_center_x: 0.2,
        eye_center_y: 0.3,
        confidence: 0.9,
        face_size_score: 0.8,
        frontality_score: 0.7,
        visibility_score: 0.6,
        feature_clarity_score: 0.5,
        embedding: vec![1.0 / (512.0_f32).sqrt(); 512],
    }
}
