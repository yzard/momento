mod defaults;

use llm_service::config::{
    apply_config_environment, default_config_template, resolve_config_environment, Config,
};
use std::io::Write;
use tempfile::{NamedTempFile, TempDir};

fn local_ocr_configuration(extra: &str) -> String {
    format!(
        "[server]\napi_key = \"test-key\"\n\n[[service]]\nenabled = true\nmodel_type = \"ocr\"\nmax_tokens = 1\nmax_concurrent_jobs = 1\n{extra}"
    )
}

#[test]
fn loads_operational_runtime_configuration() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(file, "{}", local_ocr_configuration("")).expect("Failed to write config fixture");

    let config = Config::load(file.path()).expect("Config should load");

    assert_eq!(config.service[0].model_type, "ocr");
    assert_eq!(config.service[0].max_concurrent_jobs, 1);
}

#[test]
fn api_key_environment_overrides_the_llm_service_config() {
    let mut config = Config::default();

    apply_config_environment(&mut config, Some("environment-api-key")).expect("API key override");

    assert_eq!(config.server.api_key, "environment-api-key");
}

#[test]
fn api_key_environment_rejects_empty_values() {
    for api_key in ["", "   "] {
        let error = apply_config_environment(&mut Config::default(), Some(api_key))
            .expect_err("empty API key must fail");
        assert!(error.to_string().contains("LLM_SERVICE_API_KEY"));
    }
}

#[test]
fn api_key_environment_resolves_an_escaped_toml_placeholder() {
    let resolved = resolve_config_environment(
        "api_key = \"${LLM_SERVICE_API_KEY}\"",
        Some("api-key-with-\"quote"),
    )
    .expect("resolved API key");
    let config: toml::Value = toml::from_str(&resolved).expect("resolved TOML");

    assert_eq!(config["api_key"].as_str(), Some("api-key-with-\"quote"));
}

#[test]
fn rejects_removed_runtime_deployment_configuration() {
    for removed_field in [
        "docker_command = [\"docker\"]",
        "base_url = \"http://127.0.0.1:8400/v1\"",
        "model = \"baidu/Unlimited-OCR\"",
        "script_path = \"runtime.py\"",
        "device = \"cuda\"",
        "embedding_dimensions = 384",
        "model_version = \"unlimited_ocr\"",
    ] {
        let mut file = NamedTempFile::new().expect("Failed to create config fixture");
        write!(file, "{}", local_ocr_configuration(removed_field))
            .expect("Failed to write config fixture");

        let error = Config::load(file.path()).expect_err("removed field must be rejected");

        assert!(error.to_string().contains("unknown field"), "{error}");
    }
}

#[test]
fn rejects_removed_general_and_storage_sections() {
    for section in ["general", "storage"] {
        let mut file = NamedTempFile::new().expect("Failed to create config fixture");
        write!(
            file,
            "{}\n[{section}]\ndata_dir = \"/legacy\"\n",
            local_ocr_configuration("")
        )
        .expect("Failed to write config fixture");

        let error = Config::load(file.path()).expect_err("replaced section must be rejected");

        assert!(error.to_string().contains(section));
    }
}

#[test]
fn rejects_removed_callback_and_nested_scheduler_sections() {
    for configuration in [
        format!(
            "{}\n[callback]\nmax_attempts = 10\n",
            local_ocr_configuration("")
        ),
        format!(
            "{}\n[server.scheduler]\nmax_in_flight_jobs = 1\n",
            local_ocr_configuration("")
        ),
    ] {
        let mut file = NamedTempFile::new().expect("Failed to create config fixture");
        write!(file, "{configuration}").expect("Failed to write config fixture");

        let error = Config::load(file.path()).expect_err("removed section must be rejected");
        assert!(error.to_string().contains("unknown field"), "{error}");
    }
}

#[test]
fn requires_the_shared_websocket_api_key() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "[server]\n\n[[service]]\nenabled = true\nmodel_type = \"ocr\"\nmax_tokens = 1\nmax_concurrent_jobs = 1\n"
    )
    .expect("Failed to write config fixture");

    let error = Config::load(file.path()).expect_err("API key must be required");
    assert!(error.to_string().contains("server.api_key"));
}

