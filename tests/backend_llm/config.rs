use llm_service::config::{Config, DEFAULT_CONFIG_TEMPLATE};
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
    let config = Config::load(&path).expect("Playground TOML configuration should load");
    let playground = std::fs::read_to_string(&path).expect("Playground config must exist");
    let clustering = config.service_for("image_clustering").unwrap();
    let face_detection = config.service_for("face_detection").unwrap();
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
    assert_eq!(face_detection.max_concurrent_jobs, 16);
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
    assert_eq!(playground, DEFAULT_CONFIG_TEMPLATE);
}

#[test]
fn saves_commented_operational_default_configuration() {
    let directory = TempDir::new().expect("Failed to create config fixture");
    let path = directory.path().join("config").join("config_llm.toml");

    Config::save_default(&path).expect("Default config should be saved");
    let generated = std::fs::read_to_string(&path).expect("Generated config must be readable");
    let config = Config::load(&path).expect("Generated config must load");

    assert_eq!(generated, DEFAULT_CONFIG_TEMPLATE);
    assert!(generated.contains("# Durable results are retried"));
    assert_eq!(config.service.len(), 4);
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
