mod defaults;

use std::io::ErrorKind;
use std::path::PathBuf;

use momento_api::config::{
    apply_config_environment, consume_admin_password_reset, default_config_template, load_config,
    resolve_config_environment, save_default_config, Config, ConfigManager,
};
use tempfile::TempDir;

fn write_config(dir: &TempDir, contents: &str) -> PathBuf {
    let path = dir.path().join("config.toml");
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
fn security_scoped_credential_expirations_must_be_within_one_week() {
    for (field, value) in [
        ("media_access_ticket_expire_hours", 0),
        ("share_session_expire_hours", 169),
    ] {
        let directory = TempDir::new().expect("temporary directory");
        let path = write_config(&directory, &format!("[security]\n{field} = {value}\n"));

        let error = load_config(&path).expect_err("invalid scoped credential expiration");
        assert!(error.to_string().contains(
            "security media ticket and share session expirations must be within 1..=168 hours"
        ));
    }
}

#[test]
fn security_password_and_cleanup_limits_must_be_positive() {
    for field in [
        "password_attempt_window_seconds",
        "password_attempts_per_identity",
        "password_attempts_per_source",
        "password_lockout_seconds",
        "password_hash_max_concurrent",
        "refresh_token_cleanup_interval_seconds",
    ] {
        let directory = TempDir::new().expect("temporary directory");
        let path = write_config(&directory, &format!("[security]\n{field} = 0\n"));

        let error = load_config(&path).expect_err("zero security limit must fail");
        assert!(error
            .to_string()
            .contains("security password limits and refresh-token cleanup interval"));
    }
}

#[test]
fn media_process_limits_must_be_positive_and_ordered() {
    let directory = TempDir::new().expect("temporary config directory");
    for invalid_config in [
        "timeout_seconds = 0",
        "maximum_decoded_image_pixels = 0",
        "imagemagick_memory_limit_mebibytes = 2048\nimagemagick_map_limit_mebibytes = 1024",
    ] {
        let path = write_config(&directory, &format!("[media_process]\n{invalid_config}\n"));
        let error = load_config(&path).expect_err("invalid media process limit must fail");
        assert!(error.to_string().contains("media_process"));
    }
}

#[test]
fn config_environment_resolves_the_llm_service_address() {
    let resolved = resolve_config_environment(
        "server_address = \"${LLM_SERVICE_ADDRESS}\"",
        Some("momento-llm-service:8100"),
        None,
        None,
        None,
    )
    .expect("resolved config");

    assert_eq!(resolved, "server_address = \"momento-llm-service:8100\"");
}

#[test]
fn config_environment_requires_a_non_empty_llm_service_address() {
    for address in [None, Some(""), Some("   ")] {
        let error = resolve_config_environment(
            "server_address = \"${LLM_SERVICE_ADDRESS}\"",
            address,
            None,
            None,
            None,
        )
        .expect_err("missing address must fail");
        assert!(error.to_string().contains("LLM_SERVICE_ADDRESS"));
    }
}

#[test]
fn config_environment_leaves_literal_server_addresses_unchanged() {
    let content = "server_address = \"llm.example.com:8100\"";

    assert_eq!(
        resolve_config_environment(content, None, None, None, None).expect("literal URL"),
        content
    );
}

#[test]
fn config_environment_resolves_boolean_and_escaped_secret_placeholders() {
    let resolved = resolve_config_environment(
        "reset_admin_password = \"${RESET_ADMIN_PASSWORD}\"\nsecret_key = \"${SECRET_KEY}\"\napi_key = \"${LLM_SERVICE_API_KEY}\"",
        None,
        Some("true"),
        Some("secret-with-\"quote"),
        Some("api\\key"),
    )
    .expect("resolved config");
    let config: toml::Value = toml::from_str(&resolved).expect("resolved TOML");

    assert_eq!(config["reset_admin_password"].as_bool(), Some(true));
    assert_eq!(config["secret_key"].as_str(), Some("secret-with-\"quote"));
    assert_eq!(config["api_key"].as_str(), Some("api\\key"));
}

#[test]
fn config_environment_overrides_recovery_and_shared_secrets() {
    let mut config = Config::default();

    apply_config_environment(
        &mut config,
        Some("true"),
        Some("environment-secret"),
        Some("environment-api-key"),
    )
    .expect("environment overrides");

    assert!(config.server.reset_admin_password);
    assert_eq!(config.security.secret_key, "environment-secret");
    assert_eq!(config.llm.api_key, "environment-api-key");

    apply_config_environment(&mut config, Some("false"), None, None)
        .expect("disabled recovery override");
    assert!(!config.server.reset_admin_password);
}

#[test]
fn config_environment_rejects_invalid_recovery_and_empty_secrets() {
    for reset_admin_password in ["TRUE", "1", "yes", ""] {
        let error = apply_config_environment(
            &mut Config::default(),
            Some(reset_admin_password),
            None,
            None,
        )
        .expect_err("invalid recovery value must fail");
        assert!(error.to_string().contains("RESET_ADMIN_PASSWORD"));
    }
    for (secret_key, api_key, expected_name) in [
        (Some(""), None, "SECRET_KEY"),
        (Some("   "), None, "SECRET_KEY"),
        (None, Some(""), "LLM_SERVICE_API_KEY"),
        (None, Some("   "), "LLM_SERVICE_API_KEY"),
    ] {
        let error = apply_config_environment(&mut Config::default(), None, secret_key, api_key)
            .expect_err("empty secret must fail");
        assert!(error.to_string().contains(expected_name));
    }
}

#[test]
fn test_load_config_rejects_configurable_log_file_path() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(&dir, "[logging]\nfile_path = \"/var/log/momento.log\"\n");

    let error = load_config(&path).expect_err("Logging path must not be configurable");

    assert!(error.to_string().contains("logging"));
}

