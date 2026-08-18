use std::io::ErrorKind;
use std::path::PathBuf;

use momento_api::config::{
    consume_admin_password_reset, load_config, save_default_config, Config, DEFAULT_CONFIG_TEMPLATE,
};
use tempfile::TempDir;

fn write_config(dir: &TempDir, contents: &str) -> PathBuf {
    let path = dir.path().join("config.toml");
    let contents = format!(
        "{contents}\n[cronjob]\ntimezone = \"Etc/UTC\"\nocr_cron = \"0 1 * * *\"\nimage_tagging_cron = \"0 2 * * *\"\ndeduplicate_cron = \"0 3 * * *\"\nface_detection_cron = \"0 4 * * *\"\n"
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
    assert!(!config.server.reset_admin_password);
    assert_eq!(config.server.data_dir, defaults.server.data_dir);
    assert_eq!(config.server.static_dir, defaults.server.static_dir);
}

#[test]
fn test_consume_admin_password_reset_persists_false_and_preserves_comments() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "# keep this comment\n[server]\nreset_admin_password = true # one-shot\n",
    )
    .expect("Failed to write config");
    let mut config = load_config(&path).expect("Failed to load config");

    assert!(consume_admin_password_reset(&path, &mut config).expect("Failed to consume reset"));

    assert!(!config.server.reset_admin_password);
    let saved = std::fs::read_to_string(&path).expect("Failed to read saved config");
    assert!(saved.contains("# keep this comment"));
    assert!(saved.contains("reset_admin_password = false"));
    assert!(
        !load_config(&path)
            .expect("Failed to reload config")
            .server
            .reset_admin_password
    );
}

#[test]
fn test_consume_admin_password_reset_does_not_rewrite_false_config() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[server]\nreset_admin_password = false\n")
        .expect("Failed to write config");
    let original = std::fs::read_to_string(&path).expect("Failed to read config");
    let mut config = load_config(&path).expect("Failed to load config");

    assert!(!consume_admin_password_reset(&path, &mut config).expect("Failed to check reset"));
    assert_eq!(
        std::fs::read_to_string(&path).expect("Failed to reread config"),
        original
    );
}

#[test]
fn test_existing_cronjob_section_receives_new_schedule_defaults() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "[cronjob]\ntimezone = \"Etc/UTC\"\ndeduplicate_cron = \"30 3 * * *\"\n",
    )
    .expect("Failed to write existing config");

    let config = load_config(&path).expect("Existing cronjob config should remain valid");

    assert_eq!(config.cronjob.ocr_cron, "0 1 * * *");
    assert_eq!(config.cronjob.image_tagging_cron, "0 2 * * *");
    assert_eq!(config.cronjob.deduplicate_cron, "30 3 * * *");
    assert_eq!(config.cronjob.face_detection_cron, "0 4 * * *");
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
fn test_load_config_rejects_removed_admin_section() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(
        &dir,
        "[admin]\nusername = \"admin\"\npassword = \"admin\"\n",
    );

    let error = load_config(&path).expect_err("Admin section must be rejected");

    assert!(error.to_string().contains("admin"));
}

#[test]
fn test_load_config_reads_flat_webdav_settings() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(
        &dir,
        "[webdav]\nmount_path = \"/photos\"\nrealm = \"Photos\"\nmax_upload_bytes = 1234\nmax_concurrent_requests = 7\npoll_interval_seconds = 3\nstable_file_age_seconds = 11\nmax_concurrent_processing = 4\n",
    );

    let config = load_config(&path).expect("Failed to load flat WebDAV config");

    assert_eq!(config.webdav.mount_path, "/photos");
    assert_eq!(config.webdav.realm, "Photos");
    assert_eq!(config.webdav.max_upload_bytes, 1234);
    assert_eq!(config.webdav.max_concurrent_requests, 7);
    assert_eq!(config.webdav.poll_interval_seconds, 3);
    assert_eq!(config.webdav.stable_file_age_seconds, 11);
    assert_eq!(config.webdav.max_concurrent_processing, 4);
}

#[test]
fn test_load_config_rejects_removed_webdav_enabled_setting() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(&dir, "[webdav]\nenabled = false\n");

    let error = load_config(&path).expect_err("WebDAV enablement must not be configurable");

    assert!(error.to_string().contains("enabled"));
}

