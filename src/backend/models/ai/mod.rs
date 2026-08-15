use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmCallbackRequest {
    pub job_id: String,
    pub media_id: i64,
    pub task: String,
    pub attempt: i64,
    pub status: String,
    pub model_type: Option<String>,
    pub model_version: Option<String>,
    pub result: Option<Value>,
    pub input_results: Option<Vec<LlmInputResult>>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmInputResult {
    pub sequence: i64,
    pub frame_timestamp_ms: Option<i64>,
    pub result: Value,
}

#[derive(Debug, Deserialize)]
pub struct AiRequest {}

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
