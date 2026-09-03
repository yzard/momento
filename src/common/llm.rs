use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod result_payload;
pub mod result_stream;

pub const WEBSOCKET_PROTOCOL: &str = "momento-llm-v1";
pub const MAX_CLIENT_ID_BYTES: usize = 128;
pub const MAX_CONTROL_MESSAGE_BYTES: usize = 1024 * 1024;
pub const MAX_BINARY_CHUNK_BYTES: usize = 64 * 1024;
pub const MAX_MOMENTO_WS_MESSAGE_BYTES: usize = 128 * 1024;
pub const MAX_LLM_SERVICE_WS_MESSAGE_BYTES: usize = MAX_CONTROL_MESSAGE_BYTES + 1024;
pub const MAX_WS_WRITE_BUFFER_BYTES: usize = 256 * 1024;
pub const MAX_LLM_JOB_ID_BYTES: usize = 128;
pub const MAX_LLM_INPUTS_PER_JOB: usize = 1024;
pub const MAX_LLM_INPUT_BYTES: u64 = 32 * 1024 * 1024 * 1024;
pub const MAX_LLM_JOB_INPUT_BYTES: u64 = 32 * 1024 * 1024 * 1024;
pub const QUEUE_CAPACITY_RETRY_AFTER_MS: u64 = 30_000;
pub const QUEUE_CAPACITY_MAX_RETRY_AFTER_MS: u64 = 24 * 60 * 60 * 1_000;
pub const MAX_LLM_INPUT_FILENAME_BYTES: usize = 255;
pub const MAX_LLM_MIME_TYPE_BYTES: usize = 127;
pub const MAX_LLM_INPUT_KIND_BYTES: usize = 63;
pub const RESULT_CHUNK_HEADER_BYTES: usize = 20;
pub const MAX_RESULT_CHUNK_MESSAGE_BYTES: usize =
    RESULT_CHUNK_HEADER_BYTES + MAX_LLM_JOB_ID_BYTES + MAX_BINARY_CHUNK_BYTES;
pub const RESULT_RECORD_HEADER_BYTES: usize = 24;
pub const MAX_LLM_RESULT_RECORD_BYTES: usize = 1024 * 1024;
pub const MAX_LLM_RESULT_RECORD_PAYLOAD_BYTES: usize =
    MAX_LLM_RESULT_RECORD_BYTES - RESULT_RECORD_HEADER_BYTES;
