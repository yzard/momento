use chrono::Utc;
use momento_common::llm::IMAGE_CLUSTERING_MODEL_VERSION;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use tracing::info;

use crate::database::{fetch_all, fetch_one, insert_returning_id, queries};
use crate::error::{AppError, AppResult};
use crate::executor::ExecutorError;
use crate::runtime::ExecutorHandles;
use crate::utils::embedding::{blob_to_embedding, cosine_similarity};

const FINALIZATION_PAGE_SIZE: usize = 64;
const NEAR_DUPLICATE_COSINE_SIMILARITY: f32 = 0.97;
const BURST_COSINE_SIMILARITY: f32 = 0.90;
const PERCEPTUAL_HASH_DISTANCE: u32 = 10;
const BURST_WINDOW_SECONDS: i64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClusterKind {
    NearDuplicate,
    Burst,
}

impl ClusterKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NearDuplicate => "near_duplicate",
            Self::Burst => "burst",
        }
    }

    fn parse(value: &str) -> AppResult<Self> {
        match value {
            "near_duplicate" => Ok(Self::NearDuplicate),
            "burst" => Ok(Self::Burst),
            _ => Err(AppError::Internal(format!(
                "unknown deduplicate cluster kind {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SimilarityVector {
    pub media_id: i64,
    pub embedding: Vec<f32>,
    pub perceptual_hash: u64,
    pub capture_time_seconds: Option<i64>,
}

#[derive(Debug)]
pub struct DeduplicateComparisonPage {
    pub run_id: i64,
    pub source: SimilarityVector,
    pub kind: ClusterKind,
    pub candidates: Vec<SimilarityVector>,
    pub exhausted: bool,
}

#[derive(Debug)]
pub struct DeduplicateGroupPage {
    pub run_id: i64,
    pub cluster_id: i64,
    pub kind: ClusterKind,
    pub representative: SimilarityVector,
    pub members: Vec<SimilarityVector>,
    pub exhausted: bool,
}

#[derive(Debug)]
pub enum DeduplicateFinalizationWork {
    Idle,
    Progressed,
    Compare(DeduplicateComparisonPage),
    MeasureGroup(DeduplicateGroupPage),
}

#[derive(Debug)]
pub struct DeduplicateEdge {
    pub left_media_id: i64,
    pub right_media_id: i64,
    pub cosine_similarity: f32,
    pub perceptual_hash_distance: u32,
}

#[derive(Debug)]
pub struct DeduplicateMemberMetric {
    pub media_id: i64,
    pub cosine_similarity: f32,
    pub perceptual_hash_distance: u32,
}

#[derive(Debug)]
pub enum DeduplicateCpuResult {
    Comparison {
        run_id: i64,
        source_media_id: i64,
        kind: ClusterKind,
        candidate_cursor: i64,
        exhausted: bool,
        comparisons: i64,
        edges: Vec<DeduplicateEdge>,
    },
    GroupMetrics {
        run_id: i64,
        cluster_id: i64,
        kind: ClusterKind,
        member_cursor: i64,
        exhausted: bool,
        metrics: Vec<DeduplicateMemberMetric>,
    },
}

#[derive(Debug)]
struct FinalizationState {
    generation_id: i64,
    phase: String,
    source_media_id: Option<i64>,
    source_cursor: i64,
    candidate_kind: ClusterKind,
    candidate_cursor: i64,
    label_kind: ClusterKind,
    label_media_cursor: i64,
    label_edge_left_cursor: i64,
    label_edge_right_cursor: i64,
    label_pass_changed: bool,
    group_kind: ClusterKind,
    group_label_cursor: i64,
    group_member_cursor: i64,
    group_cluster_id: Option<i64>,
    completion_error: Option<String>,
    dirty_cursor: i64,
}

#[derive(Debug)]
struct ComparisonCommit {
    run_id: i64,
    source_media_id: i64,
    kind: ClusterKind,
    candidate_cursor: i64,
    exhausted: bool,
    comparisons: i64,
    edges: Vec<DeduplicateEdge>,
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

pub fn create_run(
    connection: &Connection,
    trigger: &str,
    scheduled_for: Option<&str>,
) -> AppResult<i64> {
    insert_returning_id(
        connection,
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

pub fn latest_run(connection: &Connection) -> AppResult<Option<DeduplicateRunStatus>> {
    fetch_one(
        connection,
        queries::deduplicate::SELECT_LATEST_RUN,
        &[],
        map_run_status,
    )
}

pub(crate) fn map_run_status(row: &rusqlite::Row) -> rusqlite::Result<DeduplicateRunStatus> {
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
}

pub fn request_cancel(connection: &Connection) -> AppResult<bool> {
    let Some(run) = latest_run(connection)? else {
        return Ok(false);
    };
    if run.status != "running" {
        return Ok(false);
    }
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(
        queries::ai_jobs::QUEUE_CANCELLATION_SCOPE_FOR_TASK,
        ["image_clustering"],
    )?;
    transaction.execute(
        queries::ai_jobs::QUEUE_CANCELLATIONS_FOR_TASK,
        ["image_clustering"],
    )?;
    transaction.execute(queries::ai_jobs::CANCEL_FOR_TASK, ["image_clustering"])?;
    let cancelled = transaction.execute(queries::deduplicate::REQUEST_CANCEL, [run.id])? > 0;
    transaction.commit()?;
    Ok(cancelled)
}

pub fn queue_clustering_jobs(connection: &Connection, run_id: i64) -> AppResult<usize> {
    let run_status: String =
        connection.query_row(queries::deduplicate::SELECT_RUN_STATUS, [run_id], |row| {
            row.get(0)
        })?;
    if run_status != "running" {
        return Ok(0);
    }
    let transaction = connection.unchecked_transaction()?;
    let indexes_from_other_model: i64 = transaction.query_row(
        queries::deduplicate::COUNT_INDEXES_FROM_OTHER_MODEL,
        [IMAGE_CLUSTERING_MODEL_VERSION],
        |row| row.get(0),
    )?;
    if indexes_from_other_model > 0 {
        transaction.execute(
            queries::deduplicate::DELETE_HASH_BANDS_FROM_OTHER_MODEL,
            [IMAGE_CLUSTERING_MODEL_VERSION],
        )?;
        transaction.execute(
            queries::deduplicate::DELETE_INDEXES_FROM_OTHER_MODEL,
            [IMAGE_CLUSTERING_MODEL_VERSION],
        )?;
        transaction.execute(queries::deduplicate::MARK_ALL_DIRTY, [])?;
    }
    let queued_jobs = transaction.execute(
        queries::deduplicate::CREATE_CLUSTERING_JOBS,
        rusqlite::params![run_id, run_id],
    )?;
    transaction.execute(queries::ai_jobs::SNAPSHOT_QUEUED_INPUTS, [])?;
    transaction.commit()?;
    Ok(queued_jobs)
}

pub async fn finalize_ready_runs(executors: &ExecutorHandles) -> Result<(), ExecutorError> {
    loop {
        let work = executors
            .sqlite
            .load_deduplicate_finalization_work()
            .await?;
        match work {
            DeduplicateFinalizationWork::Idle => return Ok(()),
            DeduplicateFinalizationWork::Progressed => {}
            DeduplicateFinalizationWork::Compare(page) => {
                let result = executors.cpu.compare_deduplicate_page(page).await?;
                executors
                    .sqlite
                    .commit_deduplicate_cpu_result(result)
                    .await?;
            }
            DeduplicateFinalizationWork::MeasureGroup(page) => {
                let result = executors.cpu.measure_deduplicate_group_page(page).await?;
                executors
                    .sqlite
                    .commit_deduplicate_cpu_result(result)
                    .await?;
            }
        }
    }
}

pub fn load_finalization_work(connection: &Connection) -> AppResult<DeduplicateFinalizationWork> {
    if cleanup_finalization_page(connection)? || cleanup_retired_generation_page(connection)? {
        return Ok(DeduplicateFinalizationWork::Progressed);
    }

    let run = connection
        .query_row(queries::deduplicate::SELECT_ACTIVE_RUNS, [], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .optional()?;
    let Some((run_id, status)) = run else {
        return Ok(DeduplicateFinalizationWork::Idle);
    };
    if status == "cancelling" {
        cancel_finalization(connection, run_id)?;
        return Ok(DeduplicateFinalizationWork::Progressed);
    }

    let transaction = connection.unchecked_transaction()?;
    transaction.execute(queries::deduplicate::REQUEUE_MISSING_INPUT_JOBS, [run_id])?;
    transaction.execute(queries::ai_jobs::SNAPSHOT_QUEUED_INPUTS, [])?;
    transaction.commit()?;
    let pending: i64 =
        connection.query_row(queries::deduplicate::COUNT_PENDING_JOBS, [run_id], |row| {
            row.get(0)
        })?;
    if pending != 0 {
        return Ok(DeduplicateFinalizationWork::Idle);
    }

    let Some(state) = load_finalization_state(connection, run_id)? else {
        initialize_finalization(connection, run_id)?;
        return Ok(DeduplicateFinalizationWork::Progressed);
    };
    match state.phase.as_str() {
        "dirty_snapshot" => snapshot_dirty_page(connection, run_id, &state),
        "comparison" => load_comparison_work(connection, run_id, &state),
        "label_initialization" => initialize_label_page(connection, run_id, &state),
        "label_propagation" => propagate_label_page(connection, run_id, &state),
        "grouping" => load_group_work(connection, run_id, &state),
        "publishing" => publish_generation(connection, run_id, &state),
        "cleanup" => Ok(DeduplicateFinalizationWork::Progressed),
        phase => Err(AppError::Internal(format!(
            "unknown deduplicate finalization phase {phase}"
        ))),
    }
}

pub fn compare_page(page: DeduplicateComparisonPage) -> Result<DeduplicateCpuResult, String> {
    validate_vector(&page.source)?;
    let mut edges = Vec::new();
    edges
        .try_reserve_exact(page.candidates.len())
        .map_err(|error| format!("could not reserve deduplicate edge page: {error}"))?;
    let candidate_cursor = page
        .candidates
        .last()
        .map_or(0, |candidate| candidate.media_id);
    let comparisons = i64::try_from(page.candidates.len())
        .map_err(|_| "deduplicate comparison count overflowed".to_string())?;
    for candidate in page.candidates {
        validate_vector(&candidate)?;
        let Some(similarity) = cosine_similarity(&page.source.embedding, &candidate.embedding)
        else {
            continue;
        };
        let hash_distance = (page.source.perceptual_hash ^ candidate.perceptual_hash).count_ones();
        let qualifies = match page.kind {
            ClusterKind::NearDuplicate => {
                similarity >= NEAR_DUPLICATE_COSINE_SIMILARITY
                    && hash_distance <= PERCEPTUAL_HASH_DISTANCE
            }
            ClusterKind::Burst => {
                similarity >= BURST_COSINE_SIMILARITY
                    && hash_distance <= PERCEPTUAL_HASH_DISTANCE.saturating_mul(2)
                    && page
                        .source
                        .capture_time_seconds
                        .zip(candidate.capture_time_seconds)
                        .is_some_and(|(source_time, candidate_time)| {
                            (source_time - candidate_time).abs() <= BURST_WINDOW_SECONDS
                        })
            }
        };
        if !qualifies {
            continue;
        }
        edges.push(DeduplicateEdge {
            left_media_id: candidate.media_id.min(page.source.media_id),
            right_media_id: candidate.media_id.max(page.source.media_id),
            cosine_similarity: similarity,
            perceptual_hash_distance: hash_distance,
        });
    }
    Ok(DeduplicateCpuResult::Comparison {
        run_id: page.run_id,
        source_media_id: page.source.media_id,
        kind: page.kind,
        candidate_cursor,
        exhausted: page.exhausted,
        comparisons,
        edges,
    })
}

pub fn measure_group_page(page: DeduplicateGroupPage) -> Result<DeduplicateCpuResult, String> {
    validate_vector(&page.representative)?;
    let member_cursor = page.members.last().map_or(0, |member| member.media_id);
    let mut metrics = Vec::new();
    metrics
        .try_reserve_exact(page.members.len())
        .map_err(|error| format!("could not reserve deduplicate group metric page: {error}"))?;
    for member in page.members {
        validate_vector(&member)?;
        let (similarity, distance) = if member.media_id == page.representative.media_id {
            (1.0, 0)
        } else {
            let similarity = cosine_similarity(&page.representative.embedding, &member.embedding)
                .ok_or_else(|| {
                "deduplicate group embeddings do not have equal dimensions".to_string()
            })?;
            let distance =
                (page.representative.perceptual_hash ^ member.perceptual_hash).count_ones();
            (similarity, distance)
        };
        metrics.push(DeduplicateMemberMetric {
            media_id: member.media_id,
            cosine_similarity: similarity,
            perceptual_hash_distance: distance,
        });
    }
    Ok(DeduplicateCpuResult::GroupMetrics {
        run_id: page.run_id,
        cluster_id: page.cluster_id,
        kind: page.kind,
        member_cursor,
        exhausted: page.exhausted,
        metrics,
    })
}

pub fn commit_cpu_result(connection: &Connection, result: DeduplicateCpuResult) -> AppResult<()> {
    match result {
        DeduplicateCpuResult::Comparison {
            run_id,
            source_media_id,
            kind,
            candidate_cursor,
            exhausted,
            comparisons,
            edges,
        } => commit_comparison_page(
            connection,
            ComparisonCommit {
                run_id,
                source_media_id,
                kind,
                candidate_cursor,
                exhausted,
                comparisons,
                edges,
            },
        ),
        DeduplicateCpuResult::GroupMetrics {
            run_id,
            cluster_id,
            kind,
            member_cursor,
            exhausted,
            metrics,
        } => commit_group_page(
            connection,
            run_id,
            cluster_id,
            kind,
            member_cursor,
            exhausted,
            metrics,
        ),
    }
}

fn validate_vector(vector: &SimilarityVector) -> Result<(), String> {
    if vector.media_id <= 0 || vector.embedding.is_empty() || vector.embedding.len() > 4_096 {
        return Err("deduplicate vector has invalid identity or dimensions".to_string());
    }
    if vector.embedding.iter().any(|value| !value.is_finite()) {
        return Err("deduplicate vector contains a non-finite value".to_string());
    }
    Ok(())
}

fn map_similarity_vector(row: &rusqlite::Row<'_>) -> rusqlite::Result<SimilarityVector> {
    Ok(SimilarityVector {
        media_id: row.get(0)?,
        embedding: blob_to_embedding(&row.get::<_, Vec<u8>>(1)?),
        perceptual_hash: row.get::<_, i64>(2)? as u64,
        capture_time_seconds: row.get(3)?,
    })
}

fn load_vector(connection: &Connection, media_id: i64) -> AppResult<SimilarityVector> {
    fetch_one(
        connection,
        queries::deduplicate::SELECT_CURRENT_INDEX_BY_MEDIA_ID,
        &[&media_id],
        map_similarity_vector,
    )?
    .ok_or_else(|| AppError::Internal(format!("missing similarity vector for media {media_id}")))
}

fn load_finalization_state(
    connection: &Connection,
    run_id: i64,
) -> AppResult<Option<FinalizationState>> {
    fetch_one(
        connection,
        queries::deduplicate::SELECT_FINALIZATION_STATE,
        &[&run_id],
        |row| {
            let candidate_kind = row.get::<_, String>(4)?;
            let label_kind = row.get::<_, String>(6)?;
            let group_kind = row.get::<_, String>(11)?;
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, i64>(3)?,
                candidate_kind,
                row.get::<_, i64>(5)?,
                label_kind,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, i64>(9)?,
                row.get::<_, i64>(10)? != 0,
                group_kind,
                row.get::<_, i64>(12)?,
                row.get::<_, i64>(13)?,
                row.get::<_, Option<i64>>(14)?,
                row.get::<_, Option<String>>(15)?,
                row.get::<_, i64>(16)?,
            ))
        },
    )?
    .map(|row| {
        Ok(FinalizationState {
            generation_id: row.0,
            phase: row.1,
            source_media_id: row.2,
            source_cursor: row.3,
            candidate_kind: ClusterKind::parse(&row.4)?,
            candidate_cursor: row.5,
            label_kind: ClusterKind::parse(&row.6)?,
            label_media_cursor: row.7,
            label_edge_left_cursor: row.8,
            label_edge_right_cursor: row.9,
            label_pass_changed: row.10,
            group_kind: ClusterKind::parse(&row.11)?,
            group_label_cursor: row.12,
            group_member_cursor: row.13,
            group_cluster_id: row.14,
            completion_error: row.15,
            dirty_cursor: row.16,
        })
    })
    .transpose()
}

fn initialize_finalization(connection: &Connection, run_id: i64) -> AppResult<()> {
    let failed_jobs: i64 =
        connection.query_row(queries::deduplicate::COUNT_FAILED_JOBS, [run_id], |row| {
            row.get(0)
        })?;
    let completion_error = (failed_jobs > 0).then(|| {
        format!(
            "{failed_jobs} image clustering jobs failed; groups generated from successful results"
        )
    });
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(queries::deduplicate::INSERT_GENERATION, [run_id])?;
    let generation_id = transaction.last_insert_rowid();
    transaction.execute(
        queries::deduplicate::INSERT_FINALIZATION,
        rusqlite::params![run_id, generation_id, completion_error],
    )?;
    transaction.commit()?;
    Ok(())
}

fn snapshot_dirty_page(
    connection: &Connection,
    run_id: i64,
    state: &FinalizationState,
) -> AppResult<DeduplicateFinalizationWork> {
    let dirty_rows = fetch_all(
        connection,
        queries::deduplicate::SELECT_FINALIZATION_DIRTY_SOURCE_PAGE,
        &[&state.dirty_cursor, &(FINALIZATION_PAGE_SIZE as i64)],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
    )?;
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    for (media_id, marked_at) in &dirty_rows {
        transaction.execute(
            queries::deduplicate::INSERT_FINALIZATION_DIRTY,
            rusqlite::params![run_id, media_id, marked_at],
        )?;
    }
    if let Some((media_id, _)) = dirty_rows.last() {
        transaction.execute(
            queries::deduplicate::ADVANCE_FINALIZATION_DIRTY_CURSOR,
            rusqlite::params![media_id, run_id],
        )?;
    } else {
        transaction.execute(
            queries::deduplicate::FINISH_FINALIZATION_DIRTY_SNAPSHOT,
            [run_id],
        )?;
    }
    transaction.commit()?;
    Ok(DeduplicateFinalizationWork::Progressed)
}

fn load_comparison_work(
    connection: &Connection,
    run_id: i64,
    state: &FinalizationState,
) -> AppResult<DeduplicateFinalizationWork> {
    let Some(source_media_id) = state.source_media_id else {
        let next_media_id = connection
            .query_row(
                queries::deduplicate::SELECT_NEXT_INDEX_ID,
                [state.source_cursor],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        if let Some(next_media_id) = next_media_id {
            connection.execute(
                queries::deduplicate::START_COMPARISON_SOURCE,
                rusqlite::params![next_media_id, run_id],
            )?;
        } else {
            connection.execute(queries::deduplicate::FINISH_COMPARISONS, [run_id])?;
        }
        return Ok(DeduplicateFinalizationWork::Progressed);
    };
    let source = load_vector(connection, source_media_id)?;
    let candidates = match state.candidate_kind {
        ClusterKind::NearDuplicate => fetch_all(
            connection,
            queries::deduplicate::SELECT_BAND_CANDIDATE_PAGE,
            &[
                &source_media_id,
                &state.candidate_cursor,
                &source_media_id,
                &(FINALIZATION_PAGE_SIZE as i64),
            ],
            map_similarity_vector,
        )?,
        ClusterKind::Burst => {
            let Some(capture_time) = source.capture_time_seconds else {
                return Ok(DeduplicateFinalizationWork::Compare(
                    DeduplicateComparisonPage {
                        run_id,
                        source,
                        kind: state.candidate_kind,
                        candidates: Vec::new(),
                        exhausted: true,
                    },
                ));
            };
            fetch_all(
                connection,
                queries::deduplicate::SELECT_TIME_CANDIDATE_PAGE,
                &[
                    &(capture_time - BURST_WINDOW_SECONDS),
                    &(capture_time + BURST_WINDOW_SECONDS),
                    &source_media_id,
                    &state.candidate_cursor,
                    &(FINALIZATION_PAGE_SIZE as i64),
                ],
                map_similarity_vector,
            )?
        }
    };
    let exhausted = candidates.len() < FINALIZATION_PAGE_SIZE;
    Ok(DeduplicateFinalizationWork::Compare(
        DeduplicateComparisonPage {
            run_id,
            source,
            kind: state.candidate_kind,
            candidates,
            exhausted,
        },
    ))
}

fn commit_comparison_page(connection: &Connection, request: ComparisonCommit) -> AppResult<()> {
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    for edge in request.edges {
        transaction.execute(
            queries::deduplicate::INSERT_EDGE,
            rusqlite::params![
                request.run_id,
                request.kind.as_str(),
                edge.left_media_id,
                edge.right_media_id,
                edge.cosine_similarity,
                edge.perceptual_hash_distance,
            ],
        )?;
    }
    transaction.execute(
        queries::deduplicate::UPDATE_RUN_PROGRESS,
        rusqlite::params![0_i64, 0_i64, request.comparisons, 0_i64, request.run_id],
    )?;
    if !request.exhausted {
        transaction.execute(
            queries::deduplicate::ADVANCE_COMPARISON_PAGE,
            rusqlite::params![
                request.candidate_cursor,
                request.run_id,
                request.source_media_id,
                request.kind.as_str(),
                request.candidate_cursor,
            ],
        )?;
    } else if request.kind == ClusterKind::NearDuplicate {
        transaction.execute(
            queries::deduplicate::ADVANCE_COMPARISON_KIND,
            rusqlite::params![request.run_id, request.source_media_id],
        )?;
    } else {
        transaction.execute(
            queries::deduplicate::FINISH_COMPARISON_SOURCE,
            rusqlite::params![
                request.source_media_id,
                request.run_id,
                request.source_media_id,
            ],
        )?;
        transaction.execute(
            queries::deduplicate::UPDATE_RUN_PROGRESS,
            rusqlite::params![0_i64, 1_i64, 0_i64, 0_i64, request.run_id],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

fn initialize_label_page(
    connection: &Connection,
    run_id: i64,
    state: &FinalizationState,
) -> AppResult<DeduplicateFinalizationWork> {
    let media_ids = fetch_all(
        connection,
        queries::deduplicate::SELECT_LABEL_INITIALIZATION_PAGE,
        &[&state.label_media_cursor, &(FINALIZATION_PAGE_SIZE as i64)],
        |row| row.get::<_, i64>(0),
    )?;
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    for media_id in &media_ids {
        transaction.execute(
            queries::deduplicate::INSERT_LABEL,
            rusqlite::params![run_id, state.label_kind.as_str(), media_id, media_id],
        )?;
    }
    if let Some(last_media_id) = media_ids.last() {
        transaction.execute(
            queries::deduplicate::ADVANCE_LABEL_INITIALIZATION,
            rusqlite::params![last_media_id, run_id, state.label_kind.as_str()],
        )?;
    } else if state.label_kind == ClusterKind::NearDuplicate {
        transaction.execute(
            queries::deduplicate::SWITCH_LABEL_INITIALIZATION_KIND,
            [run_id],
        )?;
    } else {
        transaction.execute(queries::deduplicate::FINISH_LABEL_INITIALIZATION, [run_id])?;
    }
    transaction.commit()?;
    Ok(DeduplicateFinalizationWork::Progressed)
}

fn propagate_label_page(
    connection: &Connection,
    run_id: i64,
    state: &FinalizationState,
) -> AppResult<DeduplicateFinalizationWork> {
    let edges = fetch_all(
        connection,
        queries::deduplicate::SELECT_EDGE_PAGE,
        &[
            &run_id,
            &state.label_kind.as_str(),
            &state.label_edge_left_cursor,
            &state.label_edge_left_cursor,
            &state.label_edge_right_cursor,
            &(FINALIZATION_PAGE_SIZE as i64),
        ],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    )?;
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    let mut changed = false;
    for (left_media_id, right_media_id) in &edges {
        let left_label: i64 = transaction.query_row(
            queries::deduplicate::SELECT_LABEL,
            rusqlite::params![run_id, state.label_kind.as_str(), left_media_id],
            |row| row.get(0),
        )?;
        let right_label: i64 = transaction.query_row(
            queries::deduplicate::SELECT_LABEL,
            rusqlite::params![run_id, state.label_kind.as_str(), right_media_id],
            |row| row.get(0),
        )?;
        let minimum = left_label.min(right_label);
        changed |= transaction.execute(
            queries::deduplicate::LOWER_LABEL,
            rusqlite::params![
                minimum,
                run_id,
                state.label_kind.as_str(),
                left_media_id,
                minimum,
            ],
        )? > 0;
        changed |= transaction.execute(
            queries::deduplicate::LOWER_LABEL,
            rusqlite::params![
                minimum,
                run_id,
                state.label_kind.as_str(),
                right_media_id,
                minimum,
            ],
        )? > 0;
    }
    if let Some((left_media_id, right_media_id)) = edges.last() {
        transaction.execute(
            queries::deduplicate::ADVANCE_LABEL_EDGE_PAGE,
            rusqlite::params![
                left_media_id,
                right_media_id,
                i64::from(changed),
                run_id,
                state.label_kind.as_str(),
            ],
        )?;
    } else if state.label_pass_changed {
        transaction.execute(
            queries::deduplicate::RESTART_LABEL_PASS,
            rusqlite::params![run_id, state.label_kind.as_str()],
        )?;
    } else if state.label_kind == ClusterKind::NearDuplicate {
        transaction.execute(
            queries::deduplicate::SWITCH_LABEL_PROPAGATION_KIND,
            [run_id],
        )?;
    } else {
        transaction.execute(queries::deduplicate::FINISH_LABEL_PROPAGATION, [run_id])?;
    }
    transaction.commit()?;
    Ok(DeduplicateFinalizationWork::Progressed)
}

fn load_group_work(
    connection: &Connection,
    run_id: i64,
    state: &FinalizationState,
) -> AppResult<DeduplicateFinalizationWork> {
    let cluster_id = if let Some(cluster_id) = state.group_cluster_id {
        cluster_id
    } else {
        let next_label = connection
            .query_row(
                queries::deduplicate::SELECT_NEXT_COMPONENT,
                rusqlite::params![run_id, state.group_kind.as_str(), state.group_label_cursor],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(next_label) = next_label else {
            if state.group_kind == ClusterKind::NearDuplicate {
                connection.execute(queries::deduplicate::SWITCH_GROUP_KIND, [run_id])?;
            } else {
                connection.execute(queries::deduplicate::FINISH_GROUPING, [run_id])?;
            }
            return Ok(DeduplicateFinalizationWork::Progressed);
        };
        if state.group_kind == ClusterKind::Burst
            && burst_component_duplicates_near(connection, run_id, next_label)?
        {
            connection.execute(
                queries::deduplicate::SKIP_GROUP,
                rusqlite::params![next_label, run_id, state.group_kind.as_str()],
            )?;
            return Ok(DeduplicateFinalizationWork::Progressed);
        }
        let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
        transaction.execute(
            queries::deduplicate::INSERT_GENERATION_CLUSTER,
            rusqlite::params![state.generation_id, state.group_kind.as_str(), next_label],
        )?;
        let cluster_id = transaction.last_insert_rowid();
        transaction.execute(
            queries::deduplicate::START_GROUP,
            rusqlite::params![
                next_label,
                cluster_id,
                run_id,
                state.group_kind.as_str(),
                next_label,
            ],
        )?;
        transaction.commit()?;
        return Ok(DeduplicateFinalizationWork::Progressed);
    };
    let representative_media_id: i64 = connection.query_row(
        queries::deduplicate::SELECT_CURRENT_INDEX_BY_MEDIA_ID,
        [state.group_label_cursor],
        |row| row.get(0),
    )?;
    let representative = load_vector(connection, representative_media_id)?;
    let members = fetch_all(
        connection,
        queries::deduplicate::SELECT_COMPONENT_MEMBER_PAGE,
        &[
            &run_id,
            &state.group_kind.as_str(),
            &state.group_label_cursor,
            &state.group_member_cursor,
            &(FINALIZATION_PAGE_SIZE as i64),
        ],
        map_similarity_vector,
    )?;
    let exhausted = members.len() < FINALIZATION_PAGE_SIZE;
    Ok(DeduplicateFinalizationWork::MeasureGroup(
        DeduplicateGroupPage {
            run_id,
            cluster_id,
            kind: state.group_kind,
            representative,
            members,
            exhausted,
        },
    ))
}

fn burst_component_duplicates_near(
    connection: &Connection,
    run_id: i64,
    burst_label: i64,
) -> AppResult<bool> {
    let near_label = connection
        .query_row(
            queries::deduplicate::SELECT_NEAR_COMPONENT_LABEL,
            rusqlite::params![run_id, burst_label],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let Some(near_label) = near_label else {
        return Ok(false);
    };
    let burst_count: i64 = connection.query_row(
        queries::deduplicate::SELECT_COMPONENT_COUNT,
        rusqlite::params![run_id, "burst", burst_label],
        |row| row.get(0),
    )?;
    let near_count: i64 = connection.query_row(
        queries::deduplicate::SELECT_COMPONENT_COUNT,
        rusqlite::params![run_id, "near_duplicate", near_label],
        |row| row.get(0),
    )?;
    if burst_count != near_count {
        return Ok(false);
    }
    let outside_count: i64 = connection.query_row(
        queries::deduplicate::COUNT_COMPONENT_MEMBERS_OUTSIDE_NEAR_LABEL,
        rusqlite::params![run_id, burst_label, near_label],
        |row| row.get(0),
    )?;
    Ok(outside_count == 0)
}

fn commit_group_page(
    connection: &Connection,
    run_id: i64,
    cluster_id: i64,
    kind: ClusterKind,
    member_cursor: i64,
    exhausted: bool,
    metrics: Vec<DeduplicateMemberMetric>,
) -> AppResult<()> {
    let state = load_finalization_state(connection, run_id)?
        .ok_or_else(|| AppError::Conflict("deduplicate finalization changed".to_string()))?;
    if state.phase != "grouping"
        || state.group_cluster_id != Some(cluster_id)
        || state.group_kind != kind
    {
        return Err(AppError::Conflict(
            "deduplicate grouping generation changed".to_string(),
        ));
    }
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    for metric in metrics {
        transaction.execute(
            queries::deduplicate::INSERT_GENERATION_MEMBER,
            rusqlite::params![
                cluster_id,
                metric.media_id,
                metric.cosine_similarity,
                metric.perceptual_hash_distance,
            ],
        )?;
    }
    if exhausted {
        transaction.execute(
            queries::deduplicate::FINISH_GROUP,
            rusqlite::params![state.group_label_cursor, run_id, cluster_id, kind.as_str(),],
        )?;
        transaction.execute(
            queries::deduplicate::UPDATE_RUN_PROGRESS,
            rusqlite::params![0_i64, 0_i64, 0_i64, 1_i64, run_id],
        )?;
    } else {
        transaction.execute(
            queries::deduplicate::ADVANCE_GROUP_MEMBER_PAGE,
            rusqlite::params![member_cursor, run_id, cluster_id, kind.as_str()],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

fn publish_generation(
    connection: &Connection,
    run_id: i64,
    state: &FinalizationState,
) -> AppResult<DeduplicateFinalizationWork> {
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    let locked = transaction.execute(queries::deduplicate::LOCK_RUN_FOR_REPLACEMENT, [run_id])?;
    if locked == 0 {
        transaction.rollback()?;
        return Ok(DeduplicateFinalizationWork::Progressed);
    }
    transaction.execute(
        queries::deduplicate::RETIRE_ACTIVE_GENERATION,
        [state.generation_id],
    )?;
    transaction.execute(
        queries::deduplicate::ACTIVATE_GENERATION,
        [state.generation_id],
    )?;
    transaction.execute(
        queries::deduplicate::SWITCH_ACTIVE_GENERATION,
        [state.generation_id],
    )?;
    transaction.execute(
        queries::deduplicate::COMPLETE_RUN,
        rusqlite::params!["completed", state.completion_error, run_id],
    )?;
    transaction.execute(
        queries::deduplicate::UPDATE_RUN_PROGRESS,
        rusqlite::params![0_i64, 0_i64, 0_i64, 0_i64, run_id],
    )?;
    transaction.execute(queries::deduplicate::ENTER_FINALIZATION_CLEANUP, [run_id])?;
    transaction.commit()?;
    Ok(DeduplicateFinalizationWork::Progressed)
}

fn cancel_finalization(connection: &Connection, run_id: i64) -> AppResult<()> {
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    transaction.execute(queries::deduplicate::CANCEL_UNSUBMITTED_JOBS, [run_id])?;
    transaction.execute(queries::deduplicate::CANCEL_BUILDING_GENERATION, [run_id])?;
    transaction.execute(queries::deduplicate::CANCEL_FINALIZATION, [run_id])?;
    transaction.execute(queries::deduplicate::MARK_RUN_CANCELLED, [run_id])?;
    transaction.commit()?;
    Ok(())
}

fn cleanup_finalization_page(connection: &Connection) -> AppResult<bool> {
    let run_id = connection
        .query_row(
            queries::deduplicate::SELECT_FINALIZATION_CLEANUP,
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let Some(run_id) = run_id else {
        return Ok(false);
    };
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    let deleted_edges = transaction.execute(
        queries::deduplicate::DELETE_FINALIZATION_EDGES_PAGE,
        rusqlite::params![run_id, FINALIZATION_PAGE_SIZE as i64],
    )?;
    if deleted_edges == 0 {
        let deleted_labels = transaction.execute(
            queries::deduplicate::DELETE_FINALIZATION_LABELS_PAGE,
            rusqlite::params![run_id, FINALIZATION_PAGE_SIZE as i64],
        )?;
        if deleted_labels == 0 {
            transaction.execute(
                queries::deduplicate::CLEAR_FINALIZATION_DIRTY_PAGE,
                rusqlite::params![run_id, FINALIZATION_PAGE_SIZE as i64],
            )?;
            transaction.execute(
                queries::deduplicate::DELETE_FINALIZATION_DIRTY_PAGE,
                rusqlite::params![run_id, FINALIZATION_PAGE_SIZE as i64],
            )?;
            transaction.execute(
                queries::deduplicate::DELETE_FINALIZATION,
                rusqlite::params![run_id, run_id, run_id, run_id],
            )?;
        }
    }
    transaction.commit()?;
    Ok(true)
}

fn cleanup_retired_generation_page(connection: &Connection) -> AppResult<bool> {
    let generation_id = connection
        .query_row(queries::deduplicate::SELECT_RETIRED_GENERATION, [], |row| {
            row.get::<_, i64>(0)
        })
        .optional()?;
    let Some(generation_id) = generation_id else {
        return Ok(false);
    };
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    let deleted_members = transaction.execute(
        queries::deduplicate::DELETE_RETIRED_MEMBERS_PAGE,
        rusqlite::params![generation_id, FINALIZATION_PAGE_SIZE as i64],
    )?;
    if deleted_members == 0 {
        let deleted_clusters = transaction.execute(
            queries::deduplicate::DELETE_RETIRED_CLUSTERS_PAGE,
            rusqlite::params![generation_id, FINALIZATION_PAGE_SIZE as i64],
        )?;
        if deleted_clusters == 0 {
            transaction.execute(
                queries::deduplicate::DELETE_RETIRED_GENERATION,
                [generation_id],
            )?;
        }
    }
    transaction.commit()?;
    Ok(true)
}

pub fn log_schedule_start(scheduled_for: &str) {
    info!(
        "Starting scheduled deduplicate scan for {} at {}",
        scheduled_for,
        Utc::now().to_rfc3339()
    );
}
