mod defaults;

use llm_service::config::{
    apply_config_environment, default_config_template, resolve_config_environment, Config,
};
use std::io::Write;
use tempfile::{NamedTempFile, TempDir};

#[test]
fn release_version_matches_the_llm_service_package() {
    let release_version = include_str!("../../src/backend/version.txt").trim();

    assert_eq!(llm_service::VERSION, release_version);
    assert_eq!(env!("CARGO_PKG_VERSION"), release_version);
}

fn local_ocr_configuration(extra: &str) -> String {
    format!(
        "[server]\napi_key = \"test-key\"\n\n[[service]]\nenabled = true\nmodel_type = \"ocr\"\nmax_tokens = 1\nmax_concurrent_jobs = 1\n{extra}"
    )
}

fn local_face_configuration(face_detection_size: u32, extra: &str) -> String {
    format!(
        "{}\n[[service]]\nenabled = true\nmodel_type = \"face_detection\"\nstartup_timeout_seconds = 1\nrequest_timeout_seconds = 1\ncpu_processing_concurrency = 8\nmodel_concurrency = 8\nface_detection_size = {face_detection_size}\nrecognition_batch_size = 64\nrecognition_batch_wait_milliseconds = 5\nminimum_face_likelihood = 0.8\nminimum_face_resolution_pixels = 100\n{extra}",
        local_ocr_configuration("")
    )
}

#[test]
fn loads_operational_runtime_configuration() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(file, "{}", local_ocr_configuration("")).expect("Failed to write config fixture");

    let config = Config::load(file.path()).expect("Config should load");

    assert_eq!(config.service[0].model_type, "ocr");
    assert_eq!(config.service[0].max_concurrent_jobs, Some(1));
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
        "embedding_dimensions = 768",
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
    assert_eq!(ocr.max_concurrent_jobs, Some(100));
    assert_eq!(tagging.max_concurrent_jobs, Some(8));
    assert_eq!(clustering.cpu_processing_concurrency, Some(16));
    assert_eq!(clustering.model_concurrency, Some(16));
    assert_eq!(clustering.model_batch_wait_milliseconds, Some(5));
    assert_eq!(aesthetics.cpu_processing_concurrency, Some(16));
    assert_eq!(aesthetics.model_concurrency, Some(64));
    assert_eq!(aesthetics.model_batch_wait_milliseconds, Some(5));
    assert_eq!(face_detection.cpu_processing_concurrency, Some(8));
    assert_eq!(face_detection.model_concurrency, Some(8));
    assert_eq!(face_detection.face_detection_size, Some(960));
    assert_eq!(face_detection.recognition_batch_size, Some(64));
    assert_eq!(face_detection.recognition_batch_wait_milliseconds, Some(5));
    assert_eq!(screenshot_detection.cpu_processing_concurrency, Some(8));
    assert_eq!(screenshot_detection.model_concurrency, Some(8));
    assert_eq!(document_detection.cpu_processing_concurrency, Some(8));
    assert_eq!(document_detection.model_concurrency, Some(8));
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
fn validates_image_aesthetics_staged_concurrency() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "{}\n[[service]]\nenabled = true\nmodel_type = \"image_aesthetics\"\nstartup_timeout_seconds = 1\nrequest_timeout_seconds = 1\ncpu_processing_concurrency = 3\nmodel_concurrency = 64\nmodel_batch_wait_milliseconds = 5\n",
        local_ocr_configuration("")
    )
    .expect("Failed to write config fixture");

    let config = Config::load(file.path()).expect("Image aesthetics config should load");
    let aesthetics = config
        .service_for("image_aesthetics")
        .expect("enabled aesthetics service");

    assert_eq!(aesthetics.cpu_processing_concurrency, Some(3));
    assert_eq!(aesthetics.model_concurrency, Some(64));
    assert_eq!(aesthetics.model_batch_wait_milliseconds, Some(5));
    assert_eq!(aesthetics.startup_timeout_seconds, 1);
    assert_eq!(aesthetics.request_timeout_seconds, 1);
}

