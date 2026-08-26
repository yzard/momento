use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const WEBSOCKET_PROTOCOL: &str = "momento-llm-v1";
pub const MAX_CLIENT_ID_BYTES: usize = 128;
pub const MAX_CONTROL_MESSAGE_BYTES: usize = 1024 * 1024;
pub const MAX_BINARY_CHUNK_BYTES: usize = 64 * 1024;
pub const IMAGE_CLUSTERING_MODEL_VERSION: &str = "dinov2-base";
pub const IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS: usize = 768;

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
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
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
    ResultReceived {
        job_id: String,
        attempt: u32,
    },
    ResultReceiptRejected {
        job_id: String,
        attempt: u32,
        error: String,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum ServiceControlMessage {
    SubmissionReady {
        job_id: String,
        attempt: u32,
    },
    SubmissionAcknowledged {
        job_id: String,
        attempt: u32,
        status: String,
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
    Result {
        result: JobResult,
    },
}

pub fn encode_input_chunk(job_id: &str, sequence: u32, bytes: &[u8]) -> Result<Vec<u8>, String> {
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
    if job_id.is_empty() {
        return Err("binary input frame job ID is empty".to_string());
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

pub fn is_valid_client_id(client_id: &str) -> bool {
    !client_id.is_empty()
        && client_id.len() <= MAX_CLIENT_ID_BYTES
        && client_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}
