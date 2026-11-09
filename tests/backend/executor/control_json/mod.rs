use momento_api::executor::{ControlRequest, ControlRequestKind, ControlResponse, MessageResponse};

#[tokio::test]
async fn typed_control_parse_returns_the_dto_and_redacted_log_from_one_cpu_operation() {
    let (_application, pool) = crate::test_utils::create_test_app();
    let executors = crate::test_utils::test_executor_handles(pool);
    let parsed = executors
        .cpu
        .parse_control_request(
            ControlRequestKind::UserCreate,
            br#"{"username":"new-user","email":"new@example.com","password":"secret-value"}"#
                .to_vec(),
        )
        .await
        .expect("control parse");

    let ControlRequest::UserCreate(request) = parsed.request.expect("typed request") else {
        panic!("wrong typed request");
    };
    assert_eq!(request.username, "new-user");
    assert_eq!(request.email, "new@example.com");
    assert_eq!(request.password, "secret-value");
    assert_eq!(
        parsed.log_payload,
        r#"{"email":"new@example.com","password":"[redacted]","username":"new-user"}"#
    );
}

#[tokio::test]
async fn typed_control_parse_distinguishes_json_syntax_from_dto_data_errors() {
    let (_application, pool) = crate::test_utils::create_test_app();
    let executors = crate::test_utils::test_executor_handles(pool);
    let syntax = executors
        .cpu
        .parse_control_request(ControlRequestKind::UserDelete, b"{".to_vec())
        .await
        .expect("syntax result")
        .request
        .expect_err("syntax error");
    assert!(!syntax.data_error);

    let data = executors
        .cpu
        .parse_control_request(ControlRequestKind::UserDelete, br#"{}"#.to_vec())
        .await
        .expect("data result")
        .request
        .expect_err("data error");
    assert!(data.data_error);
}

#[tokio::test]
async fn control_response_serialization_runs_on_the_cpu_executor() {
    let (_application, pool) = crate::test_utils::create_test_app();
    let executors = crate::test_utils::test_executor_handles(pool);
    let bytes = executors
        .cpu
        .serialize_control_response(ControlResponse::from(MessageResponse {
            message: "ready".to_string(),
        }))
        .await
        .expect("response serialization");
    assert_eq!(bytes, br#"{"message":"ready"}"#);
}
