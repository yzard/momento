use momento_api::utils::path::{
    resolve_existing_storage_path, resolve_existing_storage_path_sync, resolve_storage_path,
};

#[test]
fn storage_paths_reject_absolute_and_parent_components() {
    let root = std::path::Path::new("/data/originals");

    assert!(resolve_storage_path(root, "album/photo.jpg").is_ok());
    assert!(resolve_storage_path(root, "../config.toml").is_err());
    assert!(resolve_storage_path(root, "/etc/passwd").is_err());
    assert!(resolve_storage_path(root, "album/./photo.jpg").is_err());
    assert!(resolve_storage_path(root, "").is_err());
}

#[tokio::test]
async fn existing_storage_paths_reject_symlink_escapes() {
    let storage = tempfile::tempdir().expect("storage directory");
    let outside = tempfile::tempdir().expect("outside directory");
    let outside_file = outside.path().join("secret.txt");
    std::fs::write(&outside_file, "secret").expect("outside file");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside_file, storage.path().join("escape.txt"))
        .expect("storage symlink");

    #[cfg(unix)]
    assert!(resolve_existing_storage_path(storage.path(), "escape.txt")
        .await
        .is_err());
    #[cfg(unix)]
    assert!(resolve_existing_storage_path_sync(storage.path(), "escape.txt").is_err());
}
