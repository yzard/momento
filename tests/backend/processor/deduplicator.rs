use momento_api::database::queries;
use momento_api::processor::deduplicator::{
    create_run as create_run_on_connection, latest_run as latest_run_on_connection,
    queue_clustering_jobs as queue_clustering_jobs_on_connection,
    request_cancel as request_cancel_on_connection, DeduplicateRunStatus,
};
use momento_api::utils::embedding::embedding_to_blob;
use rusqlite::OptionalExtension;

use crate::test_utils::{create_test_db, create_test_media};

fn create_run(
    pool: &momento_api::database::DbPool,
    trigger: &str,
    scheduled_for: Option<&str>,
) -> momento_api::error::AppResult<i64> {
    let connection = pool.get()?;
    create_run_on_connection(&connection, trigger, scheduled_for)
}

fn latest_run(
    pool: &momento_api::database::DbPool,
) -> momento_api::error::AppResult<Option<DeduplicateRunStatus>> {
    let connection = pool.get()?;
    latest_run_on_connection(&connection)
}

fn request_cancel(pool: &momento_api::database::DbPool) -> momento_api::error::AppResult<bool> {
    let connection = pool.get()?;
    request_cancel_on_connection(&connection)
}

fn queue_clustering_jobs(
    pool: &momento_api::database::DbPool,
    run_id: i64,
) -> momento_api::error::AppResult<usize> {
    let connection = pool.get()?;
    queue_clustering_jobs_on_connection(&connection, run_id)
}

async fn finalize_ready_runs(
    pool: &momento_api::database::DbPool,
) -> Result<(), momento_api::executor::ExecutorError> {
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    momento_api::processor::deduplicator::finalize_ready_runs(&executors).await
}

fn insert_similarity_index(
    pool: &momento_api::database::DbPool,
    media_id: i64,
    model_version: &str,
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
                model_version,
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

fn prepare_clustering_input(pool: &momento_api::database::DbPool, media_id: i64, filename: &str) {
    let connection = pool.get().expect("connection");
    connection
        .execute(
            "INSERT INTO media_metadata_jobs (media_id, status) VALUES (?, 'completed')",
            [media_id],
        )
        .expect("metadata job");
    connection.execute("INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, 'image_clustering', 0, 'image', 'previews', ?, ?, 'image/jpeg', 4, 'hash')", rusqlite::params![media_id, format!("ai/{filename}"), filename]).expect("prepared input");
}

#[tokio::test]
async fn scan_claim_is_persistent_and_exclusive() {
    let pool = create_test_db();
    let run_id = create_run(&pool, "manual", None).expect("First claim should succeed");

    assert!(create_run(&pool, "manual", None).is_err());
    assert_eq!(latest_run(&pool).unwrap().unwrap().id, run_id);

    crate::test_utils::test_executor_handles(pool.clone())
        .sqlite
        .recover_deduplicate_runs_durable()
        .await
        .expect("Recovery should succeed");
    assert_eq!(latest_run(&pool).unwrap().unwrap().status, "running");
}

#[tokio::test]
async fn clustering_jobs_snapshot_inputs_and_repair_missing_input_failures() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "clustering-input.jpg");
    prepare_clustering_input(&pool, media_id, "clustering.jpg");
    let run_id = create_run(&pool, "manual", None).expect("run");

    assert_eq!(queue_clustering_jobs(&pool, run_id).expect("queue"), 1);

    let connection = pool.get().expect("connection");
    let job_id: String = connection
        .query_row(
            "SELECT id FROM llm_jobs WHERE deduplicate_run_id = ?",
            [run_id],
            |row| row.get(0),
        )
        .expect("job id");
    let snapshot_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM llm_job_inputs WHERE job_id = ?",
            [&job_id],
            |row| row.get(0),
        )
        .expect("snapshot count");
    assert_eq!(snapshot_count, 1);
    connection
        .execute("DELETE FROM llm_job_inputs WHERE job_id = ?", [&job_id])
        .expect("remove snapshot");
    connection.execute("UPDATE llm_jobs SET status = 'failed', last_error = 'missing prepared AI inputs' WHERE id = ?", [&job_id]).expect("failed job");
    drop(connection);

    finalize_ready_runs(&pool).await.expect("repair run");

    let connection = pool.get().expect("connection");
    let repaired_status: String = connection
        .query_row(
            "SELECT status FROM llm_jobs WHERE id = ?",
            [&job_id],
            |row| row.get(0),
        )
        .expect("repaired status");
    let repaired_snapshot_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM llm_job_inputs WHERE job_id = ?",
            [&job_id],
            |row| row.get(0),
        )
        .expect("repaired snapshot count");
    assert_eq!(repaired_status, "queued");
    assert_eq!(repaired_snapshot_count, 1);
}

