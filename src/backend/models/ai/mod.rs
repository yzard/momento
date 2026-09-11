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
    pub schedules: Vec<AiFeatureScheduleResponse>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AiScheduleUpdateRequest {
    pub feature: String,
    pub cron_expression: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiFeatureScheduleResponse {
    pub feature: String,
    pub cron_expression: String,
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
    pub faces: Vec<FaceDetectionResponse>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RejectFacesRequest {
    pub request_id: String,
    pub group_ids: Vec<i64>,
    pub face_group_id: Option<i64>,
    pub face_ids: Vec<i64>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RejectFacesResponse {
    pub rejected_count: usize,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceDetectionResponse {
    pub face_id: i64,
    pub media_id: i64,
    pub input_sequence: i64,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
