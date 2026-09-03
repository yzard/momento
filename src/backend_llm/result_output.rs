use base64::{engine::general_purpose::STANDARD, Engine};
use momento_common::llm::result_payload::{
    decode_payload, encode_classification, encode_face, encode_failure, encode_image_aesthetics,
    encode_image_clustering, encode_input_started, encode_tags, encode_text, normalized_heap_bytes,
    ClassificationPayload, FacePayload, FailurePayload, ImageAestheticsPayload,
    ImageClusteringPayload, InputStartedPayload, TagsPayload, TextPayload,
    FACE_EMBEDDING_DIMENSIONS, MAX_RESULT_TAGS_PER_RECORD, MAX_RESULT_TAG_BYTES,
    MAX_RESULT_TEXT_BYTES,
};
use momento_common::llm::result_stream::{
    ResultInputCorrelation, ResultManifest, ResultRecordStreamValidator, ResultStatus,
    RESULT_RECORDS_ENCODING,
};
use momento_common::llm::{
    decode_result_record, encode_result_record, JobInputDescriptor, ResultRecord, ResultRecordKind,
    IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS, MAX_LLM_RESULT_CONTINUATIONS_PER_VALUE,
    MAX_LLM_RESULT_RECORD_PAYLOAD_BYTES, MAX_NORMALIZED_RESULT_RECORD_BYTES,
};
use sha2::{Digest, Sha256};

use crate::provider::{FaceDetection, InferenceResponse, InputInferenceResponse};

#[derive(Debug, Clone)]
pub struct DurableResultOutput {
    pub manifest: ResultManifest,
    pub records: Vec<u8>,
}

pub fn encode_completed_result(
    job_id: &str,
    media_id: i64,
    task: &str,
    attempt: u32,
    inputs: &[JobInputDescriptor],
    responses: Vec<InputInferenceResponse>,
) -> Result<DurableResultOutput, String> {
    if responses.len() != inputs.len() || responses.is_empty() {
        return Err("inference responses do not match submitted inputs".to_string());
    }
    let first = &responses[0].response;
    if first.task != task {
        return Err("inference response task does not match its manifest".to_string());
    }
    let model_type = first.model_type.clone();
    let model_version = first.model_version.clone();
    let mut builder = RecordBuilder::new();
    for (descriptor, response) in inputs.iter().zip(responses) {
        if descriptor.sequence != response.sequence
            || descriptor.frame_timestamp_ms != response.frame_timestamp_ms
            || response.response.task != task
            || response.response.model_type != model_type
            || response.response.model_version != model_version
        {
            return Err(
                "inference response correlation or model provenance is inconsistent".to_string(),
            );
        }
        builder.push(
            ResultRecordKind::InputStarted,
            descriptor.sequence,
            encode_input_started(InputStartedPayload {
                frame_timestamp_ms: descriptor.frame_timestamp_ms,
            }),
        )?;
        encode_response_records(&mut builder, descriptor.sequence, task, &response.response)?;
        builder.push(
            ResultRecordKind::InputFinished,
            descriptor.sequence,
            Vec::new(),
        )?;
    }
    builder.finish(ResultIdentity {
        job_id,
        media_id,
        task,
        attempt,
        status: ResultStatus::Completed,
        model_type: Some(model_type),
        model_version: Some(model_version),
        inputs,
    })
}

pub fn encode_failed_result(
    job_id: &str,
    media_id: i64,
    task: &str,
    attempt: u32,
    inputs: &[JobInputDescriptor],
    error: String,
) -> Result<DurableResultOutput, String> {
    let mut builder = RecordBuilder::new();
    builder.push(
        ResultRecordKind::Failure,
        u32::MAX,
        encode_failure(&FailurePayload { error })?,
    )?;
    builder.finish(ResultIdentity {
        job_id,
        media_id,
        task,
        attempt,
        status: ResultStatus::Failed,
        model_type: None,
        model_version: None,
        inputs,
    })
}

