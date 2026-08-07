use momento_api::database::queries;
use momento_api::processor::deduplicator::{
    blob_to_embedding, cosine_similarity, create_run, embedding_to_blob, latest_run,
    recover_interrupted_runs,
};

use crate::test_utils::{create_test_db, create_test_media};

#[test]
fn float32_embedding_blob_round_trips() {
    let embedding = vec![0.25, -1.5, 3.0];

    assert_eq!(blob_to_embedding(&embedding_to_blob(&embedding)), embedding);
}

#[test]
fn cosine_similarity_rejects_invalid_vectors() {
    assert_eq!(cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]), Some(1.0));
    assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 0.0]), None);
    assert_eq!(cosine_similarity(&[1.0], &[1.0, 0.0]), None);
}

#[test]
fn scan_claim_is_persistent_and_exclusive() {
    let pool = create_test_db();
    let run_id = create_run(&pool, "manual", None).expect("First claim should succeed");

    assert!(create_run(&pool, "manual", None).is_err());
    assert_eq!(latest_run(&pool).unwrap().unwrap().id, run_id);

    recover_interrupted_runs(&pool).expect("Recovery should succeed");
    assert_eq!(latest_run(&pool).unwrap().unwrap().status, "interrupted");
}

#[test]
fn stored_index_is_skipped_regardless_of_provenance() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "indexed.jpg");
    let connection = pool.get().expect("Failed to get connection");
    connection
        .execute(
            queries::deduplicate::UPSERT_INDEX,
            rusqlite::params![
                media_id,
                "old-content-hash",
                "old-model",
                "old-preprocessing",
                vec![0_u8; 4],
                1_i64,
                Option::<i64>::None,
            ],
        )
        .expect("Failed to insert index");
    let mut statement = connection
        .prepare(queries::deduplicate::SELECT_INDEX_PAGE)
        .expect("Failed to prepare index query");
    let rows = statement
        .query_map(rusqlite::params![0_i64, 1_i64], |row| row.get::<_, i64>(0))
        .expect("Failed to query indexes")
        .collect::<Result<Vec<_>, _>>()
        .expect("Failed to collect indexes");

    assert!(rows.is_empty());
}

#[test]
fn stored_decode_failure_is_skipped_until_indexes_are_cleaned() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "unreadable.jpg");
    let connection = pool.get().expect("Failed to get connection");
    connection
        .execute(
            queries::deduplicate::UPSERT_FAILED_INDEX,
            rusqlite::params![
                media_id,
                format!("hash_{media_id}"),
                "could not decode image"
            ],
        )
        .expect("Failed to insert decode failure");
    let mut statement = connection
        .prepare(queries::deduplicate::SELECT_INDEX_PAGE)
        .expect("Failed to prepare index query");
    let rows = statement
        .query_map(rusqlite::params![0_i64, 1_i64], |row| row.get::<_, i64>(0))
        .expect("Failed to query indexes")
        .collect::<Result<Vec<_>, _>>()
        .expect("Failed to collect indexes");

    assert!(rows.is_empty());
    let status: i64 = connection
        .query_row(
            "SELECT processing_status FROM media_similarity_index WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("Failed to query failure sentinel");
    assert_eq!(status, -1);
}

#[test]
fn run_progress_updates_all_page_counters_atomically() {
    let pool = create_test_db();
    let run_id = create_run(&pool, "manual", None).expect("Failed to create run");
    let connection = pool.get().expect("Failed to get connection");

    connection
        .execute(
            queries::deduplicate::UPDATE_RUN_PROGRESS,
            rusqlite::params![3_i64, 64_i64, 1_024_i64, 2_i64, run_id],
        )
        .expect("Failed to update page progress");

    let run = latest_run(&pool)
        .expect("Failed to load run")
        .expect("Run should exist");
    assert_eq!(run.indexed_media, 3);
    assert_eq!(run.processed_media, 64);
    assert_eq!(run.candidate_comparisons, 1_024);
    assert_eq!(run.clusters_created, 2);
}