#[test]
fn validates_image_clustering_staged_concurrency() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "{}\n[[service]]\nenabled = true\nmodel_type = \"image_clustering\"\nstartup_timeout_seconds = 1\nrequest_timeout_seconds = 1\ncpu_processing_concurrency = 16\nmodel_concurrency = 16\nmodel_batch_wait_milliseconds = 5\n",
        local_ocr_configuration("")
    )
    .expect("Failed to write config fixture");

    let config = Config::load(file.path()).expect("Image clustering config should load");
    let clustering = config
        .service_for("image_clustering")
        .expect("enabled clustering service");

    assert_eq!(clustering.cpu_processing_concurrency, Some(16));
    assert_eq!(clustering.configured_model_concurrency().unwrap(), 16);
    assert_eq!(clustering.model_batch_wait_milliseconds, Some(5));
}

#[test]
fn validates_both_classifier_services_without_model_configuration() {
    for model_type in ["screenshot_detection", "document_detection"] {
        let mut file = NamedTempFile::new().expect("Failed to create config fixture");
        write!(
            file,
            "{}\n[[service]]\nenabled = true\nmodel_type = \"{model_type}\"\nstartup_timeout_seconds = 1\nrequest_timeout_seconds = 1\ncpu_processing_concurrency = 3\nmodel_concurrency = 2\n",
            local_ocr_configuration("")
        )
        .expect("Failed to write config fixture");

        let config = Config::load(file.path()).expect("classifier config should load");
        let classifier = config
            .service_for(model_type)
            .expect("enabled classifier service");

        assert_eq!(classifier.cpu_processing_concurrency, Some(3));
        assert_eq!(classifier.configured_model_concurrency().unwrap(), 2);
        assert_eq!(classifier.startup_timeout_seconds, 1);
        assert_eq!(classifier.request_timeout_seconds, 1);
    }
}

#[test]
fn face_detection_requires_staged_concurrency_and_batch_configuration() {
    let face_configuration = local_face_configuration(960, "");
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(file, "{face_configuration}").expect("Failed to write config fixture");

    let config = Config::load(file.path()).expect("face detection config should load");
    let face_detection = config
        .service_for("face_detection")
        .expect("enabled face service");

    assert_eq!(face_detection.configured_model_concurrency().unwrap(), 8);
    assert_eq!(face_detection.face_detection_size, Some(960));
    assert_eq!(face_detection.recognition_batch_size, Some(64));
}

#[test]
fn face_detection_rejects_unsupported_detection_sizes() {
    for unsupported_size in [0, 800, 1024] {
        let mut file = NamedTempFile::new().expect("Failed to create config fixture");
        write!(file, "{}", local_face_configuration(unsupported_size, ""))
            .expect("Failed to write config fixture");

        let error = Config::load(file.path()).expect_err("unsupported size must fail");

        assert!(error.to_string().contains("640, 960, or 1280"));
    }
}

#[test]
fn face_detection_accepts_each_supported_detection_size() {
    for supported_size in [640, 960, 1280] {
        let mut file = NamedTempFile::new().expect("Failed to create config fixture");
        write!(file, "{}", local_face_configuration(supported_size, ""))
            .expect("Failed to write config fixture");

        let config = Config::load(file.path()).expect("supported size should load");

        assert_eq!(
            config
                .service_for("face_detection")
                .expect("enabled face service")
                .face_detection_size,
            Some(supported_size)
        );
    }
}

#[test]
fn face_detection_rejects_removed_max_concurrent_jobs() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "{}",
        local_face_configuration(960, "max_concurrent_jobs = 8\n")
    )
    .expect("Failed to write config fixture");

    let error = Config::load(file.path()).expect_err("old face concurrency must fail");

    assert!(error.to_string().contains("not max_concurrent_jobs"));
}

