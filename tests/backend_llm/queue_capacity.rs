use llm_service::queue_capacity::{
    QueueCapacityDecision, QueueCapacityInput, QueueCapacityManager,
};

fn capacity_input(content_hash: &str, byte_size: u64, is_cached: bool) -> QueueCapacityInput {
    QueueCapacityInput {
        content_hash: content_hash.to_string(),
        byte_size,
        is_cached,
    }
}

#[test]
fn reservations_are_atomic_and_release_on_drop() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let content = directory.path().join("content");
    std::fs::create_dir(&content).expect("content directory");
    let capacity = QueueCapacityManager::new(content, directory.path().to_path_buf(), 100, 1)
        .expect("queue capacity");

    let first = capacity
        .try_reserve("first", &[capacity_input("first-hash", 60, false)])
        .expect("capacity decision")
        .expect("first reservation");
    assert_eq!(first.reserved_bytes(), 60);
    let second = capacity
        .try_reserve("second", &[capacity_input("second-hash", 50, false)])
        .expect("capacity decision")
        .expect_err("second reservation must be deferred");
    assert!(matches!(second, QueueCapacityDecision::Deferred(_)));

    drop(first);
    capacity
        .try_reserve("second", &[capacity_input("second-hash", 50, false)])
        .expect("capacity decision")
        .expect("released capacity must be reusable");
}

#[test]
fn an_in_progress_content_hash_is_not_uploaded_twice() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let content = directory.path().join("content");
    std::fs::create_dir(&content).expect("content directory");
    let capacity = QueueCapacityManager::new(content, directory.path().to_path_buf(), 1_000, 1)
        .expect("queue capacity");
    let hash = "shared-hash".to_string();
    let first = capacity
        .try_reserve("first", &[capacity_input(&hash, 10, false)])
        .expect("capacity decision")
        .expect("first reservation");

    let second = capacity
        .try_reserve("second", &[capacity_input(&hash, 10, true)])
        .expect("capacity decision")
        .expect_err("duplicate upload must wait for the first uploader");

    assert!(matches!(second, QueueCapacityDecision::Deferred(_)));
    drop(first);
}

#[test]
fn duplicate_descriptors_in_one_job_reserve_unique_content_once() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let content = directory.path().join("content");
    std::fs::create_dir(&content).expect("content directory");
    let capacity = QueueCapacityManager::new(content, directory.path().to_path_buf(), 100, 1)
        .expect("queue capacity");
    let hash = "shared-hash".to_string();

    let reservation = capacity
        .try_reserve(
            "job",
            &[
                capacity_input(&hash, 60, false),
                capacity_input(&hash, 60, false),
            ],
        )
        .expect("capacity decision")
        .expect("unique content reservation");

    assert_eq!(reservation.reserved_bytes(), 60);
}

#[test]
fn committed_content_is_released_explicitly() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let content = directory.path().join("content");
    std::fs::create_dir(&content).expect("content directory");
    let capacity = QueueCapacityManager::new(content, directory.path().to_path_buf(), 100, 1)
        .expect("queue capacity");
    capacity
        .try_reserve("first", &[capacity_input("first-hash", 80, false)])
        .expect("capacity decision")
        .expect("reservation")
        .commit(80)
        .expect("commit reservation");
    assert_eq!(
        capacity.snapshot(0).expect("capacity snapshot").used_bytes,
        80
    );

    capacity.release_content(80).expect("release content");

    assert_eq!(
        capacity.snapshot(0).expect("capacity snapshot").used_bytes,
        0
    );
}

#[test]
fn one_job_larger_than_the_budget_is_permanently_rejected() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let content = directory.path().join("content");
    std::fs::create_dir(&content).expect("content directory");
    let capacity = QueueCapacityManager::new(content, directory.path().to_path_buf(), 100, 1)
        .expect("queue capacity");

    let decision = capacity
        .try_reserve("oversized", &[capacity_input("oversized-hash", 101, false)])
        .expect("capacity decision")
        .expect_err("oversized job must be rejected");

    assert!(matches!(decision, QueueCapacityDecision::JobTooLarge(_)));
}

#[test]
fn startup_reconstructs_unique_content_usage() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let content = directory.path().join("content");
    let source_directory = content.join("hash");
    std::fs::create_dir_all(&source_directory).expect("content directory");
    std::fs::write(source_directory.join("source"), b"1234567890").expect("content source");
    let capacity = QueueCapacityManager::new(content, directory.path().to_path_buf(), 15, 1)
        .expect("queue capacity");

    let decision = capacity
        .try_reserve("next", &[capacity_input("next-hash", 6, false)])
        .expect("capacity decision")
        .expect_err("reconstructed content must consume capacity");

    assert!(matches!(decision, QueueCapacityDecision::Deferred(_)));
}

#[test]
fn unsatisfied_working_reserve_pauses_admission_without_blocking_startup() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let content = directory.path().join("content");
    std::fs::create_dir(&content).expect("content directory");
    let capacity =
        QueueCapacityManager::new(content, directory.path().to_path_buf(), 100, u64::MAX)
            .expect("queue capacity must start so existing jobs can drain");

    let decision = capacity
        .try_reserve("next", &[capacity_input("next-hash", 1, false)])
        .expect("capacity decision");

    assert!(matches!(decision, Err(QueueCapacityDecision::Deferred(_))));
}
