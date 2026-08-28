use axum_test::TestServer;

use crate::test_utils::create_test_app;

#[tokio::test]
async fn unknown_api_routes_do_not_fall_through_to_webdav() {
    let (application, _) = create_test_app();
    let server = TestServer::new(application).expect("test server");

    server
        .post("/api/v1/removed/operation")
        .json(&serde_json::json!({}))
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn responses_include_browser_security_headers() {
    let (application, _) = create_test_app();
    let server = TestServer::new(application).expect("test server");

    let response = server.get("/api/v1/healthcheck").await;

    response.assert_status_ok();
    let headers = response.headers();
    assert_eq!(headers.get("x-content-type-options").unwrap(), "nosniff");
    assert_eq!(headers.get("x-frame-options").unwrap(), "DENY");
    assert_eq!(headers.get("referrer-policy").unwrap(), "no-referrer");
    assert_eq!(
        headers.get("permissions-policy").unwrap(),
        "camera=(), geolocation=(), microphone=()",
    );
    let content_security_policy = headers
        .get("content-security-policy")
        .expect("content security policy")
        .to_str()
        .expect("valid content security policy");
    assert!(content_security_policy.contains("script-src 'self'"));
    assert!(content_security_policy.contains("frame-ancestors 'none'"));
}
