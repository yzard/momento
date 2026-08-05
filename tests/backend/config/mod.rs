use std::io::ErrorKind;
use std::path::PathBuf;

use momento_api::config::{load_config, save_default_config, Config};
use tempfile::TempDir;

fn write_config(dir: &TempDir, contents: &str) -> PathBuf {
    let path = dir.path().join("config.toml");
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
}
