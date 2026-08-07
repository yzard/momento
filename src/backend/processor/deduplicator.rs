use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use chrono::Utc;
use tracing::{info, warn};

use crate::config::Config;
use crate::constants::paths;
use crate::database::{execute_query, fetch_all, fetch_one, insert_returning_id, queries, DbPool};
use crate::error::{AppError, AppResult};
use crate::llm_client::{begin_inference_batch, LlmClient};
use crate::utils::datetime::parse_datetime;

const INDEX_PAGE_SIZE: usize = 64;
const CANDIDATE_PAGE_SIZE: usize = 256;
const PREPROCESSING_VERSION: &str = "original-or-first-frame-v1";
const NEAR_DUPLICATE_COSINE_SIMILARITY: f32 = 0.97;
const BURST_COSINE_SIMILARITY: f32 = 0.90;
const PERCEPTUAL_HASH_DISTANCE: u32 = 10;
const BURST_WINDOW_SECONDS: i64 = 10;

#[derive(Debug)]
struct IndexMediaRow {
    id: i64,
    file_path: String,
    media_type: String,
    content_hash: String,
    date_taken: Option<String>,
}

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
    execute_query(&connection, queries::deduplicate::INTERRUPT_RUNNING, &[])?;
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

pub async fn run_scan(config: &Config, pool: &DbPool, run_id: i64) {
    let scan_result = run_scan_inner(config, pool, run_id).await;
    let completion = match scan_result {
        Ok(true) => ("completed", None),
        Ok(false) => ("cancelled", None),
        Err(error) => {
            warn!("Deduplicate scan {} failed: {}", run_id, error);
            ("failed", Some(error.to_string()))
        }
    };
    let connection = match pool.get() {
        Ok(connection) => connection,
        Err(error) => {
            warn!("Could not persist completion for deduplicate run {run_id}: {error}");
            return;
        }
    };
    if completion.0 != "completed" {
        if let Err(error) = execute_query(&connection, queries::deduplicate::MARK_ALL_DIRTY, &[]) {
            warn!("Could not preserve dirty work for deduplicate run {run_id}: {error}");
        }
    }
    if let Err(error) = execute_query(
        &connection,
        queries::deduplicate::COMPLETE_RUN,
        &[&completion.0, &completion.1, &run_id],
    ) {
        warn!("Could not persist completion for deduplicate run {run_id}: {error}");
    }
}

async fn run_scan_inner(config: &Config, pool: &DbPool, run_id: i64) -> AppResult<bool> {
    if !config.llm.enabled {
        return Err(AppError::Validation(
            "deduplicate requires llm.enabled = true".to_string(),
        ));
    }
    if !config.llm.deduplicate_enabled {
        return Err(AppError::Validation(
            "deduplication is disabled in LLM configuration".to_string(),
        ));
    }
    let client =
        LlmClient::new(&config.llm).map_err(|error| AppError::Internal(error.to_string()))?;
    client
        .wait_until_ready()
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?;

    let inference_batch = begin_inference_batch().await;
    index_missing_media(pool, run_id, &client).await?;
    drop(inference_batch);
    if cancellation_requested(pool, run_id)? {
        return Ok(false);
    }
    generate_clusters(pool, run_id)?;
    Ok(!cancellation_requested(pool, run_id)?)
}

async fn index_missing_media(pool: &DbPool, run_id: i64, client: &LlmClient) -> AppResult<()> {
    let mut cursor = 0_i64;
    let mut failure_count = 0_usize;
    let mut first_failure = None;
    loop {
        if cancellation_requested(pool, run_id)? {
            return Ok(());
        }
        let rows = {
            let connection = pool.get().map_err(AppError::Pool)?;
            fetch_all(
                &connection,
                queries::deduplicate::SELECT_INDEX_PAGE,
                &[&cursor, &(INDEX_PAGE_SIZE as i64)],
                |row| {
                    Ok(IndexMediaRow {
                        id: row.get(0)?,
                        file_path: row.get(1)?,
                        media_type: row.get(2)?,
                        content_hash: row.get(3)?,
                        date_taken: row.get(4)?,
                    })
                },
            )?
        };
        if rows.is_empty() {
            if failure_count == 0 {
                return Ok(());
            }
            return Err(AppError::Internal(format!(
                "{} media failed similarity indexing; first error: {}",
                failure_count,
                first_failure.unwrap_or_else(|| "unknown indexing error".to_string())
            )));
        }
        let mut indexed_media = 0_i64;
        for media in rows {
            if cancellation_requested(pool, run_id)? {
                return Ok(());
            }
            cursor = media.id;
            match index_media(pool, client, &media).await {
                Ok(()) => indexed_media += 1,
                Err(AppError::BadRequest(error)) => {
                    warn!(
                        "Skipping unreadable media {} during similarity indexing: {}",
                        media.id, error
                    );
                    record_index_failure(pool, &media, &error)?;
                }
                Err(error) => {
                    warn!(
                        "Similarity indexing failed for media {}: {}",
                        media.id, error
                    );
                    failure_count += 1;
                    if first_failure.is_none() {
                        first_failure = Some(format!("media {}: {}", media.id, error));
                    }
                }
            }
        }
        update_run_progress(pool, run_id, indexed_media, 0, 0, 0)?;
    }
}