#[test]
fn clustering_run_replaces_indexes_from_the_previous_model_without_hiding_the_visible_generation() {
    let pool = create_test_db();
    let stale_media_id = create_test_media(&pool, "stale-small.jpg");
    let current_media_id = create_test_media(&pool, "current-base.jpg");
    prepare_clustering_input(&pool, stale_media_id, "stale-small.jpg");
    prepare_clustering_input(&pool, current_media_id, "current-base.jpg");
    insert_similarity_index(&pool, stale_media_id, "dinov2-small", &[1.0, 0.0], 7, 1_000);
    insert_similarity_index(
        &pool,
        current_media_id,
        "dinov2-base",
        &[1.0, 0.0],
        7,
        1_005,
    );
    let connection = pool.get().expect("connection");
    let cluster_id: i64 = connection
        .query_row(
            "INSERT INTO media_similarity_clusters (kind, representative_media_id) VALUES ('near_duplicate', ?) RETURNING id",
            [current_media_id],
            |row| row.get(0),
        )
        .expect("cluster");
    connection.execute("INSERT INTO media_similarity_cluster_members (cluster_id, media_id, cosine_similarity, perceptual_hash_distance) VALUES (?, ?, 1.0, 0), (?, ?, 0.99, 0)", rusqlite::params![cluster_id, current_media_id, cluster_id, stale_media_id]).expect("cluster members");
    drop(connection);
    let run_id = create_run(&pool, "manual", None).expect("run");

    assert_eq!(queue_clustering_jobs(&pool, run_id).expect("queue"), 1);

    let connection = pool.get().expect("connection");
    let queued_media_id: i64 = connection
        .query_row(
            "SELECT media_id FROM llm_jobs WHERE deduplicate_run_id = ?",
            [run_id],
            |row| row.get(0),
        )
        .expect("queued media");
    let remaining_models = connection
        .prepare("SELECT model_version FROM media_similarity_index ORDER BY media_id")
        .expect("models query")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("models")
        .collect::<Result<Vec<_>, _>>()
        .expect("model versions");
    let stale_band_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_similarity_hash_bands WHERE media_id = ?",
            [stale_media_id],
            |row| row.get(0),
        )
        .expect("stale bands");
    let cluster_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_similarity_clusters",
            [],
            |row| row.get(0),
        )
        .expect("clusters");
    assert_eq!(queued_media_id, stale_media_id);
    assert_eq!(remaining_models, vec!["dinov2-base"]);
    assert_eq!(stale_band_count, 0);
    assert_eq!(cluster_count, 1);
}

#[tokio::test]
async fn cancelled_run_cancels_queued_jobs_without_finalizing_clusters() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "cancel.jpg");
    let run_id = create_run(&pool, "manual", None).expect("run");
    let connection = pool.get().expect("connection");
    connection.execute("INSERT INTO llm_jobs (id, media_id, deduplicate_run_id, task, status) VALUES ('cancel-job', ?, ?, 'image_clustering', 'queued')", rusqlite::params![media_id, run_id]).expect("job");
    drop(connection);
    assert!(request_cancel(&pool).expect("cancel"));
    finalize_ready_runs(&pool)
        .await
        .expect("finalize cancellation");
    let connection = pool.get().expect("connection");
    let run_status: String = connection
        .query_row(
            "SELECT status FROM media_similarity_runs WHERE id = ?",
            [run_id],
            |row| row.get(0),
        )
        .expect("run status");
    let job_status: String = connection
        .query_row(
            "SELECT status FROM llm_jobs WHERE id = 'cancel-job'",
            [],
            |row| row.get(0),
        )
        .expect("job status");
    assert_eq!(run_status, "cancelled");
    assert_eq!(job_status, "cancelled");
}

