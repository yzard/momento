use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct MetadataRequest {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataActionResponse {
    pub message: String,
    pub queued_jobs: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataStatusResponse {
    pub status: String,
    pub queued_jobs: i64,
    pub processing_jobs: i64,
    pub completed_jobs: i64,
    pub failed_jobs: i64,
    pub errors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub face_groups: Option<i64>,
}
