#[path = "../../src/common/build_version.rs"]
mod version_build_script;

#[test]
fn release_version_matches_the_common_package() {
    let release_version = include_str!("../../src/backend/version.txt").trim();

    assert_eq!(momento_common::VERSION, release_version);
    assert_eq!(env!("CARGO_PKG_VERSION"), release_version);
}

#[test]
fn release_version_validation_accepts_supported_semantic_versions() {
    let _build_script_entrypoint: fn() = version_build_script::main;
    for version in ["1.0.0", "2.4.6-beta.1", "2.4.6+build.7"] {
        assert!(version_build_script::is_release_version(version));
    }
}

#[test]
fn release_version_validation_rejects_invalid_versions() {
    for version in ["", "1", "1.0", "1.0.x", "1.0.0-", "v1.0.0", "1.0.0 beta"] {
        assert!(!version_build_script::is_release_version(version));
    }
}