#[tokio::test]
async fn failed_clustering_jobs_still_generate_groups_from_successful_indexes() {
    let pool = create_test_db();
    let first_media_id = create_test_media(&pool, "successful-a.jpg");
    let second_media_id = create_test_media(&pool, "successful-b.jpg");
    let failed_media_id = create_test_media(&pool, "failed.jpg");
    insert_similarity_index(&pool, first_media_id, "dinov2-base", &[1.0, 0.0], 7, 1_000);
    insert_similarity_index(&pool, second_media_id, "dinov2-base", &[1.0, 0.0], 7, 1_005);
    let run_id = create_run(&pool, "scheduled", Some("2026-08-12T03:00:00Z")).expect("run");
    let connection = pool.get().expect("connection");
    connection.execute("INSERT INTO llm_jobs (id, media_id, deduplicate_run_id, task, status) VALUES ('failed-job', ?, ?, 'image_clustering', 'failed')", rusqlite::params![failed_media_id, run_id]).expect("job");
    drop(connection);
    finalize_ready_runs(&pool).await.expect("finalize failure");

    let run = latest_run(&pool).expect("latest").expect("run");
    assert_eq!(run.status, "completed");
    assert_eq!(
        run.error.as_deref(),
        Some("1 image clustering jobs failed; groups generated from successful results")
    );
    let connection = pool.get().expect("connection");
    let cluster_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_similarity_clusters",
            [],
            |row| row.get(0),
        )
        .expect("cluster count");
    assert_eq!(cluster_count, 1);
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

