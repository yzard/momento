use std::io::ErrorKind;
use std::path::PathBuf;

use momento_api::config::{load_config, save_default_config, Config};
use tempfile::TempDir;

fn write_config(dir: &TempDir, contents: &str) -> PathBuf {
    let path = dir.path().join("config.toml");
    let contents = format!(
        "{contents}\n[cronjob]\ntimezone = \"Etc/UTC\"\ndeduplicate_cron = \"0 3 * * *\"\n"
    );
    std::fs::write(&path, contents).expect("Failed to write test config");
    path
}

#[test]
fn test_load_config_reads_server_paths() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(
        &dir,
        "[server]\ndata_dir = \"/srv/momento/data\"\nstatic_dir = \"/srv/momento/static\"\n",
    );

    let config = load_config(&path).expect("Failed to load config");

    assert_eq!(config.server.data_dir, PathBuf::from("/srv/momento/data"));
    assert_eq!(
        config.server.static_dir,
        PathBuf::from("/srv/momento/static")
    );
}

#[test]
fn test_load_config_rejects_configurable_log_file_path() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(&dir, "[logging]\nfile_path = \"/var/log/momento.log\"\n");

    let error = load_config(&path).expect_err("Logging path must not be configurable");

    assert!(error.to_string().contains("logging"));
}

#[test]
fn test_load_config_reads_llm_submission_window() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(
        &dir,
        "[llm_submission_worker]\nmax_in_flight = 17\npoll_interval_seconds = 2\n",
    );

    let config = load_config(&path).expect("Failed to load config");

    assert_eq!(config.llm_submission_worker.max_in_flight, 17);
    assert_eq!(config.llm_submission_worker.poll_interval_seconds, 2);
}

#[test]
fn test_load_config_rejects_removed_llm_submission_batch_size() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(&dir, "[llm_submission_worker]\nbatch_size = 64\n");

    let error = load_config(&path).expect_err("Submission batch size has been removed");

    assert!(error.to_string().contains("batch_size"));
}

#[test]
fn test_load_config_missing_file_is_an_error() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let missing = dir.path().join("does-not-exist.toml");

    let error = load_config(&missing).expect_err("Missing config must not fall back to defaults");

    assert_eq!(error.kind(), ErrorKind::NotFound);
}

#[test]
fn test_load_config_malformed_toml_is_an_error() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(&dir, "[server]\nport = \"not-a-number\"\n");

    let error = load_config(&path).expect_err("Malformed config must not fall back to defaults");

    assert!(error.to_string().contains("invalid config"));
}

#[test]
fn test_load_config_omitted_sections_use_defaults() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(&dir, "[server]\nport = 9001\n");

    let config = load_config(&path).expect("Failed to load config");
    let defaults = Config::default();

    assert_eq!(config.server.port, 9001);
    assert_eq!(config.server.data_dir, defaults.server.data_dir);
    assert_eq!(config.server.static_dir, defaults.server.static_dir);
}

#[test]
fn test_server_path_defaults_match_container_layout() {
    let config = Config::default();

    assert_eq!(config.server.data_dir, PathBuf::from("/data"));
    assert_eq!(config.server.static_dir, PathBuf::from("/app/static"));
}

#[test]
fn test_load_config_rejects_removed_storage_section() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(&dir, "[storage]\ndata_dir = \"/data\"\n");

    let error = load_config(&path).expect_err("Storage section must be rejected");

    assert!(error.to_string().contains("storage"));
}

#[test]
fn test_load_config_reads_flat_webdav_settings() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(
        &dir,
        "[webdav]\nenabled = true\nmount_path = \"/photos\"\nrealm = \"Photos\"\nmax_upload_bytes = 1234\nmax_concurrent_requests = 7\npoll_interval_seconds = 3\nstable_file_age_seconds = 11\nmax_concurrent_processing = 4\n",
    );

    let config = load_config(&path).expect("Failed to load flat WebDAV config");

    assert!(config.webdav.enabled);
    assert_eq!(config.webdav.mount_path, "/photos");
    assert_eq!(config.webdav.realm, "Photos");
    assert_eq!(config.webdav.max_upload_bytes, 1234);
    assert_eq!(config.webdav.max_concurrent_requests, 7);
    assert_eq!(config.webdav.poll_interval_seconds, 3);
    assert_eq!(config.webdav.stable_file_age_seconds, 11);
    assert_eq!(config.webdav.max_concurrent_processing, 4);
}

