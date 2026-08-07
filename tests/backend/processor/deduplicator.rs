use momento_api::database::queries;
use momento_api::processor::deduplicator::{
    blob_to_embedding, cosine_similarity, create_run, embedding_to_blob, generate_clusters,
    latest_run, recover_interrupted_runs, request_cancel,
};

use crate::test_utils::{create_test_db, create_test_media};

fn insert_similarity_index(
    pool: &momento_api::database::DbPool,
    media_id: i64,
    embedding: &[f32],
    band_value: i64,
    capture_time: i64,
) {
    let connection = pool.get().expect("Failed to get connection");
    connection
        .execute(
            queries::deduplicate::UPSERT_INDEX,
            rusqlite::params![
                media_id,
                format!("hash_{media_id}"),
                "model",
                "preprocess",
                embedding_to_blob(embedding),
                0_i64,
                capture_time,
            ],
        )
        .expect("Failed to insert similarity index");
    connection
        .execute(
            queries::deduplicate::INSERT_BAND,
            rusqlite::params![media_id, 0_i64, band_value],
        )
        .expect("Failed to insert similarity hash band");
}

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

#[test]
fn identical_near_duplicate_and_burst_sets_are_persisted_once() {
    let pool = create_test_db();
    let first = create_test_media(&pool, "first.jpg");
    let second = create_test_media(&pool, "second.jpg");
    insert_similarity_index(&pool, first, &[1.0, 0.0], 7, 1_000);
    insert_similarity_index(&pool, second, &[1.0, 0.0], 7, 1_005);
    let run_id = create_run(&pool, "scheduled", None).expect("Failed to create run");

    generate_clusters(&pool, run_id).expect("Failed to generate clusters");

    let connection = pool.get().expect("Failed to get connection");
    let (cluster_count, kind, member_count): (i64, String, i64) = connection
        .query_row(
            "SELECT COUNT(DISTINCT clusters.id), MIN(clusters.kind), COUNT(members.media_id) \
             FROM media_similarity_clusters AS clusters \
             JOIN media_similarity_cluster_members AS members ON members.cluster_id = clusters.id",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("Failed to inspect generated clusters");
    assert_eq!(cluster_count, 1);
    assert_eq!(kind, "near_duplicate");
    assert_eq!(member_count, 2);
    let run = latest_run(&pool).unwrap().unwrap();
    assert_eq!(run.clusters_created, 1);
    assert_eq!(run.status, "completed");
    assert!(!request_cancel(&pool).expect("Completed replacement must reject cancellation"));
}

#[test]
fn distinct_media_sets_remain_separate_after_canonicalization() {
    let pool = create_test_db();
    let first = create_test_media(&pool, "first-set-a.jpg");
    let second = create_test_media(&pool, "first-set-b.jpg");
    let third = create_test_media(&pool, "second-set-a.jpg");
    let fourth = create_test_media(&pool, "second-set-b.jpg");
    insert_similarity_index(&pool, first, &[1.0, 0.0], 7, 1_000);
    insert_similarity_index(&pool, second, &[1.0, 0.0], 7, 1_005);
    insert_similarity_index(&pool, third, &[0.0, 1.0], 8, 2_000);
    insert_similarity_index(&pool, fourth, &[0.0, 1.0], 8, 2_005);
    let run_id = create_run(&pool, "scheduled", None).expect("Failed to create run");

    generate_clusters(&pool, run_id).expect("Failed to generate clusters");

    let connection = pool.get().expect("Failed to get connection");
    let cluster_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_similarity_clusters",
            [],
            |row| row.get(0),
        )
        .expect("Failed to count generated clusters");
    assert_eq!(cluster_count, 2);
    assert_eq!(latest_run(&pool).unwrap().unwrap().clusters_created, 2);
}

#[test]
fn cancelled_generation_keeps_the_previous_complete_clusters() {
    let pool = create_test_db();
    let first = create_test_media(&pool, "existing-a.jpg");
    let second = create_test_media(&pool, "existing-b.jpg");
    insert_similarity_index(&pool, first, &[1.0, 0.0], 7, 1_000);
    insert_similarity_index(&pool, second, &[1.0, 0.0], 7, 1_005);
    let connection = pool.get().expect("Failed to get connection");
    connection
        .execute(
            queries::deduplicate::INSERT_CLUSTER,
            rusqlite::params!["burst", first],
        )
        .expect("Failed to create existing cluster");
    let existing_cluster_id = connection.last_insert_rowid();
    for media_id in [first, second] {
        connection
            .execute(
                queries::deduplicate::INSERT_CLUSTER_MEMBER,
                rusqlite::params![existing_cluster_id, media_id, 1.0_f32, 0_u32],
            )
            .expect("Failed to create existing cluster member");
    }
    drop(connection);
    let run_id = create_run(&pool, "scheduled", None).expect("Failed to create run");
    let connection = pool.get().expect("Failed to get connection");
    connection
        .execute(
            "UPDATE media_similarity_runs SET status = 'cancelling' WHERE id = ?",
            [run_id],
        )
        .expect("Failed to request cancellation");
    drop(connection);

    generate_clusters(&pool, run_id).expect("Cancelled generation should stop cleanly");

    let connection = pool.get().expect("Failed to get connection");
    let remaining_cluster: (i64, String) = connection
        .query_row(
            "SELECT id, kind FROM media_similarity_clusters",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("Previous cluster should remain available");
    assert_eq!(
        remaining_cluster,
        (existing_cluster_id, "burst".to_string())
    );
}
