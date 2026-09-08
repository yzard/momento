#[test]
fn fixtures_live_below_build_tmp_and_cleanup_on_drop() {
    let root = super::root();
    let directory = super::tempdir().unwrap();
    let file = super::tempfile().unwrap();
    let directory_path = directory.path().to_owned();
    let file_path = file.path().to_owned();
    assert_eq!(directory_path.parent(), Some(root.as_path()));
    assert_eq!(file_path.parent(), Some(root.as_path()));
    drop(directory);
    drop(file);
    assert!(!directory_path.exists());
    assert!(!file_path.exists());
}

#[test]
fn exit_child() {
    let Ok(marker) = std::env::var("MOMENTO_TEMP_CLEANUP_TEST_MARKER") else {
        return;
    };
    std::fs::write(marker, super::root().to_str().unwrap()).unwrap();
    // Deliberately model fixtures held by static database/runtime registries.
    std::mem::forget(super::tempdir().unwrap());
    std::mem::forget(super::tempfile().unwrap());
    if std::env::var("MOMENTO_TEMP_CLEANUP_TEST_FAIL").unwrap() == "true" {
        panic!("intentional failing test to verify process cleanup");
    }
}

#[test]
fn process_exit_cleans_retained_fixtures_after_success_and_failure() {
    let parent_fixture = super::tempdir().unwrap();
    let marker = parent_fixture.path().join("child-root");
    for fail in [false, true] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "temporary::tests::exit_child"])
            .env("MOMENTO_TEMP_CLEANUP_TEST_MARKER", &marker)
            .env("MOMENTO_TEMP_CLEANUP_TEST_FAIL", fail.to_string())
            .output()
            .unwrap();
        assert_eq!(output.status.success(), !fail, "{output:?}");
        let child_root = std::path::PathBuf::from(std::fs::read_to_string(&marker).unwrap());
        assert!(
            !child_root.exists(),
            "child temporary directory leaked: {}",
            child_root.display()
        );
        assert!(
            parent_fixture.path().exists(),
            "child removed another process's fixtures"
        );
    }
}
