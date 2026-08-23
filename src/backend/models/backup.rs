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
    pub device_id: String,
    pub client_asset_id: String,
    pub operation_id: String,
    pub original_filename: String,
    pub mime_type: String,
    pub byte_size: u64,
    pub source_modified_at: String,
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupUploadResponse {
    pub upload_id: String,
    pub status: String,
    pub uploaded_size: i64,
    pub expected_size: i64,
    pub media_id: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupStatusResponse {
    pub assets: Vec<BackupUploadResponse>,
}
