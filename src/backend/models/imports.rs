use serde::Serialize;

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