#[test]
fn server_data_dir_derives_runtime_directories() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "[server]\napi_key = \"test-key\"\ndata_dir = \"/srv/momento\"\n\n[[service]]\nenabled = true\nmodel_type = \"ocr\"\nmax_tokens = 1\nmax_concurrent_jobs = 1\n"
    )
    .expect("Failed to write config fixture");

    let config = Config::load(file.path()).expect("server data directory must load");

    assert_eq!(config.server.data_dir, std::path::Path::new("/srv/momento"));
    assert_eq!(
        config.server.llm_dir(),
        std::path::Path::new("/srv/momento/llm")
    );
    assert_eq!(
        config.server.queue_dir(),
        std::path::Path::new("/srv/momento/llm/queue")
    );
    assert_eq!(
        config.server.processing_dir(),
        std::path::Path::new("/srv/momento/llm/queue/processing")
    );
    assert_eq!(
        config.server.cache_dir(),
        std::path::Path::new("/srv/momento/llm/cache")
    );
}

#[test]
fn rejects_configurable_queue_directory() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "[server]\napi_key = \"test-key\"\nqueue_dir = \"/separate/queue\"\n\n[[service]]\nenabled = true\nmodel_type = \"ocr\"\nmax_tokens = 1\nmax_concurrent_jobs = 1\n"
    )
    .expect("Failed to write config fixture");

    let error = Config::load(file.path()).expect_err("queue directory must not be configurable");

    assert!(error.to_string().contains("queue_dir"));
}

#[test]
fn rejects_configurable_logging_path() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "{}\n[logging]\nfile_path = \"/var/log/llm-service.log\"\n",
        local_ocr_configuration("")
    )
    .expect("Failed to write config fixture");

    let error = Config::load(file.path()).expect_err("Logging path must not be configurable");

    assert!(error.to_string().contains("logging"));
}

#[test]
fn loads_playground_toml_configuration() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../playground/config_llm.toml");
    let playground = std::fs::read_to_string(&path).expect("Playground config must exist");
    let resolved = resolve_config_environment(&playground, Some("change-me-llm-service-key"))
        .expect("Playground API key should resolve");
    let mut file = NamedTempFile::new().expect("Resolved playground config fixture");
    write!(file, "{resolved}").expect("Resolved playground config");
    let config = Config::load(file.path()).expect("Playground TOML configuration should load");
    let clustering = config.service_for("image_clustering").unwrap();
    let aesthetics = config.service_for("image_aesthetics").unwrap();
    let face_detection = config.service_for("face_detection").unwrap();
    let screenshot_detection = config.service_for("screenshot_detection").unwrap();
    let document_detection = config.service_for("document_detection").unwrap();
    let tagging = config.service_for("image_tagging").unwrap();
    let ocr = config.service_for("ocr").unwrap();

    assert_eq!(config.scheduler.max_in_flight_jobs, 128);
    assert_eq!(config.server.data_dir, std::path::Path::new("/data"));
    assert_eq!(
        config.server.queue_dir(),
        std::path::Path::new("/data/llm/queue")
    );
    assert_eq!(
        config.server.cache_dir(),
        std::path::Path::new("/data/llm/cache")
    );
    assert_eq!(ocr.max_concurrent_jobs, 100);
    assert_eq!(tagging.max_concurrent_jobs, 16);
    assert_eq!(clustering.max_concurrent_jobs, 32);
    assert_eq!(aesthetics.max_concurrent_jobs, 16);
    assert_eq!(face_detection.max_concurrent_jobs, 16);
    assert_eq!(screenshot_detection.max_concurrent_jobs, 8);
    assert_eq!(document_detection.max_concurrent_jobs, 8);
    assert_eq!(
        config.scheduler.result_delivery_max_concurrent_deliveries,
        16
    );
    assert!(face_detection
        .minimum_face_likelihood
        .is_some_and(|value| value > 0.0 && value <= 1.0));
    assert!(face_detection
        .minimum_face_resolution_pixels
        .is_some_and(|resolution| resolution > 0));
    assert_eq!(playground, default_config_template());
}

