use std::os::unix::fs::PermissionsExt;

use momento_common::config_file::write_new_config;
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
