use std::env;
use std::fs;
use std::path::PathBuf;

pub fn main() {
    let manifest_directory = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").expect("Cargo must provide CARGO_MANIFEST_DIR"),
    );
    let source_directory = manifest_directory
        .parent()
        .expect("backend crate must be below the source directory");
    let version_path = source_directory.join("backend/version.txt");
    println!("cargo:rerun-if-changed={}", version_path.display());

    let version_file = fs::read_to_string(&version_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", version_path.display()));
    let release_version = version_file.trim();
    assert!(
        is_release_version(release_version),
        "{} must contain a semantic version",
        version_path.display()
    );

    let package_version =
        env::var("CARGO_PKG_VERSION").expect("Cargo must provide CARGO_PKG_VERSION");
    assert_eq!(
        package_version,
        release_version,
        "Cargo package version must match {}",
        version_path.display()
    );

    println!("cargo:rustc-env=MOMENTO_VERSION={release_version}");
}

pub fn is_release_version(version: &str) -> bool {
    let core_end = version.find(['-', '+']).unwrap_or(version.len());
    let core_version = &version[..core_end];
    let core_parts = core_version.split('.').collect::<Vec<_>>();
    if core_parts.len() != 3
        || core_parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return false;
    }

    if core_end == version.len() {
        return true;
    }

    let suffix = &version[core_end + 1..];
    !suffix.is_empty()
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
}