#[test]
fn saves_commented_operational_default_configuration() {
    let directory = TempDir::new().expect("Failed to create config fixture");
    let path = directory.path().join("config").join("config_llm.toml");

    Config::save_default(&path).expect("Default config should be saved");
    let generated = std::fs::read_to_string(&path).expect("Generated config must be readable");
    let resolved = resolve_config_environment(&generated, Some("change-me-llm-service-key"))
        .expect("Generated API key should resolve");
    std::fs::write(&path, resolved).expect("Resolved config must be writable");
    let config = Config::load(&path).expect("Generated config must load");

    assert_eq!(generated, default_config_template());
    assert!(generated.contains("# Durable results are retried"));
    assert_eq!(config.service.len(), 7);
    assert_eq!(config.server.api_key, "change-me-llm-service-key");
}

#[test]
fn default_configuration_does_not_replace_an_existing_file() {
    let directory = TempDir::new().expect("Failed to create config fixture");
    let path = directory.path().join("config_llm.toml");
    std::fs::write(&path, "existing").expect("Failed to write existing config");

    let error = Config::save_default(&path).expect_err("Existing config must not be replaced");

    assert!(error.to_string().contains("File exists"));
    assert_eq!(std::fs::read_to_string(path).unwrap(), "existing");
}

#[test]
fn rejects_non_positive_scheduler_configuration() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "{}\n[scheduler]\nmax_in_flight_jobs = 0\n",
        local_ocr_configuration("")
    )
    .expect("Failed to write config fixture");

    let error = Config::load(file.path()).expect_err("zero in-flight window must be rejected");
    assert!(error.to_string().contains("max in-flight jobs"));
}

#[test]
fn rejects_enabled_service_without_concurrency_limit() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    let configuration =
        local_ocr_configuration("").replace("max_concurrent_jobs = 1", "max_concurrent_jobs = 0");
    write!(file, "{configuration}").expect("Failed to write config fixture");

    let error = Config::load(file.path()).expect_err("zero limit must be rejected");
    assert!(error.to_string().contains("max_concurrent_jobs"));
}

#[test]
fn validates_image_aesthetics_timeouts_without_model_configuration() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "{}\n[[service]]\nenabled = true\nmodel_type = \"image_aesthetics\"\nstartup_timeout_seconds = 1\nrequest_timeout_seconds = 1\nmax_concurrent_jobs = 2\n",
        local_ocr_configuration("")
    )
    .expect("Failed to write config fixture");

    let config = Config::load(file.path()).expect("Image aesthetics config should load");
    let aesthetics = config
        .service_for("image_aesthetics")
        .expect("enabled aesthetics service");

    assert_eq!(aesthetics.max_concurrent_jobs, 2);
    assert_eq!(aesthetics.startup_timeout_seconds, 1);
    assert_eq!(aesthetics.request_timeout_seconds, 1);
}

#[test]
fn validates_both_classifier_services_without_model_configuration() {
    for model_type in ["screenshot_detection", "document_detection"] {
        let mut file = NamedTempFile::new().expect("Failed to create config fixture");
        write!(
            file,
            "{}\n[[service]]\nenabled = true\nmodel_type = \"{model_type}\"\nstartup_timeout_seconds = 1\nrequest_timeout_seconds = 1\nmax_concurrent_jobs = 2\n",
            local_ocr_configuration("")
        )
        .expect("Failed to write config fixture");

        let config = Config::load(file.path()).expect("classifier config should load");
        let classifier = config
            .service_for(model_type)
            .expect("enabled classifier service");

        assert_eq!(classifier.max_concurrent_jobs, 2);
        assert_eq!(classifier.startup_timeout_seconds, 1);
        assert_eq!(classifier.request_timeout_seconds, 1);
    }
}
