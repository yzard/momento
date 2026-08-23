use sha2::{Digest, Sha256};

#[tokio::test]
async fn file_hash_matches_sha256_across_multiple_read_buffers() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let file_path = directory.path().join("large-input.bin");
    let bytes = (0..(2 * 1024 * 1024 + 17))
        .map(|index| (index % 251) as u8)
        .collect::<Vec<_>>();
    std::fs::write(&file_path, &bytes).expect("hash fixture");
    let expected = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    let actual = momento_api::utils::hash::calculate_file_hash(&file_path)
        .await
        .expect("file hash");

    assert_eq!(actual, expected);
}
