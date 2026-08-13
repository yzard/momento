use std::collections::{BTreeMap, HashMap};

use chrono::Utc;
use tracing::info;

use crate::database::{execute_query, fetch_all, fetch_one, insert_returning_id, queries, DbPool};
use crate::error::{AppError, AppResult};

const CANDIDATE_PAGE_SIZE: usize = 256;
const INDEX_PAGE_SIZE: usize = 64;
const NEAR_DUPLICATE_COSINE_SIMILARITY: f32 = 0.97;
const BURST_COSINE_SIMILARITY: f32 = 0.90;
const PERCEPTUAL_HASH_DISTANCE: u32 = 10;
const BURST_WINDOW_SECONDS: i64 = 10;

#[derive(Debug)]
struct SimilarityRow {
    media_id: i64,
    embedding: Vec<f32>,
    perceptual_hash: u64,
    capture_time_seconds: Option<i64>,
}

#[derive(Debug)]
struct CandidateMatch {
    media_id: i64,
    cosine_similarity: f32,
    perceptual_hash_distance: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ClusterKind {
    NearDuplicate,
    Burst,
}

impl ClusterKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::NearDuplicate => "near_duplicate",
            Self::Burst => "burst",
        }
    }
}

#[derive(Debug)]
struct PendingClusterMember {
    media_id: i64,
    cosine_similarity: f32,
    perceptual_hash_distance: u32,
}

#[derive(Debug)]
struct PendingCluster {
    kind: ClusterKind,
    representative_media_id: i64,
    members: Vec<PendingClusterMember>,
}

#[derive(Debug, Default)]
struct PendingClusters {
    clusters: Vec<PendingCluster>,
    cluster_by_member: HashMap<(ClusterKind, i64), usize>,
}

impl PendingClusters {
    fn attach(
        &mut self,
        pool: &DbPool,
        source: &SimilarityRow,
        candidate: CandidateMatch,
        kind: ClusterKind,
    ) -> AppResult<()> {
        let candidate_media_id = candidate.media_id;
        if let Some(&cluster_index) = self.cluster_by_member.get(&(kind, candidate_media_id)) {
            let representative_media_id = self.clusters[cluster_index].representative_media_id;
            let Some(representative) = load_similarity_row(pool, representative_media_id)? else {
                return Ok(());
            };
            let Some((similarity, hash_distance)) =
                representative_match(source, &representative, kind)
            else {
                return Ok(());
            };
            self.clusters[cluster_index]
                .members
                .push(PendingClusterMember {
                    media_id: source.media_id,
                    cosine_similarity: similarity,
                    perceptual_hash_distance: hash_distance,
                });
            self.cluster_by_member
                .insert((kind, source.media_id), cluster_index);
            return Ok(());
        }

        let cluster_index = self.clusters.len();
        let members = vec![
            PendingClusterMember {
                media_id: candidate_media_id,
                cosine_similarity: 1.0,
                perceptual_hash_distance: 0,
            },
            PendingClusterMember {
                media_id: source.media_id,
                cosine_similarity: candidate.cosine_similarity,
                perceptual_hash_distance: candidate.perceptual_hash_distance,
            },
        ];
        self.clusters.push(PendingCluster {
            kind,
            representative_media_id: candidate_media_id,
            members,
        });
        self.cluster_by_member
            .insert((kind, candidate_media_id), cluster_index);
        self.cluster_by_member
            .insert((kind, source.media_id), cluster_index);
        Ok(())
    }

    fn canonicalize(self) -> Vec<PendingCluster> {
        let mut canonical = BTreeMap::<Vec<i64>, PendingCluster>::new();
        for cluster in self.clusters {
            let mut signature = cluster
                .members
                .iter()
                .map(|member| member.media_id)
                .collect::<Vec<_>>();
            signature.sort_unstable();
            match canonical.entry(signature) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(cluster);
                }
                std::collections::btree_map::Entry::Occupied(mut entry)
                    if entry.get().kind == ClusterKind::Burst
                        && cluster.kind == ClusterKind::NearDuplicate =>
                {
                    entry.insert(cluster);
                }
                std::collections::btree_map::Entry::Occupied(_) => {}
            }
        }
        canonical.into_values().collect()
    }
}