#[test]
fn classifier_services_reject_the_removed_max_concurrent_jobs_field() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "{}\n[[service]]\nenabled = true\nmodel_type = \"screenshot_detection\"\nstartup_timeout_seconds = 1\nrequest_timeout_seconds = 1\nmax_concurrent_jobs = 2\ncpu_processing_concurrency = 2\nmodel_concurrency = 2\n",
        local_ocr_configuration("")
    )
    .expect("Failed to write config fixture");

    let error = Config::load(file.path()).expect_err("old classifier concurrency must fail");

    assert!(error.to_string().contains("not max_concurrent_jobs"));
}

#[test]
fn classifier_services_require_positive_cpu_and_model_concurrency() {
    for (field, value) in [
        ("cpu_processing_concurrency", "0"),
        ("model_concurrency", "0"),
    ] {
        let configuration = format!(
            "{}\n[[service]]\nenabled = true\nmodel_type = \"screenshot_detection\"\nstartup_timeout_seconds = 1\nrequest_timeout_seconds = 1\ncpu_processing_concurrency = 2\nmodel_concurrency = 2\n",
            local_ocr_configuration("")
        )
        .replace(&format!("{field} = 2"), &format!("{field} = {value}"));
        let mut file = NamedTempFile::new().expect("Failed to create config fixture");
        write!(file, "{configuration}").expect("Failed to write config fixture");

        let error = Config::load(file.path()).expect_err("zero concurrency must fail");

        assert!(error.to_string().contains(field));
    }
}

#[test]
fn classifier_services_reject_removed_concurrency_names() {
    for removed_field in [
        "cpu_resize_concurrency = 2",
        "model_screenshot_detection_concurrency = 2",
        "model_document_detection_concurrency = 2",
    ] {
        let mut file = NamedTempFile::new().expect("Failed to create config fixture");
        write!(
            file,
            "{}\n[[service]]\nenabled = true\nmodel_type = \"screenshot_detection\"\nstartup_timeout_seconds = 1\nrequest_timeout_seconds = 1\ncpu_processing_concurrency = 2\nmodel_concurrency = 2\n{removed_field}\n",
            local_ocr_configuration("")
        )
        .expect("Failed to write config fixture");

        let error = Config::load(file.path()).expect_err("removed field must fail");

        assert!(error.to_string().contains("unknown field"));
        assert!(error
            .to_string()
            .contains(removed_field.split(' ').next().unwrap()));
    }
}

#[test]
fn standard_services_reject_classifier_concurrency_fields() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "{}\n[[service]]\nenabled = true\nmodel_type = \"image_tagging\"\nstartup_timeout_seconds = 1\nrequest_timeout_seconds = 1\nmax_concurrent_jobs = 2\ncpu_processing_concurrency = 2\n",
        local_ocr_configuration("")
    )
    .expect("Failed to write config fixture");

    let error = Config::load(file.path()).expect_err("classifier concurrency must fail");

    assert!(error
        .to_string()
        .contains("does not accept staged concurrency fields"));
}

#[test]
fn image_aesthetics_rejects_removed_max_concurrent_jobs() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "{}\n[[service]]\nenabled = true\nmodel_type = \"image_aesthetics\"\nstartup_timeout_seconds = 1\nrequest_timeout_seconds = 1\nmax_concurrent_jobs = 16\ncpu_processing_concurrency = 8\nmodel_concurrency = 64\nmodel_batch_wait_milliseconds = 5\n",
        local_ocr_configuration("")
    )
    .expect("Failed to write config fixture");

    let error = Config::load(file.path()).expect_err("old aesthetics concurrency must fail");

    assert!(error.to_string().contains("not max_concurrent_jobs"));
}

#[test]
fn image_clustering_rejects_removed_max_concurrent_jobs() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "{}\n[[service]]\nenabled = true\nmodel_type = \"image_clustering\"\nstartup_timeout_seconds = 1\nrequest_timeout_seconds = 1\nmax_concurrent_jobs = 32\ncpu_processing_concurrency = 16\nmodel_concurrency = 16\nmodel_batch_wait_milliseconds = 5\n",
        local_ocr_configuration("")
    )
    .expect("Failed to write config fixture");

    let error = Config::load(file.path()).expect_err("old clustering concurrency must fail");

    assert!(error.to_string().contains("not max_concurrent_jobs"));
}
