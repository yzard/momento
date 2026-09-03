use momento_api::models::{FileOperationListRequest, FileOperationRetryRequest};

#[test]
fn retry_hash_input_is_length_delimited_and_versioned() {
    let first = FileOperationRetryRequest {
        retry_request_id: "a".to_string(),
        operation_id: "bc".to_string(),
        expected_version: 1,
    };
    let second = FileOperationRetryRequest {
        retry_request_id: "ab".to_string(),
        operation_id: "c".to_string(),
        expected_version: 1,
    };

    assert_ne!(
        first.canonical_hash_input().expect("first hash input"),
        second.canonical_hash_input().expect("second hash input")
    );
    assert!(first.validate().is_ok());
}

#[test]
fn retry_validation_rejects_invalid_identifiers_and_versions() {
    let request = FileOperationRetryRequest {
        retry_request_id: String::new(),
        operation_id: "operation".to_string(),
        expected_version: 0,
    };

    assert!(request.validate().is_err());
}

#[test]
fn list_validation_requires_explicit_known_states_and_a_bounded_limit() {
    assert!(FileOperationListRequest {
        states: vec!["publishing".to_string()],
        cursor: None,
        limit: 100,
    }
    .validate()
    .is_ok());
    assert!(FileOperationListRequest {
        states: vec!["unknown".to_string()],
        cursor: None,
        limit: 101,
    }
    .validate()
    .is_err());
}