#[derive(Debug, Clone)]
pub struct DeduplicateRunStatus {
    pub id: i64,
    pub trigger: String,
    pub status: String,
    pub scheduled_for: Option<String>,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub indexed_media: i64,
    pub processed_media: i64,
    pub candidate_comparisons: i64,
    pub clusters_created: i64,
    pub error: Option<String>,
}

pub fn recover_interrupted_runs(pool: &DbPool) -> AppResult<()> {
    let connection = pool.get().map_err(AppError::Pool)?;
    connection.execute(queries::deduplicate::RECOVER_SUBMITTING_JOBS, [])?;
    connection.execute(queries::deduplicate::CANCEL_SUBMITTED_JOBS, [])?;
    connection.execute(queries::deduplicate::FAIL_INTERRUPTED_RUNS, [])?;
    execute_query(&connection, queries::deduplicate::MARK_ALL_DIRTY, &[])?;
    Ok(())
}

pub fn create_run(pool: &DbPool, trigger: &str, scheduled_for: Option<&str>) -> AppResult<i64> {
    let connection = pool.get().map_err(AppError::Pool)?;
    insert_returning_id(
        &connection,
        queries::deduplicate::INSERT_RUN,
        &[&trigger, &scheduled_for],
    )
    .map_err(|error| match error {
        AppError::Database(rusqlite::Error::SqliteFailure(_, _)) => {
            AppError::Conflict("Deduplicate scan already in progress".to_string())
        }
        other => other,
    })
}

pub fn latest_run(pool: &DbPool) -> AppResult<Option<DeduplicateRunStatus>> {
    let connection = pool.get().map_err(AppError::Pool)?;
    fetch_one(
        &connection,
        queries::deduplicate::SELECT_LATEST_RUN,
        &[],
        |row| {
            Ok(DeduplicateRunStatus {
                id: row.get(0)?,
                trigger: row.get(1)?,
                status: row.get(2)?,
                scheduled_for: row.get(3)?,
                started_at: row.get(4)?,
                completed_at: row.get(5)?,
                indexed_media: row.get(6)?,
                processed_media: row.get(7)?,
                candidate_comparisons: row.get(8)?,
                clusters_created: row.get(9)?,
                error: row.get(10)?,
            })
        },
    )
}

pub fn request_cancel(pool: &DbPool) -> AppResult<bool> {
    let Some(run) = latest_run(pool)? else {
        return Ok(false);
    };
    if run.status != "running" {
        return Ok(false);
    }
    let connection = pool.get().map_err(AppError::Pool)?;
    Ok(execute_query(
        &connection,
        queries::deduplicate::REQUEST_CANCEL,
        &[&run.id],
    )? > 0)
}

pub fn clean(pool: &DbPool) -> AppResult<()> {
    let connection = pool.get().map_err(AppError::Pool)?;
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(queries::deduplicate::LOCK_RUNS, [])?;
    let active_runs: i64 =
        transaction.query_row(queries::deduplicate::COUNT_ACTIVE_RUNS, [], |row| {
            row.get(0)
        })?;
    if active_runs > 0 {
        transaction.rollback()?;
        return Err(AppError::Conflict(
            "Deduplicate scan already in progress".to_string(),
        ));
    }
    transaction.execute(queries::deduplicate::CLEAN_CLUSTERS, [])?;
    transaction.execute(queries::deduplicate::CLEAN_BANDS, [])?;
    transaction.execute(queries::deduplicate::CLEAN_INDEX, [])?;
    transaction.execute(queries::deduplicate::CLEAN_DIRTY, [])?;
    transaction.execute(queries::deduplicate::CLEAN_RUNS, [])?;
    transaction.execute(queries::deduplicate::MARK_ALL_DIRTY, [])?;
    transaction.commit()?;
    Ok(())
}

pub fn queue_clustering_jobs(pool: &DbPool, run_id: i64) -> AppResult<usize> {
    let connection = pool.get().map_err(AppError::Pool)?;
    let run_status: String =
        connection.query_row(queries::deduplicate::SELECT_RUN_STATUS, [run_id], |row| {
            row.get(0)
        })?;
    if run_status != "running" {
        return Ok(0);
    }
    Ok(connection.execute(
        queries::deduplicate::CREATE_CLUSTERING_JOBS,
        rusqlite::params![run_id, run_id],
    )?)
}

