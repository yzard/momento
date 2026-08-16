use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelJobsRequest {
    pub all: bool,
    pub tasks: Vec<String>,
    pub job_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelJobsResponse {
    pub requested_jobs: usize,
    pub cancelled_jobs: usize,
    pub running_jobs: usize,
    pub missing_jobs: usize,
}