fn encode_response_records(
    builder: &mut RecordBuilder,
    input_sequence: u32,
    task: &str,
    response: &InferenceResponse,
) -> Result<(), String> {
    match task {
        "ocr" => encode_text_records(builder, input_sequence, &response.text),
        "image_tagging" => encode_tag_records(builder, input_sequence, &response.tags),
        "image_clustering" => builder.push(
            ResultRecordKind::ImageClustering,
            input_sequence,
            encode_image_clustering(&decode_clustering(response)?)?,
        ),
        "image_aesthetics" => builder.push(
            ResultRecordKind::ImageAesthetics,
            input_sequence,
            encode_image_aesthetics(ImageAestheticsPayload {
                aesthetic: required_score(response.aesthetic_score, "aestheticScore")?,
                scenic: required_score(response.scenic_score, "scenicScore")?,
                simplicity: required_score(response.simplicity_score, "simplicityScore")?,
                landscape: required_score(response.landscape_score, "landscapeScore")?,
                technical_quality: required_score(
                    response.technical_quality_score,
                    "technicalQualityScore",
                )?,
            })?,
        ),
        "face_detection" => {
            for face in &response.faces {
                builder.push(
                    ResultRecordKind::Face,
                    input_sequence,
                    encode_face(&decode_face(face)?)?,
                )?;
            }
            Ok(())
        }
        "screenshot_detection" | "document_detection" => {
            let kind = if task == "screenshot_detection" {
                ResultRecordKind::ScreenshotDetection
            } else {
                ResultRecordKind::DocumentDetection
            };
            builder.push(
                kind,
                input_sequence,
                encode_classification(ClassificationPayload {
                    detected: response
                        .detected
                        .ok_or_else(|| format!("{task} detected is required"))?,
                    confidence: required_score(response.confidence, "confidence")?,
                })?,
            )
        }
        _ => Err("inference response task is unsupported".to_string()),
    }
}

fn encode_text_records(
    builder: &mut RecordBuilder,
    input_sequence: u32,
    text: &str,
) -> Result<(), String> {
    let chunks = split_utf8(text, MAX_RESULT_TEXT_BYTES)?;
    if chunks.len() > usize::from(MAX_LLM_RESULT_CONTINUATIONS_PER_VALUE) + 1 {
        return Err("OCR result requires too many continuation records".to_string());
    }
    for (index, chunk) in chunks.into_iter().enumerate() {
        builder.push(
            if index == 0 {
                ResultRecordKind::OcrText
            } else {
                ResultRecordKind::OcrTextContinuation
            },
            input_sequence,
            encode_text(&TextPayload {
                text: chunk.to_string(),
            })?,
        )?;
    }
    Ok(())
}

fn split_utf8(value: &str, maximum_bytes: usize) -> Result<Vec<&str>, String> {
    if value.is_empty() {
        return Ok(vec![""]);
    }
    let mut chunks = Vec::new();
    let mut remaining = value;
    while !remaining.is_empty() {
        let mut end = remaining.len().min(maximum_bytes);
        while !remaining.is_char_boundary(end) {
            end -= 1;
        }
        if end == 0 {
            return Err(
                "result text contains a code point larger than its record bound".to_string(),
            );
        }
        chunks.push(&remaining[..end]);
        remaining = &remaining[end..];
    }
    Ok(chunks)
}