pub fn finalize_ready_runs(pool: &DbPool) -> AppResult<()> {
    let connection = pool.get().map_err(AppError::Pool)?;
    let runs = connection
        .prepare(queries::deduplicate::SELECT_ACTIVE_RUNS)?
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (run_id, status) in runs {
        if status == "cancelling" {
            connection.execute(queries::deduplicate::CANCEL_UNSUBMITTED_JOBS, [run_id])?;
            connection.execute(queries::deduplicate::MARK_RUN_CANCELLED, [run_id])?;
            continue;
        }
        let current_status: String =
            connection.query_row(queries::deduplicate::SELECT_RUN_STATUS, [run_id], |row| {
                row.get(0)
            })?;
        if current_status != "running" {
            continue;
        }
        let pending: i64 =
            connection.query_row(queries::deduplicate::COUNT_PENDING_JOBS, [run_id], |row| {
                row.get(0)
            })?;
        if pending != 0 {
            continue;
        }
        let failures: i64 =
            connection.query_row(queries::deduplicate::COUNT_FAILED_JOBS, [run_id], |row| {
                row.get(0)
            })?;
        if failures > 0 {
            connection.execute(queries::deduplicate::MARK_RUN_FAILED, [run_id])?;
            continue;
        }
        generate_clusters(pool, run_id)?;
        connection.execute(queries::deduplicate::MARK_RUN_COMPLETED, [run_id])?;
    }
    Ok(())
}

pub fn generate_clusters(pool: &DbPool, run_id: i64) -> AppResult<()> {
    let connection = pool.get().map_err(AppError::Pool)?;
    let dirty_count = fetch_one(&connection, queries::deduplicate::COUNT_DIRTY, &[], |row| {
        row.get::<_, i64>(0)
    })?
    .unwrap_or(0);
    if dirty_count == 0 {
        return Ok(());
    }
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(queries::deduplicate::CLEAN_DIRTY, [])?;
    transaction.commit()?;
    drop(connection);

    let mut cursor = 0_i64;
    let mut pending_clusters = PendingClusters::default();
    loop {
        if cancellation_requested(pool, run_id)? {
            return Ok(());
        }
        let index_rows = load_current_index_page(pool, cursor, INDEX_PAGE_SIZE)?;
        if index_rows.is_empty() {
            break;
        }
        let mut processed_media = 0_i64;
        let mut candidate_comparisons = 0_i64;
        for source in index_rows {
            cursor = source.media_id;
            let (near_duplicate, burst, comparison_count) = find_best_candidates(pool, &source)?;
            candidate_comparisons += comparison_count;
            if let Some(candidate) = near_duplicate {
                pending_clusters.attach(pool, &source, candidate, ClusterKind::NearDuplicate)?;
            }
            if let Some(candidate) = burst {
                pending_clusters.attach(pool, &source, candidate, ClusterKind::Burst)?;
            }
            processed_media += 1;
        }
        update_run_progress(pool, run_id, 0, processed_media, candidate_comparisons, 0)?;
    }

    if cancellation_requested(pool, run_id)? {
        return Ok(());
    }
    replace_clusters(pool, run_id, pending_clusters.canonicalize())
}

fn load_current_index_page(
    pool: &DbPool,
    cursor: i64,
    page_size: usize,
) -> AppResult<Vec<SimilarityRow>> {
    let connection = pool.get().map_err(AppError::Pool)?;
    fetch_all(
        &connection,
        queries::deduplicate::SELECT_CURRENT_INDEX_PAGE,
        &[&cursor, &(page_size as i64)],
        map_similarity_row,
    )
}

fn map_similarity_row(row: &rusqlite::Row) -> rusqlite::Result<SimilarityRow> {
    let embedding_blob: Vec<u8> = row.get(1)?;
    Ok(SimilarityRow {
        media_id: row.get(0)?,
        embedding: blob_to_embedding(&embedding_blob),
        perceptual_hash: row.get::<_, i64>(2)? as u64,
        capture_time_seconds: row.get(3)?,
    })
}

fn load_similarity_row(pool: &DbPool, media_id: i64) -> AppResult<Option<SimilarityRow>> {
    let connection = pool.get().map_err(AppError::Pool)?;
    fetch_one(
        &connection,
        queries::deduplicate::SELECT_CURRENT_INDEX_BY_MEDIA_ID,
        &[&media_id],
        map_similarity_row,
    )
}