#[test]
fn test_load_config_reads_llm_async_submission_task_limit() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(
        &dir,
        "[llm_submission_worker]\nmax_async_submission_tasks = 17\npoll_interval_seconds = 2\n",
    );

    let config = load_config(&path).expect("Failed to load config");

    assert_eq!(config.llm_submission_worker.max_async_submission_tasks, 17);
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
fn test_load_config_rejects_renamed_llm_submission_max_in_flight() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(&dir, "[llm_submission_worker]\nmax_in_flight = 17\n");

    let error = load_config(&path).expect_err("Renamed submission setting must fail");

    assert!(error.to_string().contains("max_in_flight"));
}

#[test]
fn test_load_config_reads_llm_result_worker_concurrency() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(
        &dir,
        "[llm_result_worker]\npoll_interval_seconds = 3\nconcurrency = 7\n",
    );

    let config = load_config(&path).expect("Failed to load config");

    assert_eq!(config.llm_result_worker.poll_interval_seconds, 3);
    assert_eq!(config.llm_result_worker.concurrency, 7);
}

#[test]
fn test_load_config_rejects_removed_llm_result_worker_batch_size() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(&dir, "[llm_result_worker]\nbatch_size = 64\n");

    let error = load_config(&path).expect_err("Result worker batch size has been removed");

    assert!(error.to_string().contains("batch_size"));
}

#[test]
fn test_load_config_rejects_renamed_llm_result_cpu_processing_concurrency() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(
        &dir,
        "[llm_result_worker]\ncpu_processing_concurrency = 7\n",
    );

    let error = load_config(&path).expect_err("Renamed result worker setting must fail");

    assert!(error.to_string().contains("cpu_processing_concurrency"));
}