async fn index_media(pool: &DbPool, client: &LlmClient, media: &IndexMediaRow) -> AppResult<()> {
    let original_path = paths().originals.join(&media.file_path);
    let prepared_path = if media.media_type == "video" {
        Some(extract_first_video_frame(media.id, &original_path).await?)
    } else {
        None
    };
    let clustering_path = prepared_path.as_deref().unwrap_or(&original_path);
    let clustering_result = client.image_clustering_in_batch(clustering_path).await;
    if let Some(frame_path) = prepared_path.as_ref() {
        let _ = tokio::fs::remove_file(frame_path).await;
    }
    let clustering = clustering_result.map_err(|error| match error {
        crate::llm_client::LlmClientError::InvalidImage(message)
        | crate::llm_client::LlmClientError::ReadImage(message)
        | crate::llm_client::LlmClientError::ConvertImage(message) => AppError::BadRequest(message),
        other => AppError::Internal(other.to_string()),
    })?;
    let embedding_blob = embedding_to_blob(&clustering.embedding);
    let capture_time_seconds = media
        .date_taken
        .as_deref()
        .and_then(parse_datetime)
        .map(|date| date.timestamp());
    let connection = pool.get().map_err(AppError::Pool)?;
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(
        queries::deduplicate::UPSERT_INDEX,
        rusqlite::params![
            media.id,
            media.content_hash,
            clustering.model_version,
            PREPROCESSING_VERSION,
            embedding_blob,
            clustering.perceptual_hash as i64,
            capture_time_seconds,
        ],
    )?;
    transaction.execute(queries::deduplicate::DELETE_BANDS, [media.id])?;
    for band_index in 0..4_i64 {
        let band_value = ((clustering.perceptual_hash >> (band_index * 16)) & 0xffff) as i64;
        transaction.execute(
            queries::deduplicate::INSERT_BAND,
            rusqlite::params![media.id, band_index, band_value],
        )?;
    }
    transaction.execute(queries::deduplicate::MARK_DIRTY, [media.id])?;
    transaction.commit()?;
    Ok(())
}

fn record_index_failure(pool: &DbPool, media: &IndexMediaRow, error: &str) -> AppResult<()> {
    let connection = pool.get().map_err(AppError::Pool)?;
    execute_query(
        &connection,
        queries::deduplicate::UPSERT_FAILED_INDEX,
        &[&media.id, &media.content_hash, &error],
    )?;
    Ok(())
}

async fn extract_first_video_frame(media_id: i64, source_path: &Path) -> AppResult<PathBuf> {
    let output_directory = paths().previews.join("deduplicate");
    tokio::fs::create_dir_all(&output_directory).await?;
    let output_path = output_directory.join(format!("{media_id}.jpg"));
    let mut command = tokio::process::Command::new("ffmpeg");
    command.kill_on_drop(true).args([
        "-y",
        "-i",
        source_path.to_str().unwrap_or(""),
        "-frames:v",
        "1",
        "-vf",
        "scale='min(1920,iw)':-2",
        output_path.to_str().unwrap_or(""),
    ]);
    let output = tokio::time::timeout(std::time::Duration::from_secs(60), command.output())
        .await
        .map_err(|_| AppError::Internal("FFmpeg frame extraction timed out".to_string()))??;
    if !output.status.success() {
        return Err(AppError::Internal(format!(
            "FFmpeg frame extraction failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(output_path)
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
    let connection = pool.get().map_err(AppError::Pool)?;
    let transaction = connection.unchecked_transaction()?;
    let locked_run =
        transaction.execute(queries::deduplicate::LOCK_RUN_FOR_REPLACEMENT, [run_id])?;
    if locked_run == 0 {
        transaction.rollback()?;
        return Ok(());
    }
    transaction.execute(queries::deduplicate::CLEAN_CLUSTERS, [])?;
    let cluster_count = clusters.len() as i64;
    for cluster in clusters {
        transaction.execute(
            queries::deduplicate::INSERT_CLUSTER,
            rusqlite::params![cluster.kind.as_str(), cluster.representative_media_id],
        )?;
        let cluster_id = transaction.last_insert_rowid();
        for member in cluster.members {
            transaction.execute(
                queries::deduplicate::INSERT_CLUSTER_MEMBER,
                rusqlite::params![
                    cluster_id,
                    member.media_id,
                    member.cosine_similarity,
                    member.perceptual_hash_distance,
                ],
            )?;
        }
    }
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