fn find_best_candidates(
    pool: &DbPool,
    source: &SimilarityRow,
) -> AppResult<(Option<CandidateMatch>, Option<CandidateMatch>, i64)> {
    let mut near_duplicate = None;
    let mut burst = None;
    let mut comparisons = 0_i64;
    let mut candidate_cursor = 0_i64;
    loop {
        let candidates =
            load_band_candidates(pool, source.media_id, candidate_cursor, CANDIDATE_PAGE_SIZE)?;
        if candidates.is_empty() {
            break;
        }
        for candidate in candidates {
            candidate_cursor = candidate.media_id;
            comparisons += 1;
            let Some(similarity) = cosine_similarity(&source.embedding, &candidate.embedding)
            else {
                continue;
            };
            let hash_distance = (source.perceptual_hash ^ candidate.perceptual_hash).count_ones();
            if similarity >= NEAR_DUPLICATE_COSINE_SIMILARITY
                && hash_distance <= PERCEPTUAL_HASH_DISTANCE
            {
                replace_if_better(
                    &mut near_duplicate,
                    CandidateMatch {
                        media_id: candidate.media_id,
                        cosine_similarity: similarity,
                        perceptual_hash_distance: hash_distance,
                    },
                );
            }
        }
    }

    let Some(capture_time) = source.capture_time_seconds else {
        return Ok((near_duplicate, burst, comparisons));
    };
    candidate_cursor = 0;
    loop {
        let candidates = load_time_candidates(
            pool,
            source.media_id,
            candidate_cursor,
            capture_time,
            BURST_WINDOW_SECONDS,
            CANDIDATE_PAGE_SIZE,
        )?;
        if candidates.is_empty() {
            break;
        }
        for candidate in candidates {
            candidate_cursor = candidate.media_id;
            comparisons += 1;
            let Some(similarity) = cosine_similarity(&source.embedding, &candidate.embedding)
            else {
                continue;
            };
            let hash_distance = (source.perceptual_hash ^ candidate.perceptual_hash).count_ones();
            if similarity >= BURST_COSINE_SIMILARITY
                && hash_distance <= PERCEPTUAL_HASH_DISTANCE.saturating_mul(2)
            {
                replace_if_better(
                    &mut burst,
                    CandidateMatch {
                        media_id: candidate.media_id,
                        cosine_similarity: similarity,
                        perceptual_hash_distance: hash_distance,
                    },
                );
            }
        }
    }
    Ok((near_duplicate, burst, comparisons))
}

fn load_band_candidates(
    pool: &DbPool,
    media_id: i64,
    cursor: i64,
    page_size: usize,
) -> AppResult<Vec<SimilarityRow>> {
    let connection = pool.get().map_err(AppError::Pool)?;
    fetch_all(
        &connection,
        queries::deduplicate::SELECT_BAND_CANDIDATES,
        &[&media_id, &media_id, &cursor, &(page_size as i64)],
        map_similarity_row,
    )
}

fn load_time_candidates(
    pool: &DbPool,
    media_id: i64,
    cursor: i64,
    capture_time: i64,
    window_seconds: i64,
    page_size: usize,
) -> AppResult<Vec<SimilarityRow>> {
    let connection = pool.get().map_err(AppError::Pool)?;
    fetch_all(
        &connection,
        queries::deduplicate::SELECT_TIME_CANDIDATES,
        &[
            &(capture_time - window_seconds),
            &(capture_time + window_seconds),
            &media_id,
            &cursor,
            &(page_size as i64),
        ],
        map_similarity_row,
    )
}

fn replace_if_better(current: &mut Option<CandidateMatch>, candidate: CandidateMatch) {
    if current
        .as_ref()
        .is_some_and(|existing| existing.cosine_similarity >= candidate.cosine_similarity)
    {
        return;
    }
    *current = Some(candidate);
}

fn representative_match(
    source: &SimilarityRow,
    representative: &SimilarityRow,
    kind: ClusterKind,
) -> Option<(f32, u32)> {
    let similarity = cosine_similarity(&source.embedding, &representative.embedding)?;
    let distance = (source.perceptual_hash ^ representative.perceptual_hash).count_ones();
    let (threshold, maximum_hash_distance) = match kind {
        ClusterKind::NearDuplicate => (NEAR_DUPLICATE_COSINE_SIMILARITY, PERCEPTUAL_HASH_DISTANCE),
        ClusterKind::Burst => (
            BURST_COSINE_SIMILARITY,
            PERCEPTUAL_HASH_DISTANCE.saturating_mul(2),
        ),
    };
    if similarity < threshold || distance > maximum_hash_distance {
        return None;
    }
    if kind != ClusterKind::Burst {
        return Some((similarity, distance));
    }
    let time_matches = source
        .capture_time_seconds
        .zip(representative.capture_time_seconds)
        .is_some_and(|(source_time, representative_time)| {
            (source_time - representative_time).abs() <= BURST_WINDOW_SECONDS
        });
    time_matches.then_some((similarity, distance))
}

