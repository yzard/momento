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
