use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportStatusResponse {
    pub status: String,
    pub total_files: i64,
    pub processed_files: i64,
    pub successful_imports: i64,
    pub failed_imports: i64,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportTriggerResponse {
    pub message: String,
    pub status: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegenerateRequest {
    #[serde(default = "default_missing_only")]
    pub missing_only: bool,
}

fn default_missing_only() -> bool {
    true
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegenerateResponse {
    pub message: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegenerationStatusResponse {
    pub status: String,
    pub total_jobs: i64,
    pub completed_jobs: i64,
    pub metadata_jobs: i64,
    pub metadata_completed: i64,
    pub thumbnail_jobs: i64,
    pub thumbnails_completed: i64,
    pub image_text_jobs: i64,
    pub image_text_completed: i64,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub errors: Vec<String>,
}
