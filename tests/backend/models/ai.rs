use momento_api::models::{AiActionResponse, AiFeatureActionResult};

#[test]
fn action_response_uses_exact_feature_and_camel_case_job_count() {
    let response = AiActionResponse {
        action: "start".to_string(),
        results: vec![AiFeatureActionResult {
            feature: "face_detection".to_string(),
            outcome: "queued".to_string(),
            affected_jobs: 3,
            error: None,
        }],
    };

    let json = serde_json::to_value(response).expect("AI action response should serialize");

    assert_eq!(json["action"], "start");
    assert_eq!(json["results"][0]["feature"], "face_detection");
    assert_eq!(json["results"][0]["outcome"], "queued");
    assert_eq!(json["results"][0]["affectedJobs"], 3);
    assert_eq!(json["results"][0]["error"], serde_json::Value::Null);
}
