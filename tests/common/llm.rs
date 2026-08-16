use momento_common::llm::{CancelJobsRequest, CancelJobsResponse};

#[test]
fn cancellation_wire_contract_uses_camel_case() {
    let request = CancelJobsRequest {
        all: false,
        tasks: vec!["ocr".to_string()],
        job_ids: vec!["abcdef12".to_string()],
    };
    let response = CancelJobsResponse {
        requested_jobs: 6,
        cancelled_jobs: 1,
        running_jobs: 2,
        missing_jobs: 3,
    };

    assert_eq!(
        serde_json::to_value(request).expect("request JSON"),
        serde_json::json!({"all": false, "tasks": ["ocr"], "jobIds": ["abcdef12"]})
    );
    assert_eq!(
        serde_json::to_value(response).expect("response JSON"),
        serde_json::json!({"requestedJobs": 6, "cancelledJobs": 1, "runningJobs": 2, "missingJobs": 3})
    );
}