fn encode_tag_records(
    builder: &mut RecordBuilder,
    input_sequence: u32,
    tags: &[String],
) -> Result<(), String> {
    if tags.is_empty() {
        return builder.push(
            ResultRecordKind::ImageTags,
            input_sequence,
            encode_tags(&TagsPayload { tags: Vec::new() })?,
        );
    }
    let mut chunks = Vec::<Vec<String>>::new();
    let mut current = Vec::new();
    let mut encoded_bytes = 4_usize;
    for tag in tags {
        if tag.len() > MAX_RESULT_TAG_BYTES {
            return Err("image tag exceeds its byte bound".to_string());
        }
        let next_bytes = encoded_bytes
            .checked_add(4 + tag.len())
            .ok_or_else(|| "image tag encoded size overflowed".to_string())?;
        if !current.is_empty()
            && (current.len() == MAX_RESULT_TAGS_PER_RECORD
                || next_bytes > MAX_LLM_RESULT_RECORD_PAYLOAD_BYTES)
        {
            chunks.push(std::mem::take(&mut current));
            encoded_bytes = 4;
        }
        encoded_bytes = encoded_bytes
            .checked_add(4 + tag.len())
            .ok_or_else(|| "image tag encoded size overflowed".to_string())?;
        current.push(tag.clone());
    }
    chunks.push(current);
    if chunks.len() > usize::from(MAX_LLM_RESULT_CONTINUATIONS_PER_VALUE) + 1 {
        return Err("image tags require too many continuation records".to_string());
    }
    for (index, tags) in chunks.into_iter().enumerate() {
        builder.push(
            if index == 0 {
                ResultRecordKind::ImageTags
            } else {
                ResultRecordKind::ImageTagsContinuation
            },
            input_sequence,
            encode_tags(&TagsPayload { tags })?,
        )?;
    }
    Ok(())
}

fn decode_clustering(response: &InferenceResponse) -> Result<ImageClusteringPayload, String> {
    if response.embedding_encoding.as_deref() != Some("float32_le")
        || response.embedding_dimensions != Some(IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS)
    {
        return Err("image clustering embedding descriptor is invalid".to_string());
    }
    let embedding = decode_embedding(
        response
            .embedding
            .as_deref()
            .ok_or_else(|| "image clustering embedding is required".to_string())?,
        IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS,
    )?;
    let perceptual_hash = u64::from_str_radix(
        response
            .perceptual_hash
            .as_deref()
            .ok_or_else(|| "image clustering perceptual hash is required".to_string())?,
        16,
    )
    .map_err(|error| format!("image clustering perceptual hash is invalid: {error}"))?;
    Ok(ImageClusteringPayload {
        embedding,
        perceptual_hash,
        quality_score: required_score(response.quality_score, "qualityScore")?,
    })
}

fn decode_face(face: &FaceDetection) -> Result<FacePayload, String> {
    if face.embedding_encoding != "float32_le"
        || face.embedding_dimensions != FACE_EMBEDDING_DIMENSIONS
    {
        return Err("face embedding descriptor is invalid".to_string());
    }
    Ok(FacePayload {
        index: u32::try_from(face.index).map_err(|_| "face index exceeds u32".to_string())?,
        x: face.bounding_box.x,
        y: face.bounding_box.y,
        width: face.bounding_box.width,
        height: face.bounding_box.height,
        eye_center_x: face.eye_center.x,
        eye_center_y: face.eye_center.y,
        confidence: face.confidence,
        face_size_score: face.face_size_score,
        frontality_score: face.frontality_score,
        visibility_score: face.visibility_score,
        feature_clarity_score: face.feature_clarity_score,
        embedding: decode_embedding(&face.embedding, FACE_EMBEDDING_DIMENSIONS)?,
    })
}

fn decode_embedding(value: &str, dimensions: usize) -> Result<Vec<f32>, String> {
    let bytes = STANDARD
        .decode(value)
        .map_err(|error| format!("embedding is not valid base64: {error}"))?;
    if bytes.len() != dimensions * 4 {
        return Err("embedding byte size does not match its dimensions".to_string());
    }
    let (chunks, remainder) = bytes.as_chunks::<4>();
    if !remainder.is_empty() {
        return Err("embedding contains a partial float32 value".to_string());
    }
    Ok(chunks
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect())
}

