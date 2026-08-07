use serde::{Deserialize, Serialize};

use crate::models::MediaResponse;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeduplicateGroupsRequest {
    pub cursor: Option<String>,
    pub limit: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeduplicateGroup {
    pub cluster_id: i64,
    pub items: Vec<MediaResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeduplicateGroupsResponse {
    pub groups: Vec<DeduplicateGroup>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeduplicateActionResponse {
    pub message: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeduplicateStatusResponse {
    pub status: String,
    pub run_id: Option<i64>,
    pub trigger: Option<String>,
    pub scheduled_for: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub indexed_media: i64,
    pub processed_media: i64,
    pub candidate_comparisons: i64,
    pub clusters_created: i64,
    pub error: Option<String>,
    pub next_scheduled_at: Option<String>,
}