pub const MAX_NORMALIZED_RESULT_RECORD_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_LLM_RESULT_RECORDS: u32 = 1_000_000;
pub const MAX_LLM_RESULT_CONTINUATIONS_PER_VALUE: u8 = 4;
const RESULT_CHUNK_MAGIC: [u8; 4] = *b"MRCH";
const RESULT_CHUNK_VERSION: u8 = 1;
const RESULT_RECORD_VERSION: u8 = 1;
pub const IMAGE_CLUSTERING_MODEL_VERSION: &str = "dinov2-base";
pub const IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS: usize = 768;
pub const LLM_TASKS: [&str; 7] = [
    "ocr",
    "image_tagging",
    "image_clustering",
    "image_aesthetics",
    "face_detection",
    "screenshot_detection",
    "document_detection",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ResultRecordKind {
    Failure = 1,
    InputStarted = 2,
    OcrText = 3,
    ImageTags = 4,
    ImageClustering = 5,
    ImageAesthetics = 6,
    Face = 7,
    ScreenshotDetection = 8,
    DocumentDetection = 9,
    InputFinished = 10,
    OcrTextContinuation = 11,
    ImageTagsContinuation = 12,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResultRecordKindSpec {
    pub kind: ResultRecordKind,
    pub maximum_encoded_payload_bytes: usize,
    pub maximum_normalized_heap_bytes: usize,
}

pub const RESULT_RECORD_KIND_SPECS: [ResultRecordKindSpec; 12] = [
    ResultRecordKindSpec {
        kind: ResultRecordKind::Failure,
        maximum_encoded_payload_bytes: 4 * 1024,
        maximum_normalized_heap_bytes: 4 * 1024 - 4,
    },
    ResultRecordKindSpec {
        kind: ResultRecordKind::InputStarted,
        maximum_encoded_payload_bytes: 16,
        maximum_normalized_heap_bytes: 0,
    },
    ResultRecordKindSpec {
        kind: ResultRecordKind::OcrText,
        maximum_encoded_payload_bytes: MAX_LLM_RESULT_RECORD_PAYLOAD_BYTES,
        maximum_normalized_heap_bytes: MAX_NORMALIZED_RESULT_RECORD_BYTES,
    },
    ResultRecordKindSpec {
        kind: ResultRecordKind::ImageTags,
        maximum_encoded_payload_bytes: MAX_LLM_RESULT_RECORD_PAYLOAD_BYTES,
        maximum_normalized_heap_bytes: MAX_NORMALIZED_RESULT_RECORD_BYTES,
    },
    ResultRecordKindSpec {
        kind: ResultRecordKind::ImageClustering,
        maximum_encoded_payload_bytes: 4 + IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS * 4 + 8 + 4,
        maximum_normalized_heap_bytes: IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS * 4,
    },
    ResultRecordKindSpec {
        kind: ResultRecordKind::ImageAesthetics,
        maximum_encoded_payload_bytes: 20,
        maximum_normalized_heap_bytes: 0,
    },
    ResultRecordKindSpec {
        kind: ResultRecordKind::Face,
        maximum_encoded_payload_bytes: 8 + 11 * 4 + 512 * 4,
        maximum_normalized_heap_bytes: 512 * 4,
    },
    ResultRecordKindSpec {
        kind: ResultRecordKind::ScreenshotDetection,
        maximum_encoded_payload_bytes: 8,
        maximum_normalized_heap_bytes: 0,
    },
    ResultRecordKindSpec {
        kind: ResultRecordKind::DocumentDetection,
        maximum_encoded_payload_bytes: 8,
        maximum_normalized_heap_bytes: 0,
    },
    ResultRecordKindSpec {
        kind: ResultRecordKind::InputFinished,
        maximum_encoded_payload_bytes: 0,
        maximum_normalized_heap_bytes: 0,
    },
    ResultRecordKindSpec {
        kind: ResultRecordKind::OcrTextContinuation,
        maximum_encoded_payload_bytes: MAX_LLM_RESULT_RECORD_PAYLOAD_BYTES,
        maximum_normalized_heap_bytes: MAX_NORMALIZED_RESULT_RECORD_BYTES,
    },
    ResultRecordKindSpec {
        kind: ResultRecordKind::ImageTagsContinuation,
        maximum_encoded_payload_bytes: MAX_LLM_RESULT_RECORD_PAYLOAD_BYTES,
        maximum_normalized_heap_bytes: MAX_NORMALIZED_RESULT_RECORD_BYTES,
    },
];

const _: () = {
    let mut index = 0;
    while index < RESULT_RECORD_KIND_SPECS.len() {
        let spec = RESULT_RECORD_KIND_SPECS[index];
        assert!(spec.kind as usize == index + 1);
        assert!(
            RESULT_RECORD_HEADER_BYTES + spec.maximum_encoded_payload_bytes
                <= MAX_LLM_RESULT_RECORD_BYTES
        );
        assert!(spec.maximum_normalized_heap_bytes <= MAX_NORMALIZED_RESULT_RECORD_BYTES);
        index += 1;
    }
};

impl ResultRecordKind {
    fn from_byte(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Failure),
            2 => Some(Self::InputStarted),
            3 => Some(Self::OcrText),
            4 => Some(Self::ImageTags),
            5 => Some(Self::ImageClustering),
            6 => Some(Self::ImageAesthetics),
            7 => Some(Self::Face),
            8 => Some(Self::ScreenshotDetection),
            9 => Some(Self::DocumentDetection),
            10 => Some(Self::InputFinished),
            11 => Some(Self::OcrTextContinuation),
            12 => Some(Self::ImageTagsContinuation),
            _ => None,
        }
    }

    pub const fn spec(self) -> &'static ResultRecordKindSpec {
        &RESULT_RECORD_KIND_SPECS[self as usize - 1]
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobManifest {
    pub job_id: String,
    pub media_id: i64,
    pub task: String,
    pub attempt: u32,
    pub inputs: Vec<JobInputDescriptor>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobInputDescriptor {
    pub sequence: u32,
    pub filename: String,
    pub mime_type: String,
    pub byte_size: u64,
    pub content_hash: String,
    pub input_kind: String,
    pub frame_timestamp_ms: Option<i64>,
}

impl JobManifest {
    pub fn validate(&self) -> Result<(), String> {
        validate_job_manifest_fields(
            &self.job_id,
            self.media_id,
            &self.task,
            self.attempt,
            &self.inputs,
        )
    }
}

pub fn validate_job_manifest_fields(
    job_id: &str,
    media_id: i64,
    task: &str,
    attempt: u32,
    inputs: &[JobInputDescriptor],
) -> Result<(), String> {
    if !is_valid_job_id(job_id) {
        return Err("LLM job ID must be 1 to 128 hexadecimal ASCII characters".to_string());
    }
    if media_id <= 0 {
        return Err("LLM job media ID must be positive".to_string());
    }
    if attempt == 0 {
        return Err("LLM job attempt must be positive".to_string());
    }
    if !is_valid_llm_task(task) {
        return Err("LLM job task is unknown".to_string());
    }
    if inputs.is_empty() || inputs.len() > MAX_LLM_INPUTS_PER_JOB {
        return Err("LLM job must contain between 1 and 1024 inputs".to_string());
    }
    let mut previous_sequence = None;
    let mut total_bytes = 0_u64;
    for input in inputs {
        if previous_sequence.is_some_and(|sequence| sequence >= input.sequence) {
            return Err("LLM input sequences must be strictly ordered".to_string());
        }
        previous_sequence = Some(input.sequence);
        validate_bounded_text(
            &input.filename,
            MAX_LLM_INPUT_FILENAME_BYTES,
            "LLM input filename",
        )?;
        if input.filename.contains(['/', '\\']) || input.filename.contains('\0') {
            return Err("LLM input filename must be one path-free component".to_string());
        }
        validate_bounded_text(
            &input.mime_type,
            MAX_LLM_MIME_TYPE_BYTES,
            "LLM input MIME type",
        )?;
        if !input.mime_type.starts_with("image/") {
            return Err("LLM input MIME type must be an image".to_string());
        }
        validate_bounded_text(
            &input.input_kind,
            MAX_LLM_INPUT_KIND_BYTES,
            "LLM input kind",
        )?;
        if !matches!(input.input_kind.as_str(), "image" | "video_frame") {
            return Err("LLM input kind is unknown".to_string());
        }
        if input.byte_size == 0 || input.byte_size > MAX_LLM_INPUT_BYTES {
            return Err("LLM input byte size is outside its bound".to_string());
        }
        total_bytes = total_bytes
            .checked_add(input.byte_size)
            .ok_or_else(|| "LLM job input byte size overflowed".to_string())?;
        if total_bytes > MAX_LLM_JOB_INPUT_BYTES {
            return Err("LLM job input bytes exceed 32 GiB".to_string());
        }
        if input.content_hash.len() != 64
            || !input
                .content_hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("LLM input content hash must be SHA-256 hexadecimal".to_string());
        }
    }
    Ok(())
}

pub fn is_valid_llm_task(task: &str) -> bool {
    matches!(
        task,
        "ocr"
            | "image_tagging"
            | "image_clustering"
            | "image_aesthetics"
            | "face_detection"
            | "screenshot_detection"
            | "document_detection"
    )
}

fn validate_bounded_text(value: &str, maximum_bytes: usize, field: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > maximum_bytes {
        return Err(format!("{field} is outside its byte bound"));
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelJobsRequest {
    pub all: bool,
    pub tasks: Vec<String>,
    pub job_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelJobsResponse {
    pub requested_jobs: usize,
    pub cancelled_jobs: usize,
    pub running_jobs: usize,
    pub missing_jobs: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobResult {
    pub job_id: String,
    pub media_id: i64,
    pub task: String,
    pub attempt: u32,
    pub status: String,
    pub model_type: Option<String>,
    pub model_version: Option<String>,
    pub result: Option<Value>,
    pub input_results: Option<Vec<JobInputResult>>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobInputResult {
    pub sequence: u32,
    pub frame_timestamp_ms: Option<i64>,
    pub result: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ClientControlMessage {
    SubmissionStart {
        manifest: JobManifest,
    },
    InputFinished {
        job_id: String,
        sequence: u32,
    },
    SubmissionFinished {
        job_id: String,
    },
    CancelJobs {
        request_id: String,
        request: CancelJobsRequest,
    },
    ResultReady {
        job_id: String,
        attempt: u32,
    },
    ResultChunkReady {
        job_id: String,
        attempt: u32,
        offset: u64,
    },
    ResultReceived {
        job_id: String,
        attempt: u32,
    },
    ResultReceiptDeferred {
        job_id: String,
        attempt: u32,
        retry_after_ms: u64,
    },
    ResultReceiptRejected {
        job_id: String,
        attempt: u32,
        error: String,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ServiceControlMessage {
    SubmissionReady {
        job_id: String,
        attempt: u32,
        required_input_sequences: Vec<u32>,
    },
    SubmissionAcknowledged {
        job_id: String,
        attempt: u32,
        status: String,
    },
    SubmissionDeferred {
        job_id: String,
        attempt: u32,
        reason: SubmissionDeferredReason,
        required_bytes: u64,
        available_bytes: u64,
        retry_after_ms: u64,
    },
    SubmissionRejected {
        job_id: String,
        attempt: u32,
        retryable: bool,
        error: String,
    },
    CancellationAcknowledged {
        request_id: String,
        response: CancelJobsResponse,
    },
    CancellationRejected {
        request_id: String,
        retryable: bool,
        error: String,
    },
    ResultStart {
        manifest: result_stream::ResultManifest,
    },
    ResultFinished {
        job_id: String,
        attempt: u32,
    },
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SubmissionDeferredReason {
    QueueCapacity,
}

pub fn encode_input_chunk(job_id: &str, sequence: u32, bytes: &[u8]) -> Result<Vec<u8>, String> {
    if !is_valid_job_id(job_id) {
        return Err(
            "binary input frame job ID must be 1 to 128 hexadecimal ASCII characters".to_string(),
        );
    }
    let job_id_bytes = job_id.as_bytes();
    let job_id_length = u16::try_from(job_id_bytes.len())
        .map_err(|_| "job ID is too long for a binary input frame".to_string())?;
    if bytes.is_empty() || bytes.len() > MAX_BINARY_CHUNK_BYTES {
        return Err("binary input chunks must contain between 1 and 65536 bytes".to_string());
    }
    let mut frame = Vec::with_capacity(2 + job_id_bytes.len() + 4 + bytes.len());
    frame.extend_from_slice(&job_id_length.to_be_bytes());
    frame.extend_from_slice(job_id_bytes);
    frame.extend_from_slice(&sequence.to_be_bytes());
    frame.extend_from_slice(bytes);
    Ok(frame)
}

pub fn decode_input_chunk(frame: &[u8]) -> Result<(&str, u32, &[u8]), String> {
    if frame.len() < 7 {
        return Err("binary input frame is truncated".to_string());
    }
    let job_id_length = usize::from(u16::from_be_bytes([frame[0], frame[1]]));
    let sequence_offset = 2_usize
        .checked_add(job_id_length)
        .ok_or_else(|| "binary input frame header is invalid".to_string())?;
    let payload_offset = sequence_offset
        .checked_add(4)
        .ok_or_else(|| "binary input frame header is invalid".to_string())?;
    if frame.len() <= payload_offset {
        return Err("binary input frame is truncated".to_string());
    }
    let job_id = std::str::from_utf8(&frame[2..sequence_offset])
        .map_err(|_| "binary input frame job ID is not UTF-8".to_string())?;
    if !is_valid_job_id(job_id) {
        return Err(
            "binary input frame job ID must be 1 to 128 hexadecimal ASCII characters".to_string(),
        );
    }
    let sequence = u32::from_be_bytes(
        frame[sequence_offset..payload_offset]
            .try_into()
            .expect("validated binary frame sequence"),
    );
    let payload = &frame[payload_offset..];
    if payload.len() > MAX_BINARY_CHUNK_BYTES {
        return Err("binary input frame payload exceeds 65536 bytes".to_string());
    }
    Ok((job_id, sequence, payload))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResultChunk<'a> {
    pub job_id: &'a str,
    pub offset: u64,
    pub payload: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResultRecord<'a> {
    pub kind: ResultRecordKind,
    pub flags: u16,
    pub record_sequence: u32,
    pub input_sequence: u32,
    pub payload: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResultRecordHeader {
    pub total_length: usize,
    pub kind: ResultRecordKind,
    pub flags: u16,
    pub record_sequence: u32,
    pub input_sequence: u32,
    pub payload_length: usize,
    pub checksum: u32,
}

pub fn encode_result_record(record: ResultRecord<'_>) -> Result<Vec<u8>, String> {
    if record.payload.len() > record.kind.spec().maximum_encoded_payload_bytes {
        return Err("result record payload exceeds its kind-specific bound".to_string());
    }
    let payload_length = u32::try_from(record.payload.len())
        .map_err(|_| "result record payload length overflowed".to_string())?;
    let total_length = RESULT_RECORD_HEADER_BYTES
        .checked_add(record.payload.len())
        .ok_or_else(|| "result record total length overflowed".to_string())?;
    let total_length_u32 = u32::try_from(total_length)
        .map_err(|_| "result record total length overflowed".to_string())?;
    let mut encoded = Vec::new();
    encoded
        .try_reserve_exact(total_length)
        .map_err(|error| format!("could not reserve result record: {error}"))?;
    encoded.extend_from_slice(&total_length_u32.to_le_bytes());
    encoded.push(RESULT_RECORD_VERSION);
    encoded.push(record.kind as u8);
    encoded.extend_from_slice(&record.flags.to_le_bytes());
    encoded.extend_from_slice(&record.record_sequence.to_le_bytes());
    encoded.extend_from_slice(&record.input_sequence.to_le_bytes());
    encoded.extend_from_slice(&payload_length.to_le_bytes());
    encoded.extend_from_slice(&0_u32.to_le_bytes());
    encoded.extend_from_slice(record.payload);
    let checksum = result_record_crc32c(&encoded);
    encoded[20..24].copy_from_slice(&checksum.to_le_bytes());
    Ok(encoded)
}

pub fn decode_result_record(encoded: &[u8]) -> Result<ResultRecord<'_>, String> {
    if encoded.len() < RESULT_RECORD_HEADER_BYTES {
        return Err("result record header is truncated".to_string());
    }
    if encoded.len() > MAX_LLM_RESULT_RECORD_BYTES {
        return Err("result record exceeds 1048576 bytes".to_string());
    }
    let header = decode_result_record_header(&encoded[..RESULT_RECORD_HEADER_BYTES])?;
    if header.total_length != encoded.len() {
        return Err("result record total length does not match its bytes".to_string());
    }
    decode_result_record_parts(
        &encoded[..RESULT_RECORD_HEADER_BYTES],
        &encoded[RESULT_RECORD_HEADER_BYTES..],
    )
}

pub fn decode_result_record_header(header: &[u8]) -> Result<ResultRecordHeader, String> {
    if header.len() != RESULT_RECORD_HEADER_BYTES {
        return Err("result record header must contain exactly 24 bytes".to_string());
    }
    let total_length = usize::try_from(u32::from_le_bytes(
        header[0..4]
            .try_into()
            .expect("exact result record header length"),
    ))
    .map_err(|_| "result record total length exceeds this platform".to_string())?;
    if !(RESULT_RECORD_HEADER_BYTES..=MAX_LLM_RESULT_RECORD_BYTES).contains(&total_length) {
        return Err("result record total length is outside its bound".to_string());
    }
    if header[4] != RESULT_RECORD_VERSION {
        return Err("result record version is unsupported".to_string());
    }
    let kind = ResultRecordKind::from_byte(header[5])
        .ok_or_else(|| "result record kind is unknown".to_string())?;
    let flags = u16::from_le_bytes([header[6], header[7]]);
    let record_sequence = u32::from_le_bytes(
        header[8..12]
            .try_into()
            .expect("exact result record sequence header"),
    );
    let input_sequence = u32::from_le_bytes(
        header[12..16]
            .try_into()
            .expect("exact result input sequence header"),
    );
    let payload_length = usize::try_from(u32::from_le_bytes(
        header[16..20]
            .try_into()
            .expect("exact result record payload header"),
    ))
    .map_err(|_| "result record payload length exceeds this platform".to_string())?;
    if RESULT_RECORD_HEADER_BYTES.checked_add(payload_length) != Some(total_length) {
        return Err("result record payload length does not match its total length".to_string());
    }
    if payload_length > kind.spec().maximum_encoded_payload_bytes {
        return Err("result record payload exceeds its kind-specific bound".to_string());
    }
    let checksum = u32::from_le_bytes(
        header[20..24]
            .try_into()
            .expect("exact result record checksum header"),
    );
    Ok(ResultRecordHeader {
        total_length,
        kind,
        flags,
        record_sequence,
        input_sequence,
        payload_length,
        checksum,
    })
}

pub fn decode_result_record_parts<'a>(
    header_bytes: &[u8],
    payload: &'a [u8],
) -> Result<ResultRecord<'a>, String> {
    let header = decode_result_record_header(header_bytes)?;
    if payload.len() != header.payload_length {
        return Err("result record payload length does not match its bytes".to_string());
    }
    let checksum = crc32c::crc32c(&header_bytes[4..20]);
    if crc32c::crc32c_append(checksum, payload) != header.checksum {
        return Err("result record CRC32C does not match".to_string());
    }
    Ok(ResultRecord {
        kind: header.kind,
        flags: header.flags,
        record_sequence: header.record_sequence,
        input_sequence: header.input_sequence,
        payload,
    })
}

fn result_record_crc32c(encoded: &[u8]) -> u32 {
    let checksum = crc32c::crc32c(&encoded[4..20]);
    crc32c::crc32c_append(checksum, &encoded[RESULT_RECORD_HEADER_BYTES..])
}

pub fn encode_result_chunk(job_id: &str, offset: u64, payload: &[u8]) -> Result<Vec<u8>, String> {
    validate_result_chunk_job_id(job_id)?;
    if payload.is_empty() || payload.len() > MAX_BINARY_CHUNK_BYTES {
        return Err("result chunk payload must contain between 1 and 65536 bytes".to_string());
    }
    let job_id_length = u16::try_from(job_id.len())
        .map_err(|_| "result chunk job ID length overflowed".to_string())?;
    let payload_length = u32::try_from(payload.len())
        .map_err(|_| "result chunk payload length overflowed".to_string())?;
    let message_length = RESULT_CHUNK_HEADER_BYTES
        .checked_add(job_id.len())
        .and_then(|length| length.checked_add(payload.len()))
        .ok_or_else(|| "result chunk message length overflowed".to_string())?;
    let mut message = Vec::new();
    message
        .try_reserve_exact(message_length)
        .map_err(|error| format!("could not reserve result chunk: {error}"))?;
    message.extend_from_slice(&RESULT_CHUNK_MAGIC);
    message.push(RESULT_CHUNK_VERSION);
    message.push(0);
    message.extend_from_slice(&job_id_length.to_le_bytes());
    message.extend_from_slice(&offset.to_le_bytes());
    message.extend_from_slice(&payload_length.to_le_bytes());
    message.extend_from_slice(job_id.as_bytes());
    message.extend_from_slice(payload);
    Ok(message)
}

pub fn decode_result_chunk(message: &[u8]) -> Result<ResultChunk<'_>, String> {
    if message.len() < RESULT_CHUNK_HEADER_BYTES {
        return Err("result chunk header is truncated".to_string());
    }
    if message[..4] != RESULT_CHUNK_MAGIC {
        return Err("result chunk magic is invalid".to_string());
    }
    if message[4] != RESULT_CHUNK_VERSION {
        return Err("result chunk version is unsupported".to_string());
    }
    if message[5] != 0 {
        return Err("result chunk reserved bits are nonzero".to_string());
    }
    let job_id_length = usize::from(u16::from_le_bytes([message[6], message[7]]));
    let offset = u64::from_le_bytes(
        message[8..16]
            .try_into()
            .expect("validated result chunk offset header"),
    );
    let payload_length = usize::try_from(u32::from_le_bytes(
        message[16..20]
            .try_into()
            .expect("validated result chunk payload header"),
    ))
    .map_err(|_| "result chunk payload length exceeds this platform".to_string())?;
    if job_id_length == 0 || job_id_length > MAX_LLM_JOB_ID_BYTES {
        return Err("result chunk job ID length is invalid".to_string());
    }
    if payload_length == 0 || payload_length > MAX_BINARY_CHUNK_BYTES {
        return Err("result chunk payload length is invalid".to_string());
    }
    let payload_offset = RESULT_CHUNK_HEADER_BYTES
        .checked_add(job_id_length)
        .ok_or_else(|| "result chunk job ID offset overflowed".to_string())?;
    let expected_length = payload_offset
        .checked_add(payload_length)
        .ok_or_else(|| "result chunk message length overflowed".to_string())?;
    if message.len() != expected_length {
        return Err("result chunk length does not match its header".to_string());
    }
    let job_id = std::str::from_utf8(&message[RESULT_CHUNK_HEADER_BYTES..payload_offset])
        .map_err(|_| "result chunk job ID is not UTF-8".to_string())?;
    validate_result_chunk_job_id(job_id)?;
    Ok(ResultChunk {
        job_id,
        offset,
        payload: &message[payload_offset..],
    })
}

fn validate_result_chunk_job_id(job_id: &str) -> Result<(), String> {
    if !is_valid_job_id(job_id) {
        return Err(
            "result chunk job ID must be 1 to 128 hexadecimal ASCII characters".to_string(),
        );
    }
    Ok(())
}

pub fn is_valid_job_id(job_id: &str) -> bool {
    !job_id.is_empty()
        && job_id.len() <= MAX_LLM_JOB_ID_BYTES
        && job_id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn is_valid_client_id(client_id: &str) -> bool {
    !client_id.is_empty()
        && client_id.len() <= MAX_CLIENT_ID_BYTES
        && client_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}