#[test]
fn test_load_config_rejects_invalid_llm_result_worker_settings() {
    for setting in ["poll_interval_seconds", "concurrency"] {
        let dir = TempDir::new().expect("Failed to create temp dir");
        let path = write_config(&dir, &format!("[llm_result_worker]\n{setting} = 0\n"));

        let error = load_config(&path).expect_err("Invalid result worker setting must fail");

        assert!(error.to_string().contains("llm result"), "{error}");
    }
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
fn test_llm_section_owns_ai_schedules_and_defaults() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[llm]\ndeduplicate_cron = \"30 3 * * *\"\n")
        .expect("Failed to write existing config");

    let config = load_config(&path).expect("LLM schedules should be valid");

    assert_eq!(config.llm.ocr_cron, "0 1 * * *");
    assert_eq!(config.llm.image_tagging_cron, "0 2 * * *");
    assert_eq!(config.llm.deduplicate_cron, "30 3 * * *");
    assert_eq!(config.llm.face_detection_cron, "0 4 * * *");
    assert_eq!(config.llm.image_aesthetics_cron, "0 5 * * *");
    assert_eq!(config.llm.screenshot_detection_cron, "0 6 * * *");
    assert_eq!(config.llm.document_detection_cron, "0 7 * * *");
}

#[tokio::test]
async fn config_manager_updates_ai_cron_and_preserves_config() {
    let directory = TempDir::new().expect("temporary directory");
    let path = write_config(
        &directory,
        "# keep this comment\n[server]\ndata_dir = \"/srv/momento\"\n\n[llm]\nocr_cron = \"0 1 * * *\"\nimage_tagging_cron = \"0 2 * * *\"\n",
    );

    let config = load_config(&path).expect("load config");
    let config_manager = ConfigManager::new(path.clone(), config);
    let mut config_updates = config_manager.subscribe();
    config_manager
        .update_llm_cron_expression("ocr_cron", "ocr", " 15  4 * * 1-5 ".to_string())
        .await
        .expect("update OCR cron");
    config_updates
        .changed()
        .await
        .expect("runtime config update");

    let updated = std::fs::read_to_string(&path).expect("updated config");
    assert!(updated.contains("# keep this comment"));
    assert!(updated.contains("ocr_cron = \"15 4 * * 1-5\""));
    assert!(updated.contains("image_tagging_cron = \"0 2 * * *\""));
    let before_invalid_update = updated;
    assert_eq!(config_manager.current().llm.ocr_cron, "15 4 * * 1-5");
    assert_eq!(config_updates.borrow().llm.ocr_cron, "15 4 * * 1-5");
    let error = config_manager
        .update_llm_cron_expression("ocr_cron", "ocr", "not a cron".to_string())
        .await
        .expect_err("invalid cron must fail");
    assert!(error.to_string().contains("invalid ocr cronjob"));
    assert_eq!(
        std::fs::read_to_string(&path).expect("unchanged config"),
        before_invalid_update
    );
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
fn test_load_config_reads_backup_settings_and_rejects_invalid_limits() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(
        &dir,
        "[backup]\nmax_upload_bytes = 4096\nmax_chunk_bytes = 1024\nmax_active_uploads_per_user = 3\nsession_expiry_hours = 12\nworker_poll_interval_seconds = 4\nworker_concurrency = 2\n",
    );

    let config = load_config(&path).expect("Failed to load backup config");
    assert_eq!(config.backup.max_upload_bytes, 4096);
    assert_eq!(config.backup.max_chunk_bytes, 1024);
    assert_eq!(config.backup.max_active_uploads_per_user, 3);
    assert_eq!(config.backup.session_expiry_hours, 12);
    assert_eq!(config.backup.worker_poll_interval_seconds, 4);
    assert_eq!(config.backup.worker_concurrency, 2);

    let path = write_config(
        &dir,
        "[backup]\nmax_upload_bytes = 1\nmax_chunk_bytes = 2\n",
    );
    let error = load_config(&path).expect_err("Chunk limit larger than upload limit must fail");
    assert!(error.to_string().contains("backup"));
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
    let generated = std::fs::read_to_string(&path).expect("Failed to read generated config");
    let resolved = resolve_config_environment(
        &generated,
        Some("llm-service:8100"),
        Some("false"),
        Some("generated-secret"),
        Some("generated-api-key"),
    )
    .expect("Failed to resolve generated config");
    std::fs::write(&path, resolved).expect("Failed to write resolved config");
    let config = load_config(&path).expect("Failed to reload saved config");

    assert_eq!(config.server.data_dir, PathBuf::from("/data"));
    assert_eq!(config.server.static_dir, PathBuf::from("/app/static"));
    assert_eq!(config.server.port, 8000);
    assert_eq!(config.server.api_request_body_max_bytes, 8_388_608);
    assert_eq!(config.server.request_log_body_max_bytes, 1_048_576);
    assert!(!config.server.reset_admin_password);
    assert!(config.llm.enabled);
    assert_eq!(config.metadata.thumbnails_max_size, 1200);

    assert_eq!(generated, default_config_template());
    assert!(generated.contains("Five-field cron expressions"));
    let generated: toml::Value = toml::from_str(&generated).expect("Generated config must be TOML");
    assert!(generated["metadata_worker"].get("batch_size").is_none());
    assert!(generated["llm_result_worker"].get("batch_size").is_none());
    assert!(generated.get("admin").is_none());
    assert!(generated.get("storage").is_none());
    assert_eq!(generated["server"]["data_dir"].as_str(), Some("/data"));
    assert_eq!(
        generated["webdav"]["max_upload_bytes"].as_integer(),
        Some(53_687_091_200)
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
fn test_load_config_reads_and_validates_server_body_limits() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(
        &dir,
        "[server]\napi_request_body_max_bytes = 4096\nrequest_log_body_max_bytes = 1024\n",
    );

    let config = load_config(&path).expect("Failed to load server body limits");
    assert_eq!(config.server.api_request_body_max_bytes, 4096);
    assert_eq!(config.server.request_log_body_max_bytes, 1024);

    for field in ["api_request_body_max_bytes", "request_log_body_max_bytes"] {
        let dir = TempDir::new().expect("Failed to create temp dir");
        let path = write_config(&dir, &format!("[server]\n{field} = 0\n"));
        let error = load_config(&path).expect_err("Zero body limit must fail");
        assert!(error.to_string().contains("server"), "{error}");
    }
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
fn test_load_config_uses_disabled_global_llm_default_when_section_is_missing() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[server]\nport = 9001\n").expect("Failed to write test config");

    let config = load_config(&path).expect("Missing LLM config should use safe defaults");

    assert!(!config.llm.enabled);
    assert_eq!(config.llm.ocr_cron, "0 1 * * *");
    assert_eq!(config.face_group.similarity_threshold, 0.50);
}

#[test]
fn test_load_config_rejects_removed_ai_feature_enablement_fields() {
    for removed_field in [
        "ocr_enabled",
        "image_tagging_enabled",
        "deduplicate_enabled",
        "face_detection_enabled",
        "image_aesthetics_enabled",
        "screenshot_detection_enabled",
        "document_detection_enabled",
    ] {
        let dir = TempDir::new().expect("Failed to create temp dir");
        let path = write_config(&dir, &format!("[llm]\n{removed_field} = true\n"));

        let error = load_config(&path).expect_err("Removed feature switch must fail");

        assert!(error.to_string().contains(removed_field), "{error}");
    }
}

#[test]
fn test_load_config_rejects_invalid_face_group_similarity_threshold() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(&dir, "[face_group]\nsimilarity_threshold = 1.1\n");

    let error = load_config(&path).expect_err("Invalid face similarity threshold must fail");

    assert!(error.to_string().contains("similarity_threshold"));
}

