use crate::test_utils::create_test_app;
use axum_test::TestServer;
use serde_json::Value;

#[tokio::test]
async fn capabilities_exposes_version_extensions_features_and_backup_limits() {
    let (app, _) = create_test_app();
    let server = TestServer::new(app).expect("server");

    let response = server.get("/api/v1/client/capabilities").await;
    response.assert_status_ok();
    let body = response.json::<Value>();

    assert_eq!(body["appVersion"], momento_api::VERSION);
    assert_eq!(body["apiVersion"], 1);
    assert!(body["supportedMediaExtensions"]
        .as_array()
        .expect("extensions")
        .iter()
        .any(|extension| extension == ".jpg"));
    for lossless_camera_extension in [".avif", ".dng", ".arw", ".srw"] {
        assert!(body["supportedMediaExtensions"]
            .as_array()
            .expect("extensions")
            .iter()
            .any(|extension| extension == lossless_camera_extension));
    }
    for feature in [
        "llm",
        "imageTagging",
        "deduplicate",
        "faceDetection",
        "imageAesthetics",
        "screenshotDetection",
        "documentDetection",
    ] {
        assert_eq!(body["features"][feature], false);
    }
    assert_eq!(body["backup"]["enabled"], true);
    assert_eq!(body["backup"]["protocolVersion"], 2);
    assert!(body["backup"]["maxUploadBytes"].as_u64().is_some());
    assert!(body["backup"]["maxChunkBytes"].as_u64().is_some());
}
