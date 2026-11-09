use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupDeviceRegisterRequest {
    pub device_id: String,
    pub device_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupUploadCreateRequest {
    pub protocol_version: u32,
    pub device_id: String,
    pub client_asset_id: String,
    pub operation_id: String,
    pub original_filename: String,
    pub mime_type: String,
    pub byte_size: u64,
    pub content_hash: String,
    pub source_modified_at: String,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupUploadIdRequest {
    pub upload_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupDeviceRegisterResponse {
    pub registered: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupUploadResponse {
    pub upload_id: String,
    pub status: String,
    pub uploaded_size: i64,
    pub expected_size: i64,
    pub content_hash: Option<String>,
    pub media_id: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupBatchCapabilities {
    pub version: u8,
    pub max_bytes: u64,
    pub max_items: usize,
    pub max_manifest_bytes: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupBatchPrepareRequest {
    pub uploads: Vec<BackupUploadCreateRequest>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupUploadIdsRequest {
    pub upload_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupBatchManifest {
    pub version: u8,
    pub entries: Vec<BackupBatchEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupBatchEntry {
    pub upload_id: String,
    pub byte_size: u64,
    pub content_hash: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupBatchResponse {
    pub results: Vec<BackupBatchResult>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupBatchResult {
    pub key: String,
    pub upload: Option<BackupUploadResponse>,
    pub error: Option<BackupBatchError>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupBatchError {
    pub status: u16,
    pub detail: String,
}
