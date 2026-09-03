use std::fs;

use llm_service::content_store::ContentStore;
use momento_common::llm::JobInputDescriptor;
use sha2::{Digest, Sha256};

fn descriptor(bytes: &[u8]) -> JobInputDescriptor {
    JobInputDescriptor {
        sequence: 0,
        filename: "source.dng".to_string(),
        mime_type: "image/x-adobe-dng".to_string(),
        byte_size: bytes.len() as u64,
        content_hash: format!("{:x}", Sha256::digest(bytes)),
        input_kind: "image".to_string(),
        frame_timestamp_ms: None,
    }
}

#[test]
fn content_store_hard_links_identical_inputs_and_removes_the_last_reference() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let queue = directory.path().join("queue");
    fs::create_dir_all(&queue).expect("queue directory");
    let store = ContentStore::new(&queue).expect("content store");
    let bytes = b"durable raw input";
    let input = descriptor(bytes);

    let first_job = queue.join("first-job");
    fs::create_dir(&first_job).expect("first job directory");
    let first_input = first_job.join("input-0");
    fs::write(&first_input, bytes).expect("first input");
    store
        .publish_input(&input, &first_input)
        .expect("publish input");

    let second_job = queue.join("second-job");
    fs::create_dir(&second_job).expect("second job directory");
    let second_input = second_job.join("input-0");
    assert!(store
        .link_cached_input(&input, &second_input)
        .expect("link cached input"));
    assert_eq!(fs::read(&second_input).expect("read linked input"), bytes);

    store
        .remove_job_directory(&first_job, std::slice::from_ref(&input))
        .expect("remove first job");
    assert!(store
        .normalized_path(&input.content_hash)
        .parent()
        .unwrap()
        .exists());
    store
        .remove_job_directory(&second_job, std::slice::from_ref(&input))
        .expect("remove second job");
    assert!(!store
        .normalized_path(&input.content_hash)
        .parent()
        .unwrap()
        .exists());
}

#[test]
fn content_store_startup_removes_interrupted_raw_normalization_temporaries() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let queue = directory.path().join("queue");
    fs::create_dir_all(&queue).expect("queue directory");
    let bytes = b"durable raw input";
    let input = descriptor(bytes);
    let store = ContentStore::new(&queue).expect("content store");
    let job = queue.join("job");
    fs::create_dir(&job).expect("job directory");
    let job_input = job.join("input-0");
    fs::write(&job_input, bytes).expect("job input");
    store
        .publish_input(&input, &job_input)
        .expect("publish input");
    let content_directory = store
        .normalized_path(&input.content_hash)
        .parent()
        .expect("content parent")
        .to_path_buf();
    let raw_temporary = content_directory.join(".normalized-job-0.tiff");
    let descriptor_temporary = content_directory.join("normalized.json.tmp");
    fs::write(&raw_temporary, b"partial tiff").expect("RAW temporary");
    fs::write(&descriptor_temporary, b"partial descriptor").expect("descriptor temporary");
    drop(store);

    let recovered = ContentStore::new(&queue).expect("recovered content store");

    assert!(!raw_temporary.exists());
    assert!(!descriptor_temporary.exists());
    assert!(recovered.input_is_cached(&input).expect("cached source"));
}