#[test]
fn test_load_config_rejects_invalid_face_representative_weights() {
    for invalid_weights in [
        "confidence_weight = -0.1\nface_size_weight = 0.2\ncenter_proximity_weight = 0.1\nfrontality_weight = 0.25\nvisibility_weight = 0.3\nfeature_clarity_weight = 0.25\n",
        "confidence_weight = 0.1\nface_size_weight = 0.1\ncenter_proximity_weight = 0.1\nfrontality_weight = 0.1\nvisibility_weight = 0.1\nfeature_clarity_weight = 0.1\n",
    ] {
        let dir = TempDir::new().expect("Failed to create temp dir");
        let path = write_config(
            &dir,
            &format!("[face_group]\n{invalid_weights}"),
        );

        let error = load_config(&path).expect_err("Invalid representative weights must fail");

        assert!(error.to_string().contains("face_group"));
    }
}

#[test]
fn test_load_config_rejects_each_invalid_ai_schedule() {
    for field in [
        "ocr_cron",
        "image_tagging_cron",
        "deduplicate_cron",
        "face_detection_cron",
        "image_aesthetics_cron",
        "screenshot_detection_cron",
        "document_detection_cron",
    ] {
        let dir = TempDir::new().expect("Failed to create temp dir");
        let path = write_config(&dir, &format!("[llm]\n{field} = \"invalid\"\n"));

        let error = load_config(&path).expect_err("Invalid schedule must fail");
        assert!(error.to_string().contains(field.trim_end_matches("_cron")));
    }
}