#[test]
fn test_load_config_rejects_nested_webdav_settings() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(&dir, "[webdav.limits]\nmax_upload_bytes = 1234\n");

    let error = load_config(&path).expect_err("Nested WebDAV settings must be rejected");

    assert!(error.to_string().contains("limits"));
}

#[test]
fn test_load_config_reads_combined_metadata_settings() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(
        &dir,
        "[metadata]\nthumbnails_max_size = 1600\nthumbnails_tiny_size = 400\nthumbnails_quality = 90\nthumbnails_video_frame_quality = 80\nreverse_geocoding_enabled = false\nreverse_geocoding_base_url = \"https://example.com/reverse\"\nreverse_geocoding_user_agent = \"Momento test\"\nreverse_geocoding_timeout_seconds = 12\nreverse_geocoding_rate_limit_seconds = 2.5\n",
    );

    let config = load_config(&path).expect("Failed to load combined metadata config");

    assert_eq!(config.metadata.thumbnails_max_size, 1600);
    assert_eq!(config.metadata.thumbnails_tiny_size, 400);
    assert_eq!(config.metadata.thumbnails_quality, 90);
    assert_eq!(config.metadata.thumbnails_video_frame_quality, 80);
    assert!(!config.metadata.reverse_geocoding_enabled);
    assert_eq!(
        config.metadata.reverse_geocoding_base_url,
        "https://example.com/reverse"
    );
    assert_eq!(config.metadata.reverse_geocoding_user_agent, "Momento test");
    assert_eq!(config.metadata.reverse_geocoding_timeout_seconds, 12);
    assert_eq!(config.metadata.reverse_geocoding_rate_limit_seconds, 2.5);
}

#[test]
fn test_load_config_rejects_replaced_metadata_sections() {
    let dir = TempDir::new().expect("Failed to create temp dir");

    for section in ["thumbnails", "reverse_geocoding"] {
        let path = write_config(&dir, &format!("[{section}]\nenabled = true\n"));
        let error = load_config(&path).expect_err("Replaced metadata section must be rejected");

        assert!(error.to_string().contains(section));
    }
}

#[test]
fn test_load_config_rejects_unprefixed_metadata_settings() {
    let dir = TempDir::new().expect("Failed to create temp dir");

    for (setting, value) in [("max_size", "1600"), ("enabled", "false")] {
        let path = write_config(&dir, &format!("[metadata]\n{setting} = {value}\n"));
        let error = load_config(&path).expect_err("Unprefixed metadata setting must be rejected");

        assert!(error.to_string().contains(setting));
    }
}

#[test]
fn test_save_default_config_round_trips() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = dir.path().join("generated").join("config.toml");

    save_default_config(&path).expect("Failed to save default config");
    let config = load_config(&path).expect("Failed to reload saved config");
    let defaults = Config::default();

    assert_eq!(config.server.data_dir, defaults.server.data_dir);
    assert_eq!(config.server.static_dir, defaults.server.static_dir);
    assert_eq!(config.server.port, defaults.server.port);

    let generated = std::fs::read_to_string(path).expect("Failed to read generated config");
    let generated: toml::Value = toml::from_str(&generated).expect("Generated config must be TOML");
    assert!(generated["metadata_worker"].get("batch_size").is_none());
    assert!(generated.get("storage").is_none());
    assert_eq!(generated["server"]["data_dir"].as_str(), Some("/data"));
    assert_eq!(
        generated["webdav"]["max_upload_bytes"].as_integer(),
        Some(10_737_418_240)
    );
    assert!(generated["webdav"].get("limits").is_none());
    assert!(generated["webdav"].get("processing").is_none());
    assert_eq!(
        generated["metadata"]["thumbnails_max_size"].as_integer(),
        Some(i64::from(defaults.metadata.thumbnails_max_size))
    );
    assert!(generated.get("thumbnails").is_none());
    assert!(generated.get("reverse_geocoding").is_none());
}

