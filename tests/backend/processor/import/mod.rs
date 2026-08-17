use std::sync::Arc;
use std::time::Duration;

use momento_api::{
    config::Config,
    constants::paths,
    processor::import::{recover_webdav_claims, run_webdav_import_cycle},
};

use crate::test_utils::{create_test_db, create_test_user, init_test_paths, lock_webdav_test};

#[tokio::test]
async fn test_webdav_import_waits_for_active_uploads_before_claiming() {
    let _webdav_test_guard = lock_webdav_test().await;
    init_test_paths();
    let pool = create_test_db();
    let username = format!("import-gate-{}", uuid::Uuid::new_v4());
    create_test_user(&pool, &username, "import-gate@example.com");
    let user_root = paths().webdav.join(&username);
    std::fs::create_dir_all(&user_root).expect("WebDAV user directory");
    let source_path = user_root.join("photo.jpg");
    std::fs::write(&source_path, b"incomplete upload").expect("staged upload");

    let mut config = Config::default();
    config.webdav.max_concurrent_requests = 1;
    config.webdav.stable_file_age_seconds = 0;
    let gate = Arc::new(tokio::sync::Semaphore::new(1));
    let upload_permit = gate.acquire().await.expect("upload permit");
    let cycle_config = config.clone();
    let cycle_pool = pool.clone();
    let cycle_gate = Arc::clone(&gate);
    let cycle = tokio::spawn(async move {
        run_webdav_import_cycle(&cycle_config, &cycle_pool, &cycle_gate).await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!cycle.is_finished());
    assert!(source_path.exists());

    std::fs::write(&source_path, b"complete upload").expect("complete staged upload");
    drop(upload_permit);
    tokio::time::timeout(Duration::from_secs(5), cycle)
        .await
        .expect("import cycle timeout")
        .expect("import cycle");
    assert!(!source_path.exists());
    let imported_size: i64 = pool
        .get()
        .expect("database")
        .query_row(
            "SELECT file_size FROM media WHERE original_filename = 'photo.jpg'",
            [],
            |row| row.get(0),
        )
        .expect("imported media");
    assert_eq!(imported_size, b"complete upload".len() as i64);

    std::fs::remove_dir_all(user_root).expect("remove WebDAV test directory");
}

#[tokio::test]
async fn test_webdav_claim_recovery_restores_nested_source_file() {
    let _webdav_test_guard = lock_webdav_test().await;
    init_test_paths();
    let username = format!("claim-recovery-{}", uuid::Uuid::new_v4());
    let source_directory = paths().webdav.join(&username).join("Camera Roll");
    let claim_directory = source_directory.join(".processing/claim-id");
    std::fs::create_dir_all(&claim_directory).expect("claim directory");
    std::fs::write(claim_directory.join("photo.jpg"), b"photo bytes").expect("claimed file");

    recover_webdav_claims(&paths().webdav).expect("recover WebDAV claims");

    assert_eq!(
        std::fs::read(source_directory.join("recovered-claim-id/photo.jpg"))
            .expect("restored source"),
        b"photo bytes"
    );
    assert!(!source_directory.join(".processing").exists());
    std::fs::remove_dir_all(paths().webdav.join(username)).expect("remove WebDAV test directory");
}

#[tokio::test]
async fn test_webdav_claim_recovery_exposes_colliding_claim_in_recovered_directory() {
    let _webdav_test_guard = lock_webdav_test().await;
    init_test_paths();
    let username = format!("claim-collision-{}", uuid::Uuid::new_v4());
    let source_directory = paths().webdav.join(&username);
    let claim_directory = source_directory.join(".processing/claim-id");
    std::fs::create_dir_all(&claim_directory).expect("claim directory");
    std::fs::write(source_directory.join("photo.jpg"), b"new upload").expect("new source");
    std::fs::write(claim_directory.join("photo.jpg"), b"older claim").expect("claimed file");
    std::fs::write(
        claim_directory.join("photo.jpg.supplemental-metadata.json"),
        b"{}",
    )
    .expect("claimed supplemental metadata");

    recover_webdav_claims(&paths().webdav).expect("recover WebDAV claims");

    let recovered_directory = source_directory.join("recovered-claim-id");
    assert_eq!(
        std::fs::read(recovered_directory.join("photo.jpg")).expect("recovered claim"),
        b"older claim"
    );
    assert!(recovered_directory
        .join("photo.jpg.supplemental-metadata.json")
        .is_file());
    assert_eq!(
        std::fs::read(source_directory.join("photo.jpg")).expect("new source"),
        b"new upload"
    );
    std::fs::remove_dir_all(paths().webdav.join(username)).expect("remove WebDAV test directory");
}