#[test]
fn playground_config_matches_the_generated_template() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../playground/config.toml");
    let playground = std::fs::read_to_string(path).expect("Playground config must exist");

    assert_eq!(playground, default_config_template());
}

#[test]
fn test_load_config_rejects_removed_cronjob_section() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(&dir, "[cronjob]\nocr_cron = \"0 1 * * *\"\n");

    let error = load_config(&path).expect_err("Removed cronjob section must fail");
    assert!(error.to_string().contains("cronjob"));
}

#[test]
fn test_load_config_requires_llm_server_address() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(&dir, "[llm]\nenabled = true\nserver_address = \"\"\n");

    let error = load_config(&path).expect_err("Enabled LLM must have a server address");

    assert!(error.to_string().contains("server_address"));
}

#[test]
fn test_load_config_requires_host_and_port_only_for_llm_server_address() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    for server_address in [
        "ws://127.0.0.1:8100",
        "127.0.0.1:8100/api/v1/llm/connect",
        "127.0.0.1",
        "127.0.0.1:0",
    ] {
        let path = write_config(
            &dir,
            &format!(
                "[llm]\nenabled = true\nserver_address = \"{server_address}\"\nclient_id = \"client_a\"\napi_key = \"key\"\n"
            ),
        );
        let error = load_config(&path).expect_err("invalid LLM server address must fail");
        assert!(error.to_string().contains("server_address"));
    }
}

#[test]
fn test_load_config_requires_websocket_client_identity_and_key() {
    let dir = TempDir::new().expect("Failed to create temp dir");

    let path = write_config(
        &dir,
        "[llm]\nenabled = true\nserver_address = \"127.0.0.1:8100\"\napi_key = \"key\"\n",
    );
    let error = load_config(&path).expect_err("LLM client ID must be required");
    assert!(error.to_string().contains("client_id"));

    let path = write_config(
        &dir,
        "[llm]\nenabled = true\nserver_address = \"127.0.0.1:8100\"\nclient_id = \"client_a\"\n",
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
        "[llm]\nenabled = true\nserver_address = \"127.0.0.1:8100\"\ninference_endpoint = \"/custom\"\n",
    );

    let error = load_config(&path).expect_err("LLM inference endpoint is not configurable");

    assert!(error.to_string().contains("inference_endpoint"));
}

#[test]
fn test_load_config_rejects_removed_llm_service_url() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(
        &dir,
        "[llm]\nenabled = true\nservice_url = \"ws://127.0.0.1:8100/api/v1/llm/connect\"\n",
    );

    let error = load_config(&path).expect_err("removed service_url must fail");
    assert!(error.to_string().contains("service_url"));
}

#[test]
fn test_load_config_rejects_removed_deduplicate_section() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(&dir, "[deduplicate]\nenabled = true\n");

    let error = load_config(&path).expect_err("Deduplicate settings moved to llm");

    assert!(error.to_string().contains("deduplicate"));
}
