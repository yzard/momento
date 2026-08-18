use std::os::unix::fs::PermissionsExt;

use momento_common::config_file::{replace_config, write_new_config};
use tempfile::TempDir;

#[test]
fn writes_private_config_atomically_without_overwriting() {
    let directory = TempDir::new().expect("Failed to create config fixture");
    let path = directory.path().join("nested").join("config.toml");

    write_new_config(&path, "first").expect("Config should be written");
    let error = write_new_config(&path, "second").expect_err("Config must not be overwritten");

    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "first");
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn replaces_existing_config_atomically() {
    let directory = TempDir::new().expect("Failed to create config fixture");
    let path = directory.path().join("config.toml");
    write_new_config(&path, "first").expect("Config should be written");

    replace_config(&path, "second").expect("Config should be replaced");

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn replacing_missing_config_is_an_error() {
    let directory = TempDir::new().expect("Failed to create config fixture");
    let path = directory.path().join("config.toml");

    let error = replace_config(&path, "contents").expect_err("Config must already exist");

    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn replaces_config_with_a_filename_only_relative_path() {
    let unique = format!(
        ".momento-config-replace-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    );
    let path = std::path::PathBuf::from(unique);
    std::fs::write(&path, "first").expect("write relative config");

    replace_config(&path, "second").expect("replace relative config");

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
    std::fs::remove_file(path).expect("remove relative config");
}
