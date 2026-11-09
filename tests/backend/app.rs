use axum_test::TestServer;
use momento_api::app::{create_app, AppDependencies};
use momento_api::config::{load_config_with_identity, Config, ConfigManager, ThreadPoolConfig};
use momento_api::database::{create_pool_at, init_database};
use momento_api::runtime::{ExecutorRuntime, RuntimeSizing};

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
    assert_eq!(
        headers.get("referrer-policy").unwrap(),
        "strict-origin-when-cross-origin"
    );
    assert_eq!(headers.get_all("referrer-policy").iter().count(), 1);
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
    assert!(content_security_policy
        .contains("img-src 'self' data: blob: https://tile.openstreetmap.org"));
    assert!(content_security_policy.contains("frame-ancestors 'none'"));

    let missing = server.post("/api/v1/removed/operation").await;
    missing.assert_status_not_found();
    missing.assert_header("referrer-policy", "strict-origin-when-cross-origin");
}

#[tokio::test]
async fn protocol_guard_rejects_encoded_bodies_and_overlong_uris_before_consumption() {
    let (application, _) = create_test_app();
    let server = TestServer::new(application).expect("test server");

    let encoded = server
        .post("/api/v1/auth/login")
        .add_header("content-encoding", "gzip")
        .bytes(vec![0_u8; 32].into())
        .await;
    encoded.assert_status(axum::http::StatusCode::UNSUPPORTED_MEDIA_TYPE);
    encoded.assert_header("connection", "close");

    let oversized_uri = format!("/api/v1/healthcheck?value={}", "a".repeat(8 * 1024));
    let oversized = server.get(&oversized_uri).await;
    oversized.assert_status(axum::http::StatusCode::URI_TOO_LONG);
    oversized.assert_header("connection", "close");
}

#[tokio::test]
async fn static_assets_use_root_relative_file_sessions_and_safe_spa_fallback() {
    let directory = crate::temporary::tempdir().expect("application directory");
    let data_directory = directory.path().join("data");
    let static_directory = directory.path().join("static");
    std::fs::create_dir(&data_directory).expect("data directory");
    std::fs::create_dir(&static_directory).expect("static directory");
    std::fs::write(static_directory.join("index.html"), b"<main>momento</main>")
        .expect("index asset");
    std::fs::create_dir(static_directory.join("assets")).unwrap();
    std::fs::write(
        static_directory.join("assets/app.js"),
        b"console.log('momento')",
    )
    .expect("script asset");
    let brand_assets = [
        ("brand/momento-32.png", "image/png"),
        ("brand/momento-180.png", "image/png"),
        ("brand/momento-192.png", "image/png"),
        ("brand/momento-512.png", "image/png"),
        ("site.webmanifest", "application/manifest+json"),
    ];
    std::fs::create_dir(static_directory.join("brand")).unwrap();
    for (path, _) in brand_assets {
        std::fs::write(static_directory.join(path), b"public brand asset").unwrap();
    }
    std::fs::write(static_directory.join("brand/private.png"), b"PRIVATE").unwrap();
    let association =
        include_bytes!("../../src/backend/resources/webcredentials/apple-app-site-association");
    let association_path = static_directory.join(".well-known/apple-app-site-association");
    std::fs::create_dir(static_directory.join(".well-known")).unwrap();
    std::fs::write(&association_path, association).unwrap();
    std::fs::write(
        static_directory.join(".well-known/private.json"),
        b"PRIVATE",
    )
    .unwrap();

    let pool = create_pool_at(&data_directory.join("database.sqlite"), 2).expect("database pool");
    init_database(&pool.get().expect("schema connection")).expect("database schema");
    let mut config = crate::test_utils::test_config();
    config.server.data_dir = data_directory.clone();
    config.server.static_dir = static_directory.clone();
    config.webdav.mount_path = "/webdav".to_string();
    config.thread_pool = ThreadPoolConfig {
        cpu_workers: 1,
        network_io_workers: 2,
        storage_io_workers: 2,
        sqlite_workers: 2,
    };
    let config_path = directory.path().join("config.toml");
    std::fs::write(&config_path, toml::to_string(&config).expect("config text"))
        .expect("config file");
    let loaded = load_config_with_identity(&config_path).expect("loaded config");
    let sizing = RuntimeSizing::new(&loaded.config.thread_pool, 4 * 1024 * 1024 * 1024)
        .expect("runtime sizing");
    let (runtime, executors) = ExecutorRuntime::start(
        &sizing,
        pool.clone(),
        loaded.identity.clone(),
        data_directory,
        Some(static_directory),
    )
    .expect("executor runtime");
    let config_manager = ConfigManager::new(loaded, &executors);
    let application = create_app(
        config_manager,
        AppDependencies {
            executors,
            authentication_dummy_hash: crate::test_utils::test_authentication_dummy_hash(),
            llm_transport: Default::default(),
            webdav_request_gate: std::sync::Arc::new(tokio::sync::RwLock::new(())),
            admin_password_reset_user_id: None,
        },
    );
    let server = TestServer::new(application).expect("static server");

    for (path, content_type) in brand_assets {
        let path = format!("/{path}");
        let response = server.get(&path).await;
        response.assert_status_ok();
        response.assert_header("content-type", content_type);
        assert_eq!(response.as_bytes().as_ref(), b"public brand asset");
        let head = server.method(axum::http::Method::HEAD, &path).await;
        head.assert_status_ok();
        head.assert_header("content-type", content_type);
        assert!(head.as_bytes().is_empty());
    }

    let association_response = server.get("/.well-known/apple-app-site-association").await;
    association_response.assert_status_ok();
    association_response.assert_header("content-type", "application/json");
    assert_eq!(association_response.as_bytes().as_ref(), association);
    let head = server
        .method(
            axum::http::Method::HEAD,
            "/.well-known/apple-app-site-association",
        )
        .await;
    head.assert_status_ok();
    head.assert_header("content-type", "application/json");
    assert!(head.as_bytes().is_empty());
    std::fs::remove_file(association_path).unwrap();
    server
        .get("/.well-known/apple-app-site-association")
        .add_header("accept", "text/html")
        .await
        .assert_status_not_found();

    let script = server.get("/assets/app.js").await;
    script.assert_status_ok();
    script.assert_header("content-type", "text/javascript");
    assert_eq!(script.as_bytes().as_ref(), b"console.log('momento')");
    let navigation = server
        .get("/albums")
        .add_header("accept", "text/html")
        .await;
    navigation.assert_status_ok();
    assert_eq!(navigation.as_bytes().as_ref(), b"<main>momento</main>");
    server
        .get("/missing.js")
        .add_header("accept", "text/html")
        .await
        .assert_status_not_found();
    std::fs::write(
        config_path.parent().unwrap().join("static/.env"),
        b"PRIVATE",
    )
    .unwrap();
    for path in [
        "/.env",
        "/%2eenv",
        "/wp-config.php",
        "/database.sql",
        "/unknown-page",
        "/assets/../.env",
        "/assets/.secret.js",
        "/.well-known/private.json",
        "/.well-known/../.env",
        "/brand/private.png",
        "/brand/../.env",
    ] {
        let response = server.get(path).add_header("accept", "text/html").await;
        response.assert_status_not_found();
        assert_eq!(response.text(), "Not Found");
    }
    server
        .get("/webdav/missing")
        .add_header("accept", "text/html")
        .await
        .assert_status_unauthorized();

    drop(server);
    runtime.shutdown().await.expect("runtime shutdown");
}
