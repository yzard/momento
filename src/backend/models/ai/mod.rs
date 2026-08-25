use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiActionResponse {
    pub action: String,
    pub results: Vec<AiFeatureActionResult>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiFeatureActionResult {
    pub feature: String,
    pub outcome: String,
    pub affected_jobs: i64,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiJobCounts {
    pub queued: i64,
    pub submitting: i64,
    pub submitted: i64,
    pub completed: i64,
    pub failed: i64,
    pub cancelled: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiTaskStatusResponse {
    pub task: String,
    pub enabled: bool,
    pub state: String,
    pub jobs: AiJobCounts,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStatusResponse {
    pub tasks: Vec<AiTaskStatusResponse>,
    pub deduplicate: crate::models::DeduplicateStatusResponse,
    pub face_groups: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceGroupsListRequest {
    pub cursor: Option<String>,
    pub limit: Option<i64>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceGroupRequest {
    pub face_group_id: i64,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceGroupsMergeRequest {
    pub face_group_ids: Vec<i64>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceGroupResponse {
    pub face_group_id: i64,
    pub face_count: i64,
    pub media_count: i64,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceGroupsListResponse {
    pub groups: Vec<FaceGroupResponse>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceGroupMediaResponse {
    pub group: FaceGroupResponse,
    pub media: Vec<crate::models::MediaResponse>,
}