#[test]
fn test_load_config_rejects_removed_metadata_batch_size() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(&dir, "[metadata_worker]\nbatch_size = 64\n");

    let error = load_config(&path).expect_err("Metadata batch size has been removed");

    assert!(error.to_string().contains("batch_size"));
}

#[test]
fn test_load_config_uses_disabled_deduplicate_llm_default_when_section_is_missing() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[server]\nport = 9001\n").expect("Failed to write test config");

    let config = load_config(&path).expect("Missing deduplicate config should use safe defaults");

    assert!(!config.llm.deduplicate_enabled);
    assert_eq!(config.llm.face_group_similarity_threshold, 0.55);
}

#[test]
fn test_load_config_rejects_invalid_face_group_similarity_threshold() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(&dir, "[llm]\nface_group_similarity_threshold = 1.1\n");

    let error = load_config(&path).expect_err("Invalid face similarity threshold must fail");

    assert!(error
        .to_string()
        .contains("face_group_similarity_threshold"));
}

#[test]
fn test_load_config_rejects_invalid_deduplicate_schedule() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(&dir, "");
    let contents = std::fs::read_to_string(&path)
        .expect("Failed to read config")
        .replace(
            "deduplicate_cron = \"0 3 * * *\"",
            "deduplicate_cron = \"invalid\"",
        );
    std::fs::write(&path, contents).expect("Failed to update config");

    assert!(load_config(&path).is_err());
}

#[test]
fn test_load_config_rejects_invalid_deduplicate_timezone() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(&dir, "");
    let contents = std::fs::read_to_string(&path)
        .expect("Failed to read config")
        .replace("timezone = \"Etc/UTC\"", "timezone = \"Mars/Olympus\"");
    std::fs::write(&path, contents).expect("Failed to update config");

    assert!(load_config(&path).is_err());
}

#[test]
fn test_load_config_requires_llm_service_url() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(&dir, "[llm]\nenabled = true\nservice_url = \"\"\n");

    let error = load_config(&path).expect_err("Enabled LLM must have a service URL");

    assert!(error.to_string().contains("service_url"));
}

#[test]
fn test_load_config_requires_websocket_client_identity_and_key() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(
        &dir,
        "[llm]\nenabled = true\nservice_url = \"http://127.0.0.1:8100\"\nclient_id = \"client_a\"\napi_key = \"key\"\n",
    );
    let error = load_config(&path).expect_err("HTTP LLM URL must be rejected");
    assert!(error.to_string().contains("ws:// or wss://"));

    let path = write_config(
        &dir,
        "[llm]\nenabled = true\nservice_url = \"ws://127.0.0.1:8100/api/v1/llm/connect\"\napi_key = \"key\"\n",
    );
    let error = load_config(&path).expect_err("LLM client ID must be required");
    assert!(error.to_string().contains("client_id"));

    let path = write_config(
        &dir,
        "[llm]\nenabled = true\nservice_url = \"ws://127.0.0.1:8100/api/v1/llm/connect\"\nclient_id = \"client_a\"\n",
    );
    let error = load_config(&path).expect_err("LLM API key must be required");
    assert!(error.to_string().contains("api_key"));
}

#[test]
fn test_load_config_rejects_configurable_security_algorithm() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(&dir, "[security]\nalgorithm = \"HS512\"\n");

    let error = load_config(&path).expect_err("JWT algorithm is not configurable");

    assert!(error.to_string().contains("algorithm"));
}

#[test]
fn test_load_config_rejects_configurable_llm_inference_endpoint() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(
        &dir,
        "[llm]\nenabled = true\nservice_url = \"http://127.0.0.1:8100\"\ninference_endpoint = \"/custom\"\n",
    );

    let error = load_config(&path).expect_err("LLM inference endpoint is not configurable");

    assert!(error.to_string().contains("inference_endpoint"));
}

#[test]
fn test_load_config_rejects_removed_deduplicate_section() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(&dir, "[deduplicate]\nenabled = true\n");

    let error = load_config(&path).expect_err("Deduplicate settings moved to llm and cronjob");

    assert!(error.to_string().contains("deduplicate"));
}