#[test]
fn test_load_config_rejects_invalid_webdav_runtime_settings() {
    for (setting, value) in [
        ("max_upload_bytes", "0"),
        ("max_concurrent_requests", "0"),
        ("poll_interval_seconds", "0"),
        ("max_concurrent_processing", "0"),
        ("mount_path", "\"/nested/photos\""),
        ("mount_path", "\"/photos/\""),
        ("mount_path", "\"/.\""),
        ("mount_path", "\"/..\""),
        ("mount_path", "\"/photo%73\""),
    ] {
        let dir = TempDir::new().expect("Failed to create temp dir");
        let path = write_config(&dir, &format!("[webdav]\n{setting} = {value}\n"));
        let error = load_config(&path).expect_err("Invalid WebDAV setting must be rejected");

        assert!(error.to_string().contains("webdav"), "{error}");
    }
}

#[test]
fn test_load_config_rejects_nested_webdav_settings() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(&dir, "[webdav.limits]\nmax_upload_bytes = 1234\n");

    let error = load_config(&path).expect_err("Nested WebDAV settings must be rejected");

    assert!(error.to_string().contains("limits"));
}

#[test]
fn test_load_config_reads_thumbnail_metadata_settings() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(
        &dir,
        "[metadata]\nthumbnails_max_size = 1600\nthumbnails_tiny_size = 400\nthumbnails_quality = 90\nthumbnails_video_frame_quality = 80\n",
    );

    let config = load_config(&path).expect("Failed to load combined metadata config");

    assert_eq!(config.metadata.thumbnails_max_size, 1600);
    assert_eq!(config.metadata.thumbnails_tiny_size, 400);
    assert_eq!(config.metadata.thumbnails_quality, 90);
    assert_eq!(config.metadata.thumbnails_video_frame_quality, 80);
}

#[test]
fn test_load_config_rejects_removed_reverse_geocoding_settings() {
    for (setting, value) in [
        ("reverse_geocoding_enabled", "false"),
        (
            "reverse_geocoding_base_url",
            "\"https://example.com/reverse\"",
        ),
        ("reverse_geocoding_user_agent", "\"Momento test\""),
        ("reverse_geocoding_timeout_seconds", "12"),
        ("reverse_geocoding_rate_limit_seconds", "2.5"),
    ] {
        let dir = TempDir::new().expect("Failed to create temp dir");
        let path = write_config(&dir, &format!("[metadata]\n{setting} = {value}\n"));

        let error = load_config(&path).expect_err("Removed setting must be rejected");

        assert!(error.to_string().contains(setting), "{error}");
    }
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

    assert_eq!(config.server.data_dir, PathBuf::from("/data"));
    assert_eq!(config.server.static_dir, PathBuf::from("/app/static"));
    assert_eq!(config.server.port, 8000);
    assert!(!config.server.reset_admin_password);
    assert!(config.llm.enabled);
    assert_eq!(config.metadata.thumbnails_max_size, 1200);

    let generated = std::fs::read_to_string(path).expect("Failed to read generated config");
    assert_eq!(generated, DEFAULT_CONFIG_TEMPLATE);
    assert!(generated.contains("# Five-field cron expressions"));
    let generated: toml::Value = toml::from_str(&generated).expect("Generated config must be TOML");
    assert!(generated["metadata_worker"].get("batch_size").is_none());
    assert!(generated.get("admin").is_none());
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
        Some(1200)
    );
    assert!(generated.get("thumbnails").is_none());
    assert!(generated.get("reverse_geocoding").is_none());
    assert!(generated["metadata"]
        .get("reverse_geocoding_enabled")
        .is_none());
}

#[test]
fn test_save_default_config_does_not_replace_existing_config() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "existing").expect("Failed to write existing config");

    let error = save_default_config(&path).expect_err("Existing config must not be replaced");

    assert_eq!(error.kind(), ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "existing");
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
fn test_load_config_rejects_each_invalid_ai_schedule() {
    for (field, expression) in [
        ("ocr_cron", "0 1 * * *"),
        ("image_tagging_cron", "0 2 * * *"),
        ("deduplicate_cron", "0 3 * * *"),
        ("face_detection_cron", "0 4 * * *"),
    ] {
        let dir = TempDir::new().expect("Failed to create temp dir");
        let path = write_config(&dir, "");
        let contents = std::fs::read_to_string(&path)
            .expect("Failed to read config")
            .replace(
                &format!("{field} = \"{expression}\""),
                &format!("{field} = \"invalid\""),
            );
        std::fs::write(&path, contents).expect("Failed to update config");

        let error = load_config(&path).expect_err("Invalid schedule must fail");
        assert!(error.to_string().contains(field.trim_end_matches("_cron")));
    }
}

#[test]
fn playground_config_matches_the_generated_template() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../playground/config.toml");
    let playground = std::fs::read_to_string(path).expect("Playground config must exist");

    assert_eq!(playground, DEFAULT_CONFIG_TEMPLATE);
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
