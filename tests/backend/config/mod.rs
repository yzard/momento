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
fn test_load_config_reads_storage_paths() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(
        &dir,
        "[storage]\ndata_dir = \"/srv/momento/data\"\nstatic_dir = \"/srv/momento/static\"\n",
    );

    let config = load_config(&path).expect("Failed to load config");

    assert_eq!(config.storage.data_dir, PathBuf::from("/srv/momento/data"));
    assert_eq!(
        config.storage.static_dir,
        PathBuf::from("/srv/momento/static")
    );
}

#[test]
fn test_load_config_reads_log_file_path() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = write_config(&dir, "[logging]\nfile_path = \"/var/log/momento.log\"\n");

    let config = load_config(&path).expect("Failed to load config");

    assert_eq!(
        config.logging.file_path,
        PathBuf::from("/var/log/momento.log")
    );
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
    assert_eq!(config.storage.data_dir, defaults.storage.data_dir);
    assert_eq!(config.storage.static_dir, defaults.storage.static_dir);
}

#[test]
fn test_storage_defaults_match_container_layout() {
    let config = Config::default();

    assert_eq!(config.storage.data_dir, PathBuf::from("/data"));
    assert_eq!(config.storage.static_dir, PathBuf::from("/app/static"));
}

#[test]
fn test_save_default_config_round_trips() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = dir.path().join("generated").join("config.toml");

    save_default_config(&path).expect("Failed to save default config");
    let config = load_config(&path).expect("Failed to reload saved config");
    let defaults = Config::default();

    assert_eq!(config.storage.data_dir, defaults.storage.data_dir);
    assert_eq!(config.storage.static_dir, defaults.storage.static_dir);
    assert_eq!(config.server.port, defaults.server.port);

    let generated = std::fs::read_to_string(path).expect("Failed to read generated config");
    let generated: toml::Value = toml::from_str(&generated).expect("Generated config must be TOML");
    assert!(generated["metadata_worker"].get("batch_size").is_none());
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