fn required_score(value: Option<f32>, field: &str) -> Result<f32, String> {
    value.ok_or_else(|| format!("inference response {field} is required"))
}

struct RecordBuilder {
    records: Vec<u8>,
    count: u32,
    normalized_bytes: usize,
}

struct ResultIdentity<'a> {
    job_id: &'a str,
    media_id: i64,
    task: &'a str,
    attempt: u32,
    status: ResultStatus,
    model_type: Option<String>,
    model_version: Option<String>,
    inputs: &'a [JobInputDescriptor],
}

impl RecordBuilder {
    fn new() -> Self {
        Self {
            records: Vec::new(),
            count: 0,
            normalized_bytes: 0,
        }
    }

    fn push(
        &mut self,
        kind: ResultRecordKind,
        input_sequence: u32,
        payload: Vec<u8>,
    ) -> Result<(), String> {
        let normalized_bytes = normalized_heap_bytes(&decode_payload(kind, &payload)?)?;
        self.normalized_bytes = self
            .normalized_bytes
            .checked_add(normalized_bytes)
            .ok_or_else(|| "result normalized aggregate overflowed".to_string())?;
        if self.normalized_bytes > MAX_NORMALIZED_RESULT_RECORD_BYTES {
            return Err("result normalized aggregate exceeds 2 MiB".to_string());
        }
        let encoded = encode_result_record(ResultRecord {
            kind,
            flags: 0,
            record_sequence: self.count,
            input_sequence,
            payload: &payload,
        })?;
        self.records
            .try_reserve_exact(encoded.len())
            .map_err(|error| format!("could not reserve durable result stream: {error}"))?;
        self.records.extend_from_slice(&encoded);
        self.count = self
            .count
            .checked_add(1)
            .ok_or_else(|| "result record count overflowed".to_string())?;
        Ok(())
    }

    fn finish(self, identity: ResultIdentity<'_>) -> Result<DurableResultOutput, String> {
        let manifest = ResultManifest {
            job_id: identity.job_id.to_string(),
            media_id: identity.media_id,
            task: identity.task.to_string(),
            attempt: identity.attempt,
            status: identity.status,
            model_type: identity.model_type,
            model_version: identity.model_version,
            encoding: RESULT_RECORDS_ENCODING.to_string(),
            record_count: self.count,
            byte_size: self.records.len() as u64,
            content_hash: format!("{:x}", Sha256::digest(&self.records)),
        };
        manifest.validate()?;
        let correlations = identity
            .inputs
            .iter()
            .map(|input| ResultInputCorrelation {
                sequence: input.sequence,
                frame_timestamp_ms: input.frame_timestamp_ms,
            })
            .collect::<Vec<_>>();
        let mut validator = ResultRecordStreamValidator::new(
            identity.task,
            identity.status,
            &correlations,
            self.count,
            self.records.len() as u64,
        )?;
        let mut cursor = 0;
        while cursor < self.records.len() {
            validator.push(decode_result_record_at(&self.records, &mut cursor)?)?;
        }
        validator.finish()?;
        Ok(DurableResultOutput {
            manifest,
            records: self.records,
        })
    }
}

fn decode_result_record_at<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
) -> Result<ResultRecord<'a>, String> {
    let length_bytes = bytes
        .get(*cursor..cursor.saturating_add(4))
        .ok_or_else(|| "durable result record length is truncated".to_string())?;
    let length = usize::try_from(u32::from_le_bytes(
        length_bytes.try_into().expect("four-byte record length"),
    ))
    .map_err(|_| "durable result record length exceeds this platform".to_string())?;
    let end = cursor
        .checked_add(length)
        .ok_or_else(|| "durable result record offset overflowed".to_string())?;
    let record = decode_result_record(
        bytes
            .get(*cursor..end)
            .ok_or_else(|| "durable result record is truncated".to_string())?,
    )?;
    *cursor = end;
    Ok(record)
}
