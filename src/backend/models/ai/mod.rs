use serde::Deserialize;
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