fn replace_clusters(pool: &DbPool, run_id: i64, clusters: Vec<PendingCluster>) -> AppResult<()> {
    let cluster_count = clusters.len() as i64;
    let cluster_payload = serde_json::to_string(
        &clusters
            .into_iter()
            .enumerate()
            .map(|(index, cluster)| {
                serde_json::json!({
                    "id": index + 1,
                    "kind": cluster.kind.as_str(),
                    "representativeMediaId": cluster.representative_media_id,
                    "members": cluster.members.into_iter().map(|member| serde_json::json!({
                        "mediaId": member.media_id,
                        "cosineSimilarity": member.cosine_similarity,
                        "perceptualHashDistance": member.perceptual_hash_distance,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect::<Vec<_>>(),
    )?;
    let connection = pool.get().map_err(AppError::Pool)?;
    let transaction = connection.unchecked_transaction()?;
    let locked_run =
        transaction.execute(queries::deduplicate::LOCK_RUN_FOR_REPLACEMENT, [run_id])?;
    if locked_run == 0 {
        transaction.rollback()?;
        return Ok(());
    }
    transaction.execute(queries::deduplicate::CLEAN_CLUSTERS, [])?;
    transaction.execute(
        queries::deduplicate::INSERT_CLUSTERS_FROM_JSON,
        [&cluster_payload],
    )?;
    transaction.execute(
        queries::deduplicate::INSERT_CLUSTER_MEMBERS_FROM_JSON,
        [&cluster_payload],
    )?;
    transaction.execute(
        queries::deduplicate::UPDATE_RUN_PROGRESS,
        rusqlite::params![0_i64, 0_i64, 0_i64, cluster_count, run_id],
    )?;
    transaction.execute(
        queries::deduplicate::COMPLETE_RUN,
        rusqlite::params!["completed", Option::<String>::None, run_id],
    )?;
    transaction.commit()?;
    Ok(())
}

fn cancellation_requested(pool: &DbPool, run_id: i64) -> AppResult<bool> {
    let connection = pool.get().map_err(AppError::Pool)?;
    let status = fetch_one(
        &connection,
        queries::deduplicate::SELECT_RUN_STATUS,
        &[&run_id],
        |row| row.get::<_, String>(0),
    )?;
    Ok(status.as_deref() == Some("cancelling"))
}

fn update_run_progress(
    pool: &DbPool,
    run_id: i64,
    indexed_media: i64,
    processed_media: i64,
    candidate_comparisons: i64,
    clusters_created: i64,
) -> AppResult<()> {
    let connection = pool.get().map_err(AppError::Pool)?;
    execute_query(
        &connection,
        queries::deduplicate::UPDATE_RUN_PROGRESS,
        &[
            &indexed_media,
            &processed_media,
            &candidate_comparisons,
            &clusters_created,
            &run_id,
        ],
    )?;
    Ok(())
}

pub fn cosine_similarity(left: &[f32], right: &[f32]) -> Option<f32> {
    if left.is_empty() || left.len() != right.len() {
        return None;
    }
    let mut dot_product = 0.0_f32;
    let mut left_norm = 0.0_f32;
    let mut right_norm = 0.0_f32;
    for (left_component, right_component) in left.iter().zip(right) {
        dot_product += left_component * right_component;
        left_norm += left_component * left_component;
        right_norm += right_component * right_component;
    }
    if left_norm == 0.0 || right_norm == 0.0 {
        return None;
    }
    Some(dot_product / (left_norm.sqrt() * right_norm.sqrt()))
}

pub fn embedding_to_blob(embedding: &[f32]) -> Vec<u8> {
    embedding
        .iter()
        .flat_map(|component| component.to_le_bytes())
        .collect()
}

pub fn blob_to_embedding(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

pub fn log_schedule_start(scheduled_for: &str) {
    info!(
        "Starting scheduled deduplicate scan for {} at {}",
        scheduled_for,
        Utc::now().to_rfc3339()
    );
}
