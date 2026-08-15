use axum::body::Body;
use axum::http::{Request, StatusCode};
use llm_service::config::Config;
use llm_service::provider::ServiceManager;
use llm_service::routes::{router, AppState};
use llm_service::scheduler::Scheduler;
use sha2::Digest;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::sync::Mutex;
use tower::ServiceExt;

#[tokio::test]
async fn submission_has_no_framework_multipart_body_limit() {
    let directory = tempdir().expect("queue directory");
    let config = Arc::new(Config::default());
    let scheduler = Arc::new(
        Scheduler::new(
            directory.path().to_path_buf(),
            config.general.scheduler.clone(),
            config.callback.clone(),
            Arc::new(Mutex::new(ServiceManager::new(Arc::clone(&config)))),
        )
        .expect("scheduler"),
    );
    let app = router(AppState {
        config,
        manager: Arc::new(Mutex::new(ServiceManager::new(Arc::new(Config::default())))),
        scheduler,
    });
    let boundary = "momento-boundary";
    let image = "a".repeat(3 * 1024 * 1024);
    let content_hash = format!("{:x}", sha2::Sha256::digest(image.as_bytes()));
    let manifest = format!(
        r#"{{"jobId":"0123456789abcdef0123456789abcdef","mediaId":1,"task":"image_clustering","attempt":1,"inputs":[{{"sequence":0,"filename":"input.jpg","mimeType":"image/jpeg","byteSize":{},"contentHash":"{}","inputKind":"image","frameTimestampMs":null}}],"callbackUrl":"http://example.test/callback"}}"#,
        image.len(),
        content_hash
    );
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"manifest\"\r\nContent-Type: application/json\r\n\r\n{manifest}\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"input-0\"; filename=\"input.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n{image}\r\n--{boundary}--\r\n"
    );

    let response = app
        .oneshot(
            Request::post("/api/v1/jobs/submit")
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}
