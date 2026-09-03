use std::collections::{HashMap, HashSet};

use rusqlite::{
    params, params_from_iter, Connection, OptionalExtension, Transaction, TransactionBehavior,
};

use super::queries;
use crate::io::file::{
    NormalizedStoragePath, PathClaimMode, PathClaimScope, StorageRootId, LLM_RESULT_INBOX_DIRECTORY,
};
use crate::io::journal::{
    FileEntryAction, FileEntryPlan, FileOperationPlan, FilePathClaimPlan, PrepareJournalOutcome,
};
use crate::models::{
    map_media_response, map_media_response_with_content_hash, AlbumDetailResponse, AlbumResponse,
    BackupUploadResponse, Cluster, DeduplicateGroup, DeduplicateGroupsResponse,
    FaceGroupMediaResponse, FaceGroupResponse, FaceGroupsListResponse, MapClustersResponse,
    MapMediaListResponse, MediaResponse, ShareLinkResponse, TimelineDirection, TrashMediaResponse,
};
use crate::processor::face_detection::FaceRepresentativeCandidate;

const MAX_AUTH_ATTEMPT_BUCKETS: i64 = 65_536;
const MAX_USER_LIST_ROWS: usize = 4096;
const MAX_USER_LIST_BYTES: usize = 1024 * 1024;
const MAX_API_QUERY_ROWS: usize = 4096;
const MAX_API_QUERY_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetMetadataOutcome {
    Reset { media_count: i64 },
    PathConflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetMetadataStepOutcome {
    Progressed,
    Reset { media_count: i64 },
    PathConflict,
    Idle,
}

const METADATA_RESET_PAGE_SIZE: i64 = 256;

pub fn reset_metadata_page(
    connection: &mut Connection,
    cleanup_group_id: Option<&str>,
) -> rusqlite::Result<ResetMetadataStepOutcome> {
    let existing = connection
        .query_row(queries::metadata_jobs::SELECT_RESET_STATE, [], |_| Ok(()))
        .optional()?;
    if existing.is_none() {
        let Some(cleanup_group_id) = cleanup_group_id else {
            return Ok(ResetMetadataStepOutcome::Idle);
        };
        let plan = metadata_reset_cleanup_plan(cleanup_group_id)?;
        let prepare = crate::io::journal::prepare_file_operation_with(connection, plan, |tx| {
            tx.execute(queries::metadata_jobs::CANCEL_LLM_JOBS_FOR_RESET, [])?;
            tx.execute(
                queries::metadata_jobs::DISCARD_LLM_RESULT_RECEIPTS_FOR_RESET,
                [],
            )?;
            tx.execute(
                queries::metadata_jobs::INSERT_RESET_STATE,
                [cleanup_group_id],
            )?;
            Ok(())
        })?;
        if prepare == PrepareJournalOutcome::PathConflict {
            return Ok(ResetMetadataStepOutcome::PathConflict);
        }
        return Ok(ResetMetadataStepOutcome::Progressed);
    }

    advance_metadata_reset_page(connection)
}

fn metadata_reset_cleanup_plan(cleanup_group_id: &str) -> rusqlite::Result<FileOperationPlan> {
    let media_path =
        NormalizedStoragePath::parse("media").map_err(|_| rusqlite::Error::InvalidQuery)?;
    let faces_path =
        NormalizedStoragePath::parse("faces").map_err(|_| rusqlite::Error::InvalidQuery)?;
    let ai_path = NormalizedStoragePath::parse("ai").map_err(|_| rusqlite::Error::InvalidQuery)?;
    let preview_media_path =
        NormalizedStoragePath::parse("media").map_err(|_| rusqlite::Error::InvalidQuery)?;
    let result_inbox_path = NormalizedStoragePath::parse(LLM_RESULT_INBOX_DIRECTORY)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let cleanup_paths = [
        (StorageRootId::Thumbnails, media_path.clone(), "thumbnails"),
        (
            StorageRootId::TinyThumbnails,
            media_path.clone(),
            "tiny_thumbnails",
        ),
        (
            StorageRootId::PlaceThumbnails,
            media_path,
            "place_thumbnails",
        ),
        (StorageRootId::Previews, faces_path, "face_crops"),
        (StorageRootId::Previews, ai_path, "ai_inputs"),
        (
            StorageRootId::Previews,
            preview_media_path,
            "media_previews",
        ),
        (
            StorageRootId::Journal,
            result_inbox_path,
            "llm_result_inbox",
        ),
    ];
    Ok(FileOperationPlan {
        group_id: cleanup_group_id.to_string(),
        kind: "metadata_reset".to_string(),
        owner_kind: "metadata".to_string(),
        owner_id: "all".to_string(),
        claim_token: None,
        product_target: None,
        product_version: None,
        entries: cleanup_paths
            .iter()
            .map(|(storage_root, path, _)| FileEntryPlan {
                action: FileEntryAction::Cleanup,
                storage_root: *storage_root,
                source_path: Some(path.clone()),
                temporary_path: None,
                destination_path: None,
                tombstone_path: None,
                expected_size: None,
                expected_sha256: None,
                expected_version: None,
            })
            .collect(),
        claims: cleanup_paths
            .into_iter()
            .map(|(storage_root, path, role)| FilePathClaimPlan {
                storage_root,
                path,
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Subtree,
                role: role.to_string(),
                expected_version: None,
            })
            .collect(),
        space_reservation: None,
    })
}

fn advance_metadata_reset_page(
    connection: &mut Connection,
) -> rusqlite::Result<ResetMetadataStepOutcome> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (cleanup_group_id, phase, media_cursor, media_count) =
        transaction.query_row(queries::metadata_jobs::SELECT_RESET_STATE, [], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;

    if phase == "metadata_jobs" || phase == "queue_imported" || phase == "dirty_imported" {
        let media_ids = {
            let mut statement =
                transaction.prepare(queries::metadata_jobs::SELECT_IMPORTED_PAGE)?;
            let media_ids = statement
                .query_map(params![media_cursor, METADATA_RESET_PAGE_SIZE], |row| {
                    row.get::<_, i64>(0)
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            media_ids
        };
        if let Some(last_media_id) = media_ids.last().copied() {
            for media_id in media_ids {
                if phase == "dirty_imported" {
                    transaction.execute(queries::metadata_jobs::MARK_MEDIA_DIRTY, [media_id])?;
                } else {
                    transaction.execute(queries::metadata_jobs::RESET_JOB_FOR_MEDIA, [media_id])?;
                }
            }
            transaction.execute(
                queries::metadata_jobs::UPDATE_RESET_CURSOR,
                params![last_media_id, phase],
            )?;
        } else {
            advance_metadata_reset_phase(&transaction, &phase)?;
        }
        transaction.commit()?;
        return Ok(ResetMetadataStepOutcome::Progressed);
    }

    if phase == "llm_result_groups" {
        let group_ids = {
            let mut statement =
                transaction.prepare(queries::metadata_jobs::SELECT_LLM_RESULT_GROUPS_PAGE)?;
            let group_ids = statement
                .query_map([METADATA_RESET_PAGE_SIZE], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            group_ids
        };
        if group_ids.is_empty() {
            advance_metadata_reset_phase(&transaction, &phase)?;
        } else {
            for group_id in group_ids {
                transaction.execute(
                    queries::metadata_jobs::RETIRE_LLM_RESULT_GROUP_ENTRIES,
                    [&group_id],
                )?;
                transaction.execute(
                    queries::metadata_jobs::RELEASE_LLM_RESULT_GROUP_CLAIMS,
                    [&group_id],
                )?;
                transaction.execute(
                    queries::metadata_jobs::RELEASE_LLM_RESULT_GROUP_RESERVATIONS,
                    [&group_id],
                )?;
                transaction
                    .execute(queries::metadata_jobs::RETIRE_LLM_RESULT_GROUP, [&group_id])?;
            }
        }
        transaction.commit()?;
        return Ok(ResetMetadataStepOutcome::Progressed);
    }

    if phase == "activate_cleanup" {
        let activated = transaction.execute(
            queries::file_operations::ACTIVATE_METADATA_RESET_CLEANUP,
            [&cleanup_group_id],
        )?;
        if activated != 1 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let deleted = transaction.execute(queries::metadata_jobs::DELETE_RESET_STATE, [])?;
        if deleted != 1 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        transaction.commit()?;
        return Ok(ResetMetadataStepOutcome::Reset { media_count });
    }

    let delete_query = metadata_reset_delete_query(&phase)?;
    let changed = transaction.execute(delete_query, [METADATA_RESET_PAGE_SIZE])?;
    if changed == 0 {
        advance_metadata_reset_phase(&transaction, &phase)?;
    }
    transaction.commit()?;
    Ok(ResetMetadataStepOutcome::Progressed)
}

fn advance_metadata_reset_phase(
    transaction: &Transaction<'_>,
    current_phase: &str,
) -> rusqlite::Result<()> {
    let next_phase = metadata_reset_next_phase(current_phase)?;
    let changed = transaction.execute(
        queries::metadata_jobs::ADVANCE_RESET_PHASE,
        params![next_phase, current_phase],
    )?;
    if changed != 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(())
}

fn metadata_reset_next_phase(current: &str) -> rusqlite::Result<&'static str> {
    match current {
        "metadata_jobs" => Ok("llm_result_groups"),
        "llm_result_groups" => Ok("llm_result_staging"),
        "llm_result_staging" => Ok("llm_result_receipts"),
        "llm_result_receipts" => Ok("llm_reservations"),
        "llm_reservations" => Ok("llm_job_cancellations"),
        "llm_job_cancellations" => Ok("llm_cancellation_scopes"),
        "llm_cancellation_scopes" => Ok("llm_jobs"),
        "llm_jobs" => Ok("text_inputs"),
        "text_inputs" => Ok("text"),
        "text" => Ok("aesthetic_inputs"),
        "aesthetic_inputs" => Ok("aesthetics"),
        "aesthetics" => Ok("screenshot_inputs"),
        "screenshot_inputs" => Ok("screenshots"),
        "screenshots" => Ok("document_inputs"),
        "document_inputs" => Ok("documents"),
        "documents" => Ok("face_finalization_faces"),
        "face_finalization_faces" => Ok("face_finalization_anchors"),
        "face_finalization_anchors" => Ok("face_finalization_groups"),
        "face_finalization_groups" => Ok("face_representatives"),
        "face_representatives" => Ok("face_members"),
        "face_members" => Ok("face_groups"),
        "face_groups" => Ok("face_finalizations"),
        "face_finalizations" => Ok("face_generation_state"),
        "face_generation_state" => Ok("face_manual_state"),
        "face_manual_state" => Ok("face_generations"),
        "face_generations" => Ok("face_runs"),
        "face_runs" => Ok("face_results"),
        "face_results" => Ok("media_faces"),
        "media_faces" => Ok("similarity_cluster_members"),
        "similarity_cluster_members" => Ok("similarity_clusters"),
        "similarity_clusters" => Ok("similarity_dirty_snapshot"),
        "similarity_dirty_snapshot" => Ok("similarity_edges"),
        "similarity_edges" => Ok("similarity_labels"),
        "similarity_labels" => Ok("similarity_finalizations"),
        "similarity_finalizations" => Ok("similarity_generation_state"),
        "similarity_generation_state" => Ok("similarity_generations"),
        "similarity_generations" => Ok("similarity_bands"),
        "similarity_bands" => Ok("similarity_index"),
        "similarity_index" => Ok("similarity_dirty"),
        "similarity_dirty" => Ok("similarity_runs"),
        "similarity_runs" => Ok("ai_inputs"),
        "ai_inputs" => Ok("rtree"),
        "rtree" => Ok("metadata_sources"),
        "metadata_sources" => Ok("metadata"),
        "metadata" => Ok("queue_imported"),
        "queue_imported" => Ok("dirty_imported"),
        "dirty_imported" => Ok("activate_cleanup"),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn metadata_reset_delete_query(phase: &str) -> rusqlite::Result<&'static str> {
    let query = match phase {
        "llm_result_staging" => queries::metadata_jobs::DELETE_LLM_RESULT_STAGING_PAGE,
        "llm_result_receipts" => queries::metadata_jobs::DELETE_LLM_RESULT_RECEIPTS_PAGE,
        "llm_reservations" => queries::metadata_jobs::RELEASE_LLM_RESERVATIONS_PAGE,
        "llm_job_cancellations" => queries::metadata_jobs::DELETE_LLM_JOB_CANCELLATIONS_PAGE,
        "llm_cancellation_scopes" => queries::metadata_jobs::DELETE_LLM_CANCELLATION_SCOPES_PAGE,
        "llm_jobs" => queries::metadata_jobs::DELETE_LLM_JOBS_PAGE,
        "text_inputs" => queries::metadata_jobs::DELETE_TEXT_INPUTS_PAGE,
        "text" => queries::metadata_jobs::DELETE_TEXT_PAGE,
        "aesthetic_inputs" => queries::metadata_jobs::DELETE_AESTHETIC_INPUTS_PAGE,
        "aesthetics" => queries::metadata_jobs::DELETE_AESTHETICS_PAGE,
        "screenshot_inputs" => queries::metadata_jobs::DELETE_SCREENSHOT_INPUTS_PAGE,
        "screenshots" => queries::metadata_jobs::DELETE_SCREENSHOTS_PAGE,
        "document_inputs" => queries::metadata_jobs::DELETE_DOCUMENT_INPUTS_PAGE,
        "documents" => queries::metadata_jobs::DELETE_DOCUMENTS_PAGE,
        "face_finalization_faces" => queries::metadata_jobs::DELETE_FACE_FINALIZATION_FACES_PAGE,
        "face_finalization_anchors" => {
            queries::metadata_jobs::DELETE_FACE_FINALIZATION_ANCHORS_PAGE
        }
        "face_finalization_groups" => queries::metadata_jobs::DELETE_FACE_FINALIZATION_GROUPS_PAGE,
        "face_representatives" => queries::metadata_jobs::DELETE_FACE_REPRESENTATIVES_PAGE,
        "face_members" => queries::metadata_jobs::DELETE_FACE_MEMBERS_PAGE,
        "face_groups" => queries::metadata_jobs::DELETE_FACE_GROUPS_PAGE,
        "face_finalizations" => queries::metadata_jobs::DELETE_FACE_FINALIZATIONS_PAGE,
        "face_generation_state" => queries::metadata_jobs::DELETE_FACE_GENERATION_STATE_PAGE,
        "face_manual_state" => queries::metadata_jobs::DELETE_FACE_MANUAL_STATE_PAGE,
        "face_generations" => queries::metadata_jobs::DELETE_FACE_GENERATIONS_PAGE,
        "face_runs" => queries::metadata_jobs::DELETE_FACE_RUNS_PAGE,
        "face_results" => queries::metadata_jobs::DELETE_FACE_RESULTS_PAGE,
        "media_faces" => queries::metadata_jobs::DELETE_MEDIA_FACES_PAGE,
        "similarity_cluster_members" => {
            queries::metadata_jobs::DELETE_SIMILARITY_CLUSTER_MEMBERS_PAGE
        }
        "similarity_clusters" => queries::metadata_jobs::DELETE_SIMILARITY_CLUSTERS_PAGE,
        "similarity_dirty_snapshot" => {
            queries::metadata_jobs::DELETE_SIMILARITY_DIRTY_SNAPSHOT_PAGE
        }
        "similarity_edges" => queries::metadata_jobs::DELETE_SIMILARITY_EDGES_PAGE,
        "similarity_labels" => queries::metadata_jobs::DELETE_SIMILARITY_LABELS_PAGE,
        "similarity_finalizations" => queries::metadata_jobs::DELETE_SIMILARITY_FINALIZATIONS_PAGE,
        "similarity_generation_state" => {
            queries::metadata_jobs::DELETE_SIMILARITY_GENERATION_STATE_PAGE
        }
        "similarity_generations" => queries::metadata_jobs::DELETE_SIMILARITY_GENERATIONS_PAGE,
        "similarity_bands" => queries::metadata_jobs::DELETE_SIMILARITY_BANDS_PAGE,
        "similarity_index" => queries::metadata_jobs::DELETE_SIMILARITY_INDEX_PAGE,
        "similarity_dirty" => queries::metadata_jobs::DELETE_SIMILARITY_DIRTY_PAGE,
        "similarity_runs" => queries::metadata_jobs::DELETE_SIMILARITY_RUNS_PAGE,
        "ai_inputs" => queries::metadata_jobs::DELETE_AI_INPUTS_PAGE,
        "rtree" => queries::metadata_jobs::DELETE_RTREE_PAGE,
        "metadata_sources" => queries::metadata_jobs::DELETE_METADATA_SOURCES_PAGE,
        "metadata" => queries::metadata_jobs::DELETE_METADATA_PAGE,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    Ok(query)
}

#[derive(Debug)]
pub struct CancelBackupUpload {
    pub user_id: i64,
    pub upload_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancelBackupUploadOutcome {
    Cancelled(BackupUploadResponse),
    AlreadyCancelled(BackupUploadResponse),
    NotFound,
    Writing,
    NotCancellable,
    Changed,
    PathConflict,
}

#[derive(Debug)]
pub struct RegisterBackupDevice {
    pub user_id: i64,
    pub device_id: String,
    pub device_name: String,
}

#[derive(Debug)]
pub struct CreateBackupUpload {
    pub user_id: i64,
    pub upload_id: String,
    pub device_id: String,
    pub client_asset_id: String,
    pub operation_id: String,
    pub original_filename: String,
    pub mime_type: String,
    pub expected_size: i64,
    pub source_modified_at: String,
    pub staged_path: String,
    pub protocol_version: u32,
    pub content_hash: String,
    pub metadata_json: String,
    pub session_expiry_hours: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateBackupUploadOutcome {
    Created(BackupUploadResponse),
    Existing(BackupUploadResponse),
    DeviceNotFound,
    ContractConflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeduplicateScheduleState {
    pub latest_run_status: Option<String>,
    pub last_scheduled_for: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct FaceRepresentativeCandidatePageQuery {
    pub group_id: i64,
    pub after_face_id: i64,
    pub limit: u16,
}

#[derive(Debug)]
pub struct FaceRepresentativeCandidatePage {
    pub candidates: Vec<FaceRepresentativeCandidate>,
    pub exhausted: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct UpdateFaceRepresentative {
    pub group_id: i64,
    pub representative_face_id: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
pub struct FaceRepresentativeGroupPageQuery {
    pub after_group_id: i64,
    pub limit: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceRepresentativeGroupPage {
    pub group_ids: Vec<i64>,
}

#[derive(Debug)]
pub struct InvalidateWebdavReadiness {
    pub user_id: i64,
    pub paths: Vec<String>,
}

#[derive(Debug)]
pub struct MarkWebdavReady {
    pub user_id: i64,
    pub path: String,
}

#[derive(Debug)]
pub struct LoadBackupUpload {
    pub user_id: i64,
    pub upload_id: String,
}

#[derive(Debug)]
pub struct PrepareBackupCompletion {
    pub user_id: i64,
    pub upload_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepareBackupCompletionOutcome {
    AlreadyQueued(BackupUploadResponse),
    Ready {
        asset_id: i64,
        staged_path: String,
        expected_content_hash: String,
    },
    NotFound,
    NotReady,
    MissingManifest,
}

#[derive(Debug)]
pub struct QueueBackupCompletion {
    pub user_id: i64,
    pub upload_id: String,
    pub asset_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueBackupCompletionOutcome {
    Queued(BackupUploadResponse),
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupResumableFile {
    pub asset_id: i64,
    pub staged_path: String,
    pub uploaded_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupProcessingAsset {
    pub asset_id: i64,
    pub user_id: i64,
    pub staged_path: String,
    pub content_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupRecoveryPage<T> {
    pub rows: Vec<T>,
    pub next_after_id: Option<i64>,
}

#[derive(Debug)]
pub struct BackupRecoveryPageQuery {
    pub after_id: i64,
    pub limit: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedBackupAsset {
    pub asset_id: i64,
    pub user_id: i64,
    pub staged_path: String,
    pub source_modified_at: String,
    pub expected_content_hash: String,
    pub metadata_json: String,
}

#[derive(Debug)]
pub struct StoreBackupContentHash {
    pub asset_id: i64,
    pub content_hash: String,
}

#[derive(Debug)]
pub enum BackupProcessingTransition {
    Complete { asset_id: i64, media_id: i64 },
    Requeue { asset_id: i64 },
    Fail { asset_id: i64, error: String },
    FailMissingStaging { asset_id: i64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupProcessingTransitionOutcome {
    Transitioned { cleanup_group: bool },
    Unchanged,
    PathConflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackupSessionMaintenance {
    pub next_expiration_seconds: Option<u64>,
}

#[derive(Debug)]
pub struct ClaimBackupChunk {
    pub user_id: i64,
    pub upload_id: String,
    pub start: u64,
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimBackupChunkOutcome {
    Accepted { staged_path: String },
    Rejected,
    NotFound,
}

#[derive(Debug)]
pub struct FinishBackupChunk {
    pub user_id: i64,
    pub upload_id: String,
    pub start: u64,
    pub next_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinishBackupChunkOutcome {
    Completed(BackupUploadResponse),
    Changed,
}

#[derive(Debug)]
pub struct AbandonBackupChunk {
    pub user_id: i64,
    pub upload_id: String,
}

struct BackupUploadCancellationRow {
    asset_id: i64,
    response: BackupUploadResponse,
    session_status: String,
    staged_path: String,
}

#[derive(Debug, Clone, Copy)]
pub struct SpatialBounds {
    pub north: f64,
    pub south: f64,
    pub east: f64,
    pub west: f64,
}

#[derive(Debug)]
pub struct MapClustersQuery {
    pub user_id: i64,
    pub bounds: SpatialBounds,
    pub precision: usize,
}

#[derive(Debug)]
pub struct MapMediaQuery {
    pub user_id: i64,
    pub bounds: SpatialBounds,
    pub geohash_prefixes: Vec<String>,
}

#[derive(Debug)]
pub struct DuplicateGroupsQuery {
    pub user_id: i64,
    pub cursor: i64,
    pub limit: i64,
}

#[derive(Debug)]
pub struct PlaceIdentityQuery {
    pub user_id: i64,
    pub city: String,
    pub state: Option<String>,
    pub country: String,
}

#[derive(Debug)]
pub struct PlacePageQuery {
    pub user_id: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug)]
pub struct PlaceMediaQuery {
    pub identity: PlaceIdentityQuery,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceRecord {
    pub city: String,
    pub state: Option<String>,
    pub country: String,
    pub media_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceCoverRecord {
    pub thumbnail_path: Option<String>,
}

#[derive(Debug)]
pub struct PlaceMediaPage {
    pub place: PlaceRecord,
    pub media: Vec<MediaResponse>,
    pub has_more: bool,
}

#[derive(Debug)]
pub struct CreateAlbum {
    pub user_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub media_ids: Vec<i64>,
}

#[derive(Debug)]
pub struct UpdateAlbum {
    pub user_id: i64,
    pub album_id: i64,
    pub name: Option<String>,
    pub description: Option<String>,
    pub cover_media_id: Option<i64>,
}

#[derive(Debug)]
pub struct AlbumMediaMutation {
    pub user_id: i64,
    pub album_id: i64,
    pub media_ids: Vec<i64>,
}

#[derive(Debug)]
pub struct UserAlbum {
    pub user_id: i64,
    pub album_id: i64,
}

#[derive(Debug)]
pub enum AlbumDetailOutcome {
    NotFound,
    Found(AlbumDetailResponse),
}

#[derive(Debug)]
pub enum AlbumUpdateOutcome {
    NotFound,
    Updated(AlbumResponse),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlbumMutationOutcome {
    NotFound,
    InvalidPermutation,
    Completed,
}

#[derive(Debug)]
pub struct CreateShareLink {
    pub user_id: i64,
    pub media_id: Option<i64>,
    pub album_id: Option<i64>,
    pub token: String,
    pub password_hash: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Debug)]
pub enum CreateShareLinkOutcome {
    MediaNotFound,
    AlbumNotFound,
    Created(ShareLinkResponse),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteShareLinkOutcome {
    NotFound,
    Deleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareTargetKind {
    Media,
    Album,
}

#[derive(Debug)]
pub struct GrantShareAccess {
    pub owner_user_id: i64,
    pub target_user_id: i64,
    pub target_id: i64,
    pub access_level: i32,
    pub kind: ShareTargetKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantShareAccessOutcome {
    TargetUserNotFound,
    TargetNotFound,
    InsufficientPermission,
    Granted,
}

#[derive(Debug, Clone)]
pub struct ActiveShareRecord {
    pub id: i64,
    pub media_id: Option<i64>,
    pub album_id: Option<i64>,
    pub password_hash: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Debug)]
pub enum PublicShareContent {
    Media(Box<MediaResponse>),
    Album {
        id: i64,
        name: String,
        description: Option<String>,
        media: Vec<MediaResponse>,
    },
    NotFound,
    Invalid,
}

#[derive(Debug)]
pub struct PublicSharedMediaQuery {
    pub share: ActiveShareRecord,
    pub media_id: i64,
}

#[derive(Debug)]
pub enum PublicFileAccessOutcome {
    NotInShare,
    NotFound,
    Found(PublicFileRecord),
}

#[derive(Debug)]
pub struct PublicFileRecord {
    pub file_path: String,
    pub mime_type: Option<String>,
    pub original_filename: String,
}

#[derive(Debug)]
pub enum PublicThumbnailAccessOutcome {
    NotInShare,
    NotFound,
    Unavailable,
    Found(String),
}

#[derive(Debug)]
pub struct MediaBatchQuery {
    pub user_id: i64,
    pub media_ids: Vec<i64>,
}

#[derive(Debug)]
pub struct TimelinePageQuery {
    pub user_id: i64,
    pub cursor: Option<String>,
    pub search: String,
    pub media_type: Option<String>,
    pub classification: Option<String>,
    pub direction: TimelineDirection,
    pub anchor_date: Option<String>,
    pub limit: u32,
}

#[derive(Debug)]
pub struct TimelinePageRecord {
    pub rows: TimelineRows,
    pub has_more: bool,
    pub has_newer_candidate: bool,
}

pub type TimelineRows = Vec<(MediaResponse, Option<String>)>;

#[derive(Debug)]
pub struct TimelineMarkersQuery {
    pub user_id: i64,
    pub search: String,
    pub media_type: Option<String>,
    pub classification: Option<String>,
}

#[derive(Debug)]
pub struct TimelineMarkerRecord {
    pub label: String,
    pub anchor_date: String,
}

#[derive(Debug)]
pub struct MoveMediaToTrash {
    pub user_id: i64,
    pub media_ids: Vec<i64>,
    pub deleted_at: String,
}

#[derive(Debug)]
pub struct RestoreTrash {
    pub user_id: i64,
    pub media_ids: Vec<i64>,
}

#[derive(Debug)]
pub struct DeleteTrashMedia {
    pub user_id: i64,
    pub media_ids: Vec<i64>,
}

#[derive(Debug)]
pub struct DeleteTrashPage {
    pub user_id: i64,
    pub limit: u16,
}

#[derive(Debug)]
pub struct DeleteExpiredTrashPage {
    pub cutoff: String,
    pub limit: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrashDeletionOutcome {
    Deleted {
        affected_count: usize,
        cleanup_groups: usize,
        has_more: bool,
    },
    PathConflict,
}

struct TrashDeletionRow {
    media_id: i64,
    user_id: i64,
    file_path: String,
    thumbnail_path: Option<String>,
}

#[derive(Debug)]
pub struct FaceGroupsPageQuery {
    pub user_id: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug)]
pub struct FaceGroupQuery {
    pub user_id: i64,
    pub face_group_id: i64,
}

#[derive(Debug)]
pub struct MetadataJobStatus {
    pub counts: Vec<(String, i64)>,
    pub errors: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct MetadataJobClaim {
    pub media_id: i64,
    pub claim_token: String,
}

#[derive(Debug)]
pub struct FinishMetadataJob {
    pub media_id: i64,
    pub claim_token: String,
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct MetadataGenerationMedia {
    pub file_path: String,
    pub media_type: String,
    pub content_hash: Option<String>,
    pub original_filename: String,
    pub mime_type: Option<String>,
    pub artifact_version: i64,
    pub thumbnail_path: Option<String>,
    pub preview_path: Option<String>,
}

#[derive(Debug)]
pub struct MetadataSourceWrite {
    pub source_type: String,
    pub payload_json: String,
}

#[derive(Debug)]
pub struct MetadataValuesWrite {
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub date_taken: Option<String>,
    pub gps_latitude: Option<f64>,
    pub gps_longitude: Option<f64>,
    pub gps_altitude: Option<f64>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens_make: Option<String>,
    pub lens_model: Option<String>,
    pub iso: Option<i32>,
    pub exposure_time: Option<String>,
    pub f_number: Option<f64>,
    pub focal_length: Option<f64>,
    pub focal_length_35mm: Option<f64>,
    pub location_city: Option<String>,
    pub location_state: Option<String>,
    pub location_country: Option<String>,
    pub video_codec: Option<String>,
    pub keywords: Option<String>,
    pub duration_seconds: Option<f64>,
}

#[derive(Debug)]
pub struct MetadataAiInputWrite {
    pub task: String,
    pub sequence: i64,
    pub input_kind: String,
    pub storage_root: String,
    pub file_path: String,
    pub filename: String,
    pub mime_type: String,
    pub byte_size: i64,
    pub content_hash: String,
    pub frame_timestamp_ms: Option<i64>,
}

#[derive(Debug)]
pub struct PersistMetadataGeneration {
    pub media_id: i64,
    pub claim_token: String,
    pub metadata: MetadataValuesWrite,
    pub sources: Vec<MetadataSourceWrite>,
    pub thumbnail_path: String,
    pub preview_path: Option<String>,
    pub artifact_version: i64,
    pub artifact_group_id: String,
    pub artifact_group_version: i64,
    pub content_hash: String,
    pub geohash: Option<String>,
    pub ai_inputs: Vec<MetadataAiInputWrite>,
}

#[derive(Debug)]
pub struct LlmSubmissionJob {
    pub job_id: String,
    pub media_id: i64,
    pub task: String,
    pub attempts: i64,
}

#[derive(Debug)]
pub struct LlmPreparedInput {
    pub sequence: i64,
    pub storage_root: String,
    pub file_path: String,
    pub filename: String,
    pub mime_type: String,
    pub byte_size: i64,
    pub content_hash: String,
    pub input_kind: String,
    pub frame_timestamp_ms: Option<i64>,
}

#[derive(Debug)]
pub struct LlmCancellationBatch {
    pub scope: String,
    pub task: String,
    pub job_ids: Vec<String>,
}

#[derive(Debug)]
pub enum FinishLlmSubmission {
    Submitted {
        job_id: String,
        attempt: i64,
    },
    Deferred {
        job_id: String,
        retry_after_seconds: i64,
    },
    Retry {
        job_id: String,
        error: String,
    },
    Failed {
        job_id: String,
        error: String,
    },
    RequeueAmbiguous {
        job_id: String,
    },
}

#[derive(Debug)]
pub struct AcknowledgeLlmCancellation {
    pub scope: String,
    pub task: String,
    pub job_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmResultReceiptOutcome {
    Received,
    Ignored,
    CorrelationFailed,
    Changed,
}

#[derive(Debug)]
pub struct PrepareLlmResultReceipt {
    pub job_id: String,
    pub media_id: i64,
    pub task: String,
    pub attempt: u32,
}

#[derive(Debug)]
pub enum LlmResultReceiptPreparation {
    Ready {
        job_version: i64,
        inputs: Vec<LlmPreparedInput>,
    },
    Ignored,
    CorrelationFailed,
}

#[derive(Debug)]
pub struct CreateLlmResultReceipt {
    pub job_id: String,
    pub attempt: u32,
    pub expected_job_version: i64,
    pub media_id: i64,
    pub task: String,
    pub result_status: String,
    pub model_type: Option<String>,
    pub model_version: Option<String>,
    pub encoding: String,
    pub record_count: u32,
    pub byte_size: u64,
    pub content_hash: String,
    pub journal_group_id: String,
    pub inbox_path: String,
    pub receive_token: String,
    pub journal_plan: FileOperationPlan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateLlmResultReceiptOutcome {
    Created,
    Deferred,
    Changed,
    PathConflict,
}

#[derive(Debug)]
pub struct CommitLlmResultReceipt {
    pub job_id: String,
    pub attempt: u32,
    pub expected_job_version: i64,
    pub journal_group_id: String,
    pub expected_group_version: i64,
}

#[derive(Debug)]
pub struct StageLlmResultPage {
    pub job_id: String,
    pub attempt: u32,
    pub claim_token: String,
    pub expected_record_sequence: u32,
    pub expected_byte_offset: u64,
    pub records: Vec<StagedLlmResultRecord>,
}

#[derive(Debug)]
pub struct StagedLlmResultRecord {
    pub record_sequence: u32,
    pub input_sequence: Option<u32>,
    pub kind: String,
    pub byte_offset: u64,
    pub encoded_size: u32,
    pub normalized_payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageLlmResultPageOutcome {
    Staged,
    Changed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CleanupLlmResultStagingOutcome {
    pub deleted: usize,
    pub complete: bool,
}

#[derive(Debug)]
pub struct RejectLlmResultReceipt {
    pub job_id: String,
    pub attempt: u32,
    pub expected_job_version: Option<i64>,
    pub error: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmResultReceiptRejection {
    Failed,
    Discarded,
}

#[derive(Debug)]
pub struct MetadataAiInputVerification {
    pub media_type: String,
    pub inputs: Vec<MetadataAiInputPath>,
}

#[derive(Debug)]
pub struct MetadataAiInputPath {
    pub task: String,
    pub storage_root: String,
    pub file_path: String,
}

#[derive(Debug)]
pub struct BinaryMediaQuery {
    pub user_id: i64,
    pub media_id: i64,
    pub deleted: bool,
}

#[derive(Debug)]
pub struct BinaryMediaRecord {
    pub file_path: String,
    pub mime_type: Option<String>,
    pub original_filename: String,
    pub media_type: String,
    pub thumbnail_path: Option<String>,
    pub preview_path: Option<String>,
}

#[derive(Debug)]
pub struct PrepareMediaUpdate {
    pub user_id: i64,
    pub media_id: i64,
}

#[derive(Debug)]
pub struct EditableMediaState {
    pub gps_latitude: Option<f64>,
    pub gps_longitude: Option<f64>,
}

#[derive(Debug)]
pub struct FinalizeMediaUpdate {
    pub user_id: i64,
    pub media_id: i64,
    pub date_taken: Option<String>,
    pub gps_latitude: Option<f64>,
    pub gps_longitude: Option<f64>,
    pub effective_gps_latitude: Option<f64>,
    pub effective_gps_longitude: Option<f64>,
    pub update_editable_metadata: bool,
    pub update_location: bool,
    pub geohash: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub country: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UserRecord {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub role: String,
    pub must_change_password: bool,
    pub is_active: bool,
    pub created_at: String,
}

#[derive(Debug)]
pub(crate) struct CreateUser {
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CreateUserOutcome {
    Duplicate,
    Created(UserRecord),
}

#[derive(Debug)]
pub(crate) struct UpdateUser {
    pub administrator_id: i64,
    pub user_id: i64,
    pub role: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UpdateUserOutcome {
    NotFound,
    CannotDemoteSelf,
    CannotDeactivateSelf,
    CannotDeactivateReservedAdmin,
    Updated(UserRecord),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeleteUserOutcome {
    NotFound,
    CannotDeleteReservedAdmin,
    Deleted,
}

pub(crate) fn load_map_clusters(
    connection: &Connection,
    request: MapClustersQuery,
) -> rusqlite::Result<MapClustersResponse> {
    let longitude_clause = longitude_clause(request.bounds);
    let query = queries::map::build_clusters_query(request.precision, longitude_clause);
    let mut statement = connection.prepare(&query)?;
    let mut rows = statement.query(params![
        request.user_id,
        request.bounds.south,
        request.bounds.north,
        request.bounds.west,
        request.bounds.east,
    ])?;
    let mut clusters = Vec::new();
    let mut mapped_bytes = 0usize;
    while let Some(row) = rows.next()? {
        if clusters.len() == MAX_API_QUERY_ROWS {
            return Err(bounded_output_error("map clusters exceed 4096 rows"));
        }
        let cluster = Cluster {
            id: row.get(0)?,
            count: row.get(1)?,
            lat: row.get(2)?,
            lng: row.get(3)?,
            representative_id: row.get(4)?,
        };
        mapped_bytes = mapped_bytes
            .checked_add(cluster.id.len())
            .and_then(|bytes| bytes.checked_add(size_of::<Cluster>()))
            .ok_or_else(|| bounded_output_error("map cluster output size overflow"))?;
        if mapped_bytes > MAX_API_QUERY_BYTES {
            return Err(bounded_output_error("map clusters exceed one mebibyte"));
        }
        clusters.push(cluster);
    }
    let total_count = clusters.iter().map(|cluster| cluster.count).sum();
    Ok(MapClustersResponse {
        clusters,
        total_count,
    })
}

pub(crate) fn load_map_media(
    connection: &Connection,
    request: MapMediaQuery,
) -> rusqlite::Result<MapMediaListResponse> {
    let longitude_clause = longitude_clause(request.bounds);
    let query = queries::map::build_media_query(request.geohash_prefixes.len(), longitude_clause);
    let mut values = Vec::with_capacity(5 + request.geohash_prefixes.len());
    values.push(rusqlite::types::Value::Integer(request.user_id));
    values.push(rusqlite::types::Value::Real(request.bounds.south));
    values.push(rusqlite::types::Value::Real(request.bounds.north));
    values.push(rusqlite::types::Value::Real(request.bounds.west));
    values.push(rusqlite::types::Value::Real(request.bounds.east));
    values.extend(
        request
            .geohash_prefixes
            .into_iter()
            .map(|prefix| rusqlite::types::Value::Text(format!("{prefix}%"))),
    );
    let mut statement = connection.prepare(&query)?;
    let mut rows = statement.query(params_from_iter(values))?;
    let mut items = Vec::new();
    let mut mapped_bytes = 0usize;
    while let Some(row) = rows.next()? {
        if items.len() == MAX_API_QUERY_ROWS {
            return Err(bounded_output_error("map media exceed 4096 rows"));
        }
        let item = map_media_response_with_content_hash(row)?;
        mapped_bytes = add_media_response_bytes(mapped_bytes, &item)?;
        items.push(item);
    }
    Ok(MapMediaListResponse { items })
}

pub(crate) fn load_duplicate_groups(
    connection: &Connection,
    request: DuplicateGroupsQuery,
) -> rusqlite::Result<DeduplicateGroupsResponse> {
    let mut page_statement =
        connection.prepare(queries::deduplicate::SELECT_VISIBLE_CLUSTER_PAGE)?;
    let mut page_cursor =
        page_statement.query(params![request.user_id, request.cursor, request.limit + 1])?;
    let mut page_rows = Vec::new();
    while let Some(row) = page_cursor.next()? {
        page_rows.push((
            row.get::<_, Option<i64>>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        ));
    }
    let (total_groups, total_media) = page_rows
        .first()
        .map(|row| (row.1, row.2))
        .unwrap_or((0, 0));
    let cluster_ids = page_rows
        .into_iter()
        .filter_map(|row| row.0)
        .collect::<Vec<_>>();
    let has_more = cluster_ids.len() > request.limit as usize;
    let selected_ids = cluster_ids
        .into_iter()
        .take(request.limit as usize)
        .collect::<Vec<_>>();
    let mut items_by_cluster = HashMap::new();
    let mut mapped_rows = 0usize;
    let mut mapped_bytes = 0usize;
    if !selected_ids.is_empty() {
        let query = queries::deduplicate::build_visible_cluster_media_query(selected_ids.len());
        let mut values = selected_ids
            .iter()
            .copied()
            .map(rusqlite::types::Value::Integer)
            .collect::<Vec<_>>();
        values.push(rusqlite::types::Value::Integer(request.user_id));
        let mut statement = connection.prepare(&query)?;
        let mut rows = statement.query(params_from_iter(values))?;
        while let Some(row) = rows.next()? {
            if mapped_rows == MAX_API_QUERY_ROWS {
                return Err(bounded_output_error(
                    "duplicate-group media exceed 4096 rows",
                ));
            }
            let cluster_id = row.get::<_, i64>(28)?;
            let media = map_media_response(row)?;
            mapped_bytes = add_media_response_bytes(mapped_bytes, &media)?;
            mapped_rows += 1;
            items_by_cluster
                .entry(cluster_id)
                .or_insert_with(Vec::new)
                .push(media);
        }
    }
    let groups = selected_ids
        .into_iter()
        .filter_map(|cluster_id| {
            let items = items_by_cluster.remove(&cluster_id)?;
            (items.len() >= 2).then_some(DeduplicateGroup { cluster_id, items })
        })
        .collect::<Vec<_>>();
    let next_cursor = if has_more {
        groups.last().map(|group| group.cluster_id.to_string())
    } else {
        None
    };
    Ok(DeduplicateGroupsResponse {
        groups,
        next_cursor,
        has_more,
        total_groups,
        total_media,
    })
}

pub(crate) fn load_place_cover(
    connection: &Connection,
    request: PlaceIdentityQuery,
) -> rusqlite::Result<Option<PlaceCoverRecord>> {
    connection
        .query_row(
            &queries::places::select_cover_query(),
            params![
                request.user_id,
                request.city,
                request.state,
                request.country
            ],
            |row| {
                Ok(PlaceCoverRecord {
                    thumbnail_path: row.get(0)?,
                })
            },
        )
        .optional()
}

pub(crate) fn load_places_page(
    connection: &Connection,
    request: PlacePageQuery,
) -> rusqlite::Result<Vec<PlaceRecord>> {
    let mut statement = connection.prepare(&queries::places::select_page_query())?;
    let mut rows = statement.query(params![request.user_id, request.limit + 1, request.offset])?;
    let mut places = Vec::new();
    let mut mapped_bytes = 0usize;
    while let Some(row) = rows.next()? {
        let place = map_place_record(row)?;
        mapped_bytes = add_place_record_bytes(mapped_bytes, &place)?;
        places.push(place);
    }
    Ok(places)
}

pub(crate) fn load_place_media_page(
    connection: &Connection,
    request: PlaceMediaQuery,
) -> rusqlite::Result<Option<PlaceMediaPage>> {
    let identity = request.identity;
    let place = connection
        .query_row(
            &queries::places::select_summary_query(),
            params![
                identity.user_id,
                identity.city,
                identity.state,
                identity.country
            ],
            map_place_record,
        )
        .optional()?;
    let Some(place) = place else {
        return Ok(None);
    };
    let mut statement = connection.prepare(queries::places::SELECT_MEDIA_PAGE)?;
    let mut rows = statement.query(params![
        identity.user_id,
        place.city,
        place.state,
        place.country,
        request.limit + 1,
        request.offset
    ])?;
    let mut media = Vec::new();
    let mut mapped_bytes = add_place_record_bytes(0, &place)?;
    while let Some(row) = rows.next()? {
        let item = map_media_response(row)?;
        mapped_bytes = add_media_response_bytes(mapped_bytes, &item)?;
        media.push(item);
    }
    let has_more = media.len() > request.limit as usize;
    media.truncate(request.limit as usize);
    Ok(Some(PlaceMediaPage {
        place,
        media,
        has_more,
    }))
}

fn map_place_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<PlaceRecord> {
    Ok(PlaceRecord {
        city: row.get(0)?,
        state: row.get(1)?,
        country: row.get(2)?,
        media_count: row.get(3)?,
    })
}

fn add_place_record_bytes(current: usize, place: &PlaceRecord) -> rusqlite::Result<usize> {
    let mapped = [
        Some(place.city.as_str()),
        place.state.as_deref(),
        Some(place.country.as_str()),
    ]
    .into_iter()
    .flatten()
    .try_fold(
        current
            .checked_add(size_of::<PlaceRecord>())
            .ok_or_else(|| bounded_output_error("place output size overflow"))?,
        |bytes, value| {
            bytes
                .checked_add(value.len())
                .ok_or_else(|| bounded_output_error("place output size overflow"))
        },
    )?;
    if mapped > MAX_API_QUERY_BYTES {
        return Err(bounded_output_error("place output exceeds one mebibyte"));
    }
    Ok(mapped)
}

pub(crate) fn create_album(
    connection: &mut Connection,
    request: CreateAlbum,
) -> rusqlite::Result<AlbumDetailResponse> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute(
        queries::albums::INSERT,
        params![request.user_id, request.name, request.description],
    )?;
    let album_id = transaction.last_insert_rowid();
    transaction.execute(
        queries::access::INSERT_ALBUM_ACCESS,
        params![album_id, request.user_id, 2],
    )?;
    insert_accessible_album_media(&transaction, request.user_id, album_id, &request.media_ids)?;
    let album = load_album_detail_record(&transaction, album_id)?
        .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    transaction.commit()?;
    Ok(album)
}

pub(crate) fn list_albums(
    connection: &Connection,
    user_id: i64,
) -> rusqlite::Result<Vec<AlbumResponse>> {
    let query = queries::albums::select_all_for_user_query();
    let mut statement = connection.prepare(&query)?;
    let mut rows = statement.query([user_id])?;
    let mut albums = Vec::new();
    let mut mapped_bytes = 0usize;
    while let Some(row) = rows.next()? {
        if albums.len() == MAX_API_QUERY_ROWS {
            return Err(bounded_output_error("album list exceeds 4096 rows"));
        }
        let album = map_album_response(row)?;
        mapped_bytes = add_album_response_bytes(mapped_bytes, &album)?;
        albums.push(album);
    }
    Ok(albums)
}

pub(crate) fn load_album(
    connection: &Connection,
    request: UserAlbum,
) -> rusqlite::Result<AlbumDetailOutcome> {
    if !owns_album(connection, request.album_id, request.user_id)? {
        return Ok(AlbumDetailOutcome::NotFound);
    }
    Ok(
        match load_album_detail_record(connection, request.album_id)? {
            Some(album) => AlbumDetailOutcome::Found(album),
            None => AlbumDetailOutcome::NotFound,
        },
    )
}

pub(crate) fn update_album(
    connection: &mut Connection,
    request: UpdateAlbum,
) -> rusqlite::Result<AlbumUpdateOutcome> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if !owns_album(&transaction, request.album_id, request.user_id)? {
        transaction.rollback()?;
        return Ok(AlbumUpdateOutcome::NotFound);
    }
    transaction.execute(
        queries::albums::UPDATE,
        params![
            request.name,
            request.description,
            request.cover_media_id,
            request.album_id
        ],
    )?;
    let query = queries::albums::select_with_count_query();
    let album = transaction
        .query_row(&query, [request.album_id], map_album_response)
        .optional()?
        .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    transaction.commit()?;
    Ok(AlbumUpdateOutcome::Updated(album))
}

pub(crate) fn delete_album_access(
    connection: &mut Connection,
    request: UserAlbum,
) -> rusqlite::Result<AlbumMutationOutcome> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if !owns_album(&transaction, request.album_id, request.user_id)? {
        transaction.rollback()?;
        return Ok(AlbumMutationOutcome::NotFound);
    }
    transaction.execute(
        queries::albums::DELETE_ACCESS,
        params![request.album_id, request.user_id],
    )?;
    transaction.commit()?;
    Ok(AlbumMutationOutcome::Completed)
}

pub(crate) fn add_album_media(
    connection: &mut Connection,
    request: AlbumMediaMutation,
) -> rusqlite::Result<AlbumMutationOutcome> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if !owns_album(&transaction, request.album_id, request.user_id)? {
        transaction.rollback()?;
        return Ok(AlbumMutationOutcome::NotFound);
    }
    insert_accessible_album_media(
        &transaction,
        request.user_id,
        request.album_id,
        &request.media_ids,
    )?;
    transaction.commit()?;
    Ok(AlbumMutationOutcome::Completed)
}

pub(crate) fn remove_album_media(
    connection: &mut Connection,
    request: AlbumMediaMutation,
) -> rusqlite::Result<AlbumMutationOutcome> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if !owns_album(&transaction, request.album_id, request.user_id)? {
        transaction.rollback()?;
        return Ok(AlbumMutationOutcome::NotFound);
    }
    for media_id in request.media_ids {
        transaction.execute(
            queries::albums::REMOVE_MEDIA,
            params![request.album_id, media_id],
        )?;
    }
    transaction.commit()?;
    Ok(AlbumMutationOutcome::Completed)
}

pub(crate) fn reorder_album_media(
    connection: &mut Connection,
    request: AlbumMediaMutation,
) -> rusqlite::Result<AlbumMutationOutcome> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if !owns_album(&transaction, request.album_id, request.user_id)? {
        transaction.rollback()?;
        return Ok(AlbumMutationOutcome::NotFound);
    }
    let current_ids = transaction
        .prepare(queries::albums::SELECT_MEDIA_IDS)?
        .query_map([request.album_id], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let requested_ids = request.media_ids.iter().copied().collect::<HashSet<_>>();
    let current_id_set = current_ids.iter().copied().collect::<HashSet<_>>();
    if request.media_ids.len() != current_ids.len()
        || requested_ids.len() != request.media_ids.len()
        || requested_ids != current_id_set
    {
        transaction.rollback()?;
        return Ok(AlbumMutationOutcome::InvalidPermutation);
    }
    for (position, media_id) in request.media_ids.into_iter().enumerate() {
        transaction.execute(
            queries::albums::UPDATE_POSITION,
            params![position as i64, request.album_id, media_id],
        )?;
    }
    transaction.commit()?;
    Ok(AlbumMutationOutcome::Completed)
}

fn owns_album(connection: &Connection, album_id: i64, user_id: i64) -> rusqlite::Result<bool> {
    Ok(connection
        .query_row(
            queries::albums::CHECK_OWNERSHIP,
            [album_id, user_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .is_some())
}

fn insert_accessible_album_media(
    connection: &Connection,
    user_id: i64,
    album_id: i64,
    media_ids: &[i64],
) -> rusqlite::Result<()> {
    if media_ids.is_empty() {
        return Ok(());
    }
    let query = queries::albums::build_add_media_batch_query(media_ids.len());
    let mut parameters = Vec::with_capacity(media_ids.len() * 2 + 3);
    for (position, media_id) in media_ids.iter().enumerate() {
        parameters.push(rusqlite::types::Value::Integer(*media_id));
        parameters.push(rusqlite::types::Value::Integer(position as i64));
    }
    parameters.push(rusqlite::types::Value::Integer(user_id));
    parameters.push(rusqlite::types::Value::Integer(album_id));
    parameters.push(rusqlite::types::Value::Integer(album_id));
    connection.execute(&query, params_from_iter(parameters))?;
    Ok(())
}

fn load_album_detail_record(
    connection: &Connection,
    album_id: i64,
) -> rusqlite::Result<Option<AlbumDetailResponse>> {
    let album = connection
        .query_row(queries::albums::SELECT_BY_ID, [album_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, String>(5)?,
            ))
        })
        .optional()?;
    let Some((id, name, description, cover_media_id, created_at)) = album else {
        return Ok(None);
    };
    let mut statement = connection.prepare(queries::albums::SELECT_MEDIA)?;
    let mut rows = statement.query([album_id])?;
    let mut media = Vec::new();
    let mut mapped_bytes = name
        .len()
        .checked_add(description.as_deref().map_or(0, str::len))
        .and_then(|bytes| bytes.checked_add(created_at.len()))
        .ok_or_else(|| bounded_output_error("album detail output size overflow"))?;
    while let Some(row) = rows.next()? {
        if media.len() == MAX_API_QUERY_ROWS {
            return Err(bounded_output_error("album media exceed 4096 rows"));
        }
        let item = map_media_response(row)?;
        mapped_bytes = add_media_response_bytes(mapped_bytes, &item)?;
        media.push(item);
    }
    Ok(Some(AlbumDetailResponse {
        id,
        name,
        description,
        cover_media_id,
        media,
        created_at,
    }))
}

fn map_album_response(row: &rusqlite::Row<'_>) -> rusqlite::Result<AlbumResponse> {
    let thumbnail_media_ids = (6..10)
        .filter_map(|column| row.get::<_, Option<i64>>(column).transpose())
        .collect::<Result<Vec<_>, _>>()?;
    Ok(AlbumResponse {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        cover_media_id: row.get(3)?,
        thumbnail_media_ids,
        media_count: row.get(4)?,
        created_at: row.get(5)?,
    })
}

fn add_album_response_bytes(current: usize, album: &AlbumResponse) -> rusqlite::Result<usize> {
    let mapped = current
        .checked_add(size_of::<AlbumResponse>())
        .and_then(|bytes| bytes.checked_add(album.name.len()))
        .and_then(|bytes| bytes.checked_add(album.description.as_deref().map_or(0, str::len)))
        .and_then(|bytes| bytes.checked_add(album.created_at.len()))
        .and_then(|bytes| bytes.checked_add(album.thumbnail_media_ids.len() * size_of::<i64>()))
        .ok_or_else(|| bounded_output_error("album output size overflow"))?;
    if mapped > MAX_API_QUERY_BYTES {
        return Err(bounded_output_error("album output exceeds one mebibyte"));
    }
    Ok(mapped)
}

pub(crate) fn create_share_link(
    connection: &mut Connection,
    request: CreateShareLink,
) -> rusqlite::Result<CreateShareLinkOutcome> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if let Some(media_id) = request.media_id {
        let exists = transaction
            .query_row(
                queries::share::CHECK_MEDIA_OWNERSHIP,
                params![media_id, request.user_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            transaction.rollback()?;
            return Ok(CreateShareLinkOutcome::MediaNotFound);
        }
    }
    if let Some(album_id) = request.album_id {
        let exists = transaction
            .query_row(
                queries::share::CHECK_ALBUM_OWNERSHIP,
                params![album_id, request.user_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            transaction.rollback()?;
            return Ok(CreateShareLinkOutcome::AlbumNotFound);
        }
    }
    transaction.execute(
        queries::share::INSERT,
        params![
            request.user_id,
            request.media_id,
            request.album_id,
            request.token,
            request.password_hash,
            request.expires_at
        ],
    )?;
    let share_id = transaction.last_insert_rowid();
    let share = transaction
        .query_row(queries::share::SELECT_BY_ID, [share_id], map_share_link)
        .optional()?
        .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    transaction.commit()?;
    Ok(CreateShareLinkOutcome::Created(share))
}

pub(crate) fn list_share_links(
    connection: &Connection,
    user_id: i64,
) -> rusqlite::Result<Vec<ShareLinkResponse>> {
    let mut statement = connection.prepare(queries::share::SELECT_ALL_FOR_USER)?;
    let mut rows = statement.query([user_id])?;
    let mut shares = Vec::new();
    let mut mapped_bytes = 0usize;
    while let Some(row) = rows.next()? {
        if shares.len() == MAX_API_QUERY_ROWS {
            return Err(bounded_output_error("share list exceeds 4096 rows"));
        }
        let share = map_share_link(row)?;
        mapped_bytes = mapped_bytes
            .checked_add(size_of::<ShareLinkResponse>())
            .and_then(|bytes| bytes.checked_add(share.token.len()))
            .and_then(|bytes| bytes.checked_add(share.expires_at.as_deref().map_or(0, str::len)))
            .and_then(|bytes| bytes.checked_add(share.created_at.len()))
            .ok_or_else(|| bounded_output_error("share list output size overflow"))?;
        if mapped_bytes > MAX_API_QUERY_BYTES {
            return Err(bounded_output_error("share list exceeds one mebibyte"));
        }
        shares.push(share);
    }
    Ok(shares)
}

pub(crate) fn delete_share_link(
    connection: &mut Connection,
    user_id: i64,
    share_id: i64,
) -> rusqlite::Result<DeleteShareLinkOutcome> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let exists = transaction
        .query_row(
            queries::share::CHECK_OWNERSHIP,
            params![share_id, user_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !exists {
        transaction.rollback()?;
        return Ok(DeleteShareLinkOutcome::NotFound);
    }
    transaction.execute(queries::share::DELETE, [share_id])?;
    transaction.commit()?;
    Ok(DeleteShareLinkOutcome::Deleted)
}

pub(crate) fn grant_share_access(
    connection: &mut Connection,
    request: GrantShareAccess,
) -> rusqlite::Result<GrantShareAccessOutcome> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let target_active = transaction
        .query_row(
            queries::users::SELECT_BY_ID,
            [request.target_user_id],
            |row| row.get::<_, bool>(5),
        )
        .optional()?
        == Some(true);
    if !target_active {
        transaction.rollback()?;
        return Ok(GrantShareAccessOutcome::TargetUserNotFound);
    }
    match request.kind {
        ShareTargetKind::Media => {
            let owner_access = transaction
                .query_row(
                    queries::access::CHECK_MEDIA_ACCESS,
                    params![request.target_id, request.owner_user_id],
                    |row| row.get::<_, i32>(0),
                )
                .optional()?;
            let Some(owner_access) = owner_access else {
                transaction.rollback()?;
                return Ok(GrantShareAccessOutcome::TargetNotFound);
            };
            if owner_access < 2 {
                transaction.rollback()?;
                return Ok(GrantShareAccessOutcome::InsufficientPermission);
            }
            transaction.execute(
                queries::access::UPSERT_SHARED_MEDIA_ACCESS,
                params![
                    request.target_id,
                    request.target_user_id,
                    request.access_level
                ],
            )?;
        }
        ShareTargetKind::Album => {
            let owns = transaction
                .query_row(
                    queries::albums::CHECK_OWNERSHIP,
                    params![request.target_id, request.owner_user_id],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if !owns {
                transaction.rollback()?;
                return Ok(GrantShareAccessOutcome::TargetNotFound);
            }
            transaction.execute(
                queries::access::UPSERT_SHARED_ALBUM_ACCESS,
                params![
                    request.target_id,
                    request.target_user_id,
                    request.access_level
                ],
            )?;
        }
    }
    transaction.commit()?;
    Ok(GrantShareAccessOutcome::Granted)
}

fn map_share_link(row: &rusqlite::Row<'_>) -> rusqlite::Result<ShareLinkResponse> {
    let password_hash: Option<String> = row.get(4)?;
    Ok(ShareLinkResponse {
        id: row.get(0)?,
        token: row.get(1)?,
        media_id: row.get(2)?,
        album_id: row.get(3)?,
        has_password: password_hash.is_some(),
        expires_at: row.get(5)?,
        view_count: row.get(6)?,
        created_at: row.get(7)?,
    })
}

pub(crate) fn load_active_share(
    connection: &Connection,
    token: String,
) -> rusqlite::Result<Option<ActiveShareRecord>> {
    connection
        .query_row(queries::share::SELECT_BY_TOKEN, [token], |row| {
            Ok(ActiveShareRecord {
                id: row.get(0)?,
                media_id: row.get(1)?,
                album_id: row.get(2)?,
                password_hash: row.get(3)?,
                expires_at: row.get(4)?,
            })
        })
        .optional()
}

pub(crate) fn load_public_share_content(
    connection: &mut Connection,
    share: ActiveShareRecord,
) -> rusqlite::Result<PublicShareContent> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute(queries::share::INCREMENT_VIEW_COUNT, [share.id])?;
    let content = if let Some(media_id) = share.media_id {
        match transaction
            .query_row(queries::media::SELECT_BY_ID, [media_id], map_media_response)
            .optional()?
        {
            Some(media) => PublicShareContent::Media(Box::new(media)),
            None => PublicShareContent::NotFound,
        }
    } else if let Some(album_id) = share.album_id {
        let album = transaction
            .query_row(queries::public::SELECT_ALBUM_BASIC, [album_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .optional()?;
        match album {
            Some((id, name, description)) => {
                let mut statement = transaction.prepare(queries::public::SELECT_ALBUM_MEDIA)?;
                let mut rows = statement.query([album_id])?;
                let mut media = Vec::new();
                let mut mapped_bytes = name
                    .len()
                    .checked_add(description.as_deref().map_or(0, str::len))
                    .ok_or_else(|| bounded_output_error("public album output size overflow"))?;
                while let Some(row) = rows.next()? {
                    if media.len() == MAX_API_QUERY_ROWS {
                        return Err(bounded_output_error("public album exceeds 4096 media rows"));
                    }
                    let item = map_media_response(row)?;
                    mapped_bytes = add_media_response_bytes(mapped_bytes, &item)?;
                    media.push(item);
                }
                PublicShareContent::Album {
                    id,
                    name,
                    description,
                    media,
                }
            }
            None => PublicShareContent::NotFound,
        }
    } else {
        PublicShareContent::Invalid
    };
    transaction.commit()?;
    Ok(content)
}

pub(crate) fn load_public_shared_file(
    connection: &Connection,
    request: PublicSharedMediaQuery,
) -> rusqlite::Result<PublicFileAccessOutcome> {
    if !media_belongs_to_share(connection, &request.share, request.media_id)? {
        return Ok(PublicFileAccessOutcome::NotInShare);
    }
    Ok(connection
        .query_row(
            queries::public::SELECT_MEDIA_FILE_INFO,
            [request.media_id],
            |row| {
                Ok(PublicFileRecord {
                    file_path: row.get(0)?,
                    mime_type: row.get(1)?,
                    original_filename: row.get(2)?,
                })
            },
        )
        .optional()?
        .map_or(
            PublicFileAccessOutcome::NotFound,
            PublicFileAccessOutcome::Found,
        ))
}

pub(crate) fn load_public_shared_thumbnail(
    connection: &Connection,
    request: PublicSharedMediaQuery,
) -> rusqlite::Result<PublicThumbnailAccessOutcome> {
    if !media_belongs_to_share(connection, &request.share, request.media_id)? {
        return Ok(PublicThumbnailAccessOutcome::NotInShare);
    }
    let thumbnail = connection
        .query_row(
            queries::public::SELECT_MEDIA_THUMBNAIL,
            [request.media_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?;
    Ok(match thumbnail {
        None => PublicThumbnailAccessOutcome::NotFound,
        Some(None) => PublicThumbnailAccessOutcome::Unavailable,
        Some(Some(path)) => PublicThumbnailAccessOutcome::Found(path),
    })
}

fn media_belongs_to_share(
    connection: &Connection,
    share: &ActiveShareRecord,
    media_id: i64,
) -> rusqlite::Result<bool> {
    if let Some(shared_media_id) = share.media_id {
        return Ok(shared_media_id == media_id);
    }
    let Some(album_id) = share.album_id else {
        return Ok(false);
    };
    Ok(connection
        .query_row(
            queries::public::CHECK_ALBUM_MEDIA,
            params![album_id, media_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

pub(crate) fn load_media_batch(
    connection: &Connection,
    request: MediaBatchQuery,
) -> rusqlite::Result<Vec<MediaResponse>> {
    if request.media_ids.is_empty() {
        return Ok(Vec::new());
    }
    let query = queries::media::build_select_by_ids(request.media_ids.len());
    let mut values = Vec::with_capacity(request.media_ids.len() + 1);
    values.push(rusqlite::types::Value::Integer(request.user_id));
    values.extend(
        request
            .media_ids
            .iter()
            .copied()
            .map(rusqlite::types::Value::Integer),
    );
    let mut statement = connection.prepare(&query)?;
    let mut rows = statement.query(params_from_iter(values))?;
    let mut by_id = HashMap::new();
    let mut mapped_bytes = 0usize;
    while let Some(row) = rows.next()? {
        let media = map_media_response(row)?;
        mapped_bytes = add_media_response_bytes(mapped_bytes, &media)?;
        by_id.insert(media.id, media);
    }
    Ok(request
        .media_ids
        .iter()
        .filter_map(|media_id| by_id.get(media_id).cloned())
        .collect())
}

pub(crate) fn load_timeline_page(
    connection: &Connection,
    request: TimelinePageQuery,
) -> rusqlite::Result<TimelinePageRecord> {
    let media_type = request.media_type.as_deref().unwrap_or("");
    let classification = request.classification.as_deref().unwrap_or("");
    let filter = TimelineFilter {
        user_id: request.user_id,
        search: &request.search,
        media_type,
        classification,
        start_date: "0000-01-01T00:00:00",
        end_date: "9999-12-31T23:59:59",
    };
    let (rows, has_more) = load_timeline_window(
        connection,
        filter,
        request.direction,
        request.cursor.as_deref(),
        request.anchor_date.as_deref(),
        request.limit,
    )?;
    if rows.is_empty() {
        return Ok(TimelinePageRecord {
            rows: Vec::new(),
            has_more: false,
            has_newer_candidate: false,
        });
    }
    let has_newer_candidate =
        if request.direction == TimelineDirection::Older && request.cursor.is_none() {
            let first_cursor = rows.first().and_then(|(media, date)| {
                date.as_ref().map(|date| format!("{}_{}", date, media.id))
            });
            if let Some(cursor) = first_cursor.as_deref() {
                !load_timeline_candidate(
                    connection,
                    filter,
                    TimelineDirection::Newer,
                    Some(cursor),
                    None,
                )?
                .is_empty()
            } else {
                false
            }
        } else {
            false
        };
    Ok(TimelinePageRecord {
        rows,
        has_more,
        has_newer_candidate,
    })
}

pub(crate) fn load_timeline_markers(
    connection: &Connection,
    request: TimelineMarkersQuery,
) -> rusqlite::Result<Vec<TimelineMarkerRecord>> {
    let media_type = request.media_type.as_deref().unwrap_or("");
    let classification = request.classification.as_deref().unwrap_or("");
    let mut statement = connection.prepare(queries::timeline::SELECT_MONTH_MARKERS)?;
    let mut rows = statement.query(params![
        request.user_id,
        request.search,
        request.search,
        media_type,
        media_type,
        classification,
        classification,
        classification,
    ])?;
    let mut markers = Vec::new();
    let mut mapped_bytes = 0usize;
    while let Some(row) = rows.next()? {
        if markers.len() == MAX_API_QUERY_ROWS {
            return Err(bounded_output_error("timeline markers exceed 4096 rows"));
        }
        let marker = TimelineMarkerRecord {
            label: row.get(0)?,
            anchor_date: row.get(1)?,
        };
        mapped_bytes = mapped_bytes
            .checked_add(size_of::<TimelineMarkerRecord>())
            .and_then(|bytes| bytes.checked_add(marker.label.len()))
            .and_then(|bytes| bytes.checked_add(marker.anchor_date.len()))
            .ok_or_else(|| bounded_output_error("timeline marker output size overflow"))?;
        if mapped_bytes > MAX_API_QUERY_BYTES {
            return Err(bounded_output_error("timeline markers exceed one mebibyte"));
        }
        markers.push(marker);
    }
    Ok(markers)
}

#[derive(Clone, Copy)]
struct TimelineFilter<'a> {
    user_id: i64,
    search: &'a str,
    media_type: &'a str,
    classification: &'a str,
    start_date: &'a str,
    end_date: &'a str,
}

fn load_timeline_candidate(
    connection: &Connection,
    filter: TimelineFilter<'_>,
    direction: TimelineDirection,
    cursor: Option<&str>,
    anchor_date: Option<&str>,
) -> rusqlite::Result<TimelineRows> {
    let query_params = [
        &filter.user_id as &dyn rusqlite::ToSql,
        &filter.start_date,
        &filter.end_date,
        &filter.search,
        &filter.search,
        &filter.media_type,
        &filter.media_type,
        &filter.classification,
        &filter.classification,
        &filter.classification,
    ];
    if let Some(cursor) = cursor {
        let Some((cursor_date, cursor_id)) = parse_timeline_cursor(cursor) else {
            return Ok(Vec::new());
        };
        let query = if direction == TimelineDirection::Older {
            queries::timeline::SELECT_PAGINATED_WINDOW
        } else {
            queries::timeline::SELECT_PAGINATED_WINDOW_ASC
        };
        return query_timeline_rows(
            connection,
            query,
            &[
                query_params[0],
                query_params[1],
                query_params[2],
                query_params[3],
                query_params[4],
                query_params[5],
                query_params[6],
                query_params[7],
                query_params[8],
                query_params[9],
                &cursor_date,
                &cursor_date,
                &cursor_id,
                &1_i64,
            ],
        );
    }
    let Some(anchor) = anchor_date else {
        return Err(rusqlite::Error::InvalidParameterName(
            "anchorDate is required".to_string(),
        ));
    };
    query_timeline_rows(
        connection,
        queries::timeline::SELECT_WINDOW,
        &[
            query_params[0],
            query_params[1],
            query_params[2],
            query_params[3],
            query_params[4],
            query_params[5],
            query_params[6],
            query_params[7],
            query_params[8],
            query_params[9],
            &anchor,
            &1_i64,
        ],
    )
}

fn load_timeline_window(
    connection: &Connection,
    filter: TimelineFilter<'_>,
    direction: TimelineDirection,
    cursor: Option<&str>,
    anchor_date: Option<&str>,
    limit: u32,
) -> rusqlite::Result<(TimelineRows, bool)> {
    let max_rows = i64::from(limit) + 1;
    let query_params = [
        &filter.user_id as &dyn rusqlite::ToSql,
        &filter.start_date,
        &filter.end_date,
        &filter.search,
        &filter.search,
        &filter.media_type,
        &filter.media_type,
        &filter.classification,
        &filter.classification,
        &filter.classification,
    ];
    let rows = if direction == TimelineDirection::Older {
        if let Some((cursor_date, cursor_id)) = cursor.and_then(parse_timeline_cursor) {
            let mut parameters = query_params.to_vec();
            parameters.push(&cursor_date);
            parameters.push(&cursor_date);
            parameters.push(&cursor_id);
            parameters.push(&max_rows);
            query_timeline_rows(
                connection,
                queries::timeline::SELECT_PAGINATED_WINDOW,
                &parameters,
            )?
        } else {
            let anchor_date = anchor_date.ok_or_else(|| {
                rusqlite::Error::InvalidParameterName("anchorDate is required".to_string())
            })?;
            let mut parameters = query_params.to_vec();
            parameters.push(&anchor_date);
            parameters.push(&max_rows);
            query_timeline_rows(connection, queries::timeline::SELECT_WINDOW, &parameters)?
        }
    } else {
        let (cursor_date, cursor_id) = cursor.and_then(parse_timeline_cursor).ok_or_else(|| {
            rusqlite::Error::InvalidParameterName(
                "cursor is required when loading newer media".to_string(),
            )
        })?;
        let mut parameters = query_params.to_vec();
        parameters.push(&cursor_date);
        parameters.push(&cursor_date);
        parameters.push(&cursor_id);
        parameters.push(&max_rows);
        query_timeline_rows(
            connection,
            queries::timeline::SELECT_PAGINATED_WINDOW_ASC,
            &parameters,
        )?
    };
    let (mut rows, has_more) = limit_timeline_rows(rows, limit);
    if direction == TimelineDirection::Newer {
        rows.reverse();
    }
    Ok((rows, has_more))
}

fn query_timeline_rows(
    connection: &Connection,
    query: &str,
    parameters: &[&dyn rusqlite::ToSql],
) -> rusqlite::Result<TimelineRows> {
    let mut statement = connection.prepare(query)?;
    let mut rows = statement.query(parameters)?;
    let mut media = Vec::new();
    let mut mapped_bytes = 0usize;
    while let Some(row) = rows.next()? {
        if media.len() > 500 {
            return Err(bounded_output_error("timeline page exceeds 501 rows"));
        }
        let item = map_media_response(row)?;
        mapped_bytes = add_media_response_bytes(mapped_bytes, &item)?;
        let date_taken = item.date_taken.clone();
        media.push((item, date_taken));
    }
    Ok(media)
}

fn limit_timeline_rows(mut rows: TimelineRows, limit: u32) -> (TimelineRows, bool) {
    let has_more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    (rows, has_more)
}

fn parse_timeline_cursor(cursor: &str) -> Option<(String, i64)> {
    let (date, id) = cursor.rsplit_once('_')?;
    Some((date.to_string(), id.parse().ok()?))
}

pub(crate) fn move_media_to_trash(
    connection: &mut Connection,
    request: MoveMediaToTrash,
) -> rusqlite::Result<usize> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut deleted_count = 0usize;
    for media_id in request.media_ids {
        deleted_count = deleted_count
            .checked_add(transaction.execute(
                queries::media::UPDATE_DELETED_AT,
                params![request.deleted_at, media_id, request.user_id],
            )?)
            .ok_or_else(|| bounded_output_error("deleted media count overflow"))?;
    }
    transaction.commit()?;
    Ok(deleted_count)
}

pub(crate) fn load_trash(
    connection: &Connection,
    user_id: i64,
) -> rusqlite::Result<Vec<TrashMediaResponse>> {
    let mut statement = connection.prepare(queries::trash::SELECT_DELETED)?;
    let mut rows = statement.query([user_id])?;
    let mut items = Vec::new();
    let mut mapped_bytes = 0usize;
    while let Some(row) = rows.next()? {
        if items.len() == MAX_API_QUERY_ROWS {
            return Err(bounded_output_error("trash list exceeds 4096 rows"));
        }
        let item = TrashMediaResponse {
            id: row.get(0)?,
            filename: row.get(1)?,
            original_filename: row.get(2)?,
            media_type: row.get(3)?,
            mime_type: row.get(4)?,
            width: row.get(5)?,
            height: row.get(6)?,
            file_size: row.get(7)?,
            duration_seconds: row.get(8)?,
            date_taken: row.get(9)?,
            deleted_at: row.get(10)?,
            created_at: row.get(11)?,
        };
        mapped_bytes = [
            Some(item.filename.as_str()),
            Some(item.original_filename.as_str()),
            Some(item.media_type.as_str()),
            item.mime_type.as_deref(),
            item.date_taken.as_deref(),
            Some(item.deleted_at.as_str()),
            Some(item.created_at.as_str()),
        ]
        .into_iter()
        .flatten()
        .try_fold(
            mapped_bytes
                .checked_add(size_of::<TrashMediaResponse>())
                .ok_or_else(|| bounded_output_error("trash output size overflow"))?,
            |bytes, value| bytes.checked_add(value.len()),
        )
        .ok_or_else(|| bounded_output_error("trash output size overflow"))?;
        if mapped_bytes > MAX_API_QUERY_BYTES {
            return Err(bounded_output_error("trash list exceeds one mebibyte"));
        }
        items.push(item);
    }
    Ok(items)
}

pub(crate) fn restore_trash(
    connection: &Connection,
    request: RestoreTrash,
) -> rusqlite::Result<usize> {
    if request.media_ids.is_empty() {
        return Ok(0);
    }
    let placeholders = std::iter::repeat_n("?", request.media_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let query = queries::trash::RESTORE_MEDIA.replace("{}", &placeholders);
    let mut values = request
        .media_ids
        .into_iter()
        .map(rusqlite::types::Value::Integer)
        .collect::<Vec<_>>();
    values.push(rusqlite::types::Value::Integer(request.user_id));
    connection.execute(&query, params_from_iter(values))
}

pub(crate) fn delete_trash_media(
    connection: &mut Connection,
    request: DeleteTrashMedia,
) -> rusqlite::Result<TrashDeletionOutcome> {
    if request.media_ids.is_empty() {
        return Ok(TrashDeletionOutcome::Deleted {
            affected_count: 0,
            cleanup_groups: 0,
            has_more: false,
        });
    }
    let placeholders = std::iter::repeat_n("?", request.media_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let query = queries::trash::SELECT_FOR_DELETE.replace("{}", &placeholders);
    let mut values = request
        .media_ids
        .into_iter()
        .map(rusqlite::types::Value::Integer)
        .collect::<Vec<_>>();
    values.push(rusqlite::types::Value::Integer(request.user_id));
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let rows = {
        let mut statement = transaction.prepare(&query)?;
        let mapped = statement.query_map(params_from_iter(values), |row| {
            Ok(TrashDeletionRow {
                media_id: row.get(0)?,
                file_path: row.get(1)?,
                thumbnail_path: row.get(2)?,
                user_id: request.user_id,
            })
        })?;
        mapped.collect::<rusqlite::Result<Vec<_>>>()?
    };
    delete_trash_rows(transaction, rows, false)
}

pub(crate) fn delete_trash_page(
    connection: &mut Connection,
    request: DeleteTrashPage,
) -> rusqlite::Result<TrashDeletionOutcome> {
    if request.limit == 0 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let rows = {
        let mut statement = transaction.prepare(queries::trash::SELECT_ALL_DELETED_PAGE)?;
        let mapped =
            statement.query_map(params![request.user_id, i64::from(request.limit)], |row| {
                Ok(TrashDeletionRow {
                    media_id: row.get(0)?,
                    file_path: row.get(1)?,
                    thumbnail_path: row.get(2)?,
                    user_id: request.user_id,
                })
            })?;
        mapped.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let has_more = rows.len() == usize::from(request.limit);
    delete_trash_rows(transaction, rows, has_more)
}

pub(crate) fn delete_expired_trash_page(
    connection: &mut Connection,
    request: DeleteExpiredTrashPage,
) -> rusqlite::Result<TrashDeletionOutcome> {
    if request.limit == 0 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let rows = {
        let mut statement = transaction.prepare(queries::trash::SELECT_OLD_DELETED_PAGE)?;
        let mapped =
            statement.query_map(params![request.cutoff, i64::from(request.limit)], |row| {
                Ok(TrashDeletionRow {
                    media_id: row.get(0)?,
                    file_path: row.get(1)?,
                    thumbnail_path: row.get(2)?,
                    user_id: row.get(3)?,
                })
            })?;
        mapped.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let has_more = rows.len() == usize::from(request.limit);
    delete_trash_rows(transaction, rows, has_more)
}

fn delete_trash_rows(
    transaction: Transaction<'_>,
    rows: Vec<TrashDeletionRow>,
    has_more: bool,
) -> rusqlite::Result<TrashDeletionOutcome> {
    let mut affected_count = 0usize;
    let mut cleanup_groups = 0usize;
    for row in rows {
        let removed_access = transaction.execute(
            queries::trash::DELETE_ACCESS,
            params![row.media_id, row.user_id],
        )?;
        if removed_access == 0 {
            continue;
        }
        affected_count = affected_count
            .checked_add(1)
            .ok_or(rusqlite::Error::InvalidQuery)?;
        let access_count = transaction.query_row(
            queries::trash::CHECK_ACCESS_COUNT,
            [row.media_id],
            |result| result.get::<_, i64>(0),
        )?;
        if access_count > 0 {
            continue;
        }
        let plan = media_cleanup_plan(row.media_id, &row.file_path, row.thumbnail_path.as_deref())?;
        if crate::io::journal::prepare_committed_cleanup(&transaction, plan)?
            == PrepareJournalOutcome::PathConflict
        {
            transaction.rollback()?;
            return Ok(TrashDeletionOutcome::PathConflict);
        }
        transaction.execute(queries::trash::DELETE_PERMANENTLY, [row.media_id])?;
        cleanup_groups = cleanup_groups
            .checked_add(1)
            .ok_or(rusqlite::Error::InvalidQuery)?;
    }
    if cleanup_groups > 0 {
        transaction.execute(queries::trash::DELETE_EMPTY_FACE_GROUPS, [])?;
    }
    transaction.commit()?;
    Ok(TrashDeletionOutcome::Deleted {
        affected_count,
        cleanup_groups,
        has_more,
    })
}

fn media_cleanup_plan(
    media_id: i64,
    file_path: &str,
    thumbnail_path: Option<&str>,
) -> rusqlite::Result<FileOperationPlan> {
    let original = parse_storage_path(file_path)?;
    let sidecar = parse_storage_path(&format!("{file_path}.supplemental-metadata.json"))?;
    let media_id = media_id.to_string();
    let mut targets = vec![
        (
            StorageRootId::Originals,
            original,
            PathClaimScope::Exact,
            "original",
        ),
        (
            StorageRootId::Originals,
            sidecar,
            PathClaimScope::Exact,
            "supplemental_metadata",
        ),
        (
            StorageRootId::Previews,
            parse_storage_path(&format!("media/{media_id}"))?,
            PathClaimScope::Subtree,
            "media_preview_tree",
        ),
        (
            StorageRootId::Previews,
            parse_storage_path(&format!("faces/{media_id}"))?,
            PathClaimScope::Subtree,
            "face_crop_tree",
        ),
        (
            StorageRootId::Previews,
            parse_storage_path(&format!("ai/{media_id}"))?,
            PathClaimScope::Subtree,
            "ai_input_tree",
        ),
        (
            StorageRootId::Previews,
            parse_storage_path(&format!("deduplicate/{media_id}.jpg"))?,
            PathClaimScope::Exact,
            "legacy_clustering_frame",
        ),
    ];
    if let Some(thumbnail_path) = thumbnail_path {
        for (root, role) in [
            (StorageRootId::Thumbnails, "thumbnail"),
            (StorageRootId::TinyThumbnails, "tiny_thumbnail"),
            (StorageRootId::PlaceThumbnails, "place_thumbnail"),
        ] {
            targets.push((
                root,
                parse_storage_path(thumbnail_path)?,
                PathClaimScope::Exact,
                role,
            ));
        }
    }
    let entries = targets
        .iter()
        .map(|(storage_root, path, _, _)| FileEntryPlan {
            action: FileEntryAction::Cleanup,
            storage_root: *storage_root,
            source_path: Some(path.clone()),
            temporary_path: None,
            destination_path: None,
            tombstone_path: None,
            expected_size: None,
            expected_sha256: None,
            expected_version: None,
        })
        .collect();
    let claims = targets
        .into_iter()
        .map(|(storage_root, path, scope, role)| FilePathClaimPlan {
            storage_root,
            path,
            mode: PathClaimMode::Write,
            scope,
            role: role.to_string(),
            expected_version: None,
        })
        .collect();
    Ok(FileOperationPlan {
        group_id: format!("media-delete-{media_id}"),
        kind: "media_delete_cleanup".to_string(),
        owner_kind: "media".to_string(),
        owner_id: media_id,
        claim_token: None,
        product_target: None,
        product_version: None,
        entries,
        claims,
        space_reservation: None,
    })
}

fn parse_storage_path(path: &str) -> rusqlite::Result<NormalizedStoragePath> {
    NormalizedStoragePath::parse(path).map_err(|_| rusqlite::Error::InvalidQuery)
}

pub(crate) fn recover_backup_writing_sessions(connection: &Connection) -> rusqlite::Result<usize> {
    connection.execute(queries::backup::RECOVER_WRITING_SESSIONS, [])
}

pub(crate) fn load_backup_resumable_page(
    connection: &Connection,
    request: BackupRecoveryPageQuery,
) -> rusqlite::Result<BackupRecoveryPage<BackupResumableFile>> {
    validate_backup_recovery_page(&request)?;
    let mut statement = connection.prepare(queries::backup::SELECT_RESUMABLE_FILES_PAGE)?;
    let mapped =
        statement.query_map(params![request.after_id, i64::from(request.limit)], |row| {
            let uploaded_size = row.get::<_, i64>(2)?;
            Ok(BackupResumableFile {
                asset_id: row.get(0)?,
                staged_path: row.get(1)?,
                uploaded_size: u64::try_from(uploaded_size)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(2, uploaded_size))?,
            })
        })?;
    let rows = mapped.collect::<rusqlite::Result<Vec<_>>>()?;
    validate_backup_recovery_rows(&rows, |row| row.staged_path.len())?;
    let next_after_id = (rows.len() == usize::from(request.limit))
        .then(|| rows.last().map(|row| row.asset_id))
        .flatten();
    Ok(BackupRecoveryPage {
        rows,
        next_after_id,
    })
}

pub(crate) fn load_backup_processing_page(
    connection: &Connection,
    request: BackupRecoveryPageQuery,
) -> rusqlite::Result<BackupRecoveryPage<BackupProcessingAsset>> {
    validate_backup_recovery_page(&request)?;
    let mut statement = connection.prepare(queries::backup::SELECT_PROCESSING_ASSETS_PAGE)?;
    let mapped =
        statement.query_map(params![request.after_id, i64::from(request.limit)], |row| {
            Ok(BackupProcessingAsset {
                asset_id: row.get(0)?,
                user_id: row.get(1)?,
                staged_path: row.get(2)?,
                content_hash: row.get(3)?,
            })
        })?;
    let rows = mapped.collect::<rusqlite::Result<Vec<_>>>()?;
    validate_backup_recovery_rows(&rows, |row| {
        row.staged_path.len() + row.content_hash.as_ref().map_or(0, String::len)
    })?;
    let next_after_id = (rows.len() == usize::from(request.limit))
        .then(|| rows.last().map(|row| row.asset_id))
        .flatten();
    Ok(BackupRecoveryPage {
        rows,
        next_after_id,
    })
}

fn validate_backup_recovery_page(request: &BackupRecoveryPageQuery) -> rusqlite::Result<()> {
    if request.after_id < 0 || !(1..=256).contains(&request.limit) {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(())
}

fn validate_backup_recovery_rows<T>(
    rows: &[T],
    row_bytes: impl Fn(&T) -> usize,
) -> rusqlite::Result<()> {
    let mut bytes = 0usize;
    for row in rows {
        bytes = bytes
            .checked_add(row_bytes(row))
            .ok_or_else(|| bounded_output_error("backup recovery output overflow"))?;
        if bytes > MAX_API_QUERY_BYTES {
            return Err(bounded_output_error(
                "backup recovery output exceeds one mebibyte",
            ));
        }
    }
    Ok(())
}

pub(crate) fn maintain_backup_sessions(
    connection: &mut Connection,
) -> rusqlite::Result<BackupSessionMaintenance> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute(queries::backup::EXPIRE_SESSIONS, [])?;
    transaction.execute(queries::backup::EXPIRE_ASSETS, [])?;
    let seconds =
        transaction.query_row(queries::backup::SELECT_NEXT_EXPIRATION_SECONDS, [], |row| {
            row.get::<_, Option<i64>>(0)
        })?;
    let next_expiration_seconds = seconds
        .map(|seconds| u64::try_from(seconds.max(1)))
        .transpose()
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    transaction.commit()?;
    Ok(BackupSessionMaintenance {
        next_expiration_seconds,
    })
}

pub(crate) fn claim_backup_asset(
    connection: &mut Connection,
) -> rusqlite::Result<Option<ClaimedBackupAsset>> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let claimed = transaction
        .query_row(queries::backup::CLAIM_QUEUED, [], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .optional()?;
    let Some((asset_id, user_id, staged_path, source_modified_at)) = claimed else {
        transaction.commit()?;
        return Ok(None);
    };
    let (expected_content_hash, metadata_json) = transaction.query_row(
        queries::backup::SELECT_MANIFEST_FOR_ASSET,
        [asset_id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )?;
    if staged_path.len() > 4096
        || source_modified_at.len() > 128
        || expected_content_hash.len() > 128
        || metadata_json.len() > MAX_API_QUERY_BYTES
        || transaction.execute(queries::backup::MARK_SESSION_PROCESSING, [asset_id])? != 1
    {
        transaction.rollback()?;
        return Err(rusqlite::Error::InvalidQuery);
    }
    transaction.commit()?;
    Ok(Some(ClaimedBackupAsset {
        asset_id,
        user_id,
        staged_path,
        source_modified_at,
        expected_content_hash,
        metadata_json,
    }))
}

pub(crate) fn load_recovered_backup_media(
    connection: &Connection,
    content_hash: &str,
    user_id: i64,
) -> rusqlite::Result<Option<i64>> {
    if content_hash.len() > 128 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    connection
        .query_row(
            queries::backup::SELECT_RECOVERED_MEDIA,
            params![content_hash, user_id],
            |row| row.get(0),
        )
        .optional()
}

pub(crate) fn store_backup_content_hash(
    connection: &Connection,
    request: StoreBackupContentHash,
) -> rusqlite::Result<bool> {
    if request.content_hash.len() > 128 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(connection.execute(
        queries::backup::STORE_CONTENT_HASH,
        params![request.content_hash, request.asset_id],
    )? == 1)
}

pub(crate) fn transition_backup_processing(
    connection: &mut Connection,
    request: BackupProcessingTransition,
) -> rusqlite::Result<BackupProcessingTransitionOutcome> {
    let (asset_id, cleanup_kind) = match &request {
        BackupProcessingTransition::Complete { asset_id, .. } => {
            (*asset_id, Some("backup_complete_cleanup"))
        }
        BackupProcessingTransition::Requeue { asset_id } => (*asset_id, None),
        BackupProcessingTransition::Fail { asset_id, .. } => {
            (*asset_id, Some("backup_failure_cleanup"))
        }
        BackupProcessingTransition::FailMissingStaging { asset_id } => {
            (*asset_id, Some("backup_missing_cleanup"))
        }
    };
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let staged_path = cleanup_kind
        .map(|_| {
            transaction.query_row(
                queries::backup::SELECT_STAGED_PATH_FOR_ASSET,
                [asset_id],
                |row| row.get::<_, String>(0),
            )
        })
        .transpose()?;
    let (asset_changed, session_changed) = match request {
        BackupProcessingTransition::Complete { asset_id, media_id } => (
            transaction.execute(queries::backup::COMPLETE_ASSET, params![media_id, asset_id])?,
            transaction.execute(queries::backup::COMPLETE_SESSION, [asset_id])?,
        ),
        BackupProcessingTransition::Requeue { asset_id } => (
            transaction.execute(queries::backup::RECOVER_QUEUED_ASSET, [asset_id])?,
            transaction.execute(queries::backup::RECOVER_QUEUED_SESSION, [asset_id])?,
        ),
        BackupProcessingTransition::Fail { asset_id, error } => {
            if error.len() > 4096 {
                transaction.rollback()?;
                return Err(rusqlite::Error::InvalidQuery);
            }
            (
                transaction.execute(queries::backup::FAIL_ASSET, params![error, asset_id])?,
                transaction.execute(queries::backup::FAIL_SESSION, [asset_id])?,
            )
        }
        BackupProcessingTransition::FailMissingStaging { asset_id } => (
            transaction.execute(queries::backup::FAIL_MISSING_STAGED_ASSET, [asset_id])?,
            transaction.execute(queries::backup::FAIL_MISSING_STAGED_SESSION, [asset_id])?,
        ),
    };
    if asset_changed != 1 || session_changed != 1 {
        transaction.rollback()?;
        return Ok(BackupProcessingTransitionOutcome::Unchanged);
    }
    if let (Some(kind), Some(staged_path)) = (cleanup_kind, staged_path) {
        let plan = backup_staging_cleanup_plan(asset_id, &staged_path, kind)?;
        if crate::io::journal::prepare_committed_cleanup(&transaction, plan)?
            == PrepareJournalOutcome::PathConflict
        {
            transaction.rollback()?;
            return Ok(BackupProcessingTransitionOutcome::PathConflict);
        }
    }
    transaction.commit()?;
    Ok(BackupProcessingTransitionOutcome::Transitioned {
        cleanup_group: cleanup_kind.is_some(),
    })
}

fn backup_staging_cleanup_plan(
    asset_id: i64,
    staged_path: &str,
    kind: &str,
) -> rusqlite::Result<FileOperationPlan> {
    let staged = parse_storage_path(staged_path)?;
    let sidecar = parse_storage_path(&format!("{staged_path}.supplemental-metadata.json"))?;
    let targets = [
        (staged, "staged_media"),
        (sidecar, "staged_supplemental_metadata"),
    ];
    Ok(FileOperationPlan {
        group_id: format!("backup-finalize-{asset_id}"),
        kind: kind.to_string(),
        owner_kind: "backup_asset".to_string(),
        owner_id: asset_id.to_string(),
        claim_token: None,
        product_target: None,
        product_version: None,
        entries: targets
            .iter()
            .map(|(path, _)| FileEntryPlan {
                action: FileEntryAction::Cleanup,
                storage_root: StorageRootId::Backups,
                source_path: Some(path.clone()),
                temporary_path: None,
                destination_path: None,
                tombstone_path: None,
                expected_size: None,
                expected_sha256: None,
                expected_version: None,
            })
            .collect(),
        claims: targets
            .into_iter()
            .map(|(path, role)| FilePathClaimPlan {
                storage_root: StorageRootId::Backups,
                path,
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Exact,
                role: role.to_string(),
                expected_version: None,
            })
            .collect(),
        space_reservation: None,
    })
}

pub(crate) fn cancel_backup_upload(
    connection: &mut Connection,
    request: CancelBackupUpload,
) -> rusqlite::Result<CancelBackupUploadOutcome> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let upload = transaction
        .query_row(
            queries::backup::SELECT_UPLOAD,
            params![request.upload_id, request.user_id],
            |row| {
                Ok(BackupUploadCancellationRow {
                    asset_id: row.get(0)?,
                    response: BackupUploadResponse {
                        upload_id: row.get(1)?,
                        status: row.get(2)?,
                        uploaded_size: row.get(4)?,
                        expected_size: row.get(5)?,
                        media_id: row.get(7)?,
                        error: row.get(8)?,
                        content_hash: row.get(9)?,
                    },
                    session_status: row.get(3)?,
                    staged_path: row.get(6)?,
                })
            },
        )
        .optional()?;
    let Some(upload) = upload else {
        transaction.rollback()?;
        return Ok(CancelBackupUploadOutcome::NotFound);
    };
    if upload.response.status == "cancelled" {
        transaction.commit()?;
        return Ok(CancelBackupUploadOutcome::AlreadyCancelled(upload.response));
    }
    if upload.session_status == "writing" {
        transaction.rollback()?;
        return Ok(CancelBackupUploadOutcome::Writing);
    }
    if !matches!(upload.response.status.as_str(), "uploading" | "queued")
        || !matches!(upload.session_status.as_str(), "uploading" | "queued")
    {
        transaction.rollback()?;
        return Ok(CancelBackupUploadOutcome::NotCancellable);
    }
    if transaction.execute(
        queries::backup::CANCEL_SESSION,
        params![upload.response.upload_id, request.user_id],
    )? != 1
        || transaction.execute(queries::backup::CANCEL_ASSET, [upload.asset_id])? != 1
    {
        transaction.rollback()?;
        return Ok(CancelBackupUploadOutcome::Changed);
    }

    let staged_path = NormalizedStoragePath::parse(&upload.staged_path)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let expected_size = u64::try_from(upload.response.uploaded_size)
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(4, upload.response.uploaded_size))?;
    let plan = FileOperationPlan {
        group_id: format!("backup-cancel-{}", upload.asset_id),
        kind: "backup_cancel_cleanup".to_string(),
        owner_kind: "backup_asset".to_string(),
        owner_id: upload.asset_id.to_string(),
        claim_token: None,
        product_target: None,
        product_version: None,
        entries: vec![FileEntryPlan {
            action: FileEntryAction::Cleanup,
            storage_root: StorageRootId::Backups,
            source_path: Some(staged_path.clone()),
            temporary_path: None,
            destination_path: None,
            tombstone_path: None,
            expected_size: Some(expected_size),
            expected_sha256: None,
            expected_version: None,
        }],
        claims: vec![FilePathClaimPlan {
            storage_root: StorageRootId::Backups,
            path: staged_path,
            mode: PathClaimMode::Write,
            scope: PathClaimScope::Exact,
            role: "cancelled_staging_file".to_string(),
            expected_version: None,
        }],
        space_reservation: None,
    };
    if crate::io::journal::prepare_committed_cleanup(&transaction, plan)?
        == PrepareJournalOutcome::PathConflict
    {
        transaction.rollback()?;
        return Ok(CancelBackupUploadOutcome::PathConflict);
    }
    let mut response = upload.response;
    response.status = "cancelled".to_string();
    transaction.commit()?;
    Ok(CancelBackupUploadOutcome::Cancelled(response))
}

pub(crate) fn register_backup_device(
    connection: &Connection,
    request: RegisterBackupDevice,
) -> rusqlite::Result<()> {
    connection.execute(
        queries::backup::UPSERT_DEVICE,
        params![request.user_id, request.device_id, request.device_name],
    )?;
    Ok(())
}

pub(crate) fn load_deduplicate_schedule_state(
    connection: &Connection,
) -> rusqlite::Result<DeduplicateScheduleState> {
    let latest_run_status = connection
        .query_row(queries::deduplicate::SELECT_LATEST_RUN_STATUS, [], |row| {
            row.get(0)
        })
        .optional()?;
    let last_scheduled_for = connection
        .query_row(queries::deduplicate::SELECT_LAST_SCHEDULED_FOR, [], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(DeduplicateScheduleState {
        latest_run_status,
        last_scheduled_for,
    })
}

pub(crate) fn recover_deduplicate_runs(connection: &mut Connection) -> rusqlite::Result<()> {
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
    transaction.execute(queries::deduplicate::FAIL_INTERRUPTED_RUNS, [])?;
    transaction.execute(queries::deduplicate::MARK_ALL_DIRTY, [])?;
    transaction.commit()
}

pub(crate) fn recover_face_grouping_runs(connection: &mut Connection) -> rusqlite::Result<()> {
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(queries::faces::QUEUE_RECOVERED_CANCELLATION_SCOPE, [])?;
    transaction.execute(queries::faces::QUEUE_RECOVERED_CANCELLATIONS, [])?;
    transaction.execute(queries::faces::CANCEL_RECOVERED_CANCELLING_JOBS, [])?;
    transaction.execute(queries::faces::FINALIZE_RECOVERED_CANCELLING_RUNS, [])?;
    transaction.commit()
}

pub(crate) fn load_face_representative_group_page(
    connection: &Connection,
    request: FaceRepresentativeGroupPageQuery,
) -> rusqlite::Result<FaceRepresentativeGroupPage> {
    let group_ids = connection
        .prepare("SELECT id FROM face_groups WHERE id > ? ORDER BY id LIMIT ?")?
        .query_map(
            params![request.after_group_id, i64::from(request.limit)],
            |row| row.get::<_, i64>(0),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(FaceRepresentativeGroupPage { group_ids })
}

pub(crate) fn load_face_representative_candidate_page(
    connection: &Connection,
    request: FaceRepresentativeCandidatePageQuery,
) -> rusqlite::Result<FaceRepresentativeCandidatePage> {
    let candidates = connection
        .prepare(queries::faces::SELECT_GROUP_REPRESENTATIVE_CANDIDATE_PAGE)?
        .query_map(
            params![
                request.group_id,
                request.after_face_id,
                i64::from(request.limit)
            ],
            crate::processor::face_detection::map_representative_candidate,
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let exhausted = candidates.len() < usize::from(request.limit);
    Ok(FaceRepresentativeCandidatePage {
        candidates,
        exhausted,
    })
}

pub(crate) fn update_face_representative(
    connection: &Connection,
    request: UpdateFaceRepresentative,
) -> rusqlite::Result<()> {
    connection.execute(
        queries::faces::UPDATE_GROUP_REPRESENTATIVE_ID,
        params![request.representative_face_id, request.group_id],
    )?;
    Ok(())
}

pub(crate) fn invalidate_webdav_readiness(
    connection: &mut Connection,
    request: InvalidateWebdavReadiness,
) -> rusqlite::Result<()> {
    if request.paths.len() > 2 || request.paths.iter().any(|path| path.len() > 4096) {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for path in request.paths {
        transaction.execute(
            queries::webdav_ready::DELETE,
            params![request.user_id, path],
        )?;
    }
    transaction.commit()
}

pub(crate) fn mark_webdav_ready(
    connection: &Connection,
    request: MarkWebdavReady,
) -> rusqlite::Result<()> {
    if request.path.len() > 4096 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    connection.execute(
        queries::webdav_ready::UPSERT,
        params![request.user_id, request.path],
    )?;
    Ok(())
}

pub(crate) fn create_backup_upload(
    connection: &mut Connection,
    request: CreateBackupUpload,
) -> rusqlite::Result<CreateBackupUploadOutcome> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;

    if let Some(upload) =
        select_backup_upload_by_operation(&transaction, request.user_id, &request.operation_id)?
    {
        if !validate_or_upgrade_backup_create_contract(
            &transaction,
            request.user_id,
            &upload.upload_id,
            &request,
        )? {
            transaction.rollback()?;
            return Ok(CreateBackupUploadOutcome::ContractConflict);
        }
        let upload = select_backup_upload_by_operation(
            &transaction,
            request.user_id,
            &request.operation_id,
        )?
        .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        transaction.commit()?;
        return Ok(CreateBackupUploadOutcome::Existing(upload));
    }

    if let Some(upload) = select_backup_upload_by_client_asset(
        &transaction,
        request.user_id,
        &request.device_id,
        &request.client_asset_id,
    )? {
        if !validate_or_upgrade_backup_create_contract(
            &transaction,
            request.user_id,
            &upload.upload_id,
            &request,
        )? {
            transaction.rollback()?;
            return Ok(CreateBackupUploadOutcome::ContractConflict);
        }
        let upload = select_backup_upload_by_client_asset(
            &transaction,
            request.user_id,
            &request.device_id,
            &request.client_asset_id,
        )?
        .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        transaction.commit()?;
        return Ok(CreateBackupUploadOutcome::Existing(upload));
    }

    let device_exists: bool = transaction.query_row(
        queries::backup::DEVICE_EXISTS,
        params![request.user_id, request.device_id],
        |row| row.get(0),
    )?;
    if !device_exists {
        transaction.rollback()?;
        return Ok(CreateBackupUploadOutcome::DeviceNotFound);
    }

    transaction.execute(
        queries::backup::INSERT_ASSET,
        params![
            request.user_id,
            request.device_id,
            request.client_asset_id,
            request.operation_id,
            request.original_filename,
            request.mime_type,
            request.expected_size,
            request.source_modified_at,
            request.staged_path,
        ],
    )?;
    let asset_id = transaction.last_insert_rowid();
    transaction.execute(
        queries::backup::INSERT_SESSION,
        params![
            request.upload_id,
            asset_id,
            request.user_id,
            request.expected_size,
            format!("+{} hours", request.session_expiry_hours),
        ],
    )?;
    transaction.execute(
        queries::backup::INSERT_MANIFEST,
        params![
            asset_id,
            request.protocol_version,
            request.content_hash,
            request.metadata_json,
        ],
    )?;
    let response = BackupUploadResponse {
        upload_id: request.upload_id,
        status: "uploading".to_string(),
        uploaded_size: 0,
        expected_size: request.expected_size,
        content_hash: Some(request.content_hash),
        media_id: None,
        error: None,
    };
    transaction.commit()?;
    Ok(CreateBackupUploadOutcome::Created(response))
}

fn validate_or_upgrade_backup_create_contract(
    connection: &Connection,
    user_id: i64,
    upload_id: &str,
    request: &CreateBackupUpload,
) -> rusqlite::Result<bool> {
    let stored_contract = connection.query_row(
        queries::backup::SELECT_CREATE_CONTRACT,
        params![upload_id, user_id],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<u32>>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, Option<String>>(10)?,
            ))
        },
    )?;
    let base_contract_matches = stored_contract.1 == request.device_id
        && stored_contract.2 == request.client_asset_id
        && stored_contract.3 == request.operation_id
        && stored_contract.4 == request.original_filename
        && stored_contract.5 == request.mime_type
        && stored_contract.6 == request.expected_size
        && stored_contract.7.as_deref() == Some(request.source_modified_at.as_str());
    let manifest_is_missing =
        stored_contract.8.is_none() && stored_contract.9.is_none() && stored_contract.10.is_none();
    if base_contract_matches && manifest_is_missing {
        connection.execute(
            queries::backup::INSERT_MANIFEST,
            params![
                stored_contract.0,
                request.protocol_version,
                request.content_hash,
                request.metadata_json,
            ],
        )?;
        return Ok(true);
    }
    Ok(base_contract_matches
        && stored_contract.8 == Some(request.protocol_version)
        && stored_contract.9.as_deref() == Some(request.content_hash.as_str())
        && stored_contract.10.as_deref() == Some(request.metadata_json.as_str()))
}

fn select_backup_upload_by_operation(
    connection: &Connection,
    user_id: i64,
    operation_id: &str,
) -> rusqlite::Result<Option<BackupUploadResponse>> {
    connection
        .query_row(
            queries::backup::SELECT_BY_OPERATION,
            params![user_id, operation_id],
            backup_upload_response_from_row,
        )
        .optional()
}

fn select_backup_upload_by_client_asset(
    connection: &Connection,
    user_id: i64,
    device_id: &str,
    client_asset_id: &str,
) -> rusqlite::Result<Option<BackupUploadResponse>> {
    connection
        .query_row(
            queries::backup::SELECT_BY_CLIENT_ASSET,
            params![user_id, device_id, client_asset_id],
            backup_upload_response_from_row,
        )
        .optional()
}

fn backup_upload_response_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<BackupUploadResponse> {
    Ok(BackupUploadResponse {
        upload_id: row.get(0)?,
        status: row.get(1)?,
        uploaded_size: row.get(2)?,
        expected_size: row.get(3)?,
        content_hash: row.get(6)?,
        media_id: row.get(4)?,
        error: row.get(5)?,
    })
}

pub(crate) fn load_backup_upload(
    connection: &Connection,
    request: LoadBackupUpload,
) -> rusqlite::Result<Option<BackupUploadResponse>> {
    connection
        .query_row(
            queries::backup::SELECT_UPLOAD,
            params![request.upload_id, request.user_id],
            |row| {
                Ok(BackupUploadResponse {
                    upload_id: row.get(1)?,
                    status: row.get(2)?,
                    uploaded_size: row.get(4)?,
                    expected_size: row.get(5)?,
                    media_id: row.get(7)?,
                    error: row.get(8)?,
                    content_hash: row.get(9)?,
                })
            },
        )
        .optional()
}

pub(crate) fn prepare_backup_completion(
    connection: &Connection,
    request: PrepareBackupCompletion,
) -> rusqlite::Result<PrepareBackupCompletionOutcome> {
    let upload = connection
        .query_row(
            queries::backup::SELECT_UPLOAD,
            params![request.upload_id, request.user_id],
            |row| {
                Ok(BackupUploadCancellationRow {
                    asset_id: row.get(0)?,
                    response: BackupUploadResponse {
                        upload_id: row.get(1)?,
                        status: row.get(2)?,
                        uploaded_size: row.get(4)?,
                        expected_size: row.get(5)?,
                        media_id: row.get(7)?,
                        error: row.get(8)?,
                        content_hash: row.get(9)?,
                    },
                    session_status: row.get(3)?,
                    staged_path: row.get(6)?,
                })
            },
        )
        .optional()?;
    let Some(upload) = upload else {
        return Ok(PrepareBackupCompletionOutcome::NotFound);
    };
    if matches!(
        upload.response.status.as_str(),
        "queued" | "processing" | "completed"
    ) {
        return Ok(PrepareBackupCompletionOutcome::AlreadyQueued(
            upload.response,
        ));
    }
    if upload.response.status != "uploading"
        || upload.session_status != "uploading"
        || upload.response.uploaded_size != upload.response.expected_size
    {
        return Ok(PrepareBackupCompletionOutcome::NotReady);
    }
    let Some(expected_content_hash) = upload.response.content_hash else {
        return Ok(PrepareBackupCompletionOutcome::MissingManifest);
    };
    Ok(PrepareBackupCompletionOutcome::Ready {
        asset_id: upload.asset_id,
        staged_path: upload.staged_path,
        expected_content_hash,
    })
}

pub(crate) fn queue_backup_completion(
    connection: &mut Connection,
    request: QueueBackupCompletion,
) -> rusqlite::Result<QueueBackupCompletionOutcome> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if transaction.execute(
        queries::backup::QUEUE_SESSION,
        params![request.upload_id, request.user_id],
    )? != 1
        || transaction.execute(queries::backup::QUEUE_ASSET, [request.asset_id])? != 1
    {
        transaction.rollback()?;
        return Ok(QueueBackupCompletionOutcome::Changed);
    }
    let response = transaction.query_row(
        queries::backup::SELECT_UPLOAD,
        params![request.upload_id, request.user_id],
        |row| {
            Ok(BackupUploadResponse {
                upload_id: row.get(1)?,
                status: row.get(2)?,
                uploaded_size: row.get(4)?,
                expected_size: row.get(5)?,
                media_id: row.get(7)?,
                error: row.get(8)?,
                content_hash: row.get(9)?,
            })
        },
    )?;
    transaction.commit()?;
    Ok(QueueBackupCompletionOutcome::Queued(response))
}

pub(crate) fn claim_backup_chunk(
    connection: &mut Connection,
    request: ClaimBackupChunk,
) -> rusqlite::Result<ClaimBackupChunkOutcome> {
    let start = i64::try_from(request.start)
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, i64::MAX))?;
    let total = i64::try_from(request.total)
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, i64::MAX))?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let upload = transaction
        .query_row(
            queries::backup::SELECT_UPLOAD,
            params![request.upload_id, request.user_id],
            |row| {
                Ok((
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .optional()?;
    let Some(upload) = upload else {
        transaction.rollback()?;
        return Ok(ClaimBackupChunkOutcome::NotFound);
    };
    if upload.0 != "uploading" || upload.1 != start || upload.2 != total {
        transaction.rollback()?;
        return Ok(ClaimBackupChunkOutcome::Rejected);
    }
    if transaction.execute(
        queries::backup::CLAIM_CHUNK,
        params![request.upload_id, request.user_id, start],
    )? != 1
    {
        transaction.rollback()?;
        return Ok(ClaimBackupChunkOutcome::Rejected);
    }
    transaction.commit()?;
    Ok(ClaimBackupChunkOutcome::Accepted {
        staged_path: upload.3,
    })
}

pub(crate) fn finish_backup_chunk(
    connection: &mut Connection,
    request: FinishBackupChunk,
) -> rusqlite::Result<FinishBackupChunkOutcome> {
    let start = i64::try_from(request.start)
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, i64::MAX))?;
    let next_offset = i64::try_from(request.next_offset)
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, i64::MAX))?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if transaction.execute(
        queries::backup::COMPLETE_CHUNK,
        params![next_offset, request.upload_id, request.user_id, start],
    )? != 1
    {
        transaction.rollback()?;
        return Ok(FinishBackupChunkOutcome::Changed);
    }
    let response = transaction.query_row(
        queries::backup::SELECT_UPLOAD,
        params![request.upload_id, request.user_id],
        |row| {
            Ok(BackupUploadResponse {
                upload_id: row.get(1)?,
                status: row.get(2)?,
                uploaded_size: row.get(4)?,
                expected_size: row.get(5)?,
                media_id: row.get(7)?,
                error: row.get(8)?,
                content_hash: row.get(9)?,
            })
        },
    )?;
    transaction.commit()?;
    Ok(FinishBackupChunkOutcome::Completed(response))
}

pub(crate) fn abandon_backup_chunk(
    connection: &Connection,
    request: AbandonBackupChunk,
) -> rusqlite::Result<()> {
    connection.execute(
        queries::backup::ABANDON_CHUNK,
        params![request.upload_id, request.user_id],
    )?;
    Ok(())
}

pub(crate) fn load_face_groups_page(
    connection: &Connection,
    request: FaceGroupsPageQuery,
) -> rusqlite::Result<FaceGroupsListResponse> {
    let mut statement = connection.prepare(queries::faces::LIST_GROUPS)?;
    let groups = statement
        .query_map(
            params![request.user_id, request.limit, request.offset],
            |row| {
                Ok(FaceGroupResponse {
                    face_group_id: row.get(0)?,
                    face_count: row.get(1)?,
                    media_count: row.get(2)?,
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let total = connection.query_row(
        queries::faces::COUNT_VISIBLE_GROUPS,
        [request.user_id],
        |row| row.get::<_, i64>(0),
    )?;
    let next_offset = request
        .offset
        .checked_add(groups.len() as i64)
        .ok_or_else(|| bounded_output_error("face group offset overflow"))?;
    Ok(FaceGroupsListResponse {
        has_more: next_offset < total,
        next_cursor: (next_offset < total).then(|| next_offset.to_string()),
        groups,
    })
}

pub(crate) fn load_face_group(
    connection: &Connection,
    request: FaceGroupQuery,
) -> rusqlite::Result<Option<FaceGroupMediaResponse>> {
    let group = connection
        .query_row(
            queries::faces::SELECT_GROUP,
            params![request.face_group_id, request.user_id],
            |row| {
                Ok(FaceGroupResponse {
                    face_group_id: row.get(0)?,
                    face_count: row.get(1)?,
                    media_count: row.get(2)?,
                })
            },
        )
        .optional()?;
    let Some(group) = group else {
        return Ok(None);
    };
    let mut statement = connection.prepare(queries::faces::SELECT_GROUP_MEDIA)?;
    let mut rows = statement.query(params![request.face_group_id, request.user_id])?;
    let mut media = Vec::new();
    let mut mapped_bytes = size_of::<FaceGroupResponse>();
    while let Some(row) = rows.next()? {
        if media.len() == MAX_API_QUERY_ROWS {
            return Err(bounded_output_error("face group media exceed 4096 rows"));
        }
        let item = map_media_response(row)?;
        mapped_bytes = add_media_response_bytes(mapped_bytes, &item)?;
        media.push(item);
    }
    Ok(Some(FaceGroupMediaResponse { group, media }))
}

pub(crate) fn queue_incomplete_metadata(connection: &Connection) -> rusqlite::Result<usize> {
    connection.execute(queries::metadata_jobs::QUEUE_INCOMPLETE, [])
}

pub(crate) fn load_metadata_job_status(
    connection: &Connection,
) -> rusqlite::Result<MetadataJobStatus> {
    let counts = connection
        .prepare(queries::metadata_jobs::SELECT_STATUS_COUNTS)?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<Vec<(String, i64)>, _>>()?;
    let mut statement = connection.prepare(queries::metadata_jobs::SELECT_FAILURES)?;
    let mut rows = statement.query([])?;
    let mut errors = Vec::new();
    let mut bytes = counts.iter().map(|(status, _)| status.len()).sum::<usize>();
    while let Some(row) = rows.next()? {
        let error = row.get::<_, String>(0)?;
        if error.len() > 256 * 1024 {
            return Err(bounded_output_error(
                "metadata failure exceeds the per-row output bound",
            ));
        }
        bytes = bytes
            .checked_add(error.len())
            .ok_or_else(|| bounded_output_error("metadata status output size overflow"))?;
        if bytes > MAX_API_QUERY_BYTES {
            return Err(bounded_output_error("metadata status exceeds one mebibyte"));
        }
        errors.push(error);
    }
    Ok(MetadataJobStatus { counts, errors })
}

pub(crate) fn claim_next_metadata_job(
    connection: &mut Connection,
    claim_token: &str,
) -> rusqlite::Result<Option<MetadataJobClaim>> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let claim = transaction
        .query_row(
            queries::metadata_jobs::CLAIM_NEXT_QUEUED,
            [claim_token],
            |row| {
                Ok(MetadataJobClaim {
                    media_id: row.get(0)?,
                    claim_token: row.get(1)?,
                })
            },
        )
        .optional()?;
    transaction.commit()?;
    Ok(claim)
}

pub(crate) fn next_metadata_job_delay_seconds(
    connection: &Connection,
) -> rusqlite::Result<Option<u64>> {
    connection.query_row(
        queries::metadata_jobs::NEXT_AVAILABLE_DELAY_SECONDS,
        [],
        |row| row.get(0),
    )
}

pub(crate) fn finish_metadata_job(
    connection: &Connection,
    request: FinishMetadataJob,
) -> rusqlite::Result<()> {
    let changed = match request.error {
        None => connection.execute(
            queries::metadata_jobs::MARK_COMPLETED,
            params![request.media_id, request.claim_token],
        )?,
        Some(error) => connection.execute(
            queries::metadata_jobs::MARK_RETRY,
            params![error, request.media_id, request.claim_token],
        )?,
    };
    if changed != 1 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    Ok(())
}

pub(crate) fn recover_metadata_claims(connection: &Connection) -> rusqlite::Result<usize> {
    connection.execute(queries::metadata_jobs::RECOVER_ORPHANED_CLAIMS, [])
}

pub(crate) fn load_metadata_generation_media(
    connection: &Connection,
    media_id: i64,
) -> rusqlite::Result<MetadataGenerationMedia> {
    connection.query_row(
        queries::metadata::SELECT_IMPORTED_MEDIA,
        [media_id],
        |row| {
            Ok(MetadataGenerationMedia {
                file_path: row.get(0)?,
                media_type: row.get(1)?,
                content_hash: row.get(2)?,
                original_filename: row.get(3)?,
                mime_type: row.get(4)?,
                artifact_version: row.get(5)?,
                thumbnail_path: row.get(6)?,
                preview_path: row.get(7)?,
            })
        },
    )
}

pub(crate) fn persist_metadata_generation(
    connection: &mut Connection,
    request: PersistMetadataGeneration,
) -> rusqlite::Result<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.query_row(
        queries::metadata_jobs::VERIFY_CLAIM,
        params![request.media_id, request.claim_token],
        |_| Ok(()),
    )?;
    if transaction.execute(
        queries::metadata::FINALIZE_ARTIFACT_GROUP,
        params![
            request.artifact_group_id,
            request.artifact_group_version,
            request.artifact_version,
            request.claim_token
        ],
    )? != 1
    {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    transaction.execute(
        queries::metadata::DELETE_SOURCES_FOR_MEDIA,
        [request.media_id],
    )?;
    for source in request.sources {
        transaction.execute(
            queries::metadata::INSERT_SOURCE,
            params![request.media_id, source.source_type, 1, source.payload_json],
        )?;
    }
    let metadata = request.metadata;
    transaction.execute(
        queries::metadata::UPDATE_METADATA,
        params![
            request.media_id,
            metadata.width,
            metadata.height,
            metadata.date_taken,
            metadata.gps_latitude,
            metadata.gps_longitude,
            metadata.gps_altitude,
            metadata.camera_make,
            metadata.camera_model,
            metadata.lens_make,
            metadata.lens_model,
            metadata.iso,
            metadata.exposure_time,
            metadata.f_number,
            metadata.focal_length,
            metadata.focal_length_35mm,
            metadata.location_city,
            metadata.location_state,
            metadata.location_country,
            metadata.video_codec,
            metadata.keywords,
            metadata.duration_seconds,
        ],
    )?;
    transaction.execute(
        queries::metadata::UPDATE_ARTIFACT_GENERATION,
        params![
            request.thumbnail_path,
            request.preview_path,
            request.artifact_version,
            request.media_id
        ],
    )?;
    transaction.execute(
        queries::media::UPDATE_CONTENT_HASH,
        params![request.content_hash, request.media_id],
    )?;
    transaction.execute(
        queries::metadata::DELETE_RTREE_FOR_MEDIA,
        [request.media_id],
    )?;
    if let (Some(latitude), Some(longitude)) = (metadata.gps_latitude, metadata.gps_longitude) {
        transaction.execute(
            queries::metadata::INSERT_RTREE,
            params![request.media_id, latitude, latitude, longitude, longitude],
        )?;
    }
    transaction.execute(
        queries::metadata::UPSERT_GEOHASH,
        params![request.media_id, request.geohash],
    )?;
    transaction.execute(
        queries::metadata::DELETE_AI_INPUTS_FOR_MEDIA,
        [request.media_id],
    )?;
    for input in request.ai_inputs {
        transaction.execute(
            queries::metadata::INSERT_AI_INPUT,
            params![
                request.media_id,
                input.task,
                input.sequence,
                input.input_kind,
                input.storage_root,
                input.file_path,
                input.filename,
                input.mime_type,
                input.byte_size,
                input.content_hash,
                input.frame_timestamp_ms,
            ],
        )?;
    }
    transaction.commit()
}

pub(crate) fn prepare_llm_submission_cycle(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute(queries::ai_jobs::RECLAIM_STALE, [])?;
    connection.execute(queries::ai_jobs::SNAPSHOT_QUEUED_INPUTS, [])?;
    Ok(())
}

pub(crate) fn claim_llm_submission_jobs(
    connection: &mut Connection,
    limit: u16,
) -> rusqlite::Result<Vec<LlmSubmissionJob>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let jobs = {
        let mut statement = transaction.prepare(queries::ai_jobs::SELECT_QUEUED)?;
        let jobs = statement
            .query_map([i64::from(limit)], |row| {
                Ok(LlmSubmissionJob {
                    job_id: row.get(0)?,
                    media_id: row.get(1)?,
                    task: row.get(2)?,
                    attempts: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        jobs
    };
    let mut claimed = Vec::with_capacity(jobs.len());
    for job in jobs {
        if transaction.execute(queries::ai_jobs::CLAIM, [&job.job_id])? == 1 {
            claimed.push(job);
        }
    }
    transaction.commit()?;
    Ok(claimed)
}

pub(crate) fn next_llm_submission_delay_seconds(
    connection: &Connection,
) -> rusqlite::Result<Option<u64>> {
    connection.query_row(queries::ai_jobs::NEXT_AVAILABLE_DELAY_SECONDS, [], |row| {
        row.get(0)
    })
}

pub(crate) fn load_llm_prepared_inputs(
    connection: &Connection,
    job_id: &str,
) -> rusqlite::Result<Vec<LlmPreparedInput>> {
    let mut statement = connection.prepare(queries::ai_jobs::SELECT_INPUTS)?;
    let mut rows = statement.query([job_id])?;
    let mut inputs = Vec::new();
    let mut mapped_bytes = 0usize;
    while let Some(row) = rows.next()? {
        if inputs.len() == momento_common::llm::MAX_LLM_INPUTS_PER_JOB {
            return Err(bounded_output_error("LLM prepared inputs exceed 1024 rows"));
        }
        let input = LlmPreparedInput {
            sequence: row.get(0)?,
            storage_root: row.get(1)?,
            file_path: row.get(2)?,
            filename: row.get(3)?,
            mime_type: row.get(4)?,
            byte_size: row.get(5)?,
            content_hash: row.get(6)?,
            input_kind: row.get(7)?,
            frame_timestamp_ms: row.get(8)?,
        };
        for value in [
            &input.storage_root,
            &input.file_path,
            &input.filename,
            &input.mime_type,
            &input.content_hash,
            &input.input_kind,
        ] {
            mapped_bytes = mapped_bytes
                .checked_add(value.len())
                .ok_or_else(|| bounded_output_error("LLM prepared input size overflow"))?;
        }
        if mapped_bytes > MAX_API_QUERY_BYTES {
            return Err(bounded_output_error(
                "LLM prepared inputs exceed one mebibyte",
            ));
        }
        inputs.push(input);
    }
    Ok(inputs)
}

pub(crate) fn finish_llm_submission(
    connection: &Connection,
    request: FinishLlmSubmission,
) -> rusqlite::Result<()> {
    match request {
        FinishLlmSubmission::Submitted { job_id, attempt } => {
            connection.execute(queries::ai_jobs::MARK_SUBMITTED, params![job_id, attempt])?;
        }
        FinishLlmSubmission::Deferred {
            job_id,
            retry_after_seconds,
        } => {
            connection.execute(
                queries::ai_jobs::REQUEUE_DEFERRED,
                params![retry_after_seconds, job_id],
            )?;
        }
        FinishLlmSubmission::Retry { job_id, error } => {
            connection.execute(queries::ai_jobs::RETRY_OR_FAIL, params![error, job_id])?;
        }
        FinishLlmSubmission::Failed { job_id, error } => {
            connection.execute(queries::ai_jobs::MARK_FAILED, params![error, job_id])?;
        }
        FinishLlmSubmission::RequeueAmbiguous { job_id } => {
            connection.execute(queries::ai_jobs::REQUEUE_AMBIGUOUS, [job_id])?;
        }
    }
    Ok(())
}

pub(crate) fn load_llm_cancellation_batch(
    connection: &Connection,
    limit: u16,
) -> rusqlite::Result<Option<LlmCancellationBatch>> {
    let Some((scope, task)) = connection
        .query_row(queries::ai_jobs::SELECT_CANCELLATION_SCOPE, [], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .optional()?
    else {
        return Ok(None);
    };
    let job_ids = if scope == "all" {
        connection
            .prepare(queries::ai_jobs::SELECT_ALL_CANCELLATIONS)?
            .query_map([i64::from(limit)], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        connection
            .prepare(queries::ai_jobs::SELECT_CANCELLATIONS_FOR_TASK)?
            .query_map(params![task, i64::from(limit)], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mapped_bytes = scope
        .len()
        .checked_add(task.len())
        .and_then(|bytes| {
            job_ids
                .iter()
                .try_fold(bytes, |total, job_id| total.checked_add(job_id.len()))
        })
        .ok_or_else(|| bounded_output_error("LLM cancellation batch size overflow"))?;
    if job_ids.len() > usize::from(limit) || mapped_bytes > MAX_API_QUERY_BYTES {
        return Err(bounded_output_error(
            "LLM cancellation batch exceeds its output bound",
        ));
    }
    Ok(Some(LlmCancellationBatch {
        scope,
        task,
        job_ids,
    }))
}

pub(crate) fn acknowledge_llm_cancellation(
    connection: &mut Connection,
    request: AcknowledgeLlmCancellation,
) -> rusqlite::Result<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for job_id in request.job_ids {
        transaction.execute(queries::ai_jobs::DELETE_CANCELLATION, [job_id])?;
    }
    let remaining: i64 = if request.scope == "all" {
        transaction.query_row(queries::ai_jobs::COUNT_ALL_CANCELLATIONS, [], |row| {
            row.get(0)
        })?
    } else {
        transaction.query_row(
            queries::ai_jobs::COUNT_CANCELLATIONS_FOR_TASK,
            [&request.task],
            |row| row.get(0),
        )?
    };
    if remaining == 0 {
        if request.scope == "all" {
            transaction.execute(queries::ai_jobs::DELETE_ALL_CANCELLATION_SCOPES, [])?;
        } else {
            transaction.execute(
                queries::ai_jobs::DELETE_CANCELLATION_SCOPE_FOR_TASK,
                [&request.task],
            )?;
        }
    }
    transaction.commit()
}

pub(crate) fn prepare_llm_result_receipt(
    connection: &mut Connection,
    request: PrepareLlmResultReceipt,
) -> rusqlite::Result<LlmResultReceiptPreparation> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let Some((media_id, task, attempts, status, job_version)) = transaction
        .query_row(
            queries::llm_callback::SELECT_JOB_FOR_RESULT_RECEIPT,
            [&request.job_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .optional()?
    else {
        transaction.commit()?;
        return Ok(LlmResultReceiptPreparation::Ignored);
    };
    if matches!(status.as_str(), "completed" | "failed" | "cancelled") {
        transaction.commit()?;
        return Ok(LlmResultReceiptPreparation::Ignored);
    }
    let expected_attempt = if status == "submitted" {
        attempts
    } else {
        attempts + 1
    };
    if media_id != request.media_id
        || task != request.task
        || expected_attempt != i64::from(request.attempt)
    {
        transaction.execute(
            queries::llm_callback::MARK_RESULT_CORRELATION_FAILED,
            params![
                "received LLM result does not match the Momento job",
                request.job_id
            ],
        )?;
        transaction.commit()?;
        return Ok(LlmResultReceiptPreparation::CorrelationFailed);
    }
    let inputs = load_llm_prepared_inputs(&transaction, &request.job_id)?;
    if inputs.is_empty() {
        transaction.execute(
            queries::llm_callback::MARK_RESULT_CORRELATION_FAILED,
            params![
                "active LLM result job has no retained input correlation",
                request.job_id
            ],
        )?;
        transaction.commit()?;
        return Ok(LlmResultReceiptPreparation::CorrelationFailed);
    }
    transaction.commit()?;
    Ok(LlmResultReceiptPreparation::Ready {
        job_version,
        inputs,
    })
}

pub(crate) fn create_llm_result_receipt(
    connection: &mut Connection,
    request: CreateLlmResultReceipt,
    sqlite_reservation: crate::io::space_budget::ProvisionalSpaceToken,
) -> rusqlite::Result<CreateLlmResultReceiptOutcome> {
    if let Some((attempt, job_version, state)) = connection
        .query_row(
            queries::llm_callback::SELECT_RESULT_RECEIPT_STATE,
            [&request.job_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?
    {
        return Ok(
            if attempt == i64::from(request.attempt)
                && job_version == request.expected_job_version
                && state == "receiving"
            {
                CreateLlmResultReceiptOutcome::Deferred
            } else {
                CreateLlmResultReceiptOutcome::Changed
            },
        );
    }

    let job_id = request.job_id.clone();
    let sqlite_reservation_id = sqlite_reservation.reservation_id().to_string();
    let prepare = crate::io::journal::prepare_file_operation_with(
        connection,
        request.journal_plan,
        |transaction| {
            let Some((media_id, task, attempts, status, job_version)) = transaction
                .query_row(
                    queries::llm_callback::SELECT_JOB_FOR_RESULT_RECEIPT,
                    [&request.job_id],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, i64>(4)?,
                        ))
                    },
                )
                .optional()?
            else {
                return Err(rusqlite::Error::InvalidQuery);
            };
            let expected_attempt = if status == "submitted" {
                attempts
            } else {
                attempts + 1
            };
            if matches!(status.as_str(), "completed" | "failed" | "cancelled")
                || media_id != request.media_id
                || task != request.task
                || expected_attempt != i64::from(request.attempt)
                || job_version != request.expected_job_version
            {
                return Err(rusqlite::Error::InvalidQuery);
            }
            transaction.execute(
                queries::file_operations::INSERT_SQLITE_RESULT_RESERVATION,
                params![
                    sqlite_reservation.reservation_id(),
                    request.job_id,
                    sqlite_reservation.filesystem_id(),
                    sqlite_reservation.peak_additional_bytes(),
                    sqlite_reservation.generation(),
                ],
            )?;
            transaction.execute(
                queries::llm_callback::INSERT_RESULT_RECEIPT,
                params![
                    request.job_id,
                    request.attempt,
                    request.expected_job_version,
                    request.media_id,
                    request.task,
                    request.result_status,
                    request.model_type,
                    request.model_version,
                    request.encoding,
                    request.record_count,
                    request.byte_size,
                    request.content_hash,
                    request.journal_group_id,
                    sqlite_reservation_id,
                    request.inbox_path,
                    request.receive_token,
                ],
            )?;
            Ok(())
        },
    );
    match prepare {
        Ok(PrepareJournalOutcome::Prepared) => {
            let checkout = sqlite_reservation
                .commit_to_durable_owner("llm_result".to_string(), job_id, None)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            drop(checkout);
            Ok(CreateLlmResultReceiptOutcome::Created)
        }
        Ok(PrepareJournalOutcome::PathConflict) => Ok(CreateLlmResultReceiptOutcome::PathConflict),
        Err(rusqlite::Error::InvalidQuery) => Ok(CreateLlmResultReceiptOutcome::Changed),
        Err(error)
            if error.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) =>
        {
            let existing = connection
                .query_row(
                    queries::llm_callback::SELECT_RESULT_RECEIPT_STATE,
                    [job_id],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if existing {
                Ok(CreateLlmResultReceiptOutcome::Deferred)
            } else {
                Err(error)
            }
        }
        Err(error) => Err(error),
    }
}

pub(crate) fn commit_llm_result_receipt(
    connection: &mut Connection,
    request: CommitLlmResultReceipt,
) -> rusqlite::Result<LlmResultReceiptOutcome> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let Some((attempts, status, job_version)) = transaction
        .query_row(
            queries::llm_callback::SELECT_JOB_FOR_RESULT_RECEIPT,
            [&request.job_id],
            |row| {
                Ok((
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .optional()?
    else {
        transaction.rollback()?;
        return Ok(LlmResultReceiptOutcome::Ignored);
    };
    if matches!(status.as_str(), "completed" | "failed" | "cancelled")
        || job_version != request.expected_job_version
    {
        transaction.rollback()?;
        return Ok(LlmResultReceiptOutcome::Ignored);
    }
    let expected_attempt = if status == "submitted" {
        attempts
    } else {
        attempts + 1
    };
    if expected_attempt != i64::from(request.attempt) {
        transaction.rollback()?;
        return Ok(LlmResultReceiptOutcome::CorrelationFailed);
    }
    if matches!(status.as_str(), "queued" | "submitting")
        && transaction.execute(
            queries::llm_callback::MARK_UNACKNOWLEDGED_RESULT_SUBMITTED,
            params![request.attempt, request.job_id, request.attempt],
        )? != 1
    {
        transaction.rollback()?;
        return Ok(LlmResultReceiptOutcome::Changed);
    }
    if transaction.execute(
        queries::llm_callback::MARK_RESULT_RECEIPT_RECEIVED,
        params![
            request.job_id,
            request.attempt,
            request.expected_job_version,
            request.journal_group_id
        ],
    )? != 1
        || transaction.execute(
            queries::llm_callback::COMPLETE_RESULT_RECEIPT_GROUP,
            params![request.journal_group_id, request.expected_group_version],
        )? != 1
    {
        transaction.rollback()?;
        return Ok(LlmResultReceiptOutcome::Changed);
    }
    transaction.execute(
        queries::file_operations::RELEASE_GROUP_CLAIMS,
        [&request.journal_group_id],
    )?;
    transaction.commit()?;
    Ok(LlmResultReceiptOutcome::Received)
}

pub(crate) fn stage_llm_result_page(
    connection: &mut Connection,
    request: StageLlmResultPage,
    capacity: &SqliteResultCapacityChild,
) -> rusqlite::Result<StageLlmResultPageOutcome> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let Some((attempt, state, claim_token, next_record_sequence, next_byte_offset)) = transaction
        .query_row(
            queries::llm_callback::SELECT_RESULT_RECEIPT_PROGRESS,
            [&request.job_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .optional()?
    else {
        transaction.commit()?;
        return Ok(StageLlmResultPageOutcome::Changed);
    };
    if attempt != i64::from(request.attempt)
        || state != "processing"
        || claim_token.as_deref() != Some(&request.claim_token)
        || next_record_sequence != i64::from(request.expected_record_sequence)
        || next_byte_offset
            != i64::try_from(request.expected_byte_offset)
                .map_err(|_| rusqlite::Error::InvalidQuery)?
    {
        transaction.commit()?;
        return Ok(StageLlmResultPageOutcome::Changed);
    }

    let mut expected_sequence = request.expected_record_sequence;
    let mut expected_offset = request.expected_byte_offset;
    for record in request.records {
        if record.record_sequence != expected_sequence || record.byte_offset != expected_offset {
            return Err(rusqlite::Error::InvalidQuery);
        }
        transaction.execute(
            queries::llm_callback::INSERT_RESULT_STAGING_RECORD,
            params![
                request.job_id,
                request.attempt,
                record.record_sequence,
                record.input_sequence,
                record.kind,
                record.byte_offset,
                record.encoded_size,
                record.normalized_payload,
            ],
        )?;
        expected_sequence = expected_sequence
            .checked_add(1)
            .ok_or(rusqlite::Error::InvalidQuery)?;
        expected_offset = expected_offset
            .checked_add(u64::from(record.encoded_size))
            .ok_or(rusqlite::Error::InvalidQuery)?;
    }
    if transaction.execute(
        queries::llm_callback::ADVANCE_RESULT_RECEIPT_PROGRESS,
        params![
            expected_sequence,
            expected_offset,
            request.job_id,
            request.attempt,
            request.claim_token,
            request.expected_record_sequence,
            request.expected_byte_offset,
        ],
    )? != 1
    {
        transaction.rollback()?;
        return Ok(StageLlmResultPageOutcome::Changed);
    }
    if transaction.execute(
        queries::file_operations::CONSUME_SQLITE_RESULT_RESERVATION,
        params![
            capacity.max_growth_bytes,
            capacity.reservation_id,
            request.job_id,
            capacity.expected_version,
            capacity.max_growth_bytes,
        ],
    )? != 1
    {
        transaction.rollback()?;
        return Err(rusqlite::Error::InvalidQuery);
    }
    transaction.commit()?;
    Ok(StageLlmResultPageOutcome::Staged)
}

#[derive(Debug, Clone)]
pub(crate) struct SqliteResultCapacityChild {
    pub reservation_id: String,
    pub expected_version: u64,
    pub max_growth_bytes: u64,
    pub cleanup_remaining_bytes: u64,
}

pub(crate) fn shrink_llm_result_sqlite_reservation_to_cleanup(
    connection: &Connection,
    job_id: &str,
    capacity: &SqliteResultCapacityChild,
) -> rusqlite::Result<()> {
    if connection.execute(
        queries::file_operations::SHRINK_SQLITE_RESULT_RESERVATION_TO_CLEANUP,
        params![
            capacity.cleanup_remaining_bytes,
            capacity.reservation_id,
            job_id,
            capacity.expected_version,
            capacity.cleanup_remaining_bytes,
        ],
    )? != 1
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(())
}

pub(crate) fn select_llm_result_staging_cleanup(
    connection: &Connection,
    limit: i64,
) -> rusqlite::Result<Vec<String>> {
    connection
        .prepare(queries::llm_callback::SELECT_RESULT_STAGING_CLEANUP)?
        .query_map([limit], |row| row.get(0))?
        .collect()
}

pub(crate) fn release_llm_result_claim(
    connection: &mut Connection,
    job_id: &str,
    claim_token: &str,
) -> rusqlite::Result<bool> {
    Ok(connection.execute(
        queries::llm_callback::RELEASE_RESULT_RECEIPT_CLAIM,
        params![job_id, claim_token],
    )? == 1)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmResultRecoveryOutcome {
    pub claims_recovered: usize,
    pub replayable_receipts_retired: usize,
    pub orphaned_reservations_retired: usize,
    pub has_more: bool,
    pub(crate) released_active_reservation_ids: Vec<String>,
}

pub(crate) fn recover_llm_result_state(
    connection: &mut Connection,
) -> rusqlite::Result<LlmResultRecoveryOutcome> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let claims_recovered =
        transaction.execute(queries::llm_callback::RECOVER_RESULT_RECEIPT_CLAIMS, [])?;
    let replayable = {
        let mut statement = transaction
            .prepare(queries::file_operations::SELECT_REPLAYABLE_TERMINAL_RESULT_RECEIPTS)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    let mut replayable_receipts_retired = 0usize;
    for (group_id, reservation_id) in replayable {
        replayable_receipts_retired = replayable_receipts_retired
            .checked_add(transaction.execute(
                queries::file_operations::DELETE_REPLAYABLE_RESULT_RECEIPT_AFTER_TERMINATION,
                [group_id],
            )?)
            .ok_or(rusqlite::Error::InvalidQuery)?;
        transaction.execute(
            queries::file_operations::DELETE_RELEASED_RESULT_RESERVATION,
            [reservation_id],
        )?;
    }
    let released_active_reservation_ids = transaction
        .prepare(queries::file_operations::SELECT_ORPHANED_ACTIVE_RESULT_RESERVATIONS_PAGE)?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for reservation_id in &released_active_reservation_ids {
        transaction.execute(
            queries::file_operations::RELEASE_SQLITE_RESULT_RESERVATION,
            [reservation_id],
        )?;
    }
    let orphaned_reservations_retired = transaction.execute(
        queries::file_operations::DELETE_ORPHANED_RELEASED_RESULT_RESERVATIONS_PAGE,
        [],
    )?;
    transaction.commit()?;
    Ok(LlmResultRecoveryOutcome {
        claims_recovered,
        replayable_receipts_retired,
        orphaned_reservations_retired,
        has_more: replayable_receipts_retired == 256
            || orphaned_reservations_retired == 256
            || released_active_reservation_ids.len() == 256,
        released_active_reservation_ids,
    })
}

pub(crate) fn cleanup_llm_result_staging_page(
    connection: &mut Connection,
    job_id: &str,
    limit: i64,
) -> rusqlite::Result<CleanupLlmResultStagingOutcome> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let state = transaction
        .query_row(
            queries::llm_callback::SELECT_RESULT_RECEIPT_STATE_ONLY,
            [job_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(state) = state else {
        transaction.commit()?;
        return Ok(CleanupLlmResultStagingOutcome {
            deleted: 0,
            complete: true,
        });
    };
    if !matches!(
        state.as_str(),
        "cleanup_pending" | "file_cleanup_pending" | "cleaned" | "discarded" | "failed"
    ) {
        transaction.commit()?;
        return Ok(CleanupLlmResultStagingOutcome {
            deleted: 0,
            complete: false,
        });
    }
    let deleted = transaction.execute(
        queries::llm_callback::DELETE_RESULT_STAGING_PAGE,
        params![job_id, limit],
    )?;
    let remaining = transaction.query_row(
        queries::llm_callback::COUNT_RESULT_STAGING,
        [job_id],
        |row| row.get::<_, i64>(0),
    )?;
    if remaining == 0 && state == "cleanup_pending" {
        transaction.execute(
            queries::llm_callback::MARK_RESULT_RECEIPT_FILE_CLEANUP_PENDING,
            [job_id],
        )?;
    }
    if remaining == 0 {
        transaction.execute(
            queries::llm_callback::MARK_RESULT_RECEIPT_CLEANED_AFTER_FILE,
            [job_id],
        )?;
    }
    let complete = transaction.query_row(
        queries::llm_callback::RESULT_CLEANUP_IS_TERMINAL,
        [job_id],
        |row| row.get::<_, bool>(0),
    )?;
    transaction.commit()?;
    Ok(CleanupLlmResultStagingOutcome { deleted, complete })
}

pub(crate) fn finalize_llm_result_cleanup(
    connection: &mut Connection,
    job_id: &str,
) -> rusqlite::Result<Option<String>> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let reservation_id = transaction
        .query_row(
            queries::file_operations::SELECT_TERMINAL_SQLITE_RESULT_RESERVATION,
            [job_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(reservation_id) = reservation_id else {
        transaction.commit()?;
        return Ok(None);
    };
    if transaction.execute(
        queries::file_operations::RELEASE_SQLITE_RESULT_RESERVATION,
        [&reservation_id],
    )? != 1
    {
        transaction.rollback()?;
        return Ok(None);
    }
    transaction.commit()?;
    Ok(Some(reservation_id))
}

pub(crate) fn reject_llm_result_receipt(
    connection: &mut Connection,
    request: RejectLlmResultReceipt,
) -> rusqlite::Result<LlmResultReceiptRejection> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let job = transaction
        .query_row(
            queries::llm_callback::SELECT_JOB_FOR_RESULT_RECEIPT,
            [&request.job_id],
            |row| {
                Ok((
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((attempts, status, job_version)) = job else {
        transaction.commit()?;
        return Ok(LlmResultReceiptRejection::Discarded);
    };
    if matches!(status.as_str(), "completed" | "failed" | "cancelled") {
        transaction.commit()?;
        return Ok(LlmResultReceiptRejection::Discarded);
    }
    let expected_attempt = if status == "submitted" {
        attempts
    } else {
        attempts + 1
    };
    if expected_attempt != i64::from(request.attempt) {
        transaction.commit()?;
        return Ok(LlmResultReceiptRejection::Discarded);
    }
    if request
        .expected_job_version
        .is_some_and(|expected| expected != job_version)
    {
        transaction.commit()?;
        return Ok(LlmResultReceiptRejection::Discarded);
    }
    if transaction.execute(
        queries::llm_callback::MARK_RESULT_CORRELATION_FAILED,
        params![request.error, request.job_id],
    )? != 1
    {
        transaction.rollback()?;
        return Ok(LlmResultReceiptRejection::Discarded);
    }
    transaction.execute(
        queries::llm_callback::DISCARD_RESULT_RECEIPT,
        params![
            request.error,
            request.job_id,
            request.attempt,
            request.expected_job_version,
            request.expected_job_version,
        ],
    )?;
    transaction.commit()?;
    Ok(LlmResultReceiptRejection::Failed)
}

pub(crate) fn load_metadata_ai_input_verification(
    connection: &Connection,
    media_id: i64,
) -> rusqlite::Result<MetadataAiInputVerification> {
    let media_type = connection.query_row(
        queries::metadata::SELECT_IMPORTED_MEDIA,
        [media_id],
        |row| row.get::<_, String>(1),
    )?;
    let mut tasks = vec![
        "ocr",
        "image_tagging",
        "image_clustering",
        "face_detection",
        "image_aesthetics",
    ];
    if media_type == "image" {
        tasks.push("screenshot_detection");
        tasks.push("document_detection");
    }
    let mut inputs = Vec::new();
    let mut mapped_bytes = media_type.len();
    for task in tasks {
        let mut statement = connection.prepare(queries::metadata_jobs::SELECT_INPUT_PATHS)?;
        let mut rows = statement.query(params![media_id, task])?;
        let mut task_inputs = 0usize;
        while let Some(row) = rows.next()? {
            if inputs.len() == MAX_API_QUERY_ROWS {
                return Err(bounded_output_error(
                    "metadata AI input verification exceeds 4096 rows",
                ));
            }
            let storage_root = row.get::<_, String>(0)?;
            let file_path = row.get::<_, String>(1)?;
            mapped_bytes = mapped_bytes
                .checked_add(task.len())
                .and_then(|bytes| bytes.checked_add(storage_root.len()))
                .and_then(|bytes| bytes.checked_add(file_path.len()))
                .ok_or_else(|| bounded_output_error("metadata AI input size overflow"))?;
            if mapped_bytes > MAX_API_QUERY_BYTES {
                return Err(bounded_output_error(
                    "metadata AI input verification exceeds one mebibyte",
                ));
            }
            inputs.push(MetadataAiInputPath {
                task: task.to_string(),
                storage_root,
                file_path,
            });
            task_inputs += 1;
        }
        if task_inputs == 0 {
            return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
                std::io::Error::other(format!("missing prepared {task} AI inputs")),
            )));
        }
    }
    Ok(MetadataAiInputVerification { media_type, inputs })
}

pub(crate) fn load_binary_media(
    connection: &Connection,
    request: BinaryMediaQuery,
) -> rusqlite::Result<Option<BinaryMediaRecord>> {
    let query = if request.deleted {
        queries::media::SELECT_DELETED_BINARY_MEDIA_INFO
    } else {
        queries::media::SELECT_BINARY_MEDIA_INFO
    };
    connection
        .query_row(query, params![request.media_id, request.user_id], |row| {
            Ok(BinaryMediaRecord {
                file_path: row.get(0)?,
                mime_type: row.get(1)?,
                original_filename: row.get(2)?,
                media_type: row.get(3)?,
                thumbnail_path: row.get(4)?,
                preview_path: row.get(5)?,
            })
        })
        .optional()
}

pub(crate) fn prepare_media_update(
    connection: &Connection,
    request: PrepareMediaUpdate,
) -> rusqlite::Result<Option<EditableMediaState>> {
    connection
        .query_row(
            queries::media::SELECT_BY_ID_AND_USER,
            params![request.media_id, request.user_id],
            |row| {
                Ok(EditableMediaState {
                    gps_latitude: row.get(10)?,
                    gps_longitude: row.get(11)?,
                })
            },
        )
        .optional()
}

pub(crate) fn finalize_media_update(
    connection: &mut Connection,
    request: FinalizeMediaUpdate,
) -> rusqlite::Result<Option<MediaResponse>> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let exists = transaction
        .query_row(
            queries::media::CHECK_EXISTS,
            params![request.media_id, request.user_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !exists {
        transaction.rollback()?;
        return Ok(None);
    }
    if request.update_editable_metadata {
        transaction.execute(
            queries::media::UPSERT_EDITABLE_METADATA,
            params![
                request.media_id,
                request.date_taken,
                request.gps_latitude,
                request.gps_longitude
            ],
        )?;
    }
    if request.update_location {
        transaction.execute(
            queries::media::UPDATE_LOCATION,
            params![
                request.geohash,
                request.city,
                request.state,
                request.country,
                request.media_id
            ],
        )?;
        transaction.execute(queries::media::DELETE_RTREE, [request.media_id])?;
        if let (Some(latitude), Some(longitude)) = (
            request.effective_gps_latitude,
            request.effective_gps_longitude,
        ) {
            transaction.execute(
                queries::media::INSERT_RTREE,
                params![request.media_id, latitude, latitude, longitude, longitude],
            )?;
        }
    }
    let media = transaction
        .query_row(
            queries::media::SELECT_BY_ID_AND_USER,
            params![request.media_id, request.user_id],
            map_media_response,
        )
        .optional()?;
    transaction.commit()?;
    Ok(media)
}

fn longitude_clause(bounds: SpatialBounds) -> &'static str {
    if bounds.west <= bounds.east {
        queries::map::LONGITUDE_CLAUSE_STANDARD
    } else {
        queries::map::LONGITUDE_CLAUSE_ANTIMERIDIAN
    }
}

fn add_media_response_bytes(current: usize, media: &MediaResponse) -> rusqlite::Result<usize> {
    let strings = [
        Some(media.filename.as_str()),
        Some(media.original_filename.as_str()),
        Some(media.media_type.as_str()),
        media.mime_type.as_deref(),
        media.date_taken.as_deref(),
        media.camera_make.as_deref(),
        media.camera_model.as_deref(),
        media.lens_make.as_deref(),
        media.lens_model.as_deref(),
        media.exposure_time.as_deref(),
        media.location_city.as_deref(),
        media.location_state.as_deref(),
        media.location_country.as_deref(),
        media.video_codec.as_deref(),
        media.keywords.as_deref(),
        media.content_hash.as_deref(),
        Some(media.created_at.as_str()),
    ];
    let mapped = strings.into_iter().flatten().try_fold(
        current
            .checked_add(size_of::<MediaResponse>())
            .ok_or_else(|| bounded_output_error("media output size overflow"))?,
        |bytes, value| {
            bytes
                .checked_add(value.len())
                .ok_or_else(|| bounded_output_error("media output size overflow"))
        },
    )?;
    if mapped > MAX_API_QUERY_BYTES {
        return Err(bounded_output_error("media output exceeds one mebibyte"));
    }
    Ok(mapped)
}

pub(crate) fn create_user(
    connection: &mut Connection,
    request: CreateUser,
) -> rusqlite::Result<CreateUserOutcome> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let duplicate = transaction
        .query_row(
            queries::users::SELECT_ID_BY_CREDENTIALS,
            params![request.username, request.email],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if duplicate {
        transaction.rollback()?;
        return Ok(CreateUserOutcome::Duplicate);
    }
    transaction.execute(
        queries::users::INSERT,
        params![
            request.username,
            request.email,
            request.password_hash,
            request.role
        ],
    )?;
    let user_id = transaction.last_insert_rowid();
    let user =
        load_user_record(&transaction, user_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    transaction.commit()?;
    Ok(CreateUserOutcome::Created(user))
}

pub(crate) fn list_users(connection: &Connection) -> rusqlite::Result<Vec<UserRecord>> {
    let mut statement = connection.prepare(queries::users::SELECT_ALL)?;
    let mut rows = statement.query([])?;
    let mut users = Vec::new();
    let mut bytes = 0usize;
    while let Some(row) = rows.next()? {
        if users.len() == MAX_USER_LIST_ROWS {
            return Err(bounded_output_error("user list exceeds 4096 rows"));
        }
        let user = map_user_record(row)?;
        bytes = bytes
            .checked_add(user.username.len())
            .and_then(|value| value.checked_add(user.email.len()))
            .and_then(|value| value.checked_add(user.role.len()))
            .and_then(|value| value.checked_add(user.created_at.len()))
            .ok_or_else(|| bounded_output_error("user list size overflow"))?;
        if bytes > MAX_USER_LIST_BYTES {
            return Err(bounded_output_error("user list exceeds one mebibyte"));
        }
        users.push(user);
    }
    Ok(users)
}

pub(crate) fn load_user_record(
    connection: &Connection,
    user_id: i64,
) -> rusqlite::Result<Option<UserRecord>> {
    connection
        .query_row(queries::users::SELECT_BY_ID, [user_id], map_user_record)
        .optional()
}

pub(crate) fn update_user(
    connection: &mut Connection,
    request: UpdateUser,
) -> rusqlite::Result<UpdateUserOutcome> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let Some(username) = transaction
        .query_row(
            queries::users::SELECT_USERNAME_BY_ID,
            [request.user_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    else {
        transaction.rollback()?;
        return Ok(UpdateUserOutcome::NotFound);
    };
    if request.user_id == request.administrator_id && request.role.as_deref() == Some("user") {
        transaction.rollback()?;
        return Ok(UpdateUserOutcome::CannotDemoteSelf);
    }
    if request.is_active == Some(false) && username == crate::auth::RESERVED_ADMIN_USERNAME {
        transaction.rollback()?;
        return Ok(UpdateUserOutcome::CannotDeactivateReservedAdmin);
    }
    if request.is_active == Some(false) && request.user_id == request.administrator_id {
        transaction.rollback()?;
        return Ok(UpdateUserOutcome::CannotDeactivateSelf);
    }
    match (&request.role, request.is_active) {
        (Some(role), Some(is_active)) => transaction.execute(
            queries::users::UPDATE_ROLE_ACTIVE,
            params![role, i32::from(is_active), request.user_id],
        )?,
        (Some(role), None) => {
            transaction.execute(queries::users::UPDATE_ROLE, params![role, request.user_id])?
        }
        (None, Some(is_active)) => transaction.execute(
            queries::users::UPDATE_ACTIVE,
            params![i32::from(is_active), request.user_id],
        )?,
        (None, None) => 0,
    };
    let user = load_user_record(&transaction, request.user_id)?
        .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    transaction.commit()?;
    Ok(UpdateUserOutcome::Updated(user))
}

pub(crate) fn delete_user(
    connection: &mut Connection,
    user_id: i64,
) -> rusqlite::Result<DeleteUserOutcome> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let Some(username) = transaction
        .query_row(queries::users::SELECT_USERNAME_BY_ID, [user_id], |row| {
            row.get::<_, String>(0)
        })
        .optional()?
    else {
        transaction.rollback()?;
        return Ok(DeleteUserOutcome::NotFound);
    };
    if username == crate::auth::RESERVED_ADMIN_USERNAME {
        transaction.rollback()?;
        return Ok(DeleteUserOutcome::CannotDeleteReservedAdmin);
    }
    transaction.execute(queries::users::DELETE, [user_id])?;
    transaction.commit()?;
    Ok(DeleteUserOutcome::Deleted)
}

fn map_user_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<UserRecord> {
    Ok(UserRecord {
        id: row.get(0)?,
        username: row.get(1)?,
        email: row.get(2)?,
        role: row.get(3)?,
        must_change_password: row.get::<_, i32>(4)? != 0,
        is_active: row.get::<_, i32>(5)? != 0,
        created_at: row.get(6)?,
    })
}

fn bounded_output_error(message: &'static str) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(message)))
}

pub(crate) fn load_admin_id(connection: &Connection) -> rusqlite::Result<Option<i64>> {
    connection
        .query_row(queries::users::CHECK_ADMIN, [], |row| row.get(0))
        .optional()
}

pub(crate) fn insert_default_admin_if_missing(
    connection: &mut Connection,
    password_hash: String,
) -> rusqlite::Result<i64> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if let Some(admin_id) = transaction
        .query_row(queries::users::CHECK_ADMIN, [], |row| row.get(0))
        .optional()?
    {
        transaction.commit()?;
        return Ok(admin_id);
    }
    transaction.execute(
        queries::users::INSERT_ADMIN,
        (
            crate::auth::TEMPORARY_ADMIN_USERNAME,
            format!("{}@localhost", crate::auth::TEMPORARY_ADMIN_USERNAME),
            password_hash,
        ),
    )?;
    let admin_id = transaction.last_insert_rowid();
    transaction.commit()?;
    Ok(admin_id)
}

pub(crate) fn prepare_admin_password_reset(
    connection: &Connection,
    admin_id: i64,
) -> rusqlite::Result<bool> {
    let exists = connection
        .query_row(queries::users::CHECK_ADMIN_BY_ID, [admin_id], |_| Ok(()))
        .optional()?
        .is_some();
    if !exists {
        return Ok(false);
    }
    connection.execute(queries::auth::DELETE_ALL_USER_TOKENS, [admin_id])?;
    Ok(true)
}

pub(crate) fn cleanup_refresh_tokens(connection: &Connection) -> rusqlite::Result<usize> {
    connection.execute(queries::auth::DELETE_EXPIRED_OR_REVOKED_TOKENS, [])
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UserForToken {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub role: String,
    pub must_change_password: bool,
    pub is_active: bool,
}

pub(crate) fn load_user_for_token(
    connection: &Connection,
    user_id: i64,
) -> rusqlite::Result<Option<UserForToken>> {
    connection
        .query_row(queries::auth::SELECT_USER_FOR_TOKEN, [user_id], |row| {
            Ok(UserForToken {
                id: row.get(0)?,
                username: row.get(1)?,
                email: row.get(2)?,
                role: row.get(3)?,
                must_change_password: row.get::<_, i32>(4)? != 0,
                is_active: row.get::<_, i32>(5)? != 0,
            })
        })
        .optional()
}

#[derive(Debug)]
pub(crate) enum UserAuthIdentifier {
    Username(String),
    Id(i64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UserForAuthentication {
    pub id: i64,
    pub username: String,
    pub role: String,
    pub hashed_password: String,
    pub is_active: bool,
}

pub(crate) fn load_user_for_authentication(
    connection: &Connection,
    identifier: UserAuthIdentifier,
) -> rusqlite::Result<Option<UserForAuthentication>> {
    let mapper = |row: &rusqlite::Row<'_>| {
        Ok(UserForAuthentication {
            id: row.get(0)?,
            username: row.get(1)?,
            role: row.get(3)?,
            hashed_password: row.get(4)?,
            is_active: row.get::<_, i32>(5)? != 0,
        })
    };
    match identifier {
        UserAuthIdentifier::Username(username) => connection
            .query_row(queries::auth::SELECT_USER_BY_USERNAME, [username], mapper)
            .optional(),
        UserAuthIdentifier::Id(user_id) => connection
            .query_row(queries::auth::SELECT_USER_BY_ID, [user_id], mapper)
            .optional(),
    }
}

#[derive(Debug)]
pub(crate) struct InsertRefreshToken {
    pub token_hash: String,
    pub user_id: i64,
    pub expires_at: String,
}

pub(crate) fn insert_refresh_token(
    connection: &Connection,
    request: InsertRefreshToken,
) -> rusqlite::Result<()> {
    connection.execute(
        queries::auth::INSERT_REFRESH_TOKEN,
        params![request.token_hash, request.user_id, request.expires_at],
    )?;
    Ok(())
}

#[derive(Debug)]
pub(crate) struct RotateRefreshToken {
    pub current_token_hash: String,
    pub replacement_token_hash: String,
    pub replacement_expires_at: String,
    pub now: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RotatedRefreshIdentity {
    pub user_id: i64,
    pub username: String,
    pub role: String,
}

pub(crate) fn rotate_refresh_token(
    connection: &mut Connection,
    request: RotateRefreshToken,
) -> rusqlite::Result<Option<RotatedRefreshIdentity>> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let token = transaction
        .query_row(
            queries::auth::VALIDATE_REFRESH_TOKEN,
            params![request.current_token_hash, request.now],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i32>(6)? != 0,
                ))
            },
        )
        .optional()?;
    let Some((token_id, user_id, username, role, is_active)) = token else {
        transaction.rollback()?;
        return Ok(None);
    };
    if !is_active {
        transaction.rollback()?;
        return Ok(None);
    }
    let consumed = transaction.execute(
        queries::auth::REVOKE_REFRESH_TOKEN,
        params![token_id, request.now],
    )?;
    if consumed != 1 {
        transaction.rollback()?;
        return Ok(None);
    }
    transaction.execute(
        queries::auth::INSERT_REFRESH_TOKEN,
        params![
            request.replacement_token_hash,
            user_id,
            request.replacement_expires_at,
        ],
    )?;
    transaction.execute(queries::auth::DELETE_REVOKED_TOKEN, [token_id])?;
    transaction.commit()?;
    Ok(Some(RotatedRefreshIdentity {
        user_id,
        username,
        role,
    }))
}

pub(crate) fn revoke_refresh_token(
    connection: &Connection,
    token_hash: String,
) -> rusqlite::Result<()> {
    connection.execute(queries::auth::DELETE_REFRESH_TOKEN_BY_HASH, [token_hash])?;
    Ok(())
}

pub(crate) fn load_password_hash(
    connection: &Connection,
    user_id: i64,
) -> rusqlite::Result<Option<String>> {
    connection
        .query_row(queries::auth::SELECT_PASSWORD_HASH, [user_id], |row| {
            row.get(0)
        })
        .optional()
}

#[derive(Debug)]
pub(crate) struct ReplacePassword {
    pub user_id: i64,
    pub expected_hash: String,
    pub replacement_hash: String,
}

pub(crate) fn replace_password(
    connection: &mut Connection,
    request: ReplacePassword,
) -> rusqlite::Result<bool> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let changed = transaction.execute(
        queries::auth::UPDATE_PASSWORD_AND_RESET_FLAG_IF_UNCHANGED,
        params![
            request.replacement_hash,
            request.user_id,
            request.expected_hash,
        ],
    )?;
    if changed != 1 {
        transaction.rollback()?;
        return Ok(false);
    }
    transaction.execute(queries::auth::DELETE_ALL_USER_TOKENS, [request.user_id])?;
    transaction.commit()?;
    Ok(true)
}

#[derive(Debug)]
pub(crate) struct RegisterAuthAttempt {
    pub source_key: [u8; 32],
    pub identity_key: [u8; 32],
    pub now_epoch_seconds: i64,
    pub attempt_window_seconds: i64,
    pub identity_limit: u32,
    pub source_limit: u32,
    pub lockout_seconds: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthAttemptDecision {
    Allowed,
    RateLimited { retry_after_seconds: u64 },
    CapacityExhausted { retry_after_seconds: u64 },
}

#[derive(Debug)]
pub(crate) struct ClearAuthAttempts {
    pub source_key: [u8; 32],
    pub identity_key: [u8; 32],
}

pub(crate) fn register_auth_attempt(
    connection: &mut Connection,
    request: RegisterAuthAttempt,
) -> rusqlite::Result<AuthAttemptDecision> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let retention_seconds = request
        .attempt_window_seconds
        .max(request.lockout_seconds)
        .checked_mul(2)
        .ok_or(rusqlite::Error::IntegralValueOutOfRange(0, i64::MAX))?;
    let expiry = request
        .now_epoch_seconds
        .checked_sub(retention_seconds)
        .ok_or(rusqlite::Error::IntegralValueOutOfRange(0, i64::MIN))?;
    transaction.execute(
        queries::auth::PRUNE_ATTEMPT_BUCKETS,
        params![expiry, request.now_epoch_seconds],
    )?;

    let source = load_bucket(&transaction, &request.source_key)?;
    let identity = load_bucket(&transaction, &request.identity_key)?;
    let missing = i64::from(source.is_none()) + i64::from(identity.is_none());
    let bucket_count: i64 =
        transaction.query_row(queries::auth::COUNT_ATTEMPT_BUCKETS, [], |row| row.get(0))?;
    if bucket_count
        .checked_add(missing)
        .is_none_or(|required| required > MAX_AUTH_ATTEMPT_BUCKETS)
    {
        transaction.rollback()?;
        return Ok(AuthAttemptDecision::CapacityExhausted {
            retry_after_seconds: request.lockout_seconds.max(1) as u64,
        });
    }

    let source_result = update_bucket(
        &transaction,
        BucketUpdate {
            key: &request.source_key,
            kind: "source",
            existing: source,
            now: request.now_epoch_seconds,
            window_seconds: request.attempt_window_seconds,
            limit: request.source_limit,
            lockout_seconds: request.lockout_seconds,
        },
    )?;
    let identity_result = update_bucket(
        &transaction,
        BucketUpdate {
            key: &request.identity_key,
            kind: "identity",
            existing: identity,
            now: request.now_epoch_seconds,
            window_seconds: request.attempt_window_seconds,
            limit: request.identity_limit,
            lockout_seconds: request.lockout_seconds,
        },
    )?;
    transaction.commit()?;

    match source_result.into_iter().chain(identity_result).max() {
        Some(retry_after_seconds) => Ok(AuthAttemptDecision::RateLimited {
            retry_after_seconds,
        }),
        None => Ok(AuthAttemptDecision::Allowed),
    }
}

pub(crate) fn clear_auth_attempts(
    connection: &mut Connection,
    request: ClearAuthAttempts,
) -> rusqlite::Result<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute(
        queries::auth::DELETE_ATTEMPT_BUCKETS,
        params![
            request.source_key.as_slice(),
            request.identity_key.as_slice()
        ],
    )?;
    transaction.commit()
}

#[derive(Debug, Clone, Copy)]
struct AttemptBucket {
    window_started_at: i64,
    attempts: u32,
    locked_until: Option<i64>,
}

fn load_bucket(
    transaction: &Transaction<'_>,
    key: &[u8; 32],
) -> rusqlite::Result<Option<AttemptBucket>> {
    transaction
        .query_row(
            queries::auth::SELECT_ATTEMPT_BUCKET,
            [key.as_slice()],
            |row| {
                Ok(AttemptBucket {
                    window_started_at: row.get(0)?,
                    attempts: row.get(2)?,
                    locked_until: row.get(3)?,
                })
            },
        )
        .optional()
}

struct BucketUpdate<'a> {
    key: &'a [u8; 32],
    kind: &'static str,
    existing: Option<AttemptBucket>,
    now: i64,
    window_seconds: i64,
    limit: u32,
    lockout_seconds: i64,
}

fn update_bucket(
    transaction: &Transaction<'_>,
    update: BucketUpdate<'_>,
) -> rusqlite::Result<Option<u64>> {
    let mut bucket = update.existing.unwrap_or(AttemptBucket {
        window_started_at: update.now,
        attempts: 0,
        locked_until: None,
    });
    let retry_after_seconds = if let Some(locked_until) = bucket.locked_until {
        if locked_until > update.now {
            Some((locked_until - update.now).max(1) as u64)
        } else {
            bucket.window_started_at = update.now;
            bucket.attempts = 0;
            bucket.locked_until = None;
            None
        }
    } else {
        None
    };
    let retry_after_seconds = if retry_after_seconds.is_some() {
        retry_after_seconds
    } else if update.now.saturating_sub(bucket.window_started_at) >= update.window_seconds {
        bucket.window_started_at = update.now;
        bucket.attempts = 1;
        None
    } else if bucket.attempts >= update.limit {
        let locked_until = update
            .now
            .checked_add(update.lockout_seconds)
            .ok_or(rusqlite::Error::IntegralValueOutOfRange(0, i64::MAX))?;
        bucket.locked_until = Some(locked_until);
        Some(update.lockout_seconds.max(1) as u64)
    } else {
        bucket.attempts = bucket.attempts.saturating_add(1);
        None
    };

    transaction.execute(
        queries::auth::UPSERT_ATTEMPT_BUCKET,
        params![
            update.key.as_slice(),
            update.kind,
            bucket.window_started_at,
            update.now,
            bucket.attempts,
            bucket.locked_until,
        ],
    )?;
    Ok(retry_after_seconds)
}
