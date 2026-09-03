use super::{
    ResultRecordKind, IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS, MAX_LLM_RESULT_RECORD_PAYLOAD_BYTES,
    MAX_NORMALIZED_RESULT_RECORD_BYTES,
};

const INPUT_BOUNDARY_PAYLOAD_BYTES: usize = 16;
const CLASSIFICATION_PAYLOAD_BYTES: usize = 8;
const AESTHETICS_SCORE_COUNT: usize = 5;
const FACE_SCORE_COUNT: usize = 11;
pub const FACE_EMBEDDING_DIMENSIONS: usize = 512;
const MAX_FAILURE_TEXT_BYTES: usize = 4 * 1024 - 4;
pub const MAX_RESULT_TEXT_BYTES: usize = MAX_LLM_RESULT_RECORD_PAYLOAD_BYTES - 4;
pub const MAX_RESULT_TAGS_PER_RECORD: usize = 65_536;
pub const MAX_RESULT_TAG_BYTES: usize = 4 * 1024;
const TAG_NORMALIZED_ENTRY_BYTES: usize = 24;

const _: () = assert!(std::mem::size_of::<String>() <= TAG_NORMALIZED_ENTRY_BYTES);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailurePayload {
    pub error: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputStartedPayload {
    pub frame_timestamp_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPayload {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagsPayload {
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageClusteringPayload {
    pub embedding: Vec<f32>,
    pub perceptual_hash: u64,
    pub quality_score: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageAestheticsPayload {
    pub aesthetic: f32,
    pub scenic: f32,
    pub simplicity: f32,
    pub landscape: f32,
    pub technical_quality: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FacePayload {
    pub index: u32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub eye_center_x: f32,
    pub eye_center_y: f32,
    pub confidence: f32,
    pub face_size_score: f32,
    pub frontality_score: f32,
    pub visibility_score: f32,
    pub feature_clarity_score: f32,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClassificationPayload {
    pub detected: bool,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DecodedResultPayload {
    Failure(FailurePayload),
    InputStarted(InputStartedPayload),
    OcrText(TextPayload),
    ImageTags(TagsPayload),
    ImageClustering(ImageClusteringPayload),
    ImageAesthetics(ImageAestheticsPayload),
    Face(FacePayload),
    ScreenshotDetection(ClassificationPayload),
    DocumentDetection(ClassificationPayload),
    InputFinished,
    OcrTextContinuation(TextPayload),
    ImageTagsContinuation(TagsPayload),
}

pub fn normalized_heap_bytes(payload: &DecodedResultPayload) -> Result<usize, String> {
    match payload {
        DecodedResultPayload::Failure(payload) => Ok(payload.error.len()),
        DecodedResultPayload::OcrText(payload)
        | DecodedResultPayload::OcrTextContinuation(payload) => Ok(payload.text.len()),
        DecodedResultPayload::ImageTags(payload)
        | DecodedResultPayload::ImageTagsContinuation(payload) => payload
            .tags
            .iter()
            .try_fold(0_usize, |total, tag| {
                total.checked_add(std::mem::size_of::<String>() + tag.len())
            })
            .ok_or_else(|| "result normalized aggregate overflowed".to_string()),
        DecodedResultPayload::ImageClustering(payload) => payload
            .embedding
            .len()
            .checked_mul(std::mem::size_of::<f32>())
            .ok_or_else(|| "result normalized aggregate overflowed".to_string()),
        DecodedResultPayload::Face(payload) => payload
            .embedding
            .len()
            .checked_mul(std::mem::size_of::<f32>())
            .ok_or_else(|| "result normalized aggregate overflowed".to_string()),
        DecodedResultPayload::InputStarted(_)
        | DecodedResultPayload::ImageAesthetics(_)
        | DecodedResultPayload::ScreenshotDetection(_)
        | DecodedResultPayload::DocumentDetection(_)
        | DecodedResultPayload::InputFinished => Ok(0),
    }
}

pub fn encode_failure(payload: &FailurePayload) -> Result<Vec<u8>, String> {
    if payload.error.is_empty() {
        return Err("failure error must not be empty".to_string());
    }
    encode_single_string(&payload.error, MAX_FAILURE_TEXT_BYTES, "failure error")
}

pub fn encode_input_started(payload: InputStartedPayload) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(INPUT_BOUNDARY_PAYLOAD_BYTES);
    encoded.push(u8::from(payload.frame_timestamp_ms.is_some()));
    encoded.extend_from_slice(&[0; 7]);
    encoded.extend_from_slice(&payload.frame_timestamp_ms.unwrap_or_default().to_le_bytes());
    encoded
}

pub fn encode_text(payload: &TextPayload) -> Result<Vec<u8>, String> {
    encode_single_string(&payload.text, MAX_RESULT_TEXT_BYTES, "result text")
}

pub fn encode_tags(payload: &TagsPayload) -> Result<Vec<u8>, String> {
    if payload.tags.len() > MAX_RESULT_TAGS_PER_RECORD {
        return Err("image tags exceed the 65536-tag bound".to_string());
    }
    let mut encoded = Vec::new();
    let tag_count =
        u32::try_from(payload.tags.len()).map_err(|_| "image tag count overflowed".to_string())?;
    encoded
        .try_reserve_exact(4)
        .map_err(|error| format!("could not reserve image tags: {error}"))?;
    encoded.extend_from_slice(&tag_count.to_le_bytes());
    let mut normalized_bytes = payload
        .tags
        .len()
        .checked_mul(TAG_NORMALIZED_ENTRY_BYTES)
        .ok_or_else(|| "image tag normalized size overflowed".to_string())?;
    for tag in &payload.tags {
        normalized_bytes = normalized_bytes
            .checked_add(tag.len())
            .ok_or_else(|| "image tag normalized size overflowed".to_string())?;
        if normalized_bytes > MAX_NORMALIZED_RESULT_RECORD_BYTES {
            return Err("image tags exceed the normalized result bound".to_string());
        }
        append_string(&mut encoded, tag, MAX_RESULT_TAG_BYTES, "image tag")?;
        if encoded.len() > MAX_LLM_RESULT_RECORD_PAYLOAD_BYTES {
            return Err("image tags exceed the encoded result-record bound".to_string());
        }
    }
    Ok(encoded)
}

pub fn encode_image_clustering(payload: &ImageClusteringPayload) -> Result<Vec<u8>, String> {
    if payload.embedding.len() != IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS {
        return Err("image clustering embedding must contain 768 values".to_string());
    }
    validate_finite_values(&payload.embedding, "image clustering embedding")?;
    validate_unit(payload.quality_score, "image clustering quality score")?;
    let mut encoded = Vec::with_capacity(4 + payload.embedding.len() * 4 + 8 + 4);
    encoded.extend_from_slice(&(IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS as u32).to_le_bytes());
    append_f32_values(&mut encoded, &payload.embedding);
    encoded.extend_from_slice(&payload.perceptual_hash.to_le_bytes());
    encoded.extend_from_slice(&payload.quality_score.to_le_bytes());
    Ok(encoded)
}

pub fn encode_image_aesthetics(payload: ImageAestheticsPayload) -> Result<Vec<u8>, String> {
    let values = [
        payload.aesthetic,
        payload.scenic,
        payload.simplicity,
        payload.landscape,
        payload.technical_quality,
    ];
    validate_unit_values(&values, "image aesthetics score")?;
    let mut encoded = Vec::with_capacity(AESTHETICS_SCORE_COUNT * 4);
    append_f32_values(&mut encoded, &values);
    Ok(encoded)
}

pub fn encode_face(payload: &FacePayload) -> Result<Vec<u8>, String> {
    if payload.embedding.len() != FACE_EMBEDDING_DIMENSIONS {
        return Err("face embedding must contain 512 values".to_string());
    }
    let scores = face_scores(payload);
    validate_face(payload, &scores)?;
    let mut encoded = Vec::with_capacity(8 + FACE_SCORE_COUNT * 4 + payload.embedding.len() * 4);
    encoded.extend_from_slice(&payload.index.to_le_bytes());
    encoded.extend_from_slice(&(FACE_EMBEDDING_DIMENSIONS as u32).to_le_bytes());
    append_f32_values(&mut encoded, &scores);
    append_f32_values(&mut encoded, &payload.embedding);
    Ok(encoded)
}

pub fn encode_classification(payload: ClassificationPayload) -> Result<Vec<u8>, String> {
    validate_unit(payload.confidence, "classification confidence")?;
    let mut encoded = Vec::with_capacity(CLASSIFICATION_PAYLOAD_BYTES);
    encoded.push(u8::from(payload.detected));
    encoded.extend_from_slice(&[0; 3]);
    encoded.extend_from_slice(&payload.confidence.to_le_bytes());
    Ok(encoded)
}

pub fn decode_payload(
    kind: ResultRecordKind,
    payload: &[u8],
) -> Result<DecodedResultPayload, String> {
    match kind {
        ResultRecordKind::Failure => {
            decode_single_string(payload, MAX_FAILURE_TEXT_BYTES, "failure error").and_then(
                |error| {
                    if error.is_empty() {
                        Err("failure error must not be empty".to_string())
                    } else {
                        Ok(DecodedResultPayload::Failure(FailurePayload { error }))
                    }
                },
            )
        }
        ResultRecordKind::InputStarted => {
            decode_input_started(payload).map(DecodedResultPayload::InputStarted)
        }
        ResultRecordKind::OcrText => {
            decode_single_string(payload, MAX_RESULT_TEXT_BYTES, "result text")
                .map(|text| DecodedResultPayload::OcrText(TextPayload { text }))
        }
        ResultRecordKind::ImageTags => {
            decode_tags(payload).map(|tags| DecodedResultPayload::ImageTags(TagsPayload { tags }))
        }
        ResultRecordKind::ImageClustering => {
            decode_image_clustering(payload).map(DecodedResultPayload::ImageClustering)
        }
        ResultRecordKind::ImageAesthetics => {
            decode_image_aesthetics(payload).map(DecodedResultPayload::ImageAesthetics)
        }
        ResultRecordKind::Face => decode_face(payload).map(DecodedResultPayload::Face),
        ResultRecordKind::ScreenshotDetection => {
            decode_classification(payload).map(DecodedResultPayload::ScreenshotDetection)
        }
        ResultRecordKind::DocumentDetection => {
            decode_classification(payload).map(DecodedResultPayload::DocumentDetection)
        }
        ResultRecordKind::InputFinished => {
            if payload.is_empty() {
                Ok(DecodedResultPayload::InputFinished)
            } else {
                Err("input-finished payload must be empty".to_string())
            }
        }
        ResultRecordKind::OcrTextContinuation => {
            decode_single_string(payload, MAX_RESULT_TEXT_BYTES, "result text")
                .map(|text| DecodedResultPayload::OcrTextContinuation(TextPayload { text }))
        }
        ResultRecordKind::ImageTagsContinuation => decode_tags(payload)
            .map(|tags| DecodedResultPayload::ImageTagsContinuation(TagsPayload { tags })),
    }
}

fn decode_input_started(payload: &[u8]) -> Result<InputStartedPayload, String> {
    if payload.len() != INPUT_BOUNDARY_PAYLOAD_BYTES {
        return Err("input-started payload must contain exactly 16 bytes".to_string());
    }
    if payload[1..8] != [0; 7] {
        return Err("input-started reserved bytes must be zero".to_string());
    }
    let timestamp = i64::from_le_bytes(payload[8..16].try_into().expect("validated timestamp"));
    let frame_timestamp_ms = match payload[0] {
        0 if timestamp == 0 => None,
        1 => Some(timestamp),
        0 => return Err("absent frame timestamp must encode zero".to_string()),
        _ => return Err("input-started timestamp presence flag is invalid".to_string()),
    };
    Ok(InputStartedPayload { frame_timestamp_ms })
}

fn decode_tags(payload: &[u8]) -> Result<Vec<String>, String> {
    let mut reader = PayloadReader::new(payload);
    let tag_count = usize::try_from(reader.read_u32()?)
        .map_err(|_| "image tag count exceeds this platform".to_string())?;
    if tag_count > MAX_RESULT_TAGS_PER_RECORD {
        return Err("image tags exceed the 65536-tag bound".to_string());
    }
    let mut tags = Vec::new();
    tags.try_reserve_exact(tag_count)
        .map_err(|error| format!("could not reserve image tags: {error}"))?;
    let mut normalized_bytes = tag_count
        .checked_mul(TAG_NORMALIZED_ENTRY_BYTES)
        .ok_or_else(|| "image tag normalized size overflowed".to_string())?;
    for _ in 0..tag_count {
        let tag = reader.read_string(MAX_RESULT_TAG_BYTES, "image tag")?;
        normalized_bytes = normalized_bytes
            .checked_add(tag.len())
            .ok_or_else(|| "image tag normalized size overflowed".to_string())?;
        if normalized_bytes > MAX_NORMALIZED_RESULT_RECORD_BYTES {
            return Err("image tags exceed the normalized result bound".to_string());
        }
        tags.push(tag.to_string());
    }
    reader.finish()?;
    Ok(tags)
}

fn decode_image_clustering(payload: &[u8]) -> Result<ImageClusteringPayload, String> {
    let expected_bytes = 4 + IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS * 4 + 8 + 4;
    if payload.len() != expected_bytes {
        return Err("image clustering payload length is invalid".to_string());
    }
    let mut reader = PayloadReader::new(payload);
    if reader.read_u32()? as usize != IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS {
        return Err("image clustering embedding must contain 768 values".to_string());
    }
    let embedding = reader.read_f32_values(IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS)?;
    validate_finite_values(&embedding, "image clustering embedding")?;
    let perceptual_hash = reader.read_u64()?;
    let quality_score = reader.read_f32()?;
    validate_unit(quality_score, "image clustering quality score")?;
    reader.finish()?;
    Ok(ImageClusteringPayload {
        embedding,
        perceptual_hash,
        quality_score,
    })
}

fn decode_image_aesthetics(payload: &[u8]) -> Result<ImageAestheticsPayload, String> {
    if payload.len() != AESTHETICS_SCORE_COUNT * 4 {
        return Err(
            "image aesthetics payload must contain exactly five float32 scores".to_string(),
        );
    }
    let mut reader = PayloadReader::new(payload);
    let values = reader.read_f32_values(AESTHETICS_SCORE_COUNT)?;
    validate_unit_values(&values, "image aesthetics score")?;
    reader.finish()?;
    Ok(ImageAestheticsPayload {
        aesthetic: values[0],
        scenic: values[1],
        simplicity: values[2],
        landscape: values[3],
        technical_quality: values[4],
    })
}

fn decode_face(payload: &[u8]) -> Result<FacePayload, String> {
    let expected_bytes = 8 + FACE_SCORE_COUNT * 4 + FACE_EMBEDDING_DIMENSIONS * 4;
    if payload.len() != expected_bytes {
        return Err("face payload length is invalid".to_string());
    }
    let mut reader = PayloadReader::new(payload);
    let index = reader.read_u32()?;
    if reader.read_u32()? as usize != FACE_EMBEDDING_DIMENSIONS {
        return Err("face embedding must contain 512 values".to_string());
    }
    let scores = reader.read_f32_values(FACE_SCORE_COUNT)?;
    let embedding = reader.read_f32_values(FACE_EMBEDDING_DIMENSIONS)?;
    reader.finish()?;
    let decoded = FacePayload {
        index,
        x: scores[0],
        y: scores[1],
        width: scores[2],
        height: scores[3],
        eye_center_x: scores[4],
        eye_center_y: scores[5],
        confidence: scores[6],
        face_size_score: scores[7],
        frontality_score: scores[8],
        visibility_score: scores[9],
        feature_clarity_score: scores[10],
        embedding,
    };
    validate_face(&decoded, &scores)?;
    Ok(decoded)
}

fn decode_classification(payload: &[u8]) -> Result<ClassificationPayload, String> {
    if payload.len() != CLASSIFICATION_PAYLOAD_BYTES {
        return Err("classification payload must contain exactly 8 bytes".to_string());
    }
    if payload[1..4] != [0; 3] {
        return Err("classification reserved bytes must be zero".to_string());
    }
    let detected = match payload[0] {
        0 => false,
        1 => true,
        _ => return Err("classification detected flag is invalid".to_string()),
    };
    let confidence = f32::from_le_bytes(payload[4..8].try_into().expect("validated confidence"));
    validate_unit(confidence, "classification confidence")?;
    Ok(ClassificationPayload {
        detected,
        confidence,
    })
}

fn face_scores(payload: &FacePayload) -> [f32; FACE_SCORE_COUNT] {
    [
        payload.x,
        payload.y,
        payload.width,
        payload.height,
        payload.eye_center_x,
        payload.eye_center_y,
        payload.confidence,
        payload.face_size_score,
        payload.frontality_score,
        payload.visibility_score,
        payload.feature_clarity_score,
    ]
}

fn validate_face(payload: &FacePayload, scores: &[f32]) -> Result<(), String> {
    validate_unit_values(scores, "face score")?;
    if payload.x >= 1.0
        || payload.y >= 1.0
        || payload.width <= 0.0
        || payload.height <= 0.0
        || payload.x + payload.width > 1.0 + 1e-6
        || payload.y + payload.height > 1.0 + 1e-6
    {
        return Err("face bounding box must be normalized within the input".to_string());
    }
    validate_finite_values(&payload.embedding, "face embedding")?;
    let squared_norm = payload
        .embedding
        .iter()
        .map(|value| f64::from(*value) * f64::from(*value))
        .sum::<f64>();
    if (squared_norm.sqrt() - 1.0).abs() > 0.01 {
        return Err("face embedding must be normalized".to_string());
    }
    Ok(())
}

fn encode_single_string(value: &str, maximum_bytes: usize, field: &str) -> Result<Vec<u8>, String> {
    let mut encoded = Vec::new();
    append_string(&mut encoded, value, maximum_bytes, field)?;
    Ok(encoded)
}

fn decode_single_string(
    payload: &[u8],
    maximum_bytes: usize,
    field: &str,
) -> Result<String, String> {
    let mut reader = PayloadReader::new(payload);
    let value = reader.read_string(maximum_bytes, field)?.to_string();
    reader.finish()?;
    Ok(value)
}

fn append_string(
    encoded: &mut Vec<u8>,
    value: &str,
    maximum_bytes: usize,
    field: &str,
) -> Result<(), String> {
    if value.len() > maximum_bytes {
        return Err(format!("{field} exceeds its byte bound"));
    }
    let length = u32::try_from(value.len()).map_err(|_| format!("{field} length overflowed"))?;
    encoded
        .try_reserve_exact(4 + value.len())
        .map_err(|error| format!("could not reserve {field}: {error}"))?;
    encoded.extend_from_slice(&length.to_le_bytes());
    encoded.extend_from_slice(value.as_bytes());
    Ok(())
}

fn append_f32_values(encoded: &mut Vec<u8>, values: &[f32]) {
    for value in values {
        encoded.extend_from_slice(&value.to_le_bytes());
    }
}

fn validate_finite(value: f32, field: &str) -> Result<(), String> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(format!("{field} must be finite"))
    }
}

fn validate_unit(value: f32, field: &str) -> Result<(), String> {
    validate_finite(value, field)?;
    if (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(format!("{field} must be within [0, 1]"))
    }
}

fn validate_finite_values(values: &[f32], field: &str) -> Result<(), String> {
    values
        .iter()
        .try_for_each(|value| validate_finite(*value, field))
}

fn validate_unit_values(values: &[f32], field: &str) -> Result<(), String> {
    values
        .iter()
        .try_for_each(|value| validate_unit(*value, field))
}

struct PayloadReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> PayloadReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn read_u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(
            self.read_exact(4)?
                .try_into()
                .expect("validated u32 payload"),
        ))
    }

    fn read_u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(
            self.read_exact(8)?
                .try_into()
                .expect("validated u64 payload"),
        ))
    }

    fn read_f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_le_bytes(
            self.read_exact(4)?
                .try_into()
                .expect("validated float32 payload"),
        ))
    }

    fn read_f32_values(&mut self, count: usize) -> Result<Vec<f32>, String> {
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|error| format!("could not reserve float32 payload: {error}"))?;
        for _ in 0..count {
            values.push(self.read_f32()?);
        }
        Ok(values)
    }

    fn read_string(&mut self, maximum_bytes: usize, field: &str) -> Result<&'a str, String> {
        let length = usize::try_from(self.read_u32()?)
            .map_err(|_| format!("{field} length exceeds this platform"))?;
        if length > maximum_bytes {
            return Err(format!("{field} exceeds its byte bound"));
        }
        std::str::from_utf8(self.read_exact(length)?)
            .map_err(|_| format!("{field} is not valid UTF-8"))
    }

    fn read_exact(&mut self, length: usize) -> Result<&'a [u8], String> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or_else(|| "result payload offset overflowed".to_string())?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| "result payload is truncated".to_string())?;
        self.offset = end;
        Ok(value)
    }

    fn finish(self) -> Result<(), String> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err("result payload contains trailing bytes".to_string())
        }
    }
}
