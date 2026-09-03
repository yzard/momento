use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const FILE_OPERATION_LIST_LIMIT_MAX: u16 = 100;

const FILE_OPERATION_STATES: [&str; 11] = [
    "prepared",
    "publishing",
    "publication_failed",
    "files_committed",
    "finalize_failed",
    "completed",
    "rollback_pending",
    "rolled_back",
    "cleanup_pending",
    "cleanup_failed",
    "cleaned",
];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileOperationListRequest {
    pub states: Vec<String>,
    pub cursor: Option<String>,
    pub limit: u16,
}

impl FileOperationListRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.states.is_empty()
            || self.states.len() > FILE_OPERATION_STATES.len()
            || self
                .states
                .iter()
                .any(|state| !FILE_OPERATION_STATES.contains(&state.as_str()))
        {
            return Err("states must contain one or more known file operation states");
        }
        if self.limit == 0 || self.limit > FILE_OPERATION_LIST_LIMIT_MAX {
            return Err("limit must be between 1 and 100");
        }
        if self
            .cursor
            .as_deref()
            .is_some_and(|cursor| validate_identifier(cursor, "operationId").is_err())
        {
            return Err("cursor must be a valid operation ID");
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileOperationGetRequest {
    pub operation_id: String,
}

impl FileOperationGetRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        validate_identifier(&self.operation_id, "operationId")
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOperationSummary {
    pub operation_id: String,
    pub kind: String,
    pub owner_kind: String,
    pub owner_id: String,
    pub state: String,
    pub product_target: Option<String>,
    pub product_version: Option<i64>,
    pub cancel_requested: bool,
    pub completion_outcome: Option<String>,
    pub finalization_error_kind: Option<String>,
    pub finalization_error: Option<String>,
    pub rollback_error_kind: Option<String>,
    pub rollback_error: Option<String>,
    pub entry_count: u16,
    pub version: i64,
    pub created_at: String,
    pub updated_at: String,
    pub terminal_at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOperationListResponse {
    pub operations: Vec<FileOperationSummary>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOperationEntryDetail {
    pub sequence: u16,
    pub action: String,
    pub storage_root: String,
    pub source_path: Option<String>,
    pub temporary_path: Option<String>,
    pub destination_path: Option<String>,
    pub tombstone_path: Option<String>,
    pub expected_size: Option<u64>,
    pub expected_sha256: Option<String>,
    pub expected_version: Option<String>,
    pub state: String,
    pub cleanup_state: String,
    pub last_error_kind: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOperationPathClaimDetail {
    pub sequence: u16,
    pub storage_root: String,
    pub relative_path: String,
    pub mode: String,
    pub scope: String,
    pub role: String,
    pub expected_version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOperationDetailResponse {
    #[serde(flatten)]
    pub summary: FileOperationSummary,
    pub detail_level: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entries: Option<Vec<FileOperationEntryDetail>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_claims: Option<Vec<FileOperationPathClaimDetail>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compacted: Option<FileOperationCompactedSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOperationCompactedSummary {
    pub entry_actions: BTreeMap<String, u16>,
    pub entry_states: BTreeMap<String, u16>,
    pub cleanup_states: BTreeMap<String, u16>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileOperationRetryRequest {
    pub retry_request_id: String,
    pub operation_id: String,
    pub expected_version: i64,
}

impl FileOperationRetryRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        validate_identifier(&self.retry_request_id, "retryRequestId")?;
        validate_identifier(&self.operation_id, "operationId")?;
        if self.expected_version < 1 {
            return Err("expectedVersion must be positive");
        }
        Ok(())
    }

    pub fn canonical_hash_input(&self) -> Result<Vec<u8>, &'static str> {
        self.validate()?;
        let capacity = 32usize
            .checked_add(self.retry_request_id.len())
            .and_then(|value| value.checked_add(self.operation_id.len()))
            .ok_or("retry request is too large")?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(capacity)
            .map_err(|_| "retry request could not be allocated")?;
        bytes.extend_from_slice(b"file-operation-retry-v1\0");
        append_field(&mut bytes, self.retry_request_id.as_bytes())?;
        append_field(&mut bytes, self.operation_id.as_bytes())?;
        bytes.extend_from_slice(&self.expected_version.to_be_bytes());
        Ok(bytes)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOperationRetryResponse {
    pub operation_id: String,
    pub state: String,
    pub version: i64,
    pub replayed: bool,
}

fn validate_identifier(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.is_empty() || value.len() > 128 || value.as_bytes().contains(&0) {
        return Err(match field {
            "retryRequestId" => "retryRequestId must contain between 1 and 128 bytes",
            _ => "operationId must contain between 1 and 128 bytes",
        });
    }
    Ok(())
}

fn append_field(target: &mut Vec<u8>, value: &[u8]) -> Result<(), &'static str> {
    let length = u32::try_from(value.len()).map_err(|_| "retry request field is too large")?;
    target.extend_from_slice(&length.to_be_bytes());
    target.extend_from_slice(value);
    Ok(())
}
