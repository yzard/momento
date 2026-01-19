use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashMediaResponse {
    pub id: i64,
    pub filename: String,
    pub original_filename: String,
    pub media_type: String,
    pub mime_type: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub file_size: Option<i64>,
    pub duration_seconds: Option<f64>,
    pub date_taken: Option<String>,
    pub deleted_at: String,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashListResponse {
    pub items: Vec<TrashMediaResponse>,
    pub total_count: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashRestoreRequest {
    pub media_ids: Vec<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashDeleteRequest {
    pub media_ids: Vec<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashResponse {
    pub message: String,
    pub affected_count: i64,
}