#[tokio::test]
async fn identical_near_duplicate_and_burst_sets_are_persisted_once() {
    let pool = create_test_db();
    let first = create_test_media(&pool, "first.jpg");
    let second = create_test_media(&pool, "second.jpg");
    insert_similarity_index(&pool, first, "dinov2-base", &[1.0, 0.0], 7, 1_000);
    insert_similarity_index(&pool, second, "dinov2-base", &[1.0, 0.0], 7, 1_005);
    create_run(&pool, "scheduled", None).expect("Failed to create run");

    finalize_ready_runs(&pool)
        .await
        .expect("Failed to generate clusters");

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

#[tokio::test]
async fn distinct_media_sets_remain_separate_after_canonicalization() {
    let pool = create_test_db();
    let first = create_test_media(&pool, "first-set-a.jpg");
    let second = create_test_media(&pool, "first-set-b.jpg");
    let third = create_test_media(&pool, "second-set-a.jpg");
    let fourth = create_test_media(&pool, "second-set-b.jpg");
    insert_similarity_index(&pool, first, "dinov2-base", &[1.0, 0.0], 7, 1_000);
    insert_similarity_index(&pool, second, "dinov2-base", &[1.0, 0.0], 7, 1_005);
    insert_similarity_index(&pool, third, "dinov2-base", &[0.0, 1.0], 8, 2_000);
    insert_similarity_index(&pool, fourth, "dinov2-base", &[0.0, 1.0], 8, 2_005);
    create_run(&pool, "scheduled", None).expect("Failed to create run");

    finalize_ready_runs(&pool)
        .await
        .expect("Failed to generate clusters");

    let connection = pool.get().expect("Failed to get connection");
    let (cluster_count, member_count): (i64, i64) = connection
        .query_row(
            "SELECT COUNT(DISTINCT clusters.id), COUNT(members.media_id) FROM media_similarity_clusters AS clusters JOIN media_similarity_cluster_members AS members ON members.cluster_id = clusters.id",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("Failed to count generated clusters");
    assert_eq!(cluster_count, 2);
    assert_eq!(member_count, 4);
    assert_eq!(latest_run(&pool).unwrap().unwrap().clusters_created, 2);
}

#[tokio::test]
async fn building_generation_is_invisible_and_resumes_from_durable_state() {
    let pool = create_test_db();
    let old_first = create_test_media(&pool, "old-visible-a.jpg");
    let old_second = create_test_media(&pool, "old-visible-b.jpg");
    let new_first = create_test_media(&pool, "new-generation-a.jpg");
    let new_second = create_test_media(&pool, "new-generation-b.jpg");
    insert_similarity_index(&pool, new_first, "dinov2-base", &[1.0, 0.0], 91, 5_000);
    insert_similarity_index(&pool, new_second, "dinov2-base", &[1.0, 0.0], 91, 5_005);
    let connection = pool.get().expect("connection");
    connection
        .execute(
            queries::deduplicate::INSERT_CLUSTER,
            rusqlite::params!["near_duplicate", old_first],
        )
        .expect("old cluster");
    let old_cluster_id = connection.last_insert_rowid();
    for media_id in [old_first, old_second] {
        connection
            .execute(
                queries::deduplicate::INSERT_CLUSTER_MEMBER,
                rusqlite::params![old_cluster_id, media_id, 1.0_f32, 0_u32],
            )
            .expect("old member");
    }
    drop(connection);
    create_run(&pool, "manual", None).expect("run");
    let executors = crate::test_utils::test_executor_handles(pool.clone());

    let first_step = executors
        .sqlite
        .load_deduplicate_finalization_work()
        .await
        .expect("initialize durable generation");
    assert!(matches!(
        first_step,
        momento_api::processor::deduplicator::DeduplicateFinalizationWork::Progressed
    ));
    let connection = pool.get().expect("connection");
    let active_generation: Option<i64> = connection
        .query_row(
            "SELECT active_generation_id FROM media_similarity_generation_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .expect("active generation");
    let building_generations: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_similarity_generations WHERE status = 'building'",
            [],
            |row| row.get(0),
        )
        .expect("building generations");
    let visible_old_members: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_similarity_cluster_members WHERE cluster_id = ?",
            [old_cluster_id],
            |row| row.get(0),
        )
        .expect("old members");
    assert_eq!(active_generation, None);
    assert_eq!(building_generations, 1);
    assert_eq!(visible_old_members, 2);
    drop(connection);

    momento_api::processor::deduplicator::finalize_ready_runs(&executors)
        .await
        .expect("resume finalization");
    let connection = pool.get().expect("connection");
    let active_generation: i64 = connection
        .query_row(
            "SELECT active_generation_id FROM media_similarity_generation_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .expect("active generation");
    let old_cluster_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_similarity_clusters WHERE id = ?",
            [old_cluster_id],
            |row| row.get(0),
        )
        .expect("old cluster cleanup");
    assert!(active_generation > 0);
    assert_eq!(old_cluster_count, 0);
}

#[tokio::test]
async fn grouping_pages_more_than_sixty_four_media_without_one_large_transaction() {
    let pool = create_test_db();
    for pair_index in 0_i64..33 {
        for member_index in 0_i64..2 {
            let media_id =
                create_test_media(&pool, &format!("paged-{pair_index}-{member_index}.jpg"));
            insert_similarity_index(
                &pool,
                media_id,
                "dinov2-base",
                &[1.0, 0.0],
                pair_index,
                pair_index * 100 + member_index,
            );
        }
    }
    create_run(&pool, "manual", None).expect("run");

    finalize_ready_runs(&pool)
        .await
        .expect("paged finalization");

    let run = latest_run(&pool).expect("latest run").expect("run");
    let connection = pool.get().expect("connection");
    let active_cluster_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_similarity_clusters AS clusters JOIN media_similarity_generation_state AS state ON state.active_generation_id = clusters.generation_id WHERE state.singleton = 1",
            [],
            |row| row.get(0),
        )
        .expect("active clusters");
    let leftover_state_rows: i64 = connection
        .query_row(
            "SELECT (SELECT COUNT(*) FROM media_similarity_finalizations) + (SELECT COUNT(*) FROM media_similarity_edges) + (SELECT COUNT(*) FROM media_similarity_labels)",
            [],
            |row| row.get(0),
        )
        .expect("finalization cleanup");
    assert_eq!(run.processed_media, 66);
    assert_eq!(active_cluster_count, 33);
    assert_eq!(leftover_state_rows, 0);
}

#[tokio::test]
async fn cancelled_generation_keeps_the_previous_complete_clusters() {
    let pool = create_test_db();
    let first = create_test_media(&pool, "existing-a.jpg");
    let second = create_test_media(&pool, "existing-b.jpg");
    insert_similarity_index(&pool, first, "dinov2-base", &[1.0, 0.0], 7, 1_000);
    insert_similarity_index(&pool, second, "dinov2-base", &[1.0, 0.0], 7, 1_005);
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

    finalize_ready_runs(&pool)
        .await
        .expect("Cancelled generation should stop cleanly");

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
