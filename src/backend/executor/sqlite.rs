use std::panic::{catch_unwind, AssertUnwindSafe};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use rusqlite::{ErrorCode, OptionalExtension};
use tokio::sync::{oneshot, Notify};

use super::{
    ExecutorDomain, ExecutorError, ExecutorErrorKind, OperationSpec, MAX_PROBE_OUTPUT_BYTES,
};
use crate::config::{Config, FaceGroupConfig};
use crate::database::operations::{
    self, AcknowledgeLlmCancellation, ActiveShareRecord, AlbumDetailOutcome, AlbumMediaMutation,
    AlbumMutationOutcome, AlbumUpdateOutcome, AuthAttemptDecision, BinaryMediaQuery,
    BinaryMediaRecord, CleanupLlmResultStagingOutcome, ClearAuthAttempts, CommitLlmResultReceipt,
    CreateAlbum, CreateLlmResultReceipt, CreateLlmResultReceiptOutcome, CreateShareLink,
    CreateShareLinkOutcome, CreateUser, CreateUserOutcome, DeleteExpiredTrashPage,
    DeleteShareLinkOutcome, DeleteTrashMedia, DeleteTrashPage, DeleteUserOutcome,
    DuplicateGroupsQuery, EditableMediaState, FaceGroupQuery, FaceGroupsPageQuery,
    FaceRepresentativeCandidatePage, FaceRepresentativeCandidatePageQuery,
    FaceRepresentativeGroupPage, FaceRepresentativeGroupPageQuery, FinalizeMediaUpdate,
    FinishLlmSubmission, FinishMetadataJob, GrantShareAccess, GrantShareAccessOutcome,
    InsertRefreshToken, LlmCancellationBatch, LlmPreparedInput, LlmResultReceiptOutcome,
    LlmSubmissionJob, MapClustersQuery, MapMediaQuery, MediaBatchQuery,
    MetadataAiInputVerification, MetadataGenerationMedia, MetadataJobClaim, MetadataJobStatus,
    MoveMediaToTrash, PersistMetadataGeneration, PlaceCoverRecord, PlaceIdentityQuery,
    PlaceMediaPage, PlaceMediaQuery, PlacePageQuery, PlaceRecord, PrepareLlmResultReceipt,
    PrepareMediaUpdate, PublicFileAccessOutcome, PublicShareContent, PublicSharedMediaQuery,
    PublicThumbnailAccessOutcome, RegisterAuthAttempt, RejectLlmResultReceipt, ReplacePassword,
    RestoreTrash, RotateRefreshToken, RotatedRefreshIdentity, StageLlmResultPage,
    StageLlmResultPageOutcome, TimelineMarkerRecord, TimelineMarkersQuery, TimelinePageQuery,
    TimelinePageRecord, TrashDeletionOutcome, UpdateAlbum, UpdateFaceRepresentative, UpdateUser,
    UpdateUserOutcome, UserAlbum, UserAuthIdentifier, UserForAuthentication, UserForToken,
    UserRecord,
};
use crate::database::DbPool;
use crate::io::file::JournalMutationTicket;
use crate::io::journal::{
    DirectoryCopyConstruction, DirectoryCopyConstructionPlan, DirectoryCopyEntryCheckpoint,
    DirectoryCopyFinishedCheckpoint, FileOperationPlan, JournalCancellationOutcome,
    JournalCancellationStatus, JournalCheckpointOutcome, JournalEntryCheckpoint,
    JournalFailureStage, JournalMaintenanceOutcome, JournalMutationGrant, JournalRecoveryGroup,
    JournalRetryOutcome, PrepareJournalOutcome,
};
use crate::models::{
    AiFeatureActionResult, AiFeatureScheduleResponse, AiStatusResponse, AlbumDetailResponse,
    AlbumResponse, DeduplicateGroupsResponse, FaceGroupMediaResponse, FaceGroupsListResponse,
    FileOperationDetailResponse, FileOperationListResponse, MapClustersResponse,
    MapMediaListResponse, MediaResponse, ShareLinkResponse,
};
use crate::processor::ai::operation::AiFeature;
use crate::processor::face_detection::{FacePreparationContext, MergeFaceGroupsOutcome};
use crate::processor::import::{
    AbsorbExistingMediaDatabase, AllocateImportMedia, CreateImportJobOutcome, FinalizeImportMedia,
    ImportContentHashClaimOutcome, ImportSource, ImportStatusSnapshot, ImportTarget,
    InterruptedImport, UpdateWebdavReadyPaths, WebdavReadyFile,
};
use crate::runtime::scheduler::{SchedulerIngress, SubmissionMode};

const SQLITE_CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);
const SQLITE_OPERATION_TIMEOUT: Duration = Duration::from_secs(30);
const SQLITE_PROGRESS_HANDLER_OPS: i32 = 1_000;
const AUTH_SQLITE_MAX_GROWTH_BYTES: u64 = 1024 * 1024;
const BOUNDED_SQLITE_MAX_GROWTH_BYTES: u64 = 8 * 1024 * 1024;
const BULK_SQLITE_MAX_GROWTH_BYTES: u64 = 32 * 1024 * 1024;
const SCHEMA_SQLITE_MAX_GROWTH_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SqliteCapacitySource {
    ReadOnly,
    Fresh { max_growth_bytes: u64 },
    ProvisionalParent { max_growth_bytes: u64 },
    DurableParent { max_growth_bytes: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SqliteOperationSpec {
    resources: OperationSpec,
    capacity: SqliteCapacitySource,
}

impl SqliteOperationSpec {
    const fn read(resources: OperationSpec) -> Self {
        Self {
            resources,
            capacity: SqliteCapacitySource::ReadOnly,
        }
    }

    const fn fresh_write(resources: OperationSpec, max_growth_bytes: u64) -> Self {
        Self {
            resources,
            capacity: SqliteCapacitySource::Fresh { max_growth_bytes },
        }
    }
}

pub(crate) enum SqliteOperation {
    Probe {
        sequence: u64,
    },
    RegisterAuthAttempt(RegisterAuthAttempt),
    ClearAuthAttempts(ClearAuthAttempts),
    LoadUserForToken {
        user_id: i64,
    },
    LoadUserForAuthentication(UserAuthIdentifier),
    InsertRefreshToken(InsertRefreshToken),
    RotateRefreshToken(RotateRefreshToken),
    RevokeRefreshToken {
        token_hash: String,
    },
    LoadPasswordHash {
        user_id: i64,
    },
    ReplacePassword(ReplacePassword),
    LoadAdminId,
    InsertDefaultAdmin {
        password_hash: String,
    },
    PrepareAdminPasswordReset {
        admin_id: i64,
    },
    CleanupRefreshTokens,
    InitializeDatabase,
    CreateUser(CreateUser),
    ListUsers,
    LoadUserRecord {
        user_id: i64,
    },
    UpdateUser(UpdateUser),
    DeleteUser {
        user_id: i64,
    },
    LoadMapClusters(MapClustersQuery),
    LoadMapMedia(MapMediaQuery),
    LoadDuplicateGroups(DuplicateGroupsQuery),
    LoadPlaceCover(PlaceIdentityQuery),
    LoadPlacesPage(PlacePageQuery),
    LoadPlaceMediaPage(PlaceMediaQuery),
    CreateAlbum(CreateAlbum),
    ListAlbums {
        user_id: i64,
    },
    LoadAlbum(UserAlbum),
    UpdateAlbum(UpdateAlbum),
    DeleteAlbum(UserAlbum),
    AddAlbumMedia(AlbumMediaMutation),
    RemoveAlbumMedia(AlbumMediaMutation),
    ReorderAlbumMedia(AlbumMediaMutation),
    CreateShareLink(CreateShareLink),
    ListShareLinks {
        user_id: i64,
    },
    DeleteShareLink {
        user_id: i64,
        share_id: i64,
    },
    GrantShareAccess(GrantShareAccess),
    LoadActiveShare {
        token: String,
    },
    LoadPublicShareContent(ActiveShareRecord),
    LoadPublicSharedFile(PublicSharedMediaQuery),
    LoadPublicSharedThumbnail(PublicSharedMediaQuery),
    LoadMediaBatch(MediaBatchQuery),
    LoadTimelinePage(TimelinePageQuery),
    LoadTimelineMarkers(TimelineMarkersQuery),
    MoveMediaToTrash(MoveMediaToTrash),
    QueueIncompleteMetadata,
    ResetMetadataPage {
        cleanup_group_id: Option<String>,
    },
    LoadMetadataJobStatus,
    ClaimNextMetadataJob {
        claim_token: String,
    },
    LoadNextMetadataJobDelay,
    FinishMetadataJob(FinishMetadataJob),
    RecoverMetadataClaims,
    LoadMetadataGenerationMedia {
        media_id: i64,
    },
    PersistMetadataGeneration(Box<PersistMetadataGeneration>),
    PrepareLlmSubmissionCycle,
    ClaimLlmSubmissionJobs {
        limit: u16,
    },
    LoadNextLlmSubmissionDelay,
    LoadLlmPreparedInputs {
        job_id: String,
    },
    FinishLlmSubmission(FinishLlmSubmission),
    LoadLlmCancellationBatch {
        limit: u16,
    },
    AcknowledgeLlmCancellation(AcknowledgeLlmCancellation),
    PrepareLlmResultReceipt(PrepareLlmResultReceipt),
    CreateLlmResultReceipt(Box<CreateLlmResultReceipt>),
    CommitLlmResultReceipt(Box<CommitLlmResultReceipt>),
    StageLlmResultPage(Box<StageLlmResultPage>),
    SelectLlmResultStagingCleanup {
        limit: u16,
    },
    CleanupLlmResultStagingPage {
        job_id: String,
        limit: u16,
    },
    FinalizeLlmResultCleanup {
        job_id: String,
    },
    LoadLlmResultStagingPage {
        job_id: String,
        attempt: u32,
        after_record_sequence: Option<u32>,
        claim_token: String,
        limit: u16,
    },
    ReleaseLlmResultClaim {
        job_id: String,
        claim_token: String,
    },
    RecoverLlmResultState,
    RejectLlmResultReceipt(RejectLlmResultReceipt),
    LoadFacePreparationContext {
        job_id: String,
        media_id: i64,
    },
    SelectLlmResultCandidates {
        limit: u16,
    },
    PersistPreparedLlmResult(crate::processor::ai::result::PreparedQueuedResult),
    LoadMetadataAiInputVerification {
        media_id: i64,
    },
    LoadBinaryMedia(BinaryMediaQuery),
    PrepareMediaUpdate(PrepareMediaUpdate),
    FinalizeMediaUpdate(FinalizeMediaUpdate),
    LoadTrash {
        user_id: i64,
    },
    RestoreTrash(RestoreTrash),
    DeleteTrashMedia(DeleteTrashMedia),
    DeleteTrashPage(DeleteTrashPage),
    DeleteExpiredTrashPage(DeleteExpiredTrashPage),
    LoadFaceGroupsPage(FaceGroupsPageQuery),
    LoadFaceGroup(FaceGroupQuery),
    LoadVisibleFaceRepresentative {
        face_group_id: i64,
        user_id: i64,
        config: FaceGroupConfig,
    },
    MergeFaceGroups {
        group_ids: Vec<i64>,
        config: FaceGroupConfig,
    },
    LoadAiStatus {
        config: Box<Config>,
        schedules: Vec<AiFeatureScheduleResponse>,
    },
    StartAiFeature {
        feature: AiFeature,
        trigger: String,
        scheduled_for: Option<String>,
    },
    CancelAiFeature {
        feature: AiFeature,
    },
    CancelAllAiFeatures,
    CleanAiFeature {
        feature: AiFeature,
        cleanup_group_id: String,
    },
    LoadDeduplicateScheduleState,
    RecoverDeduplicateRuns,
    LoadDeduplicateFinalizationWork,
    CommitDeduplicateCpuResult(crate::processor::deduplicator::DeduplicateCpuResult),
    RecoverFaceGroupingRuns,
    LoadFaceGroupFinalizationWork(FaceGroupConfig),
    CommitFaceGroupCpuResult(crate::processor::face_detection::FaceGroupCpuResult),
    LoadFaceRepresentativeGroupPage(FaceRepresentativeGroupPageQuery),
    LoadFaceRepresentativeCandidatePage(FaceRepresentativeCandidatePageQuery),
    UpdateFaceRepresentative(UpdateFaceRepresentative),
    InvalidateWebdavReadiness(operations::InvalidateWebdavReadiness),
    MarkWebdavReady(operations::MarkWebdavReady),
    RegisterBackupDevice(operations::RegisterBackupDevice),
    CreateBackupUpload(operations::CreateBackupUpload),
    LoadBackupUpload(operations::LoadBackupUpload),
    PrepareBackupCompletion(operations::PrepareBackupCompletion),
    QueueBackupCompletion(operations::QueueBackupCompletion),
    ClaimBackupChunk(operations::ClaimBackupChunk),
    FinishBackupChunk(operations::FinishBackupChunk),
    AbandonBackupChunk(operations::AbandonBackupChunk),
    CancelBackupUpload(operations::CancelBackupUpload),
    RecoverBackupWritingSessions,
    LoadBackupResumablePage(operations::BackupRecoveryPageQuery),
    LoadBackupProcessingPage(operations::BackupRecoveryPageQuery),
    MaintainBackupSessions,
    ClaimBackupAsset,
    LoadRecoveredBackupMedia {
        content_hash: String,
        user_id: i64,
    },
    StoreBackupContentHash(operations::StoreBackupContentHash),
    TransitionBackupProcessing(operations::BackupProcessingTransition),
    CreateImportJob {
        source: ImportSource,
    },
    LoadImportStatus {
        source: ImportSource,
    },
    SetImportJobTotal {
        job_id: i64,
        total_files: i64,
    },
    RecordImportProgress {
        job_id: i64,
        success: bool,
        error_message: String,
    },
    CompleteImportJob {
        job_id: i64,
    },
    AllocateImportMedia(AllocateImportMedia),
    FinalizeImportMedia(FinalizeImportMedia),
    MarkImportMediaFailed {
        media_id: i64,
        error: String,
    },
    AbsorbExistingMedia(AbsorbExistingMediaDatabase),
    RecoverInterruptedImportPage {
        after_media_id: i64,
        limit: u16,
    },
    LoadWebdavReadyPage {
        after_user_id: i64,
        after_file_path: String,
        limit: u16,
    },
    CheckWebdavReady {
        user_id: i64,
        file_path: String,
    },
    UpdateWebdavReadyPaths(UpdateWebdavReadyPaths),
    AcquireImportContentHashClaim {
        content_hash: String,
        claim_token: String,
        source: ImportSource,
    },
    ReleaseImportContentHashClaim {
        content_hash: String,
        claim_token: String,
    },
    RecoverImportContentHashClaims,
    PrepareFileOperation(FileOperationPlan),
    PrepareDirectoryCopyOperation(Box<(FileOperationPlan, DirectoryCopyConstructionPlan)>),
    LoadDirectoryCopy {
        group_id: Option<String>,
    },
    CheckpointDirectoryCopyEntry(DirectoryCopyEntryCheckpoint),
    CheckpointDirectoryCopyFinished(DirectoryCopyFinishedCheckpoint),
    BeginFileOperationPublication {
        group_id: String,
        expected_version: i64,
    },
    VerifyFileOperationPublication {
        group_id: String,
        expected_version: i64,
    },
    RecordFileEntryPublished {
        group_id: String,
        expected_version: i64,
        sequence: u16,
    },
    CompleteFileOperation {
        group_id: String,
        expected_version: i64,
    },
    VerifyFileOperationCleanup {
        group_id: String,
        expected_version: i64,
    },
    RecordFileEntryCleaned {
        group_id: String,
        expected_version: i64,
        sequence: u16,
    },
    LoadNextGenericFileOperationRecovery,
    YieldFileOperationProgress {
        group_id: String,
        expected_version: i64,
    },
    RecordFileOperationFailure {
        group_id: String,
        expected_version: i64,
        sequence: u16,
        stage: JournalFailureStage,
        error_kind: String,
        error: String,
    },
    RecordFileOperationFinalizationFailure {
        group_id: String,
        expected_version: i64,
        error_kind: String,
        error: String,
    },
    RetryFileOperation {
        retry_request_id: String,
        group_id: String,
        expected_version: i64,
        request_hash: [u8; 32],
    },
    ListFileOperations {
        states: Vec<String>,
        cursor: Option<String>,
        limit: u16,
    },
    LoadFileOperationDetail {
        group_id: String,
    },
    MaintainFileOperationJournal,
    LoadFileOperationCancellationStatus {
        group_id: String,
    },
    RequestFileOperationCancellation {
        group_id: String,
        expected_version: i64,
    },
    VerifyFileOperationRollback {
        group_id: String,
        expected_version: i64,
    },
    RecordFileEntryRolledBack {
        group_id: String,
        expected_version: i64,
        sequence: u16,
    },
}

impl SqliteOperation {
    fn durable_parent_job_id(&self) -> Option<&str> {
        match self {
            Self::StageLlmResultPage(request) => Some(&request.job_id),
            Self::PersistPreparedLlmResult(prepared) => prepared.durable_parent_job_id(),
            Self::CleanupLlmResultStagingPage { job_id, .. } => Some(job_id),
            _ => None,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Probe { .. } => "sqlite_probe",
            Self::RegisterAuthAttempt(_) => "register_auth_attempt",
            Self::ClearAuthAttempts(_) => "clear_auth_attempts",
            Self::LoadUserForToken { .. } => "load_user_for_token",
            Self::LoadUserForAuthentication(_) => "load_user_for_authentication",
            Self::InsertRefreshToken(_) => "insert_refresh_token",
            Self::RotateRefreshToken(_) => "rotate_refresh_token",
            Self::RevokeRefreshToken { .. } => "revoke_refresh_token",
            Self::LoadPasswordHash { .. } => "load_password_hash",
            Self::ReplacePassword(_) => "replace_password",
            Self::LoadAdminId => "load_admin_id",
            Self::InsertDefaultAdmin { .. } => "insert_default_admin",
            Self::PrepareAdminPasswordReset { .. } => "prepare_admin_password_reset",
            Self::CleanupRefreshTokens => "cleanup_refresh_tokens",
            Self::InitializeDatabase => "initialize_database",
            Self::CreateUser(_) => "create_user",
            Self::ListUsers => "list_users",
            Self::LoadUserRecord { .. } => "load_user_record",
            Self::UpdateUser(_) => "update_user",
            Self::DeleteUser { .. } => "delete_user",
            Self::LoadMapClusters(_) => "load_map_clusters",
            Self::LoadMapMedia(_) => "load_map_media",
            Self::LoadDuplicateGroups(_) => "load_duplicate_groups",
            Self::LoadPlaceCover(_) => "load_place_cover",
            Self::LoadPlacesPage(_) => "load_places_page",
            Self::LoadPlaceMediaPage(_) => "load_place_media_page",
            Self::CreateAlbum(_) => "create_album",
            Self::ListAlbums { .. } => "list_albums",
            Self::LoadAlbum(_) => "load_album",
            Self::UpdateAlbum(_) => "update_album",
            Self::DeleteAlbum(_) => "delete_album",
            Self::AddAlbumMedia(_) => "add_album_media",
            Self::RemoveAlbumMedia(_) => "remove_album_media",
            Self::ReorderAlbumMedia(_) => "reorder_album_media",
            Self::CreateShareLink(_) => "create_share_link",
            Self::ListShareLinks { .. } => "list_share_links",
            Self::DeleteShareLink { .. } => "delete_share_link",
            Self::GrantShareAccess(_) => "grant_share_access",
            Self::LoadActiveShare { .. } => "load_active_share",
            Self::LoadPublicShareContent(_) => "load_public_share_content",
            Self::LoadPublicSharedFile(_) => "load_public_shared_file",
            Self::LoadPublicSharedThumbnail(_) => "load_public_shared_thumbnail",
            Self::LoadMediaBatch(_) => "load_media_batch",
            Self::LoadTimelinePage(_) => "load_timeline_page",
            Self::LoadTimelineMarkers(_) => "load_timeline_markers",
            Self::MoveMediaToTrash(_) => "move_media_to_trash",
            Self::QueueIncompleteMetadata => "queue_incomplete_metadata",
            Self::ResetMetadataPage { .. } => "reset_metadata_page",
            Self::LoadMetadataJobStatus => "load_metadata_job_status",
            Self::ClaimNextMetadataJob { .. } => "claim_next_metadata_job",
            Self::LoadNextMetadataJobDelay => "load_next_metadata_job_delay",
            Self::FinishMetadataJob(_) => "finish_metadata_job",
            Self::RecoverMetadataClaims => "recover_metadata_claims",
            Self::LoadMetadataGenerationMedia { .. } => "load_metadata_generation_media",
            Self::PersistMetadataGeneration(_) => "persist_metadata_generation",
            Self::PrepareLlmSubmissionCycle => "prepare_llm_submission_cycle",
            Self::ClaimLlmSubmissionJobs { .. } => "claim_llm_submission_jobs",
            Self::LoadNextLlmSubmissionDelay => "load_next_llm_submission_delay",
            Self::LoadLlmPreparedInputs { .. } => "load_llm_prepared_inputs",
            Self::FinishLlmSubmission(_) => "finish_llm_submission",
            Self::LoadLlmCancellationBatch { .. } => "load_llm_cancellation_batch",
            Self::AcknowledgeLlmCancellation(_) => "acknowledge_llm_cancellation",
            Self::PrepareLlmResultReceipt(_) => "prepare_llm_result_receipt",
            Self::CreateLlmResultReceipt(_) => "create_llm_result_receipt",
            Self::CommitLlmResultReceipt(_) => "commit_llm_result_receipt",
            Self::StageLlmResultPage(_) => "stage_llm_result_page",
            Self::SelectLlmResultStagingCleanup { .. } => "select_llm_result_staging_cleanup",
            Self::CleanupLlmResultStagingPage { .. } => "cleanup_llm_result_staging_page",
            Self::FinalizeLlmResultCleanup { .. } => "finalize_llm_result_cleanup",
            Self::LoadLlmResultStagingPage { .. } => "load_llm_result_staging_page",
            Self::ReleaseLlmResultClaim { .. } => "release_llm_result_claim",
            Self::RecoverLlmResultState => "recover_llm_result_state",
            Self::RejectLlmResultReceipt(_) => "reject_llm_result_receipt",
            Self::LoadFacePreparationContext { .. } => "load_face_preparation_context",
            Self::SelectLlmResultCandidates { .. } => "select_llm_result_candidates",
            Self::PersistPreparedLlmResult(_) => "persist_prepared_llm_result",
            Self::LoadMetadataAiInputVerification { .. } => "load_metadata_ai_input_verification",
            Self::LoadBinaryMedia(_) => "load_binary_media",
            Self::PrepareMediaUpdate(_) => "prepare_media_update",
            Self::FinalizeMediaUpdate(_) => "finalize_media_update",
            Self::LoadTrash { .. } => "load_trash",
            Self::RestoreTrash(_) => "restore_trash",
            Self::DeleteTrashMedia(_) => "delete_trash_media",
            Self::DeleteTrashPage(_) => "delete_trash_page",
            Self::DeleteExpiredTrashPage(_) => "delete_expired_trash_page",
            Self::LoadFaceGroupsPage(_) => "load_face_groups_page",
            Self::LoadFaceGroup(_) => "load_face_group",
            Self::LoadVisibleFaceRepresentative { .. } => "load_visible_face_representative",
            Self::MergeFaceGroups { .. } => "merge_face_groups",
            Self::LoadAiStatus { .. } => "load_ai_status",
            Self::StartAiFeature { .. } => "start_ai_feature",
            Self::CancelAiFeature { .. } => "cancel_ai_feature",
            Self::CancelAllAiFeatures => "cancel_all_ai_features",
            Self::CleanAiFeature { .. } => "clean_ai_feature",
            Self::LoadDeduplicateScheduleState => "load_deduplicate_schedule_state",
            Self::RecoverDeduplicateRuns => "recover_deduplicate_runs",
            Self::LoadDeduplicateFinalizationWork => "load_deduplicate_finalization_work",
            Self::CommitDeduplicateCpuResult(_) => "commit_deduplicate_cpu_result",
            Self::RecoverFaceGroupingRuns => "recover_face_grouping_runs",
            Self::LoadFaceGroupFinalizationWork(_) => "load_face_group_finalization_work",
            Self::CommitFaceGroupCpuResult(_) => "commit_face_group_cpu_result",
            Self::LoadFaceRepresentativeGroupPage(_) => "load_face_representative_group_page",
            Self::LoadFaceRepresentativeCandidatePage(_) => {
                "load_face_representative_candidate_page"
            }
            Self::UpdateFaceRepresentative(_) => "update_face_representative",
            Self::InvalidateWebdavReadiness(_) => "invalidate_webdav_readiness",
            Self::MarkWebdavReady(_) => "mark_webdav_ready",
            Self::RegisterBackupDevice(_) => "register_backup_device",
            Self::CreateBackupUpload(_) => "create_backup_upload",
            Self::LoadBackupUpload(_) => "load_backup_upload",
            Self::PrepareBackupCompletion(_) => "prepare_backup_completion",
            Self::QueueBackupCompletion(_) => "queue_backup_completion",
            Self::ClaimBackupChunk(_) => "claim_backup_chunk",
            Self::FinishBackupChunk(_) => "finish_backup_chunk",
            Self::AbandonBackupChunk(_) => "abandon_backup_chunk",
            Self::CancelBackupUpload(_) => "cancel_backup_upload",
            Self::RecoverBackupWritingSessions => "recover_backup_writing_sessions",
            Self::LoadBackupResumablePage(_) => "load_backup_resumable_page",
            Self::LoadBackupProcessingPage(_) => "load_backup_processing_page",
            Self::MaintainBackupSessions => "maintain_backup_sessions",
            Self::ClaimBackupAsset => "claim_backup_asset",
            Self::LoadRecoveredBackupMedia { .. } => "load_recovered_backup_media",
            Self::StoreBackupContentHash(_) => "store_backup_content_hash",
            Self::TransitionBackupProcessing(_) => "transition_backup_processing",
            Self::CreateImportJob { .. } => "create_import_job",
            Self::LoadImportStatus { .. } => "load_import_status",
            Self::SetImportJobTotal { .. } => "set_import_job_total",
            Self::RecordImportProgress { .. } => "record_import_progress",
            Self::CompleteImportJob { .. } => "complete_import_job",
            Self::AllocateImportMedia(_) => "allocate_import_media",
            Self::FinalizeImportMedia(_) => "finalize_import_media",
            Self::MarkImportMediaFailed { .. } => "mark_import_media_failed",
            Self::AbsorbExistingMedia(_) => "absorb_existing_media",
            Self::RecoverInterruptedImportPage { .. } => "recover_interrupted_import_page",
            Self::LoadWebdavReadyPage { .. } => "load_webdav_ready_page",
            Self::CheckWebdavReady { .. } => "check_webdav_ready",
            Self::UpdateWebdavReadyPaths(_) => "update_webdav_ready_paths",
            Self::AcquireImportContentHashClaim { .. } => "acquire_import_content_hash_claim",
            Self::ReleaseImportContentHashClaim { .. } => "release_import_content_hash_claim",
            Self::RecoverImportContentHashClaims => "recover_import_content_hash_claims",
            Self::PrepareFileOperation(_) => "prepare_file_operation",
            Self::PrepareDirectoryCopyOperation(_) => "prepare_directory_copy_operation",
            Self::LoadDirectoryCopy { .. } => "load_directory_copy",
            Self::CheckpointDirectoryCopyEntry(_) => "checkpoint_directory_copy_entry",
            Self::CheckpointDirectoryCopyFinished(_) => "checkpoint_directory_copy_finished",
            Self::BeginFileOperationPublication { .. } => "begin_file_operation_publication",
            Self::VerifyFileOperationPublication { .. } => "verify_file_operation_publication",
            Self::RecordFileEntryPublished { .. } => "record_file_entry_published",
            Self::CompleteFileOperation { .. } => "complete_file_operation",
            Self::VerifyFileOperationCleanup { .. } => "verify_file_operation_cleanup",
            Self::RecordFileEntryCleaned { .. } => "record_file_entry_cleaned",
            Self::LoadNextGenericFileOperationRecovery => {
                "load_next_generic_file_operation_recovery"
            }
            Self::YieldFileOperationProgress { .. } => "yield_file_operation_progress",
            Self::RecordFileOperationFailure { .. } => "record_file_operation_failure",
            Self::RecordFileOperationFinalizationFailure { .. } => {
                "record_file_operation_finalization_failure"
            }
            Self::RetryFileOperation { .. } => "retry_file_operation",
            Self::ListFileOperations { .. } => "list_file_operations",
            Self::LoadFileOperationDetail { .. } => "load_file_operation_detail",
            Self::MaintainFileOperationJournal => "maintain_file_operation_journal",
            Self::LoadFileOperationCancellationStatus { .. } => {
                "load_file_operation_cancellation_status"
            }
            Self::RequestFileOperationCancellation { .. } => "request_file_operation_cancellation",
            Self::VerifyFileOperationRollback { .. } => "verify_file_operation_rollback",
            Self::RecordFileEntryRolledBack { .. } => "record_file_entry_rolled_back",
        }
    }

    fn spec(
        &self,
        footprints: &crate::database::result_footprint::SqliteFootprintRegistry,
    ) -> Result<SqliteOperationSpec, ExecutorError> {
        Ok(match self {
            Self::Probe { .. } => SqliteOperationSpec::read(OperationSpec {
                domain: ExecutorDomain::Sqlite,
                maximum_input_bytes: size_of::<u64>(),
                maximum_output_bytes: MAX_PROBE_OUTPUT_BYTES,
                maximum_temporary_bytes: 0,
            }),
            Self::RegisterAuthAttempt(_) => SqliteOperationSpec::fresh_write(
                OperationSpec {
                    domain: ExecutorDomain::Sqlite,
                    maximum_input_bytes: size_of::<RegisterAuthAttempt>(),
                    maximum_output_bytes: size_of::<AuthAttemptDecision>(),
                    maximum_temporary_bytes: 256 * 128,
                },
                AUTH_SQLITE_MAX_GROWTH_BYTES,
            ),
            Self::ClearAuthAttempts(_) => SqliteOperationSpec::fresh_write(
                OperationSpec {
                    domain: ExecutorDomain::Sqlite,
                    maximum_input_bytes: size_of::<ClearAuthAttempts>(),
                    maximum_output_bytes: 0,
                    maximum_temporary_bytes: 1024,
                },
                AUTH_SQLITE_MAX_GROWTH_BYTES,
            ),
            Self::LoadUserForToken { .. } => SqliteOperationSpec::read(OperationSpec {
                domain: ExecutorDomain::Sqlite,
                maximum_input_bytes: size_of::<i64>(),
                maximum_output_bytes: 256 * 1024,
                maximum_temporary_bytes: 1024,
            }),
            Self::LoadUserForAuthentication(_) => user_read_spec(),
            Self::InsertRefreshToken(_) => auth_write_spec(size_of::<InsertRefreshToken>()),
            Self::RotateRefreshToken(_) => auth_write_spec(size_of::<RotateRefreshToken>()),
            Self::RevokeRefreshToken { .. } => auth_write_spec(256),
            Self::LoadPasswordHash { .. } => user_read_spec(),
            Self::ReplacePassword(_) => auth_write_spec(size_of::<ReplacePassword>()),
            Self::LoadAdminId => user_read_spec(),
            Self::InsertDefaultAdmin { .. } => auth_write_spec(512),
            Self::PrepareAdminPasswordReset { .. } => auth_write_spec(size_of::<i64>()),
            Self::CleanupRefreshTokens => auth_write_spec(0),
            Self::InitializeDatabase => SqliteOperationSpec::fresh_write(
                OperationSpec {
                    domain: ExecutorDomain::Sqlite,
                    maximum_input_bytes: 0,
                    maximum_output_bytes: 0,
                    maximum_temporary_bytes: 1024 * 1024,
                },
                SCHEMA_SQLITE_MAX_GROWTH_BYTES,
            ),
            Self::CreateUser(_) => auth_write_spec(size_of::<CreateUser>()),
            Self::ListUsers => SqliteOperationSpec::read(OperationSpec {
                domain: ExecutorDomain::Sqlite,
                maximum_input_bytes: 0,
                maximum_output_bytes: 1024 * 1024,
                maximum_temporary_bytes: 256 * 1024,
            }),
            Self::LoadUserRecord { .. } => user_read_spec(),
            Self::UpdateUser(_) => auth_write_spec(size_of::<UpdateUser>()),
            Self::DeleteUser { .. } => auth_write_spec(size_of::<i64>()),
            Self::LoadMapClusters(_)
            | Self::LoadMapMedia(_)
            | Self::LoadDuplicateGroups(_)
            | Self::LoadPlaceCover(_)
            | Self::LoadPlacesPage(_)
            | Self::LoadPlaceMediaPage(_)
            | Self::ListAlbums { .. }
            | Self::LoadAlbum(_)
            | Self::ListShareLinks { .. }
            | Self::LoadActiveShare { .. }
            | Self::LoadPublicSharedFile(_)
            | Self::LoadPublicSharedThumbnail(_)
            | Self::LoadMediaBatch(_)
            | Self::LoadTimelinePage(_)
            | Self::LoadTimelineMarkers(_)
            | Self::LoadMetadataJobStatus
            | Self::LoadMetadataGenerationMedia { .. }
            | Self::LoadLlmPreparedInputs { .. }
            | Self::LoadLlmCancellationBatch { .. }
            | Self::LoadMetadataAiInputVerification { .. }
            | Self::LoadBinaryMedia(_)
            | Self::PrepareMediaUpdate(_) => bounded_api_read_spec(),
            Self::VerifyFileOperationPublication { .. }
            | Self::VerifyFileOperationCleanup { .. }
            | Self::LoadNextGenericFileOperationRecovery
            | Self::ListFileOperations { .. }
            | Self::LoadFileOperationDetail { .. }
            | Self::VerifyFileOperationRollback { .. } => bounded_api_read_spec(),
            Self::LoadDirectoryCopy { .. } => bounded_api_read_spec(),
            Self::LoadTrash { .. }
            | Self::LoadFaceGroupsPage(_)
            | Self::LoadFaceGroup(_)
            | Self::LoadVisibleFaceRepresentative { .. }
            | Self::LoadAiStatus { .. }
            | Self::LoadFaceRepresentativeGroupPage(_)
            | Self::LoadFaceRepresentativeCandidatePage(_) => bounded_api_read_spec(),
            Self::LoadDeduplicateScheduleState => bounded_api_read_spec(),
            Self::RecoverDeduplicateRuns
            | Self::LoadDeduplicateFinalizationWork
            | Self::CommitDeduplicateCpuResult(_)
            | Self::RecoverFaceGroupingRuns
            | Self::LoadFaceGroupFinalizationWork(_)
            | Self::CommitFaceGroupCpuResult(_)
            | Self::UpdateFaceRepresentative(_) => bounded_api_write_spec(),
            Self::LoadImportStatus { .. }
            | Self::LoadWebdavReadyPage { .. }
            | Self::CheckWebdavReady { .. } => bounded_api_read_spec(),
            Self::CreateAlbum(_)
            | Self::UpdateAlbum(_)
            | Self::DeleteAlbum(_)
            | Self::AddAlbumMedia(_)
            | Self::RemoveAlbumMedia(_)
            | Self::ReorderAlbumMedia(_)
            | Self::CreateShareLink(_)
            | Self::DeleteShareLink { .. }
            | Self::GrantShareAccess(_)
            | Self::LoadPublicShareContent(_)
            | Self::MoveMediaToTrash(_)
            | Self::QueueIncompleteMetadata
            | Self::ResetMetadataPage { .. }
            | Self::ClaimNextMetadataJob { .. }
            | Self::FinishMetadataJob(_)
            | Self::RecoverMetadataClaims
            | Self::PrepareLlmSubmissionCycle
            | Self::ClaimLlmSubmissionJobs { .. }
            | Self::FinishLlmSubmission(_)
            | Self::AcknowledgeLlmCancellation(_)
            | Self::PrepareLlmResultReceipt(_)
            | Self::CommitLlmResultReceipt(_)
            | Self::FinalizeMediaUpdate(_) => bounded_api_write_spec(),
            Self::LoadNextMetadataJobDelay | Self::LoadNextLlmSubmissionDelay => {
                bounded_api_read_spec()
            }
            Self::RejectLlmResultReceipt(_) => SqliteOperationSpec::fresh_write(
                bounded_api_write_spec().resources,
                footprints.result_rejection_max_growth_bytes,
            ),
            Self::PersistMetadataGeneration(_) => SqliteOperationSpec::fresh_write(
                OperationSpec {
                    domain: ExecutorDomain::Sqlite,
                    maximum_input_bytes: 4 * 1024 * 1024,
                    maximum_output_bytes: 0,
                    maximum_temporary_bytes: 1024 * 1024,
                },
                BULK_SQLITE_MAX_GROWTH_BYTES,
            ),
            Self::CreateLlmResultReceipt(request) => {
                let footprint = footprints
                    .result(&request.task, request.record_count, request.byte_size)
                    .map_err(|error| {
                        ExecutorError::new(
                            ExecutorErrorKind::InvalidInput,
                            "create_llm_result_receipt",
                            error.to_string(),
                        )
                    })?;
                SqliteOperationSpec {
                    resources: bounded_api_write_spec().resources,
                    capacity: SqliteCapacitySource::ProvisionalParent {
                        max_growth_bytes: footprint.construction_max_growth_bytes,
                    },
                }
            }
            Self::StageLlmResultPage(request) => {
                let payload_bytes = request
                    .records
                    .iter()
                    .try_fold(0_u64, |total, record| {
                        total.checked_add(record.normalized_payload.len() as u64)
                    })
                    .ok_or_else(|| {
                        ExecutorError::new(
                            ExecutorErrorKind::InvalidInput,
                            "stage_llm_result_page",
                            "LLM result staging footprint overflowed",
                        )
                    })?;
                let max_growth_bytes = footprints
                    .staging_page(request.records.len(), payload_bytes)
                    .map_err(|error| {
                        ExecutorError::new(
                            ExecutorErrorKind::InvalidInput,
                            "stage_llm_result_page",
                            error.to_string(),
                        )
                    })?;
                SqliteOperationSpec {
                    resources: OperationSpec {
                        domain: ExecutorDomain::Sqlite,
                        maximum_input_bytes: 4 * 1024 * 1024 + 256 * 1024,
                        maximum_output_bytes: size_of::<StageLlmResultPageOutcome>(),
                        maximum_temporary_bytes: 4 * 1024 * 1024,
                    },
                    capacity: SqliteCapacitySource::DurableParent { max_growth_bytes },
                }
            }
            Self::SelectLlmResultStagingCleanup { .. } => bounded_api_read_spec(),
            Self::CleanupLlmResultStagingPage { .. } => SqliteOperationSpec {
                resources: bounded_api_write_spec().resources,
                capacity: SqliteCapacitySource::DurableParent {
                    max_growth_bytes: footprints.result_cleanup_recovery_max_growth_bytes,
                },
            },
            Self::FinalizeLlmResultCleanup { .. } => bounded_api_write_spec(),
            Self::LoadLlmResultStagingPage { .. } => bounded_api_read_spec(),
            Self::ReleaseLlmResultClaim { .. } => bounded_api_write_spec(),
            Self::RecoverLlmResultState => bounded_api_write_spec(),
            Self::LoadFacePreparationContext { .. } => bounded_api_read_spec(),
            Self::SelectLlmResultCandidates { .. } => bounded_api_write_spec(),
            Self::PersistPreparedLlmResult(prepared) => {
                match prepared
                    .durable_sqlite_growth_bound(footprints)
                    .map_err(|error| {
                        ExecutorError::new(
                            ExecutorErrorKind::InvalidInput,
                            "persist_prepared_llm_result",
                            error.to_string(),
                        )
                    })? {
                    Some(max_growth_bytes) => SqliteOperationSpec {
                        resources: bounded_api_write_spec().resources,
                        capacity: SqliteCapacitySource::DurableParent { max_growth_bytes },
                    },
                    None => bounded_api_write_spec(),
                }
            }
            Self::RestoreTrash(_)
            | Self::DeleteTrashMedia(_)
            | Self::DeleteTrashPage(_)
            | Self::DeleteExpiredTrashPage(_)
            | Self::MergeFaceGroups { .. }
            | Self::StartAiFeature { .. }
            | Self::CancelAiFeature { .. }
            | Self::CancelAllAiFeatures
            | Self::CleanAiFeature { .. }
            | Self::InvalidateWebdavReadiness(_)
            | Self::MarkWebdavReady(_)
            | Self::RegisterBackupDevice(_)
            | Self::QueueBackupCompletion(_)
            | Self::ClaimBackupChunk(_)
            | Self::FinishBackupChunk(_)
            | Self::AbandonBackupChunk(_)
            | Self::CancelBackupUpload(_) => bounded_api_write_spec(),
            Self::CreateBackupUpload(_) => SqliteOperationSpec::fresh_write(
                OperationSpec {
                    domain: ExecutorDomain::Sqlite,
                    maximum_input_bytes: 2 * 1024 * 1024,
                    maximum_output_bytes: 1024 * 1024,
                    maximum_temporary_bytes: 1024 * 1024,
                },
                BULK_SQLITE_MAX_GROWTH_BYTES,
            ),
            Self::LoadBackupUpload(_)
            | Self::PrepareBackupCompletion(_)
            | Self::LoadBackupResumablePage(_)
            | Self::LoadBackupProcessingPage(_)
            | Self::LoadRecoveredBackupMedia { .. } => bounded_api_read_spec(),
            Self::RecoverBackupWritingSessions
            | Self::MaintainBackupSessions
            | Self::ClaimBackupAsset
            | Self::StoreBackupContentHash(_)
            | Self::TransitionBackupProcessing(_) => bounded_api_write_spec(),
            Self::CreateImportJob { .. }
            | Self::SetImportJobTotal { .. }
            | Self::RecordImportProgress { .. }
            | Self::CompleteImportJob { .. }
            | Self::AllocateImportMedia(_)
            | Self::FinalizeImportMedia(_)
            | Self::MarkImportMediaFailed { .. }
            | Self::AbsorbExistingMedia(_)
            | Self::RecoverInterruptedImportPage { .. }
            | Self::UpdateWebdavReadyPaths(_)
            | Self::AcquireImportContentHashClaim { .. }
            | Self::ReleaseImportContentHashClaim { .. }
            | Self::RecoverImportContentHashClaims
            | Self::PrepareFileOperation(_)
            | Self::PrepareDirectoryCopyOperation(_)
            | Self::CheckpointDirectoryCopyEntry(_)
            | Self::CheckpointDirectoryCopyFinished(_)
            | Self::BeginFileOperationPublication { .. }
            | Self::RecordFileEntryPublished { .. }
            | Self::CompleteFileOperation { .. }
            | Self::RecordFileEntryCleaned { .. }
            | Self::RecordFileOperationFailure { .. }
            | Self::RecordFileOperationFinalizationFailure { .. } => bounded_api_write_spec(),
            Self::YieldFileOperationProgress { .. } => bounded_api_write_spec(),
            Self::RetryFileOperation { .. } => bounded_api_write_spec(),
            Self::LoadFileOperationCancellationStatus { .. } => bounded_api_read_spec(),
            Self::MaintainFileOperationJournal
            | Self::RequestFileOperationCancellation { .. }
            | Self::RecordFileEntryRolledBack { .. } => bounded_api_write_spec(),
        })
    }
}

pub(crate) enum SqliteOutput {
    Probe { sequence: u64, thread_name: String },
    AuthAttempt(AuthAttemptDecision),
    AuthAttemptsCleared,
    UserForToken(Option<UserForToken>),
    UserForAuthentication(Option<UserForAuthentication>),
    RefreshTokenInserted,
    RefreshTokenRotated(Option<RotatedRefreshIdentity>),
    RefreshTokenRevoked,
    PasswordHash(Option<String>),
    PasswordReplaced(bool),
    AdminId(Option<i64>),
    DefaultAdminInserted(i64),
    AdminPasswordResetPrepared(bool),
    RefreshTokensCleaned(usize),
    DatabaseInitialized,
    UserCreated(CreateUserOutcome),
    Users(Vec<UserRecord>),
    UserRecord(Option<UserRecord>),
    UserUpdated(UpdateUserOutcome),
    UserDeleted(DeleteUserOutcome),
    MapClusters(MapClustersResponse),
    MapMedia(MapMediaListResponse),
    DuplicateGroups(DeduplicateGroupsResponse),
    PlaceCover(Option<PlaceCoverRecord>),
    Places(Vec<PlaceRecord>),
    PlaceMediaPage(Option<PlaceMediaPage>),
    AlbumCreated(AlbumDetailResponse),
    Albums(Vec<AlbumResponse>),
    Album(AlbumDetailOutcome),
    AlbumUpdated(AlbumUpdateOutcome),
    AlbumMutated(AlbumMutationOutcome),
    ShareLinkCreated(CreateShareLinkOutcome),
    ShareLinks(Vec<ShareLinkResponse>),
    ShareLinkDeleted(DeleteShareLinkOutcome),
    ShareAccessGranted(GrantShareAccessOutcome),
    ActiveShare(Option<ActiveShareRecord>),
    PublicShareContent(PublicShareContent),
    PublicSharedFile(PublicFileAccessOutcome),
    PublicSharedThumbnail(PublicThumbnailAccessOutcome),
    MediaBatch(Vec<MediaResponse>),
    TimelinePage(TimelinePageRecord),
    TimelineMarkers(Vec<TimelineMarkerRecord>),
    MediaMovedToTrash(usize),
    IncompleteMetadataQueued(usize),
    MetadataResetStep(operations::ResetMetadataStepOutcome),
    MetadataJobStatus(MetadataJobStatus),
    MetadataJobClaimed(Option<MetadataJobClaim>),
    NextMetadataJobDelay(Option<u64>),
    MetadataJobFinished,
    MetadataClaimsRecovered(usize),
    MetadataGenerationMedia(MetadataGenerationMedia),
    MetadataGenerationPersisted,
    LlmSubmissionCyclePrepared,
    LlmSubmissionJobs(Vec<LlmSubmissionJob>),
    NextLlmSubmissionDelay(Option<u64>),
    LlmPreparedInputs(Vec<LlmPreparedInput>),
    LlmSubmissionFinished,
    LlmCancellationBatch(Option<LlmCancellationBatch>),
    LlmCancellationAcknowledged,
    LlmResultReceiptPrepared(operations::LlmResultReceiptPreparation),
    LlmResultReceiptCreated(CreateLlmResultReceiptOutcome),
    LlmResultReceiptCommitted(LlmResultReceiptOutcome),
    LlmResultPageStaged(StageLlmResultPageOutcome),
    LlmResultStagingCleanup(Vec<String>),
    LlmResultStagingCleaned(CleanupLlmResultStagingOutcome),
    LlmResultCleanupFinalized(bool),
    LlmResultStagingPage(Vec<crate::database::operations::StagedLlmResultRecord>),
    LlmResultClaimReleased(bool),
    LlmResultStateRecovered(operations::LlmResultRecoveryOutcome),
    LlmResultReceiptRejected(operations::LlmResultReceiptRejection),
    FacePreparationContext(FacePreparationContext),
    LlmResultCandidates(Vec<crate::processor::ai::result::QueuedResult>),
    PreparedLlmResultPersisted(Vec<crate::io::file::NormalizedStoragePath>),
    MetadataAiInputVerification(MetadataAiInputVerification),
    BinaryMedia(Option<BinaryMediaRecord>),
    MediaUpdatePrepared(Option<EditableMediaState>),
    MediaUpdateFinalized(Box<Option<MediaResponse>>),
    Trash(Vec<crate::models::TrashMediaResponse>),
    TrashRestored(usize),
    TrashDeleted(TrashDeletionOutcome),
    FaceGroupsPage(FaceGroupsListResponse),
    FaceGroup(Option<FaceGroupMediaResponse>),
    VisibleFaceRepresentative(Option<String>),
    FaceGroupsMerged(MergeFaceGroupsOutcome),
    AiStatus(Box<AiStatusResponse>),
    AiFeatureStarted(usize),
    AiFeatureCancelled(AiFeatureActionResult),
    AllAiFeaturesCancelled(Vec<AiFeatureActionResult>),
    AiFeatureCleaned(crate::processor::ai::operation::AiFeatureCleanOutcome),
    DeduplicateScheduleState(operations::DeduplicateScheduleState),
    DeduplicateRunsRecovered,
    DeduplicateFinalizationWork(crate::processor::deduplicator::DeduplicateFinalizationWork),
    DeduplicateCpuResultCommitted,
    FaceGroupingRunsRecovered,
    FaceGroupFinalizationWork(crate::processor::face_detection::FaceGroupFinalizationWork),
    FaceGroupCpuResultCommitted,
    FaceRepresentativeGroupPage(FaceRepresentativeGroupPage),
    FaceRepresentativeCandidatePage(FaceRepresentativeCandidatePage),
    FaceRepresentativeUpdated,
    WebdavReadinessInvalidated,
    WebdavReadyMarked,
    BackupDeviceRegistered,
    BackupUploadCreated(operations::CreateBackupUploadOutcome),
    BackupUpload(Option<crate::models::BackupUploadResponse>),
    BackupCompletionPrepared(operations::PrepareBackupCompletionOutcome),
    BackupCompletionQueued(operations::QueueBackupCompletionOutcome),
    BackupChunkClaimed(operations::ClaimBackupChunkOutcome),
    BackupChunkFinished(operations::FinishBackupChunkOutcome),
    BackupChunkAbandoned,
    BackupUploadCancelled(operations::CancelBackupUploadOutcome),
    BackupWritingSessionsRecovered(usize),
    BackupResumablePage(operations::BackupRecoveryPage<operations::BackupResumableFile>),
    BackupProcessingPage(operations::BackupRecoveryPage<operations::BackupProcessingAsset>),
    BackupSessionsMaintained(operations::BackupSessionMaintenance),
    BackupAssetClaimed(Option<operations::ClaimedBackupAsset>),
    RecoveredBackupMedia(Option<i64>),
    BackupContentHashStored(bool),
    BackupProcessingTransitioned(operations::BackupProcessingTransitionOutcome),
    ImportJobCreated(CreateImportJobOutcome),
    ImportStatus(ImportStatusSnapshot),
    ImportJobTotalSet(bool),
    ImportProgressRecorded(bool),
    ImportJobCompleted(bool),
    ImportMediaAllocated(ImportTarget),
    ImportMediaFinalized(bool),
    ImportMediaFailed(bool),
    ExistingMediaAbsorbed,
    InterruptedImportPage(Vec<InterruptedImport>),
    WebdavReadyPage(Vec<WebdavReadyFile>),
    WebdavReadyChecked(bool),
    WebdavReadyPathsUpdated,
    ImportContentHashClaimed(ImportContentHashClaimOutcome),
    ImportContentHashClaimReleased(bool),
    ImportContentHashClaimsRecovered(usize),
    FileOperationPrepared(PrepareJournalOutcome),
    DirectoryCopyPrepared(PrepareJournalOutcome),
    NextDirectoryCopy(Option<DirectoryCopyConstruction>),
    DirectoryCopyEntryCheckpointed(bool),
    DirectoryCopyFinishedCheckpointed(bool),
    FileOperationPublicationBegun(Option<JournalMutationGrant>),
    FileOperationPublicationVerified(Option<JournalMutationGrant>),
    FileEntryPublished(Option<JournalEntryCheckpoint>),
    FileOperationCompleted(JournalCheckpointOutcome),
    FileOperationCleanupVerified(Option<JournalMutationGrant>),
    FileEntryCleaned(Option<JournalEntryCheckpoint>),
    GenericFileOperationRecovery(Option<JournalRecoveryGroup>),
    FileOperationProgressYielded(JournalCheckpointOutcome),
    FileOperationFailureRecorded(JournalCheckpointOutcome),
    FileOperationRetried(JournalRetryOutcome),
    FileOperationsListed(FileOperationListResponse),
    FileOperationDetail(Box<Option<FileOperationDetailResponse>>),
    FileOperationJournalMaintained(JournalMaintenanceOutcome),
    FileOperationCancellationStatus(Option<JournalCancellationStatus>),
    FileOperationCancellationRequested(JournalCancellationOutcome),
    FileOperationRollbackVerified(Option<JournalMutationGrant>),
    FileEntryRolledBack(Option<JournalEntryCheckpoint>),
}

impl SqliteOutput {
    fn mismatch(self, operation: &'static str) -> ExecutorError {
        let actual = match self {
            Self::Probe { .. } => "probe",
            Self::AuthAttempt(_) => "auth_attempt",
            Self::AuthAttemptsCleared => "auth_attempts_cleared",
            Self::UserForToken(_) => "user_for_token",
            Self::UserForAuthentication(_) => "user_for_authentication",
            Self::RefreshTokenInserted => "refresh_token_inserted",
            Self::RefreshTokenRotated(_) => "refresh_token_rotated",
            Self::RefreshTokenRevoked => "refresh_token_revoked",
            Self::PasswordHash(_) => "password_hash",
            Self::PasswordReplaced(_) => "password_replaced",
            Self::AdminId(_) => "admin_id",
            Self::DefaultAdminInserted(_) => "default_admin_inserted",
            Self::AdminPasswordResetPrepared(_) => "admin_password_reset_prepared",
            Self::RefreshTokensCleaned(_) => "refresh_tokens_cleaned",
            Self::DatabaseInitialized => "database_initialized",
            Self::UserCreated(_) => "user_created",
            Self::Users(_) => "users",
            Self::UserRecord(_) => "user_record",
            Self::UserUpdated(_) => "user_updated",
            Self::UserDeleted(_) => "user_deleted",
            Self::MapClusters(_) => "map_clusters",
            Self::MapMedia(_) => "map_media",
            Self::DuplicateGroups(_) => "duplicate_groups",
            Self::PlaceCover(_) => "place_cover",
            Self::Places(_) => "places",
            Self::PlaceMediaPage(_) => "place_media_page",
            Self::AlbumCreated(_) => "album_created",
            Self::Albums(_) => "albums",
            Self::Album(_) => "album",
            Self::AlbumUpdated(_) => "album_updated",
            Self::AlbumMutated(_) => "album_mutated",
            Self::ShareLinkCreated(_) => "share_link_created",
            Self::ShareLinks(_) => "share_links",
            Self::ShareLinkDeleted(_) => "share_link_deleted",
            Self::ShareAccessGranted(_) => "share_access_granted",
            Self::ActiveShare(_) => "active_share",
            Self::PublicShareContent(_) => "public_share_content",
            Self::PublicSharedFile(_) => "public_shared_file",
            Self::PublicSharedThumbnail(_) => "public_shared_thumbnail",
            Self::MediaBatch(_) => "media_batch",
            Self::TimelinePage(_) => "timeline_page",
            Self::TimelineMarkers(_) => "timeline_markers",
            Self::MediaMovedToTrash(_) => "media_moved_to_trash",
            Self::IncompleteMetadataQueued(_) => "incomplete_metadata_queued",
            Self::MetadataResetStep(_) => "metadata_reset_step",
            Self::MetadataJobStatus(_) => "metadata_job_status",
            Self::MetadataJobClaimed(_) => "metadata_job_claimed",
            Self::NextMetadataJobDelay(_) => "next_metadata_job_delay",
            Self::MetadataJobFinished => "metadata_job_finished",
            Self::MetadataClaimsRecovered(_) => "metadata_claims_recovered",
            Self::MetadataGenerationMedia(_) => "metadata_generation_media",
            Self::MetadataGenerationPersisted => "metadata_generation_persisted",
            Self::LlmSubmissionCyclePrepared => "llm_submission_cycle_prepared",
            Self::LlmSubmissionJobs(_) => "llm_submission_jobs",
            Self::NextLlmSubmissionDelay(_) => "next_llm_submission_delay",
            Self::LlmPreparedInputs(_) => "llm_prepared_inputs",
            Self::LlmSubmissionFinished => "llm_submission_finished",
            Self::LlmCancellationBatch(_) => "llm_cancellation_batch",
            Self::LlmCancellationAcknowledged => "llm_cancellation_acknowledged",
            Self::LlmResultReceiptPrepared(_) => "llm_result_receipt_prepared",
            Self::LlmResultReceiptCreated(_) => "llm_result_receipt_created",
            Self::LlmResultReceiptCommitted(_) => "llm_result_receipt_committed",
            Self::LlmResultPageStaged(_) => "llm_result_page_staged",
            Self::LlmResultStagingCleanup(_) => "llm_result_staging_cleanup",
            Self::LlmResultStagingCleaned(_) => "llm_result_staging_cleaned",
            Self::LlmResultCleanupFinalized(_) => "llm_result_cleanup_finalized",
            Self::LlmResultStagingPage(_) => "llm_result_staging_page",
            Self::LlmResultClaimReleased(_) => "llm_result_claim_released",
            Self::LlmResultStateRecovered(_) => "llm_result_state_recovered",
            Self::LlmResultReceiptRejected(_) => "llm_result_receipt_rejected",
            Self::FacePreparationContext(_) => "face_preparation_context",
            Self::LlmResultCandidates(_) => "llm_result_candidates",
            Self::PreparedLlmResultPersisted(_) => "prepared_llm_result_persisted",
            Self::MetadataAiInputVerification(_) => "metadata_ai_input_verification",
            Self::BinaryMedia(_) => "binary_media",
            Self::MediaUpdatePrepared(_) => "media_update_prepared",
            Self::MediaUpdateFinalized(_) => "media_update_finalized",
            Self::Trash(_) => "trash",
            Self::TrashRestored(_) => "trash_restored",
            Self::TrashDeleted(_) => "trash_deleted",
            Self::FaceGroupsPage(_) => "face_groups_page",
            Self::FaceGroup(_) => "face_group",
            Self::VisibleFaceRepresentative(_) => "visible_face_representative",
            Self::FaceGroupsMerged(_) => "face_groups_merged",
            Self::AiStatus(_) => "ai_status",
            Self::AiFeatureStarted(_) => "ai_feature_started",
            Self::AiFeatureCancelled(_) => "ai_feature_cancelled",
            Self::AllAiFeaturesCancelled(_) => "all_ai_features_cancelled",
            Self::AiFeatureCleaned(_) => "ai_feature_cleaned",
            Self::DeduplicateScheduleState(_) => "deduplicate_schedule_state",
            Self::DeduplicateRunsRecovered => "deduplicate_runs_recovered",
            Self::DeduplicateFinalizationWork(_) => "deduplicate_finalization_work",
            Self::DeduplicateCpuResultCommitted => "deduplicate_cpu_result_committed",
            Self::FaceGroupingRunsRecovered => "face_grouping_runs_recovered",
            Self::FaceGroupFinalizationWork(_) => "face_group_finalization_work",
            Self::FaceGroupCpuResultCommitted => "face_group_cpu_result_committed",
            Self::FaceRepresentativeGroupPage(_) => "face_representative_group_page",
            Self::FaceRepresentativeCandidatePage(_) => "face_representative_candidate_page",
            Self::FaceRepresentativeUpdated => "face_representative_updated",
            Self::WebdavReadinessInvalidated => "webdav_readiness_invalidated",
            Self::WebdavReadyMarked => "webdav_ready_marked",
            Self::BackupDeviceRegistered => "backup_device_registered",
            Self::BackupUploadCreated(_) => "backup_upload_created",
            Self::BackupUpload(_) => "backup_upload",
            Self::BackupCompletionPrepared(_) => "backup_completion_prepared",
            Self::BackupCompletionQueued(_) => "backup_completion_queued",
            Self::BackupChunkClaimed(_) => "backup_chunk_claimed",
            Self::BackupChunkFinished(_) => "backup_chunk_finished",
            Self::BackupChunkAbandoned => "backup_chunk_abandoned",
            Self::BackupUploadCancelled(_) => "backup_upload_cancelled",
            Self::BackupWritingSessionsRecovered(_) => "backup_writing_sessions_recovered",
            Self::BackupResumablePage(_) => "backup_resumable_page",
            Self::BackupProcessingPage(_) => "backup_processing_page",
            Self::BackupSessionsMaintained(_) => "backup_sessions_maintained",
            Self::BackupAssetClaimed(_) => "backup_asset_claimed",
            Self::RecoveredBackupMedia(_) => "recovered_backup_media",
            Self::BackupContentHashStored(_) => "backup_content_hash_stored",
            Self::BackupProcessingTransitioned(_) => "backup_processing_transitioned",
            Self::ImportJobCreated(_) => "import_job_created",
            Self::ImportStatus(_) => "import_status",
            Self::ImportJobTotalSet(_) => "import_job_total_set",
            Self::ImportProgressRecorded(_) => "import_progress_recorded",
            Self::ImportJobCompleted(_) => "import_job_completed",
            Self::ImportMediaAllocated(_) => "import_media_allocated",
            Self::ImportMediaFinalized(_) => "import_media_finalized",
            Self::ImportMediaFailed(_) => "import_media_failed",
            Self::ExistingMediaAbsorbed => "existing_media_absorbed",
            Self::InterruptedImportPage(_) => "interrupted_import_page",
            Self::WebdavReadyPage(_) => "webdav_ready_page",
            Self::WebdavReadyChecked(_) => "webdav_ready_checked",
            Self::WebdavReadyPathsUpdated => "webdav_ready_paths_updated",
            Self::ImportContentHashClaimed(_) => "import_content_hash_claimed",
            Self::ImportContentHashClaimReleased(_) => "import_content_hash_claim_released",
            Self::ImportContentHashClaimsRecovered(_) => "import_content_hash_claims_recovered",
            Self::FileOperationPrepared(_) => "file_operation_prepared",
            Self::DirectoryCopyPrepared(_) => "directory_copy_prepared",
            Self::NextDirectoryCopy(_) => "next_directory_copy",
            Self::DirectoryCopyEntryCheckpointed(_) => "directory_copy_entry_checkpointed",
            Self::DirectoryCopyFinishedCheckpointed(_) => "directory_copy_finished_checkpointed",
            Self::FileOperationPublicationBegun(_) => "file_operation_publication_begun",
            Self::FileOperationPublicationVerified(_) => "file_operation_publication_verified",
            Self::FileEntryPublished(_) => "file_entry_published",
            Self::FileOperationCompleted(_) => "file_operation_completed",
            Self::FileOperationCleanupVerified(_) => "file_operation_cleanup_verified",
            Self::FileEntryCleaned(_) => "file_entry_cleaned",
            Self::GenericFileOperationRecovery(_) => "generic_file_operation_recovery",
            Self::FileOperationProgressYielded(_) => "file_operation_progress_yielded",
            Self::FileOperationFailureRecorded(_) => "file_operation_failure_recorded",
            Self::FileOperationRetried(_) => "file_operation_retried",
            Self::FileOperationsListed(_) => "file_operations_listed",
            Self::FileOperationDetail(_) => "file_operation_detail",
            Self::FileOperationJournalMaintained(_) => "file_operation_journal_maintained",
            Self::FileOperationCancellationStatus(_) => "file_operation_cancellation_status",
            Self::FileOperationCancellationRequested(_) => "file_operation_cancellation_requested",
            Self::FileOperationRollbackVerified(_) => "file_operation_rollback_verified",
            Self::FileEntryRolledBack(_) => "file_entry_rolled_back",
        };
        ExecutorError::new(
            ExecutorErrorKind::Internal,
            operation,
            format!("SQLite executor returned mismatched output {actual}"),
        )
    }
}

fn user_read_spec() -> SqliteOperationSpec {
    SqliteOperationSpec::read(OperationSpec {
        domain: ExecutorDomain::Sqlite,
        maximum_input_bytes: 1024,
        maximum_output_bytes: 256 * 1024,
        maximum_temporary_bytes: 1024,
    })
}

fn auth_write_spec(maximum_input_bytes: usize) -> SqliteOperationSpec {
    SqliteOperationSpec::fresh_write(
        OperationSpec {
            domain: ExecutorDomain::Sqlite,
            maximum_input_bytes,
            maximum_output_bytes: 256 * 1024,
            maximum_temporary_bytes: 8 * 1024,
        },
        AUTH_SQLITE_MAX_GROWTH_BYTES,
    )
}

fn validate_backup_recovery_page(
    request: &operations::BackupRecoveryPageQuery,
    operation: &'static str,
) -> Result<(), ExecutorError> {
    if request.after_id < 0 || !(1..=256).contains(&request.limit) {
        return Err(ExecutorError::new(
            ExecutorErrorKind::InvalidInput,
            operation,
            "backup recovery page is outside its bounded range",
        ));
    }
    Ok(())
}

fn validate_metadata_generation_write(
    request: &PersistMetadataGeneration,
) -> Result<(), ExecutorError> {
    const OPERATION: &str = "persist_metadata_generation";
    const MAX_SOURCE_BYTES: usize = 1024 * 1024;
    const MAX_TOTAL_BYTES: usize = 4 * 1024 * 1024;
    if request.media_id <= 0
        || uuid::Uuid::parse_str(&request.claim_token).is_err()
        || request.sources.len() > 4
        || request.ai_inputs.is_empty()
        || request.ai_inputs.len() > 7
        || request.thumbnail_path.is_empty()
        || !is_lowercase_sha256(&request.content_hash)
    {
        return Err(ExecutorError::new(
            ExecutorErrorKind::InvalidInput,
            OPERATION,
            "metadata generation write has invalid cardinality or identity",
        ));
    }
    let metadata = &request.metadata;
    let mut total_bytes =
        request.claim_token.len() + request.thumbnail_path.len() + request.content_hash.len();
    for value in [
        metadata.date_taken.as_ref(),
        metadata.camera_make.as_ref(),
        metadata.camera_model.as_ref(),
        metadata.lens_make.as_ref(),
        metadata.lens_model.as_ref(),
        metadata.exposure_time.as_ref(),
        metadata.location_city.as_ref(),
        metadata.location_state.as_ref(),
        metadata.location_country.as_ref(),
        metadata.video_codec.as_ref(),
        metadata.keywords.as_ref(),
        request.geohash.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        total_bytes = total_bytes.checked_add(value.len()).ok_or_else(|| {
            ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                OPERATION,
                "metadata generation size overflow",
            )
        })?;
    }
    for source in &request.sources {
        if source.source_type.is_empty()
            || source.payload_json.is_empty()
            || source.payload_json.len() > MAX_SOURCE_BYTES
        {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                OPERATION,
                "metadata source is outside its bounded contract",
            ));
        }
        total_bytes = total_bytes
            .checked_add(source.source_type.len())
            .and_then(|bytes| bytes.checked_add(source.payload_json.len()))
            .ok_or_else(|| {
                ExecutorError::new(
                    ExecutorErrorKind::InvalidInput,
                    OPERATION,
                    "metadata generation size overflow",
                )
            })?;
    }
    for input in &request.ai_inputs {
        if input.sequence < 0
            || input.byte_size <= 0
            || input.task.is_empty()
            || input.input_kind.is_empty()
            || input.storage_root.is_empty()
            || input.file_path.is_empty()
            || input.filename.is_empty()
            || input.mime_type.is_empty()
            || !is_lowercase_sha256(&input.content_hash)
        {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                OPERATION,
                "metadata AI input descriptor is invalid",
            ));
        }
        for value in [
            &input.task,
            &input.input_kind,
            &input.storage_root,
            &input.file_path,
            &input.filename,
            &input.mime_type,
            &input.content_hash,
        ] {
            total_bytes = total_bytes.checked_add(value.len()).ok_or_else(|| {
                ExecutorError::new(
                    ExecutorErrorKind::InvalidInput,
                    OPERATION,
                    "metadata generation size overflow",
                )
            })?;
        }
    }
    if total_bytes > MAX_TOTAL_BYTES {
        return Err(ExecutorError::new(
            ExecutorErrorKind::InvalidInput,
            OPERATION,
            "metadata generation write exceeds four mebibytes",
        ));
    }
    Ok(())
}

fn is_lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn validate_import_claim(content_hash: &str, claim_token: &str) -> Result<(), ExecutorError> {
    if !is_lowercase_sha256(content_hash) || uuid::Uuid::parse_str(claim_token).is_err() {
        return Err(ExecutorError::new(
            ExecutorErrorKind::InvalidInput,
            "import_content_hash_claim",
            "import content-hash claim requires a lowercase SHA-256 and UUID token",
        ));
    }
    Ok(())
}

fn bounded_api_read_spec() -> SqliteOperationSpec {
    SqliteOperationSpec::read(OperationSpec {
        domain: ExecutorDomain::Sqlite,
        maximum_input_bytes: 256 * 1024,
        maximum_output_bytes: 1024 * 1024,
        maximum_temporary_bytes: 1024 * 1024,
    })
}

fn bounded_api_write_spec() -> SqliteOperationSpec {
    SqliteOperationSpec::fresh_write(
        OperationSpec {
            domain: ExecutorDomain::Sqlite,
            maximum_input_bytes: 256 * 1024,
            maximum_output_bytes: 1024 * 1024,
            maximum_temporary_bytes: 1024 * 1024,
        },
        BOUNDED_SQLITE_MAX_GROWTH_BYTES,
    )
}

pub(crate) struct SqliteCommand {
    operation: SqliteOperation,
    reply: oneshot::Sender<Result<SqliteOutput, ExecutorError>>,
}

impl SqliteCommand {
    pub(crate) fn new(
        operation: SqliteOperation,
        reply: oneshot::Sender<Result<SqliteOutput, ExecutorError>>,
    ) -> Self {
        Self { operation, reply }
    }

    pub(crate) fn reject(self, error: ExecutorError) {
        let _ = self.reply.send(Err(error));
    }
}

#[derive(Clone)]
pub struct SqliteExecutorHandle {
    ingress: SchedulerIngress,
}

impl SqliteExecutorHandle {
    pub(crate) fn new(ingress: SchedulerIngress) -> Self {
        Self { ingress }
    }

    pub async fn probe_durable(&self, sequence: u64) -> Result<(u64, String), ExecutorError> {
        let operation = SqliteOperation::Probe { sequence };
        let operation_name = operation.name();
        let (reply, response) = oneshot::channel();
        self.ingress.submit_sqlite(
            SqliteCommand::new(operation, reply),
            SubmissionMode::Durable,
            operation_name,
        )?;
        match response
            .await
            .map_err(|_| ExecutorError::shutting_down(operation_name))??
        {
            SqliteOutput::Probe {
                sequence,
                thread_name,
            } => Ok((sequence, thread_name)),
            output => Err(output.mismatch("sqlite_probe")),
        }
    }

    pub(crate) async fn register_auth_attempt_request(
        &self,
        request: RegisterAuthAttempt,
    ) -> Result<AuthAttemptDecision, ExecutorError> {
        match self
            .submit(
                SqliteOperation::RegisterAuthAttempt(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::AuthAttempt(decision) => Ok(decision),
            output => Err(output.mismatch("register_auth_attempt")),
        }
    }

    pub(crate) async fn clear_auth_attempts_request(
        &self,
        request: ClearAuthAttempts,
    ) -> Result<(), ExecutorError> {
        match self
            .submit(
                SqliteOperation::ClearAuthAttempts(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::AuthAttemptsCleared => Ok(()),
            output => Err(output.mismatch("clear_auth_attempts")),
        }
    }

    pub(crate) async fn load_user_for_token_request(
        &self,
        user_id: i64,
    ) -> Result<Option<UserForToken>, ExecutorError> {
        match self
            .submit(
                SqliteOperation::LoadUserForToken { user_id },
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::UserForToken(user) => Ok(user),
            output => Err(output.mismatch("load_user_for_token")),
        }
    }

    pub(crate) async fn load_user_for_authentication_request(
        &self,
        identifier: UserAuthIdentifier,
    ) -> Result<Option<UserForAuthentication>, ExecutorError> {
        if matches!(&identifier, UserAuthIdentifier::Username(username) if username.len() > 1024) {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "load_user_for_authentication",
                "username exceeds 1024 bytes",
            ));
        }
        match self
            .submit(
                SqliteOperation::LoadUserForAuthentication(identifier),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::UserForAuthentication(user) => Ok(user),
            output => Err(output.mismatch("load_user_for_authentication")),
        }
    }

    pub(crate) async fn insert_refresh_token_request(
        &self,
        request: InsertRefreshToken,
    ) -> Result<(), ExecutorError> {
        match self
            .submit(
                SqliteOperation::InsertRefreshToken(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::RefreshTokenInserted => Ok(()),
            output => Err(output.mismatch("insert_refresh_token")),
        }
    }

    pub(crate) async fn rotate_refresh_token_request(
        &self,
        request: RotateRefreshToken,
    ) -> Result<Option<RotatedRefreshIdentity>, ExecutorError> {
        match self
            .submit(
                SqliteOperation::RotateRefreshToken(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::RefreshTokenRotated(identity) => Ok(identity),
            output => Err(output.mismatch("rotate_refresh_token")),
        }
    }

    pub(crate) async fn revoke_refresh_token_request(
        &self,
        token_hash: String,
    ) -> Result<(), ExecutorError> {
        match self
            .submit(
                SqliteOperation::RevokeRefreshToken { token_hash },
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::RefreshTokenRevoked => Ok(()),
            output => Err(output.mismatch("revoke_refresh_token")),
        }
    }

    pub(crate) async fn load_password_hash_request(
        &self,
        user_id: i64,
    ) -> Result<Option<String>, ExecutorError> {
        match self
            .submit(
                SqliteOperation::LoadPasswordHash { user_id },
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::PasswordHash(hash) => Ok(hash),
            output => Err(output.mismatch("load_password_hash")),
        }
    }

    pub(crate) async fn replace_password_request(
        &self,
        request: ReplacePassword,
    ) -> Result<bool, ExecutorError> {
        match self
            .submit(
                SqliteOperation::ReplacePassword(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::PasswordReplaced(replaced) => Ok(replaced),
            output => Err(output.mismatch("replace_password")),
        }
    }

    pub(crate) async fn load_admin_id_durable(&self) -> Result<Option<i64>, ExecutorError> {
        match self
            .submit(SqliteOperation::LoadAdminId, SubmissionMode::Durable)
            .await?
        {
            SqliteOutput::AdminId(admin_id) => Ok(admin_id),
            output => Err(output.mismatch("load_admin_id")),
        }
    }

    pub(crate) async fn insert_default_admin_durable(
        &self,
        password_hash: String,
    ) -> Result<i64, ExecutorError> {
        match self
            .submit(
                SqliteOperation::InsertDefaultAdmin { password_hash },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::DefaultAdminInserted(admin_id) => Ok(admin_id),
            output => Err(output.mismatch("insert_default_admin")),
        }
    }

    pub(crate) async fn prepare_admin_password_reset_durable(
        &self,
        admin_id: i64,
    ) -> Result<bool, ExecutorError> {
        match self
            .submit(
                SqliteOperation::PrepareAdminPasswordReset { admin_id },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::AdminPasswordResetPrepared(prepared) => Ok(prepared),
            output => Err(output.mismatch("prepare_admin_password_reset")),
        }
    }

    pub(crate) async fn cleanup_refresh_tokens_durable(&self) -> Result<usize, ExecutorError> {
        match self
            .submit(
                SqliteOperation::CleanupRefreshTokens,
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::RefreshTokensCleaned(count) => Ok(count),
            output => Err(output.mismatch("cleanup_refresh_tokens")),
        }
    }

    pub async fn initialize_database_durable(&self) -> Result<(), ExecutorError> {
        match self
            .submit(SqliteOperation::InitializeDatabase, SubmissionMode::Durable)
            .await?
        {
            SqliteOutput::DatabaseInitialized => Ok(()),
            output => Err(output.mismatch("initialize_database")),
        }
    }

    pub(crate) async fn create_user_request(
        &self,
        request: CreateUser,
    ) -> Result<CreateUserOutcome, ExecutorError> {
        match self
            .submit(SqliteOperation::CreateUser(request), SubmissionMode::Try)
            .await?
        {
            SqliteOutput::UserCreated(outcome) => Ok(outcome),
            output => Err(output.mismatch("create_user")),
        }
    }

    pub(crate) async fn list_users_request(&self) -> Result<Vec<UserRecord>, ExecutorError> {
        match self
            .submit(SqliteOperation::ListUsers, SubmissionMode::Try)
            .await?
        {
            SqliteOutput::Users(users) => Ok(users),
            output => Err(output.mismatch("list_users")),
        }
    }

    pub(crate) async fn load_user_record_request(
        &self,
        user_id: i64,
    ) -> Result<Option<UserRecord>, ExecutorError> {
        match self
            .submit(
                SqliteOperation::LoadUserRecord { user_id },
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::UserRecord(user) => Ok(user),
            output => Err(output.mismatch("load_user_record")),
        }
    }

    pub(crate) async fn update_user_request(
        &self,
        request: UpdateUser,
    ) -> Result<UpdateUserOutcome, ExecutorError> {
        match self
            .submit(SqliteOperation::UpdateUser(request), SubmissionMode::Try)
            .await?
        {
            SqliteOutput::UserUpdated(outcome) => Ok(outcome),
            output => Err(output.mismatch("update_user")),
        }
    }

    pub(crate) async fn delete_user_request(
        &self,
        user_id: i64,
    ) -> Result<DeleteUserOutcome, ExecutorError> {
        match self
            .submit(SqliteOperation::DeleteUser { user_id }, SubmissionMode::Try)
            .await?
        {
            SqliteOutput::UserDeleted(outcome) => Ok(outcome),
            output => Err(output.mismatch("delete_user")),
        }
    }

    pub async fn load_map_clusters_request(
        &self,
        request: MapClustersQuery,
    ) -> Result<MapClustersResponse, ExecutorError> {
        let request = MapClustersQuery {
            bounds: normalize_spatial_bounds(request.bounds, "load_map_clusters")?,
            ..request
        };
        if !(2..=8).contains(&request.precision) {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "load_map_clusters",
                "geohash precision must be between 2 and 8",
            ));
        }
        match self
            .submit(
                SqliteOperation::LoadMapClusters(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::MapClusters(response) => Ok(response),
            output => Err(output.mismatch("load_map_clusters")),
        }
    }

    pub async fn load_map_media_request(
        &self,
        request: MapMediaQuery,
    ) -> Result<MapMediaListResponse, ExecutorError> {
        let request = MapMediaQuery {
            bounds: normalize_spatial_bounds(request.bounds, "load_map_media")?,
            ..request
        };
        if request.geohash_prefixes.len() > 256
            || request
                .geohash_prefixes
                .iter()
                .any(|prefix| prefix.is_empty() || prefix.len() > 12 || !prefix.is_ascii())
        {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "load_map_media",
                "geohash prefixes must contain at most 256 non-empty ASCII values of 12 bytes",
            ));
        }
        match self
            .submit(SqliteOperation::LoadMapMedia(request), SubmissionMode::Try)
            .await?
        {
            SqliteOutput::MapMedia(response) => Ok(response),
            output => Err(output.mismatch("load_map_media")),
        }
    }

    pub async fn load_duplicate_groups_request(
        &self,
        request: DuplicateGroupsQuery,
    ) -> Result<DeduplicateGroupsResponse, ExecutorError> {
        if !(1..=100).contains(&request.limit) || request.cursor < 0 {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "load_duplicate_groups",
                "limit must be between 1 and 100 and cursor must be non-negative",
            ));
        }
        match self
            .submit(
                SqliteOperation::LoadDuplicateGroups(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::DuplicateGroups(response) => Ok(response),
            output => Err(output.mismatch("load_duplicate_groups")),
        }
    }

    pub async fn load_place_cover_request(
        &self,
        request: PlaceIdentityQuery,
    ) -> Result<Option<PlaceCoverRecord>, ExecutorError> {
        validate_place_identity(&request, "load_place_cover")?;
        match self
            .submit(
                SqliteOperation::LoadPlaceCover(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::PlaceCover(cover) => Ok(cover),
            output => Err(output.mismatch("load_place_cover")),
        }
    }

    pub async fn load_places_page_request(
        &self,
        request: PlacePageQuery,
    ) -> Result<Vec<PlaceRecord>, ExecutorError> {
        validate_page(request.limit, request.offset, "load_places_page")?;
        match self
            .submit(
                SqliteOperation::LoadPlacesPage(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::Places(places) => Ok(places),
            output => Err(output.mismatch("load_places_page")),
        }
    }

    pub async fn load_place_media_page_request(
        &self,
        request: PlaceMediaQuery,
    ) -> Result<Option<PlaceMediaPage>, ExecutorError> {
        validate_place_identity(&request.identity, "load_place_media_page")?;
        validate_page(request.limit, request.offset, "load_place_media_page")?;
        match self
            .submit(
                SqliteOperation::LoadPlaceMediaPage(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::PlaceMediaPage(page) => Ok(page),
            output => Err(output.mismatch("load_place_media_page")),
        }
    }

    pub async fn create_album_request(
        &self,
        request: CreateAlbum,
    ) -> Result<AlbumDetailResponse, ExecutorError> {
        validate_album_text(
            &request.name,
            request.description.as_deref(),
            "create_album",
        )?;
        validate_album_media_ids(&request.media_ids, "create_album")?;
        match self
            .submit(SqliteOperation::CreateAlbum(request), SubmissionMode::Try)
            .await?
        {
            SqliteOutput::AlbumCreated(album) => Ok(album),
            output => Err(output.mismatch("create_album")),
        }
    }

    pub async fn list_albums_request(
        &self,
        user_id: i64,
    ) -> Result<Vec<AlbumResponse>, ExecutorError> {
        match self
            .submit(SqliteOperation::ListAlbums { user_id }, SubmissionMode::Try)
            .await?
        {
            SqliteOutput::Albums(albums) => Ok(albums),
            output => Err(output.mismatch("list_albums")),
        }
    }

    pub async fn load_album_request(
        &self,
        request: UserAlbum,
    ) -> Result<AlbumDetailOutcome, ExecutorError> {
        match self
            .submit(SqliteOperation::LoadAlbum(request), SubmissionMode::Try)
            .await?
        {
            SqliteOutput::Album(outcome) => Ok(outcome),
            output => Err(output.mismatch("load_album")),
        }
    }

    pub async fn update_album_request(
        &self,
        request: UpdateAlbum,
    ) -> Result<AlbumUpdateOutcome, ExecutorError> {
        validate_album_text(
            request.name.as_deref().unwrap_or(""),
            request.description.as_deref(),
            "update_album",
        )?;
        match self
            .submit(SqliteOperation::UpdateAlbum(request), SubmissionMode::Try)
            .await?
        {
            SqliteOutput::AlbumUpdated(outcome) => Ok(outcome),
            output => Err(output.mismatch("update_album")),
        }
    }

    pub async fn delete_album_request(
        &self,
        request: UserAlbum,
    ) -> Result<AlbumMutationOutcome, ExecutorError> {
        self.submit_album_mutation(SqliteOperation::DeleteAlbum(request), "delete_album")
            .await
    }

    pub async fn add_album_media_request(
        &self,
        request: AlbumMediaMutation,
    ) -> Result<AlbumMutationOutcome, ExecutorError> {
        validate_album_media_ids(&request.media_ids, "add_album_media")?;
        self.submit_album_mutation(SqliteOperation::AddAlbumMedia(request), "add_album_media")
            .await
    }

    pub async fn remove_album_media_request(
        &self,
        request: AlbumMediaMutation,
    ) -> Result<AlbumMutationOutcome, ExecutorError> {
        validate_album_media_ids(&request.media_ids, "remove_album_media")?;
        self.submit_album_mutation(
            SqliteOperation::RemoveAlbumMedia(request),
            "remove_album_media",
        )
        .await
    }

    pub async fn reorder_album_media_request(
        &self,
        request: AlbumMediaMutation,
    ) -> Result<AlbumMutationOutcome, ExecutorError> {
        validate_album_media_ids(&request.media_ids, "reorder_album_media")?;
        self.submit_album_mutation(
            SqliteOperation::ReorderAlbumMedia(request),
            "reorder_album_media",
        )
        .await
    }

    async fn submit_album_mutation(
        &self,
        operation: SqliteOperation,
        operation_name: &'static str,
    ) -> Result<AlbumMutationOutcome, ExecutorError> {
        match self.submit(operation, SubmissionMode::Try).await? {
            SqliteOutput::AlbumMutated(outcome) => Ok(outcome),
            output => Err(output.mismatch(operation_name)),
        }
    }

    pub async fn create_share_link_request(
        &self,
        request: CreateShareLink,
    ) -> Result<CreateShareLinkOutcome, ExecutorError> {
        if request.token.len() != 22
            || !request.token.is_ascii()
            || request
                .password_hash
                .as_ref()
                .is_some_and(|value| value.len() > 1024)
            || request
                .expires_at
                .as_ref()
                .is_some_and(|value| value.len() > 128)
        {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "create_share_link",
                "share link fields are invalid",
            ));
        }
        match self
            .submit(
                SqliteOperation::CreateShareLink(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::ShareLinkCreated(outcome) => Ok(outcome),
            output => Err(output.mismatch("create_share_link")),
        }
    }

    pub async fn list_share_links_request(
        &self,
        user_id: i64,
    ) -> Result<Vec<ShareLinkResponse>, ExecutorError> {
        match self
            .submit(
                SqliteOperation::ListShareLinks { user_id },
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::ShareLinks(shares) => Ok(shares),
            output => Err(output.mismatch("list_share_links")),
        }
    }

    pub async fn delete_share_link_request(
        &self,
        user_id: i64,
        share_id: i64,
    ) -> Result<DeleteShareLinkOutcome, ExecutorError> {
        match self
            .submit(
                SqliteOperation::DeleteShareLink { user_id, share_id },
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::ShareLinkDeleted(outcome) => Ok(outcome),
            output => Err(output.mismatch("delete_share_link")),
        }
    }

    pub async fn grant_share_access_request(
        &self,
        request: GrantShareAccess,
    ) -> Result<GrantShareAccessOutcome, ExecutorError> {
        if !(1..=2).contains(&request.access_level)
            || request.owner_user_id == request.target_user_id
        {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "grant_share_access",
                "share access grant is invalid",
            ));
        }
        match self
            .submit(
                SqliteOperation::GrantShareAccess(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::ShareAccessGranted(outcome) => Ok(outcome),
            output => Err(output.mismatch("grant_share_access")),
        }
    }

    pub async fn load_active_share_request(
        &self,
        token: String,
    ) -> Result<Option<ActiveShareRecord>, ExecutorError> {
        if token.is_empty() || token.len() > 256 {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "load_active_share",
                "share token is invalid",
            ));
        }
        match self
            .submit(
                SqliteOperation::LoadActiveShare { token },
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::ActiveShare(share) => Ok(share),
            output => Err(output.mismatch("load_active_share")),
        }
    }

    pub async fn load_public_share_content_request(
        &self,
        share: ActiveShareRecord,
    ) -> Result<PublicShareContent, ExecutorError> {
        match self
            .submit(
                SqliteOperation::LoadPublicShareContent(share),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::PublicShareContent(content) => Ok(content),
            output => Err(output.mismatch("load_public_share_content")),
        }
    }

    pub async fn load_public_shared_file_request(
        &self,
        request: PublicSharedMediaQuery,
    ) -> Result<PublicFileAccessOutcome, ExecutorError> {
        match self
            .submit(
                SqliteOperation::LoadPublicSharedFile(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::PublicSharedFile(outcome) => Ok(outcome),
            output => Err(output.mismatch("load_public_shared_file")),
        }
    }

    pub async fn load_public_shared_thumbnail_request(
        &self,
        request: PublicSharedMediaQuery,
    ) -> Result<PublicThumbnailAccessOutcome, ExecutorError> {
        match self
            .submit(
                SqliteOperation::LoadPublicSharedThumbnail(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::PublicSharedThumbnail(outcome) => Ok(outcome),
            output => Err(output.mismatch("load_public_shared_thumbnail")),
        }
    }

    pub async fn load_media_batch_request(
        &self,
        request: MediaBatchQuery,
    ) -> Result<Vec<MediaResponse>, ExecutorError> {
        if request.media_ids.len() > 500 {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "load_media_batch",
                "ids must contain at most 500 values",
            ));
        }
        match self
            .submit(
                SqliteOperation::LoadMediaBatch(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::MediaBatch(media) => Ok(media),
            output => Err(output.mismatch("load_media_batch")),
        }
    }

    pub async fn load_timeline_page_request(
        &self,
        request: TimelinePageQuery,
    ) -> Result<TimelinePageRecord, ExecutorError> {
        let text_bytes = [
            request.cursor.as_deref(),
            Some(request.search.as_str()),
            request.media_type.as_deref(),
            request.classification.as_deref(),
            request.anchor_date.as_deref(),
        ]
        .into_iter()
        .flatten()
        .try_fold(0usize, |total, value| total.checked_add(value.len()));
        if !(1..=500).contains(&request.limit) || text_bytes.is_none_or(|bytes| bytes > 64 * 1024) {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "load_timeline_page",
                "timeline request exceeds its bounded input",
            ));
        }
        match self
            .submit(
                SqliteOperation::LoadTimelinePage(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::TimelinePage(page) => Ok(page),
            output => Err(output.mismatch("load_timeline_page")),
        }
    }

    pub async fn load_timeline_markers_request(
        &self,
        request: TimelineMarkersQuery,
    ) -> Result<Vec<TimelineMarkerRecord>, ExecutorError> {
        let text_bytes = [
            Some(request.search.as_str()),
            request.media_type.as_deref(),
            request.classification.as_deref(),
        ]
        .into_iter()
        .flatten()
        .try_fold(0usize, |total, value| total.checked_add(value.len()));
        if text_bytes.is_none_or(|bytes| bytes > 64 * 1024) {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "load_timeline_markers",
                "timeline marker request exceeds its bounded input",
            ));
        }
        match self
            .submit(
                SqliteOperation::LoadTimelineMarkers(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::TimelineMarkers(markers) => Ok(markers),
            output => Err(output.mismatch("load_timeline_markers")),
        }
    }

    pub async fn move_media_to_trash_request(
        &self,
        request: MoveMediaToTrash,
    ) -> Result<usize, ExecutorError> {
        if request.media_ids.len() > 500 || request.deleted_at.len() > 128 {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "move_media_to_trash",
                "trash request exceeds its bounded input",
            ));
        }
        match self
            .submit(
                SqliteOperation::MoveMediaToTrash(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::MediaMovedToTrash(count) => Ok(count),
            output => Err(output.mismatch("move_media_to_trash")),
        }
    }

    pub async fn load_trash_request(
        &self,
        user_id: i64,
    ) -> Result<Vec<crate::models::TrashMediaResponse>, ExecutorError> {
        match self
            .submit(SqliteOperation::LoadTrash { user_id }, SubmissionMode::Try)
            .await?
        {
            SqliteOutput::Trash(items) => Ok(items),
            output => Err(output.mismatch("load_trash")),
        }
    }

    pub async fn restore_trash_request(
        &self,
        request: RestoreTrash,
    ) -> Result<usize, ExecutorError> {
        if request.media_ids.len() > 500 {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "restore_trash",
                "mediaIds must contain at most 500 values",
            ));
        }
        match self
            .submit(SqliteOperation::RestoreTrash(request), SubmissionMode::Try)
            .await?
        {
            SqliteOutput::TrashRestored(count) => Ok(count),
            output => Err(output.mismatch("restore_trash")),
        }
    }

    pub async fn delete_trash_media_request(
        &self,
        request: DeleteTrashMedia,
    ) -> Result<TrashDeletionOutcome, ExecutorError> {
        if request.media_ids.len() > 500 {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "delete_trash_media",
                "mediaIds must contain at most 500 values",
            ));
        }
        match self
            .submit(
                SqliteOperation::DeleteTrashMedia(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::TrashDeleted(outcome) => Ok(outcome),
            output => Err(output.mismatch("delete_trash_media")),
        }
    }

    pub async fn delete_trash_page_request(
        &self,
        request: DeleteTrashPage,
    ) -> Result<TrashDeletionOutcome, ExecutorError> {
        if !(1..=256).contains(&request.limit) {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "delete_trash_page",
                "trash page limit must be within 1..=256",
            ));
        }
        match self
            .submit(
                SqliteOperation::DeleteTrashPage(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::TrashDeleted(outcome) => Ok(outcome),
            output => Err(output.mismatch("delete_trash_page")),
        }
    }

    pub async fn delete_expired_trash_page_durable(
        &self,
        request: DeleteExpiredTrashPage,
    ) -> Result<TrashDeletionOutcome, ExecutorError> {
        if !(1..=256).contains(&request.limit) || request.cutoff.len() > 128 {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "delete_expired_trash_page",
                "expired-trash page exceeds its bounded input",
            ));
        }
        match self
            .submit(
                SqliteOperation::DeleteExpiredTrashPage(request),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::TrashDeleted(outcome) => Ok(outcome),
            output => Err(output.mismatch("delete_expired_trash_page")),
        }
    }

    pub async fn load_face_groups_page_request(
        &self,
        request: FaceGroupsPageQuery,
    ) -> Result<FaceGroupsListResponse, ExecutorError> {
        if !(1..=200).contains(&request.limit) || request.offset < 0 {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "load_face_groups_page",
                "face group page is outside its bounded range",
            ));
        }
        match self
            .submit(
                SqliteOperation::LoadFaceGroupsPage(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::FaceGroupsPage(page) => Ok(page),
            output => Err(output.mismatch("load_face_groups_page")),
        }
    }

    pub async fn load_face_group_request(
        &self,
        request: FaceGroupQuery,
    ) -> Result<Option<FaceGroupMediaResponse>, ExecutorError> {
        match self
            .submit(SqliteOperation::LoadFaceGroup(request), SubmissionMode::Try)
            .await?
        {
            SqliteOutput::FaceGroup(group) => Ok(group),
            output => Err(output.mismatch("load_face_group")),
        }
    }

    pub async fn load_visible_face_representative_request(
        &self,
        face_group_id: i64,
        user_id: i64,
        config: FaceGroupConfig,
    ) -> Result<Option<String>, ExecutorError> {
        match self
            .submit(
                SqliteOperation::LoadVisibleFaceRepresentative {
                    face_group_id,
                    user_id,
                    config,
                },
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::VisibleFaceRepresentative(path) => Ok(path),
            output => Err(output.mismatch("load_visible_face_representative")),
        }
    }

    pub async fn merge_face_groups_request(
        &self,
        group_ids: Vec<i64>,
        config: FaceGroupConfig,
    ) -> Result<MergeFaceGroupsOutcome, ExecutorError> {
        if !(2..=500).contains(&group_ids.len()) {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "merge_face_groups",
                "faceGroupIds must contain 2..=500 values",
            ));
        }
        match self
            .submit(
                SqliteOperation::MergeFaceGroups { group_ids, config },
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::FaceGroupsMerged(outcome) => Ok(outcome),
            output => Err(output.mismatch("merge_face_groups")),
        }
    }

    pub async fn load_ai_status_request(
        &self,
        config: Config,
        schedules: Vec<AiFeatureScheduleResponse>,
    ) -> Result<AiStatusResponse, ExecutorError> {
        if schedules.len() > 7
            || schedules.iter().any(|schedule| {
                schedule.feature.len() > 64 || schedule.cron_expression.len() > 1024
            })
        {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "load_ai_status",
                "AI schedules exceed their bounded input",
            ));
        }
        match self
            .submit(
                SqliteOperation::LoadAiStatus {
                    config: Box::new(config),
                    schedules,
                },
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::AiStatus(status) => Ok(*status),
            output => Err(output.mismatch("load_ai_status")),
        }
    }

    pub async fn start_ai_feature_request(
        &self,
        feature: AiFeature,
        trigger: String,
        scheduled_for: Option<String>,
    ) -> Result<usize, ExecutorError> {
        self.start_ai_feature(feature, trigger, scheduled_for, SubmissionMode::Try)
            .await
    }

    pub async fn start_ai_feature_durable(
        &self,
        feature: AiFeature,
        trigger: String,
        scheduled_for: Option<String>,
    ) -> Result<usize, ExecutorError> {
        self.start_ai_feature(feature, trigger, scheduled_for, SubmissionMode::Durable)
            .await
    }

    async fn start_ai_feature(
        &self,
        feature: AiFeature,
        trigger: String,
        scheduled_for: Option<String>,
        mode: SubmissionMode,
    ) -> Result<usize, ExecutorError> {
        if !matches!(trigger.as_str(), "manual" | "scheduled" | "startup")
            || scheduled_for
                .as_ref()
                .is_some_and(|value| value.len() > 128)
            || (trigger == "scheduled") != scheduled_for.is_some()
        {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "start_ai_feature",
                "AI start source is invalid",
            ));
        }
        match self
            .submit(
                SqliteOperation::StartAiFeature {
                    feature,
                    trigger,
                    scheduled_for,
                },
                mode,
            )
            .await?
        {
            SqliteOutput::AiFeatureStarted(count) => Ok(count),
            output => Err(output.mismatch("start_ai_feature")),
        }
    }

    pub async fn cancel_ai_feature_request(
        &self,
        feature: AiFeature,
    ) -> Result<AiFeatureActionResult, ExecutorError> {
        match self
            .submit(
                SqliteOperation::CancelAiFeature { feature },
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::AiFeatureCancelled(result) => Ok(result),
            output => Err(output.mismatch("cancel_ai_feature")),
        }
    }

    pub async fn cancel_all_ai_features_request(
        &self,
    ) -> Result<Vec<AiFeatureActionResult>, ExecutorError> {
        match self
            .submit(SqliteOperation::CancelAllAiFeatures, SubmissionMode::Try)
            .await?
        {
            SqliteOutput::AllAiFeaturesCancelled(results) => Ok(results),
            output => Err(output.mismatch("cancel_all_ai_features")),
        }
    }

    pub(crate) async fn clean_ai_feature_request(
        &self,
        feature: AiFeature,
        cleanup_group_id: String,
    ) -> Result<crate::processor::ai::operation::AiFeatureCleanOutcome, ExecutorError> {
        if cleanup_group_id.len() > crate::io::file::MAX_FILE_OPERATION_ID_BYTES {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "clean_ai_feature",
                "cleanup group identifier is too long",
            ));
        }
        match self
            .submit(
                SqliteOperation::CleanAiFeature {
                    feature,
                    cleanup_group_id,
                },
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::AiFeatureCleaned(outcome) => Ok(outcome),
            output => Err(output.mismatch("clean_ai_feature")),
        }
    }

    pub async fn load_deduplicate_schedule_state_durable(
        &self,
    ) -> Result<operations::DeduplicateScheduleState, ExecutorError> {
        match self
            .submit(
                SqliteOperation::LoadDeduplicateScheduleState,
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::DeduplicateScheduleState(state) => Ok(state),
            output => Err(output.mismatch("load_deduplicate_schedule_state")),
        }
    }

    pub async fn recover_deduplicate_runs_durable(&self) -> Result<(), ExecutorError> {
        match self
            .submit(
                SqliteOperation::RecoverDeduplicateRuns,
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::DeduplicateRunsRecovered => Ok(()),
            output => Err(output.mismatch("recover_deduplicate_runs")),
        }
    }

    pub async fn load_deduplicate_finalization_work(
        &self,
    ) -> Result<crate::processor::deduplicator::DeduplicateFinalizationWork, ExecutorError> {
        match self
            .submit(
                SqliteOperation::LoadDeduplicateFinalizationWork,
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::DeduplicateFinalizationWork(work) => Ok(work),
            output => Err(output.mismatch("load_deduplicate_finalization_work")),
        }
    }

    pub async fn commit_deduplicate_cpu_result(
        &self,
        result: crate::processor::deduplicator::DeduplicateCpuResult,
    ) -> Result<(), ExecutorError> {
        match self
            .submit(
                SqliteOperation::CommitDeduplicateCpuResult(result),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::DeduplicateCpuResultCommitted => Ok(()),
            output => Err(output.mismatch("commit_deduplicate_cpu_result")),
        }
    }

    pub async fn recover_face_grouping_runs_durable(&self) -> Result<(), ExecutorError> {
        match self
            .submit(
                SqliteOperation::RecoverFaceGroupingRuns,
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::FaceGroupingRunsRecovered => Ok(()),
            output => Err(output.mismatch("recover_face_grouping_runs")),
        }
    }

    pub async fn load_face_group_finalization_work(
        &self,
        config: FaceGroupConfig,
    ) -> Result<crate::processor::face_detection::FaceGroupFinalizationWork, ExecutorError> {
        match self
            .submit(
                SqliteOperation::LoadFaceGroupFinalizationWork(config),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::FaceGroupFinalizationWork(work) => Ok(work),
            output => Err(output.mismatch("load_face_group_finalization_work")),
        }
    }

    pub async fn commit_face_group_cpu_result(
        &self,
        result: crate::processor::face_detection::FaceGroupCpuResult,
    ) -> Result<(), ExecutorError> {
        match self
            .submit(
                SqliteOperation::CommitFaceGroupCpuResult(result),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::FaceGroupCpuResultCommitted => Ok(()),
            output => Err(output.mismatch("commit_face_group_cpu_result")),
        }
    }

    pub async fn load_face_representative_group_page_durable(
        &self,
        request: FaceRepresentativeGroupPageQuery,
    ) -> Result<FaceRepresentativeGroupPage, ExecutorError> {
        if request.after_group_id < 0 || !(1..=256).contains(&request.limit) {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "load_face_representative_group_page",
                "face representative group page is invalid",
            ));
        }
        match self
            .submit(
                SqliteOperation::LoadFaceRepresentativeGroupPage(request),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::FaceRepresentativeGroupPage(page) => Ok(page),
            output => Err(output.mismatch("load_face_representative_group_page")),
        }
    }

    pub async fn load_face_representative_candidate_page_durable(
        &self,
        request: FaceRepresentativeCandidatePageQuery,
    ) -> Result<FaceRepresentativeCandidatePage, ExecutorError> {
        if request.group_id <= 0 || request.after_face_id < 0 || !(1..=256).contains(&request.limit)
        {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "load_face_representative_candidate_page",
                "face representative candidate page is invalid",
            ));
        }
        match self
            .submit(
                SqliteOperation::LoadFaceRepresentativeCandidatePage(request),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::FaceRepresentativeCandidatePage(page) => Ok(page),
            output => Err(output.mismatch("load_face_representative_candidate_page")),
        }
    }

    pub async fn update_face_representative_durable(
        &self,
        request: UpdateFaceRepresentative,
    ) -> Result<(), ExecutorError> {
        if request.group_id <= 0
            || request
                .representative_face_id
                .is_some_and(|face_id| face_id <= 0)
        {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "update_face_representative",
                "face representative update is invalid",
            ));
        }
        match self
            .submit(
                SqliteOperation::UpdateFaceRepresentative(request),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::FaceRepresentativeUpdated => Ok(()),
            output => Err(output.mismatch("update_face_representative")),
        }
    }

    pub async fn invalidate_webdav_readiness_request(
        &self,
        request: operations::InvalidateWebdavReadiness,
    ) -> Result<(), ExecutorError> {
        match self
            .submit(
                SqliteOperation::InvalidateWebdavReadiness(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::WebdavReadinessInvalidated => Ok(()),
            output => Err(output.mismatch("invalidate_webdav_readiness")),
        }
    }

    pub async fn mark_webdav_ready_request(
        &self,
        request: operations::MarkWebdavReady,
    ) -> Result<(), ExecutorError> {
        match self
            .submit(
                SqliteOperation::MarkWebdavReady(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::WebdavReadyMarked => Ok(()),
            output => Err(output.mismatch("mark_webdav_ready")),
        }
    }

    pub async fn cancel_backup_upload_request(
        &self,
        request: operations::CancelBackupUpload,
    ) -> Result<operations::CancelBackupUploadOutcome, ExecutorError> {
        match self
            .submit(
                SqliteOperation::CancelBackupUpload(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::BackupUploadCancelled(outcome) => Ok(outcome),
            output => Err(output.mismatch("cancel_backup_upload")),
        }
    }

    pub async fn recover_backup_writing_sessions_durable(&self) -> Result<usize, ExecutorError> {
        match self
            .submit(
                SqliteOperation::RecoverBackupWritingSessions,
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::BackupWritingSessionsRecovered(count) => Ok(count),
            output => Err(output.mismatch("recover_backup_writing_sessions")),
        }
    }

    pub async fn load_backup_resumable_page_durable(
        &self,
        request: operations::BackupRecoveryPageQuery,
    ) -> Result<operations::BackupRecoveryPage<operations::BackupResumableFile>, ExecutorError>
    {
        validate_backup_recovery_page(&request, "load_backup_resumable_page")?;
        match self
            .submit(
                SqliteOperation::LoadBackupResumablePage(request),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::BackupResumablePage(page) => Ok(page),
            output => Err(output.mismatch("load_backup_resumable_page")),
        }
    }

    pub async fn load_backup_processing_page_durable(
        &self,
        request: operations::BackupRecoveryPageQuery,
    ) -> Result<operations::BackupRecoveryPage<operations::BackupProcessingAsset>, ExecutorError>
    {
        validate_backup_recovery_page(&request, "load_backup_processing_page")?;
        match self
            .submit(
                SqliteOperation::LoadBackupProcessingPage(request),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::BackupProcessingPage(page) => Ok(page),
            output => Err(output.mismatch("load_backup_processing_page")),
        }
    }

    pub async fn maintain_backup_sessions_durable(
        &self,
    ) -> Result<operations::BackupSessionMaintenance, ExecutorError> {
        match self
            .submit(
                SqliteOperation::MaintainBackupSessions,
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::BackupSessionsMaintained(outcome) => Ok(outcome),
            output => Err(output.mismatch("maintain_backup_sessions")),
        }
    }

    pub async fn claim_backup_asset_durable(
        &self,
    ) -> Result<Option<operations::ClaimedBackupAsset>, ExecutorError> {
        match self
            .submit(SqliteOperation::ClaimBackupAsset, SubmissionMode::Durable)
            .await?
        {
            SqliteOutput::BackupAssetClaimed(asset) => Ok(asset),
            output => Err(output.mismatch("claim_backup_asset")),
        }
    }

    pub async fn load_recovered_backup_media_durable(
        &self,
        content_hash: String,
        user_id: i64,
    ) -> Result<Option<i64>, ExecutorError> {
        if content_hash.len() > 128 {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "load_recovered_backup_media",
                "backup content hash exceeds 128 bytes",
            ));
        }
        match self
            .submit(
                SqliteOperation::LoadRecoveredBackupMedia {
                    content_hash,
                    user_id,
                },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::RecoveredBackupMedia(media_id) => Ok(media_id),
            output => Err(output.mismatch("load_recovered_backup_media")),
        }
    }

    pub async fn store_backup_content_hash_durable(
        &self,
        request: operations::StoreBackupContentHash,
    ) -> Result<bool, ExecutorError> {
        if request.content_hash.len() > 128 {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "store_backup_content_hash",
                "backup content hash exceeds 128 bytes",
            ));
        }
        match self
            .submit(
                SqliteOperation::StoreBackupContentHash(request),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::BackupContentHashStored(stored) => Ok(stored),
            output => Err(output.mismatch("store_backup_content_hash")),
        }
    }

    pub async fn transition_backup_processing_durable(
        &self,
        request: operations::BackupProcessingTransition,
    ) -> Result<operations::BackupProcessingTransitionOutcome, ExecutorError> {
        if matches!(
            &request,
            operations::BackupProcessingTransition::Fail { error, .. } if error.len() > 4096
        ) {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "transition_backup_processing",
                "backup failure diagnostic exceeds 4096 bytes",
            ));
        }
        match self
            .submit(
                SqliteOperation::TransitionBackupProcessing(request),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::BackupProcessingTransitioned(changed) => Ok(changed),
            output => Err(output.mismatch("transition_backup_processing")),
        }
    }

    pub async fn register_backup_device_request(
        &self,
        request: operations::RegisterBackupDevice,
    ) -> Result<(), ExecutorError> {
        match self
            .submit(
                SqliteOperation::RegisterBackupDevice(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::BackupDeviceRegistered => Ok(()),
            output => Err(output.mismatch("register_backup_device")),
        }
    }

    pub async fn create_backup_upload_request(
        &self,
        request: operations::CreateBackupUpload,
    ) -> Result<operations::CreateBackupUploadOutcome, ExecutorError> {
        match self
            .submit(
                SqliteOperation::CreateBackupUpload(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::BackupUploadCreated(outcome) => Ok(outcome),
            output => Err(output.mismatch("create_backup_upload")),
        }
    }

    pub async fn load_backup_upload_request(
        &self,
        request: operations::LoadBackupUpload,
    ) -> Result<Option<crate::models::BackupUploadResponse>, ExecutorError> {
        match self
            .submit(
                SqliteOperation::LoadBackupUpload(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::BackupUpload(upload) => Ok(upload),
            output => Err(output.mismatch("load_backup_upload")),
        }
    }

    pub async fn prepare_backup_completion_request(
        &self,
        request: operations::PrepareBackupCompletion,
    ) -> Result<operations::PrepareBackupCompletionOutcome, ExecutorError> {
        match self
            .submit(
                SqliteOperation::PrepareBackupCompletion(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::BackupCompletionPrepared(outcome) => Ok(outcome),
            output => Err(output.mismatch("prepare_backup_completion")),
        }
    }

    pub async fn queue_backup_completion_request(
        &self,
        request: operations::QueueBackupCompletion,
    ) -> Result<operations::QueueBackupCompletionOutcome, ExecutorError> {
        match self
            .submit(
                SqliteOperation::QueueBackupCompletion(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::BackupCompletionQueued(outcome) => Ok(outcome),
            output => Err(output.mismatch("queue_backup_completion")),
        }
    }

    pub async fn claim_backup_chunk_request(
        &self,
        request: operations::ClaimBackupChunk,
    ) -> Result<operations::ClaimBackupChunkOutcome, ExecutorError> {
        match self
            .submit(
                SqliteOperation::ClaimBackupChunk(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::BackupChunkClaimed(outcome) => Ok(outcome),
            output => Err(output.mismatch("claim_backup_chunk")),
        }
    }

    pub async fn finish_backup_chunk_request(
        &self,
        request: operations::FinishBackupChunk,
    ) -> Result<operations::FinishBackupChunkOutcome, ExecutorError> {
        match self
            .submit(
                SqliteOperation::FinishBackupChunk(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::BackupChunkFinished(outcome) => Ok(outcome),
            output => Err(output.mismatch("finish_backup_chunk")),
        }
    }

    pub async fn abandon_backup_chunk_request(
        &self,
        request: operations::AbandonBackupChunk,
    ) -> Result<(), ExecutorError> {
        match self
            .submit(
                SqliteOperation::AbandonBackupChunk(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::BackupChunkAbandoned => Ok(()),
            output => Err(output.mismatch("abandon_backup_chunk")),
        }
    }

    pub async fn create_import_job_request(
        &self,
        source: ImportSource,
    ) -> Result<CreateImportJobOutcome, ExecutorError> {
        self.create_import_job(source, SubmissionMode::Try).await
    }

    pub(crate) async fn create_import_job_durable(
        &self,
        source: ImportSource,
    ) -> Result<CreateImportJobOutcome, ExecutorError> {
        self.create_import_job(source, SubmissionMode::Durable)
            .await
    }

    async fn create_import_job(
        &self,
        source: ImportSource,
        mode: SubmissionMode,
    ) -> Result<CreateImportJobOutcome, ExecutorError> {
        match self
            .submit(SqliteOperation::CreateImportJob { source }, mode)
            .await?
        {
            SqliteOutput::ImportJobCreated(outcome) => Ok(outcome),
            output => Err(output.mismatch("create_import_job")),
        }
    }

    pub async fn load_import_status_request(
        &self,
        source: ImportSource,
    ) -> Result<ImportStatusSnapshot, ExecutorError> {
        match self
            .submit(
                SqliteOperation::LoadImportStatus { source },
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::ImportStatus(status) => Ok(status),
            output => Err(output.mismatch("load_import_status")),
        }
    }

    pub(crate) async fn set_import_job_total_durable(
        &self,
        job_id: i64,
        total_files: i64,
    ) -> Result<bool, ExecutorError> {
        match self
            .submit(
                SqliteOperation::SetImportJobTotal {
                    job_id,
                    total_files,
                },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::ImportJobTotalSet(updated) => Ok(updated),
            output => Err(output.mismatch("set_import_job_total")),
        }
    }

    pub(crate) async fn record_import_progress_durable(
        &self,
        job_id: i64,
        success: bool,
        error_message: String,
    ) -> Result<bool, ExecutorError> {
        if error_message.len() > 4096 {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "record_import_progress",
                "import error exceeds 4096 bytes",
            ));
        }
        match self
            .submit(
                SqliteOperation::RecordImportProgress {
                    job_id,
                    success,
                    error_message,
                },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::ImportProgressRecorded(updated) => Ok(updated),
            output => Err(output.mismatch("record_import_progress")),
        }
    }

    pub(crate) async fn complete_import_job_durable(
        &self,
        job_id: i64,
    ) -> Result<bool, ExecutorError> {
        match self
            .submit(
                SqliteOperation::CompleteImportJob { job_id },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::ImportJobCompleted(updated) => Ok(updated),
            output => Err(output.mismatch("complete_import_job")),
        }
    }

    pub(crate) async fn allocate_import_media_durable(
        &self,
        request: AllocateImportMedia,
    ) -> Result<ImportTarget, ExecutorError> {
        match self
            .submit(
                SqliteOperation::AllocateImportMedia(request),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::ImportMediaAllocated(target) => Ok(target),
            output => Err(output.mismatch("allocate_import_media")),
        }
    }

    pub(crate) async fn finalize_import_media_durable(
        &self,
        request: FinalizeImportMedia,
    ) -> Result<bool, ExecutorError> {
        match self
            .submit(
                SqliteOperation::FinalizeImportMedia(request),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::ImportMediaFinalized(updated) => Ok(updated),
            output => Err(output.mismatch("finalize_import_media")),
        }
    }

    pub(crate) async fn mark_import_media_failed_durable(
        &self,
        media_id: i64,
        error: String,
    ) -> Result<bool, ExecutorError> {
        if error.len() > 4096 {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "mark_import_media_failed",
                "import failure exceeds 4096 bytes",
            ));
        }
        match self
            .submit(
                SqliteOperation::MarkImportMediaFailed { media_id, error },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::ImportMediaFailed(updated) => Ok(updated),
            output => Err(output.mismatch("mark_import_media_failed")),
        }
    }

    pub(crate) async fn absorb_existing_media_durable(
        &self,
        request: AbsorbExistingMediaDatabase,
    ) -> Result<(), ExecutorError> {
        match self
            .submit(
                SqliteOperation::AbsorbExistingMedia(request),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::ExistingMediaAbsorbed => Ok(()),
            output => Err(output.mismatch("absorb_existing_media")),
        }
    }

    pub(crate) async fn recover_interrupted_import_page_durable(
        &self,
        after_media_id: i64,
        limit: u16,
    ) -> Result<Vec<InterruptedImport>, ExecutorError> {
        if limit == 0 || limit > 256 {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "recover_interrupted_import_page",
                "interrupted import recovery limit must be within 1..=256",
            ));
        }
        match self
            .submit(
                SqliteOperation::RecoverInterruptedImportPage {
                    after_media_id,
                    limit,
                },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::InterruptedImportPage(imports) => Ok(imports),
            output => Err(output.mismatch("recover_interrupted_import_page")),
        }
    }

    pub(crate) async fn load_webdav_ready_page_durable(
        &self,
        after_user_id: i64,
        after_file_path: String,
        limit: u16,
    ) -> Result<Vec<WebdavReadyFile>, ExecutorError> {
        if limit == 0 || limit > 256 || after_file_path.len() > 4096 {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "load_webdav_ready_page",
                "WebDAV ready-page cursor or limit is invalid",
            ));
        }
        match self
            .submit(
                SqliteOperation::LoadWebdavReadyPage {
                    after_user_id,
                    after_file_path,
                    limit,
                },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::WebdavReadyPage(files) => Ok(files),
            output => Err(output.mismatch("load_webdav_ready_page")),
        }
    }

    pub(crate) async fn check_webdav_ready_durable(
        &self,
        user_id: i64,
        file_path: String,
    ) -> Result<bool, ExecutorError> {
        if file_path.is_empty() || file_path.len() > 4096 {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "check_webdav_ready",
                "WebDAV ready path must contain 1..=4096 bytes",
            ));
        }
        match self
            .submit(
                SqliteOperation::CheckWebdavReady { user_id, file_path },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::WebdavReadyChecked(ready) => Ok(ready),
            output => Err(output.mismatch("check_webdav_ready")),
        }
    }

    pub(crate) async fn update_webdav_ready_paths_durable(
        &self,
        request: UpdateWebdavReadyPaths,
    ) -> Result<(), ExecutorError> {
        if request.remove.len() > 2
            || request.add.len() > 2
            || request
                .remove
                .iter()
                .chain(&request.add)
                .any(|path| path.is_empty() || path.len() > 4096)
        {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "update_webdav_ready_paths",
                "WebDAV ready-path update exceeds its bounds",
            ));
        }
        match self
            .submit(
                SqliteOperation::UpdateWebdavReadyPaths(request),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::WebdavReadyPathsUpdated => Ok(()),
            output => Err(output.mismatch("update_webdav_ready_paths")),
        }
    }

    pub(crate) async fn acquire_import_content_hash_claim_durable(
        &self,
        content_hash: String,
        claim_token: String,
        source: ImportSource,
    ) -> Result<ImportContentHashClaimOutcome, ExecutorError> {
        validate_import_claim(&content_hash, &claim_token)?;
        match self
            .submit(
                SqliteOperation::AcquireImportContentHashClaim {
                    content_hash,
                    claim_token,
                    source,
                },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::ImportContentHashClaimed(outcome) => Ok(outcome),
            output => Err(output.mismatch("acquire_import_content_hash_claim")),
        }
    }

    pub(crate) async fn release_import_content_hash_claim_durable(
        &self,
        content_hash: String,
        claim_token: String,
    ) -> Result<bool, ExecutorError> {
        validate_import_claim(&content_hash, &claim_token)?;
        match self
            .submit(
                SqliteOperation::ReleaseImportContentHashClaim {
                    content_hash,
                    claim_token,
                },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::ImportContentHashClaimReleased(released) => Ok(released),
            output => Err(output.mismatch("release_import_content_hash_claim")),
        }
    }

    pub(crate) fn release_import_content_hash_claim_eventually(
        &self,
        content_hash: String,
        claim_token: String,
    ) -> Result<(), ExecutorError> {
        validate_import_claim(&content_hash, &claim_token)?;
        let operation = SqliteOperation::ReleaseImportContentHashClaim {
            content_hash,
            claim_token,
        };
        let operation_name = operation.name();
        let (reply, _response) = oneshot::channel();
        self.ingress.submit_sqlite(
            SqliteCommand::new(operation, reply),
            SubmissionMode::Durable,
            operation_name,
        )
    }

    pub async fn recover_import_content_hash_claims_durable(&self) -> Result<usize, ExecutorError> {
        match self
            .submit(
                SqliteOperation::RecoverImportContentHashClaims,
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::ImportContentHashClaimsRecovered(count) => Ok(count),
            output => Err(output.mismatch("recover_import_content_hash_claims")),
        }
    }

    pub async fn prepare_file_operation_request(
        &self,
        plan: FileOperationPlan,
    ) -> Result<PrepareJournalOutcome, ExecutorError> {
        self.prepare_file_operation(plan, SubmissionMode::Try).await
    }

    pub async fn prepare_file_operation_durable(
        &self,
        plan: FileOperationPlan,
    ) -> Result<PrepareJournalOutcome, ExecutorError> {
        self.prepare_file_operation(plan, SubmissionMode::Durable)
            .await
    }

    async fn prepare_file_operation(
        &self,
        plan: FileOperationPlan,
        mode: SubmissionMode,
    ) -> Result<PrepareJournalOutcome, ExecutorError> {
        plan.validate().map_err(|error| {
            ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "prepare_file_operation",
                format!("invalid file operation plan: {error:?}"),
            )
        })?;
        match self
            .submit(SqliteOperation::PrepareFileOperation(plan), mode)
            .await?
        {
            SqliteOutput::FileOperationPrepared(outcome) => Ok(outcome),
            output => Err(output.mismatch("prepare_file_operation")),
        }
    }

    pub async fn prepare_directory_copy_operation_durable(
        &self,
        plan: FileOperationPlan,
        construction: DirectoryCopyConstructionPlan,
    ) -> Result<PrepareJournalOutcome, ExecutorError> {
        plan.validate().map_err(|error| {
            ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "prepare_directory_copy_operation",
                format!("invalid directory copy operation plan: {error:?}"),
            )
        })?;
        match self
            .submit(
                SqliteOperation::PrepareDirectoryCopyOperation(Box::new((plan, construction))),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::DirectoryCopyPrepared(outcome) => Ok(outcome),
            output => Err(output.mismatch("prepare_directory_copy_operation")),
        }
    }

    pub async fn load_directory_copy_durable(
        &self,
        group_id: Option<String>,
    ) -> Result<Option<DirectoryCopyConstruction>, ExecutorError> {
        match self
            .submit(
                SqliteOperation::LoadDirectoryCopy { group_id },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::NextDirectoryCopy(construction) => Ok(construction),
            output => Err(output.mismatch("load_directory_copy")),
        }
    }

    pub async fn checkpoint_directory_copy_entry_durable(
        &self,
        checkpoint: DirectoryCopyEntryCheckpoint,
    ) -> Result<bool, ExecutorError> {
        match self
            .submit(
                SqliteOperation::CheckpointDirectoryCopyEntry(checkpoint),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::DirectoryCopyEntryCheckpointed(changed) => Ok(changed),
            output => Err(output.mismatch("checkpoint_directory_copy_entry")),
        }
    }

    pub async fn checkpoint_directory_copy_finished_durable(
        &self,
        checkpoint: DirectoryCopyFinishedCheckpoint,
    ) -> Result<bool, ExecutorError> {
        match self
            .submit(
                SqliteOperation::CheckpointDirectoryCopyFinished(checkpoint),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::DirectoryCopyFinishedCheckpointed(changed) => Ok(changed),
            output => Err(output.mismatch("checkpoint_directory_copy_finished")),
        }
    }

    pub async fn begin_file_operation_publication_durable(
        &self,
        ticket: &JournalMutationTicket,
        expected_version: i64,
    ) -> Result<Option<JournalMutationGrant>, ExecutorError> {
        if ticket.group_version()
            != expected_version.checked_add(1).ok_or_else(|| {
                ExecutorError::new(
                    ExecutorErrorKind::InvalidInput,
                    "begin_file_operation_publication",
                    "journal publication ticket version is invalid",
                )
            })?
        {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "begin_file_operation_publication",
                "journal publication ticket does not match the next group version",
            ));
        }
        match self
            .submit(
                SqliteOperation::BeginFileOperationPublication {
                    group_id: ticket.group_id().to_string(),
                    expected_version,
                },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::FileOperationPublicationBegun(grant) => Ok(grant),
            output => Err(output.mismatch("begin_file_operation_publication")),
        }
    }

    pub async fn record_file_entry_published_durable(
        &self,
        group_id: String,
        expected_version: i64,
        sequence: u16,
    ) -> Result<Option<JournalEntryCheckpoint>, ExecutorError> {
        match self
            .submit(
                SqliteOperation::RecordFileEntryPublished {
                    group_id,
                    expected_version,
                    sequence,
                },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::FileEntryPublished(checkpoint) => Ok(checkpoint),
            output => Err(output.mismatch("record_file_entry_published")),
        }
    }

    pub async fn verify_file_operation_publication_durable(
        &self,
        ticket: &JournalMutationTicket,
    ) -> Result<Option<JournalMutationGrant>, ExecutorError> {
        match self
            .submit(
                SqliteOperation::VerifyFileOperationPublication {
                    group_id: ticket.group_id().to_string(),
                    expected_version: ticket.group_version(),
                },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::FileOperationPublicationVerified(grant) => Ok(grant),
            output => Err(output.mismatch("verify_file_operation_publication")),
        }
    }

    pub async fn complete_no_product_file_operation_durable(
        &self,
        group_id: String,
        expected_version: i64,
    ) -> Result<JournalCheckpointOutcome, ExecutorError> {
        match self
            .submit(
                SqliteOperation::CompleteFileOperation {
                    group_id,
                    expected_version,
                },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::FileOperationCompleted(outcome) => Ok(outcome),
            output => Err(output.mismatch("complete_no_product_file_operation")),
        }
    }

    pub async fn verify_file_operation_cleanup_durable(
        &self,
        ticket: &JournalMutationTicket,
    ) -> Result<Option<JournalMutationGrant>, ExecutorError> {
        match self
            .submit(
                SqliteOperation::VerifyFileOperationCleanup {
                    group_id: ticket.group_id().to_string(),
                    expected_version: ticket.group_version(),
                },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::FileOperationCleanupVerified(grant) => Ok(grant),
            output => Err(output.mismatch("verify_file_operation_cleanup")),
        }
    }

    pub async fn record_file_entry_cleaned_durable(
        &self,
        group_id: String,
        expected_version: i64,
        sequence: u16,
    ) -> Result<Option<JournalEntryCheckpoint>, ExecutorError> {
        match self
            .submit(
                SqliteOperation::RecordFileEntryCleaned {
                    group_id,
                    expected_version,
                    sequence,
                },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::FileEntryCleaned(checkpoint) => Ok(checkpoint),
            output => Err(output.mismatch("record_file_entry_cleaned")),
        }
    }

    pub async fn load_next_generic_file_operation_recovery_durable(
        &self,
    ) -> Result<Option<JournalRecoveryGroup>, ExecutorError> {
        match self
            .submit(
                SqliteOperation::LoadNextGenericFileOperationRecovery,
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::GenericFileOperationRecovery(group) => Ok(group),
            output => Err(output.mismatch("load_next_generic_file_operation_recovery")),
        }
    }

    pub async fn yield_file_operation_progress_durable(
        &self,
        group_id: String,
        expected_version: i64,
    ) -> Result<JournalCheckpointOutcome, ExecutorError> {
        match self
            .submit(
                SqliteOperation::YieldFileOperationProgress {
                    group_id,
                    expected_version,
                },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::FileOperationProgressYielded(outcome) => Ok(outcome),
            output => Err(output.mismatch("yield_file_operation_progress")),
        }
    }

    pub async fn record_file_operation_failure_durable(
        &self,
        group_id: String,
        expected_version: i64,
        sequence: u16,
        stage: JournalFailureStage,
        error_kind: String,
        error: String,
    ) -> Result<JournalCheckpointOutcome, ExecutorError> {
        match self
            .submit(
                SqliteOperation::RecordFileOperationFailure {
                    group_id,
                    expected_version,
                    sequence,
                    stage,
                    error_kind,
                    error,
                },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::FileOperationFailureRecorded(outcome) => Ok(outcome),
            output => Err(output.mismatch("record_file_operation_failure")),
        }
    }

    pub async fn record_file_operation_finalization_failure_durable(
        &self,
        group_id: String,
        expected_version: i64,
        error_kind: String,
        error: String,
    ) -> Result<JournalCheckpointOutcome, ExecutorError> {
        match self
            .submit(
                SqliteOperation::RecordFileOperationFinalizationFailure {
                    group_id,
                    expected_version,
                    error_kind,
                    error,
                },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::FileOperationFailureRecorded(outcome) => Ok(outcome),
            output => Err(output.mismatch("record_file_operation_finalization_failure")),
        }
    }

    pub async fn retry_file_operation_request(
        &self,
        retry_request_id: String,
        group_id: String,
        expected_version: i64,
        request_hash: [u8; 32],
    ) -> Result<JournalRetryOutcome, ExecutorError> {
        match self
            .submit(
                SqliteOperation::RetryFileOperation {
                    retry_request_id,
                    group_id,
                    expected_version,
                    request_hash,
                },
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::FileOperationRetried(outcome) => Ok(outcome),
            output => Err(output.mismatch("retry_file_operation")),
        }
    }

    pub async fn list_file_operations_request(
        &self,
        states: Vec<String>,
        cursor: Option<String>,
        limit: u16,
    ) -> Result<FileOperationListResponse, ExecutorError> {
        self.list_file_operations(states, cursor, limit, SubmissionMode::Try)
            .await
    }

    pub async fn list_file_operations_durable(
        &self,
        states: Vec<String>,
        cursor: Option<String>,
        limit: u16,
    ) -> Result<FileOperationListResponse, ExecutorError> {
        self.list_file_operations(states, cursor, limit, SubmissionMode::Durable)
            .await
    }

    async fn list_file_operations(
        &self,
        states: Vec<String>,
        cursor: Option<String>,
        limit: u16,
        mode: SubmissionMode,
    ) -> Result<FileOperationListResponse, ExecutorError> {
        match self
            .submit(
                SqliteOperation::ListFileOperations {
                    states,
                    cursor,
                    limit,
                },
                mode,
            )
            .await?
        {
            SqliteOutput::FileOperationsListed(response) => Ok(response),
            output => Err(output.mismatch("list_file_operations")),
        }
    }

    pub async fn load_file_operation_detail_request(
        &self,
        group_id: String,
    ) -> Result<Option<FileOperationDetailResponse>, ExecutorError> {
        match self
            .submit(
                SqliteOperation::LoadFileOperationDetail { group_id },
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::FileOperationDetail(response) => Ok(*response),
            output => Err(output.mismatch("load_file_operation_detail")),
        }
    }

    pub async fn maintain_file_operation_journal_durable(
        &self,
    ) -> Result<JournalMaintenanceOutcome, ExecutorError> {
        match self
            .submit(
                SqliteOperation::MaintainFileOperationJournal,
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::FileOperationJournalMaintained(outcome) => Ok(outcome),
            output => Err(output.mismatch("maintain_file_operation_journal")),
        }
    }

    pub async fn request_file_operation_cancellation_durable(
        &self,
        group_id: String,
        expected_version: i64,
    ) -> Result<JournalCancellationOutcome, ExecutorError> {
        match self
            .submit(
                SqliteOperation::RequestFileOperationCancellation {
                    group_id,
                    expected_version,
                },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::FileOperationCancellationRequested(outcome) => Ok(outcome),
            output => Err(output.mismatch("request_file_operation_cancellation")),
        }
    }

    pub async fn load_file_operation_cancellation_status_durable(
        &self,
        group_id: String,
    ) -> Result<Option<JournalCancellationStatus>, ExecutorError> {
        match self
            .submit(
                SqliteOperation::LoadFileOperationCancellationStatus { group_id },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::FileOperationCancellationStatus(status) => Ok(status),
            output => Err(output.mismatch("load_file_operation_cancellation_status")),
        }
    }

    pub async fn verify_file_operation_rollback_durable(
        &self,
        ticket: &JournalMutationTicket,
    ) -> Result<Option<JournalMutationGrant>, ExecutorError> {
        match self
            .submit(
                SqliteOperation::VerifyFileOperationRollback {
                    group_id: ticket.group_id().to_string(),
                    expected_version: ticket.group_version(),
                },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::FileOperationRollbackVerified(grant) => Ok(grant),
            output => Err(output.mismatch("verify_file_operation_rollback")),
        }
    }

    pub async fn record_file_entry_rolled_back_durable(
        &self,
        group_id: String,
        expected_version: i64,
        sequence: u16,
    ) -> Result<Option<JournalEntryCheckpoint>, ExecutorError> {
        match self
            .submit(
                SqliteOperation::RecordFileEntryRolledBack {
                    group_id,
                    expected_version,
                    sequence,
                },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::FileEntryRolledBack(checkpoint) => Ok(checkpoint),
            output => Err(output.mismatch("record_file_entry_rolled_back")),
        }
    }

    pub async fn queue_incomplete_metadata_request(&self) -> Result<usize, ExecutorError> {
        self.queue_incomplete_metadata(SubmissionMode::Try).await
    }

    pub async fn reset_metadata_request(
        &self,
        cleanup_group_id: String,
    ) -> Result<operations::ResetMetadataOutcome, ExecutorError> {
        if cleanup_group_id.is_empty()
            || cleanup_group_id.len() > crate::io::file::MAX_FILE_OPERATION_ID_BYTES
        {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "reset_metadata",
                "metadata reset cleanup group ID is invalid",
            ));
        }
        let mut requested_group_id = Some(cleanup_group_id);
        loop {
            match self
                .reset_metadata_page(requested_group_id.take(), SubmissionMode::Try)
                .await?
            {
                operations::ResetMetadataStepOutcome::Progressed => {}
                operations::ResetMetadataStepOutcome::Reset { media_count } => {
                    return Ok(operations::ResetMetadataOutcome::Reset { media_count });
                }
                operations::ResetMetadataStepOutcome::PathConflict => {
                    return Ok(operations::ResetMetadataOutcome::PathConflict);
                }
                operations::ResetMetadataStepOutcome::Idle => {
                    return Err(ExecutorError::new(
                        ExecutorErrorKind::Conflict,
                        "reset_metadata",
                        "the metadata reset was completed by another request",
                    ));
                }
            }
        }
    }

    pub async fn continue_metadata_reset_durable(&self) -> Result<bool, ExecutorError> {
        match self
            .reset_metadata_page(None, SubmissionMode::Durable)
            .await?
        {
            operations::ResetMetadataStepOutcome::Idle => Ok(false),
            operations::ResetMetadataStepOutcome::Progressed
            | operations::ResetMetadataStepOutcome::Reset { .. } => Ok(true),
            operations::ResetMetadataStepOutcome::PathConflict => Err(ExecutorError::new(
                ExecutorErrorKind::Internal,
                "continue_metadata_reset",
                "an existing metadata reset unexpectedly encountered a path conflict",
            )),
        }
    }

    async fn reset_metadata_page(
        &self,
        cleanup_group_id: Option<String>,
        mode: SubmissionMode,
    ) -> Result<operations::ResetMetadataStepOutcome, ExecutorError> {
        match self
            .submit(
                SqliteOperation::ResetMetadataPage { cleanup_group_id },
                mode,
            )
            .await?
        {
            SqliteOutput::MetadataResetStep(outcome) => Ok(outcome),
            output => Err(output.mismatch("reset_metadata_page")),
        }
    }

    pub async fn queue_incomplete_metadata_durable(&self) -> Result<usize, ExecutorError> {
        self.queue_incomplete_metadata(SubmissionMode::Durable)
            .await
    }

    async fn queue_incomplete_metadata(
        &self,
        mode: SubmissionMode,
    ) -> Result<usize, ExecutorError> {
        match self
            .submit(SqliteOperation::QueueIncompleteMetadata, mode)
            .await?
        {
            SqliteOutput::IncompleteMetadataQueued(count) => Ok(count),
            output => Err(output.mismatch("queue_incomplete_metadata")),
        }
    }

    pub async fn load_metadata_job_status_request(
        &self,
    ) -> Result<MetadataJobStatus, ExecutorError> {
        match self
            .submit(SqliteOperation::LoadMetadataJobStatus, SubmissionMode::Try)
            .await?
        {
            SqliteOutput::MetadataJobStatus(status) => Ok(status),
            output => Err(output.mismatch("load_metadata_job_status")),
        }
    }

    pub async fn claim_next_metadata_job_durable(
        &self,
    ) -> Result<Option<MetadataJobClaim>, ExecutorError> {
        let claim_token = uuid::Uuid::new_v4().to_string();
        match self
            .submit(
                SqliteOperation::ClaimNextMetadataJob { claim_token },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::MetadataJobClaimed(media_id) => Ok(media_id),
            output => Err(output.mismatch("claim_next_metadata_job")),
        }
    }

    pub async fn load_next_metadata_job_delay_durable(
        &self,
    ) -> Result<Option<std::time::Duration>, ExecutorError> {
        match self
            .submit(
                SqliteOperation::LoadNextMetadataJobDelay,
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::NextMetadataJobDelay(seconds) => {
                Ok(seconds.map(std::time::Duration::from_secs))
            }
            output => Err(output.mismatch("load_next_metadata_job_delay")),
        }
    }

    pub async fn finish_metadata_job_durable(
        &self,
        request: FinishMetadataJob,
    ) -> Result<(), ExecutorError> {
        if uuid::Uuid::parse_str(&request.claim_token).is_err() {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "finish_metadata_job",
                "metadata claim token is invalid",
            ));
        }
        if request
            .error
            .as_ref()
            .is_some_and(|error| error.len() > 256 * 1024)
        {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "finish_metadata_job",
                "metadata error exceeds 256 KiB",
            ));
        }
        match self
            .submit(
                SqliteOperation::FinishMetadataJob(request),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::MetadataJobFinished => Ok(()),
            output => Err(output.mismatch("finish_metadata_job")),
        }
    }

    pub async fn recover_metadata_claims_durable(&self) -> Result<usize, ExecutorError> {
        match self
            .submit(
                SqliteOperation::RecoverMetadataClaims,
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::MetadataClaimsRecovered(count) => Ok(count),
            output => Err(output.mismatch("recover_metadata_claims")),
        }
    }

    pub async fn load_metadata_generation_media_durable(
        &self,
        media_id: i64,
    ) -> Result<MetadataGenerationMedia, ExecutorError> {
        match self
            .submit(
                SqliteOperation::LoadMetadataGenerationMedia { media_id },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::MetadataGenerationMedia(media) => Ok(media),
            output => Err(output.mismatch("load_metadata_generation_media")),
        }
    }

    pub async fn persist_metadata_generation_durable(
        &self,
        request: PersistMetadataGeneration,
    ) -> Result<(), ExecutorError> {
        validate_metadata_generation_write(&request)?;
        match self
            .submit(
                SqliteOperation::PersistMetadataGeneration(Box::new(request)),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::MetadataGenerationPersisted => Ok(()),
            output => Err(output.mismatch("persist_metadata_generation")),
        }
    }

    pub async fn load_metadata_ai_input_verification_durable(
        &self,
        media_id: i64,
    ) -> Result<MetadataAiInputVerification, ExecutorError> {
        match self
            .submit(
                SqliteOperation::LoadMetadataAiInputVerification { media_id },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::MetadataAiInputVerification(verification) => Ok(verification),
            output => Err(output.mismatch("load_metadata_ai_input_verification")),
        }
    }

    pub async fn prepare_llm_submission_cycle_durable(&self) -> Result<(), ExecutorError> {
        match self
            .submit(
                SqliteOperation::PrepareLlmSubmissionCycle,
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::LlmSubmissionCyclePrepared => Ok(()),
            output => Err(output.mismatch("prepare_llm_submission_cycle")),
        }
    }

    pub async fn claim_llm_submission_jobs_durable(
        &self,
        limit: u16,
    ) -> Result<Vec<LlmSubmissionJob>, ExecutorError> {
        if limit == 0 || limit > 256 {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "claim_llm_submission_jobs",
                "LLM submission claim limit must be between 1 and 256",
            ));
        }
        match self
            .submit(
                SqliteOperation::ClaimLlmSubmissionJobs { limit },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::LlmSubmissionJobs(jobs) => Ok(jobs),
            output => Err(output.mismatch("claim_llm_submission_jobs")),
        }
    }

    pub async fn load_next_llm_submission_delay_durable(
        &self,
    ) -> Result<Option<std::time::Duration>, ExecutorError> {
        match self
            .submit(
                SqliteOperation::LoadNextLlmSubmissionDelay,
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::NextLlmSubmissionDelay(seconds) => {
                Ok(seconds.map(std::time::Duration::from_secs))
            }
            output => Err(output.mismatch("load_next_llm_submission_delay")),
        }
    }

    pub async fn load_llm_prepared_inputs_durable(
        &self,
        job_id: String,
    ) -> Result<Vec<LlmPreparedInput>, ExecutorError> {
        match self
            .submit(
                SqliteOperation::LoadLlmPreparedInputs { job_id },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::LlmPreparedInputs(inputs) => Ok(inputs),
            output => Err(output.mismatch("load_llm_prepared_inputs")),
        }
    }

    pub async fn finish_llm_submission_durable(
        &self,
        request: FinishLlmSubmission,
    ) -> Result<(), ExecutorError> {
        if matches!(
            &request,
            FinishLlmSubmission::Deferred {
                retry_after_seconds,
                ..
            } if *retry_after_seconds <= 0
        ) {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "finish_llm_submission",
                "LLM submission defer delay must be positive",
            ));
        }
        let error_is_oversized = match &request {
            FinishLlmSubmission::Retry { error, .. }
            | FinishLlmSubmission::Failed { error, .. } => error.len() > 4096,
            FinishLlmSubmission::Submitted { .. }
            | FinishLlmSubmission::Deferred { .. }
            | FinishLlmSubmission::RequeueAmbiguous { .. } => false,
        };
        if error_is_oversized {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "finish_llm_submission",
                "LLM submission error exceeds 4096 bytes",
            ));
        }
        match self
            .submit(
                SqliteOperation::FinishLlmSubmission(request),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::LlmSubmissionFinished => Ok(()),
            output => Err(output.mismatch("finish_llm_submission")),
        }
    }

    pub async fn load_llm_cancellation_batch_durable(
        &self,
        limit: u16,
    ) -> Result<Option<LlmCancellationBatch>, ExecutorError> {
        if limit == 0 || limit > 1000 {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "load_llm_cancellation_batch",
                "LLM cancellation batch limit must be between 1 and 1000",
            ));
        }
        match self
            .submit(
                SqliteOperation::LoadLlmCancellationBatch { limit },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::LlmCancellationBatch(batch) => Ok(batch),
            output => Err(output.mismatch("load_llm_cancellation_batch")),
        }
    }

    pub async fn acknowledge_llm_cancellation_durable(
        &self,
        request: AcknowledgeLlmCancellation,
    ) -> Result<(), ExecutorError> {
        match self
            .submit(
                SqliteOperation::AcknowledgeLlmCancellation(request),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::LlmCancellationAcknowledged => Ok(()),
            output => Err(output.mismatch("acknowledge_llm_cancellation")),
        }
    }

    pub async fn prepare_llm_result_receipt_durable(
        &self,
        request: PrepareLlmResultReceipt,
    ) -> Result<operations::LlmResultReceiptPreparation, ExecutorError> {
        if !momento_common::llm::is_valid_job_id(&request.job_id)
            || request.media_id <= 0
            || !momento_common::llm::is_valid_llm_task(&request.task)
            || request.attempt == 0
        {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "prepare_llm_result_receipt",
                "LLM result receipt correlation is invalid",
            ));
        }
        match self
            .submit(
                SqliteOperation::PrepareLlmResultReceipt(request),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::LlmResultReceiptPrepared(preparation) => Ok(preparation),
            output => Err(output.mismatch("prepare_llm_result_receipt")),
        }
    }

    pub async fn reject_llm_result_receipt_durable(
        &self,
        request: RejectLlmResultReceipt,
    ) -> Result<operations::LlmResultReceiptRejection, ExecutorError> {
        if !momento_common::llm::is_valid_job_id(&request.job_id)
            || request.attempt == 0
            || request
                .expected_job_version
                .is_some_and(|version| version <= 0)
            || request.error.is_empty()
            || request.error.len() > 4_096
        {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "reject_llm_result_receipt",
                "LLM result rejection is outside its bounded contract",
            ));
        }
        match self
            .submit(
                SqliteOperation::RejectLlmResultReceipt(request),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::LlmResultReceiptRejected(rejection) => Ok(rejection),
            output => Err(output.mismatch("reject_llm_result_receipt")),
        }
    }

    pub async fn create_llm_result_receipt_durable(
        &self,
        request: CreateLlmResultReceipt,
    ) -> Result<CreateLlmResultReceiptOutcome, ExecutorError> {
        let model_metadata_valid = match request.result_status.as_str() {
            "completed" => {
                request
                    .model_type
                    .as_ref()
                    .is_some_and(|value| !value.is_empty())
                    && request
                        .model_version
                        .as_ref()
                        .is_some_and(|value| !value.is_empty())
            }
            "failed" => request.model_type.is_none() && request.model_version.is_none(),
            _ => false,
        };
        if !momento_common::llm::is_valid_job_id(&request.job_id)
            || request.attempt == 0
            || request.expected_job_version <= 0
            || request.media_id <= 0
            || !momento_common::llm::is_valid_llm_task(&request.task)
            || !model_metadata_valid
            || request.encoding != momento_common::llm::result_stream::RESULT_RECORDS_ENCODING
            || request.record_count == 0
            || request.record_count > 1_000_000
            || request.byte_size < 24
            || request.byte_size > 1024 * 1024 * 1024
            || request.content_hash.len() != 64
            || !request
                .content_hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || request.journal_group_id != request.journal_plan.group_id
            || request.inbox_path.is_empty()
            || request.inbox_path.len() > 4096
            || uuid::Uuid::parse_str(&request.receive_token).is_err()
        {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "create_llm_result_receipt",
                "LLM result Journal admission is outside its bounded contract",
            ));
        }
        match self
            .submit(
                SqliteOperation::CreateLlmResultReceipt(Box::new(request)),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::LlmResultReceiptCreated(outcome) => Ok(outcome),
            output => Err(output.mismatch("create_llm_result_receipt")),
        }
    }

    pub async fn commit_llm_result_receipt_durable(
        &self,
        request: CommitLlmResultReceipt,
    ) -> Result<LlmResultReceiptOutcome, ExecutorError> {
        if !momento_common::llm::is_valid_job_id(&request.job_id)
            || request.attempt == 0
            || request.expected_job_version <= 0
            || request.expected_group_version <= 0
            || request.journal_group_id.is_empty()
            || request.journal_group_id.len() > 128
        {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "commit_llm_result_receipt",
                "LLM result receipt commit is outside its bounded contract",
            ));
        }
        match self
            .submit(
                SqliteOperation::CommitLlmResultReceipt(Box::new(request)),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::LlmResultReceiptCommitted(outcome) => Ok(outcome),
            output => Err(output.mismatch("commit_llm_result_receipt")),
        }
    }

    pub async fn stage_llm_result_page_durable(
        &self,
        request: StageLlmResultPage,
    ) -> Result<StageLlmResultPageOutcome, ExecutorError> {
        let payload_bytes = request.records.iter().try_fold(0_usize, |total, record| {
            total.checked_add(record.normalized_payload.len())
        });
        let records_valid = !request.records.is_empty()
            && request.records.len() <= 256
            && request.records.iter().all(|record| {
                !record.kind.is_empty()
                    && record.kind.len() <= 64
                    && record.encoded_size as usize
                        == momento_common::llm::RESULT_RECORD_HEADER_BYTES
                            + record.normalized_payload.len()
                    && record.normalized_payload.len()
                        <= momento_common::llm::MAX_LLM_RESULT_RECORD_PAYLOAD_BYTES
            });
        if !momento_common::llm::is_valid_job_id(&request.job_id)
            || request.attempt == 0
            || uuid::Uuid::parse_str(&request.claim_token).is_err()
            || request.expected_record_sequence >= 1_000_000
            || request.expected_byte_offset > 1024 * 1024 * 1024
            || !records_valid
            || payload_bytes.is_none_or(|bytes| bytes > 4 * 1024 * 1024)
        {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "stage_llm_result_page",
                "LLM result staging page is outside its bounded contract",
            ));
        }
        match self
            .submit(
                SqliteOperation::StageLlmResultPage(Box::new(request)),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::LlmResultPageStaged(outcome) => Ok(outcome),
            output => Err(output.mismatch("stage_llm_result_page")),
        }
    }

    pub async fn select_llm_result_staging_cleanup_durable(
        &self,
        limit: u16,
    ) -> Result<Vec<String>, ExecutorError> {
        if limit == 0 || limit > 256 {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "select_llm_result_staging_cleanup",
                "LLM result cleanup candidate limit must be within 1..=256",
            ));
        }
        match self
            .submit(
                SqliteOperation::SelectLlmResultStagingCleanup { limit },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::LlmResultStagingCleanup(job_ids) => Ok(job_ids),
            output => Err(output.mismatch("select_llm_result_staging_cleanup")),
        }
    }

    pub async fn cleanup_llm_result_staging_page_durable(
        &self,
        job_id: String,
        limit: u16,
    ) -> Result<CleanupLlmResultStagingOutcome, ExecutorError> {
        if !momento_common::llm::is_valid_job_id(&job_id) || limit == 0 || limit > 256 {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "cleanup_llm_result_staging_page",
                "LLM result cleanup page is outside its bounded contract",
            ));
        }
        match self
            .submit(
                SqliteOperation::CleanupLlmResultStagingPage { job_id, limit },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::LlmResultStagingCleaned(outcome) => Ok(outcome),
            output => Err(output.mismatch("cleanup_llm_result_staging_page")),
        }
    }

    pub async fn finalize_llm_result_cleanup_durable(
        &self,
        job_id: String,
    ) -> Result<bool, ExecutorError> {
        if !momento_common::llm::is_valid_job_id(&job_id) {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "finalize_llm_result_cleanup",
                "LLM result cleanup finalizer requires a valid job ID",
            ));
        }
        match self
            .submit(
                SqliteOperation::FinalizeLlmResultCleanup { job_id },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::LlmResultCleanupFinalized(released) => Ok(released),
            output => Err(output.mismatch("finalize_llm_result_cleanup")),
        }
    }

    pub async fn load_llm_result_staging_page_durable(
        &self,
        job_id: String,
        attempt: u32,
        after_record_sequence: Option<u32>,
        claim_token: String,
        limit: u16,
    ) -> Result<Vec<crate::database::operations::StagedLlmResultRecord>, ExecutorError> {
        if !momento_common::llm::is_valid_job_id(&job_id)
            || attempt == 0
            || uuid::Uuid::parse_str(&claim_token).is_err()
            || limit == 0
            || limit > 256
        {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "load_llm_result_staging_page",
                "LLM result staging read is outside its bounded contract",
            ));
        }
        match self
            .submit(
                SqliteOperation::LoadLlmResultStagingPage {
                    job_id,
                    attempt,
                    after_record_sequence,
                    claim_token,
                    limit,
                },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::LlmResultStagingPage(records) => Ok(records),
            output => Err(output.mismatch("load_llm_result_staging_page")),
        }
    }

    pub async fn release_llm_result_claim_durable(
        &self,
        job_id: String,
        claim_token: String,
    ) -> Result<bool, ExecutorError> {
        if !momento_common::llm::is_valid_job_id(&job_id)
            || uuid::Uuid::parse_str(&claim_token).is_err()
        {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "release_llm_result_claim",
                "LLM result claim identity is invalid",
            ));
        }
        match self
            .submit(
                SqliteOperation::ReleaseLlmResultClaim {
                    job_id,
                    claim_token,
                },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::LlmResultClaimReleased(released) => Ok(released),
            output => Err(output.mismatch("release_llm_result_claim")),
        }
    }

    pub async fn recover_llm_result_state_durable(
        &self,
    ) -> Result<operations::LlmResultRecoveryOutcome, ExecutorError> {
        match self
            .submit(
                SqliteOperation::RecoverLlmResultState,
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::LlmResultStateRecovered(outcome) => Ok(outcome),
            output => Err(output.mismatch("recover_llm_result_state")),
        }
    }

    pub(crate) async fn load_face_preparation_context_durable(
        &self,
        job_id: String,
        media_id: i64,
    ) -> Result<FacePreparationContext, ExecutorError> {
        if job_id.is_empty() || job_id.len() > 128 || media_id <= 0 {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "load_face_preparation_context",
                "face preparation identity is invalid",
            ));
        }
        match self
            .submit(
                SqliteOperation::LoadFacePreparationContext { job_id, media_id },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::FacePreparationContext(context) => Ok(context),
            output => Err(output.mismatch("load_face_preparation_context")),
        }
    }

    pub(crate) async fn select_llm_result_candidates_durable(
        &self,
        limit: u16,
    ) -> Result<Vec<crate::processor::ai::result::QueuedResult>, ExecutorError> {
        if limit == 0 || limit > 256 {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "select_llm_result_candidates",
                "LLM result candidate limit must be within 1..=256",
            ));
        }
        match self
            .submit(
                SqliteOperation::SelectLlmResultCandidates { limit },
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::LlmResultCandidates(candidates) => Ok(candidates),
            output => Err(output.mismatch("select_llm_result_candidates")),
        }
    }

    pub(crate) async fn persist_prepared_llm_result_durable(
        &self,
        prepared: crate::processor::ai::result::PreparedQueuedResult,
    ) -> Result<Vec<crate::io::file::NormalizedStoragePath>, ExecutorError> {
        match self
            .submit(
                SqliteOperation::PersistPreparedLlmResult(prepared),
                SubmissionMode::Durable,
            )
            .await?
        {
            SqliteOutput::PreparedLlmResultPersisted(paths) => Ok(paths),
            output => Err(output.mismatch("persist_prepared_llm_result")),
        }
    }

    pub async fn load_binary_media_request(
        &self,
        request: BinaryMediaQuery,
    ) -> Result<Option<BinaryMediaRecord>, ExecutorError> {
        match self
            .submit(
                SqliteOperation::LoadBinaryMedia(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::BinaryMedia(media) => Ok(media),
            output => Err(output.mismatch("load_binary_media")),
        }
    }

    pub async fn prepare_media_update_request(
        &self,
        request: PrepareMediaUpdate,
    ) -> Result<Option<EditableMediaState>, ExecutorError> {
        match self
            .submit(
                SqliteOperation::PrepareMediaUpdate(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::MediaUpdatePrepared(state) => Ok(state),
            output => Err(output.mismatch("prepare_media_update")),
        }
    }

    pub async fn finalize_media_update_request(
        &self,
        request: FinalizeMediaUpdate,
    ) -> Result<Option<MediaResponse>, ExecutorError> {
        let strings = [
            request.date_taken.as_deref(),
            request.geohash.as_deref(),
            request.city.as_deref(),
            request.state.as_deref(),
            request.country.as_deref(),
        ];
        let string_bytes = strings
            .into_iter()
            .flatten()
            .try_fold(0usize, |total, value| total.checked_add(value.len()));
        if string_bytes.is_none_or(|bytes| bytes > 256 * 1024) {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "finalize_media_update",
                "media update text exceeds its bounded input",
            ));
        }
        match self
            .submit(
                SqliteOperation::FinalizeMediaUpdate(request),
                SubmissionMode::Try,
            )
            .await?
        {
            SqliteOutput::MediaUpdateFinalized(media) => Ok(*media),
            output => Err(output.mismatch("finalize_media_update")),
        }
    }

    async fn submit(
        &self,
        operation: SqliteOperation,
        mode: SubmissionMode,
    ) -> Result<SqliteOutput, ExecutorError> {
        let operation_name = operation.name();
        let (reply, response) = oneshot::channel();
        self.ingress
            .submit_sqlite(SqliteCommand::new(operation, reply), mode, operation_name)?;
        response
            .await
            .map_err(|_| ExecutorError::shutting_down(operation_name))?
    }
}

fn normalize_spatial_bounds(
    bounds: operations::SpatialBounds,
    operation: &'static str,
) -> Result<operations::SpatialBounds, ExecutorError> {
    let finite = [bounds.north, bounds.south, bounds.east, bounds.west]
        .into_iter()
        .all(f64::is_finite);
    if !finite || bounds.south > bounds.north {
        return Err(ExecutorError::new(
            ExecutorErrorKind::InvalidInput,
            operation,
            "map bounds are invalid",
        ));
    }

    let north = bounds.north.clamp(-90.0, 90.0);
    let south = bounds.south.clamp(-90.0, 90.0);
    let longitudes_are_canonical =
        (-180.0..=180.0).contains(&bounds.west) && (-180.0..=180.0).contains(&bounds.east);
    if longitudes_are_canonical {
        return Ok(operations::SpatialBounds {
            north,
            south,
            east: bounds.east,
            west: bounds.west,
        });
    }

    if bounds.east >= bounds.west && bounds.east - bounds.west >= 360.0 {
        return Ok(operations::SpatialBounds {
            north,
            south,
            east: 180.0,
            west: -180.0,
        });
    }

    Ok(operations::SpatialBounds {
        north,
        south,
        east: wrap_longitude(bounds.east),
        west: wrap_longitude(bounds.west),
    })
}

fn wrap_longitude(longitude: f64) -> f64 {
    let remainder = longitude.rem_euclid(360.0);
    if remainder >= 180.0 {
        return remainder - 360.0;
    }
    remainder
}

fn validate_place_identity(
    identity: &PlaceIdentityQuery,
    operation: &'static str,
) -> Result<(), ExecutorError> {
    let fields_valid = !identity.city.trim().is_empty()
        && !identity.country.trim().is_empty()
        && identity.city.len() <= 1024
        && identity.country.len() <= 1024
        && identity
            .state
            .as_ref()
            .is_none_or(|state| state.len() <= 1024);
    if !fields_valid {
        return Err(ExecutorError::new(
            ExecutorErrorKind::InvalidInput,
            operation,
            "place identity is invalid",
        ));
    }
    Ok(())
}

fn validate_page(limit: i64, offset: i64, operation: &'static str) -> Result<(), ExecutorError> {
    if !(1..=200).contains(&limit) || offset < 0 {
        return Err(ExecutorError::new(
            ExecutorErrorKind::InvalidInput,
            operation,
            "page limit or offset is invalid",
        ));
    }
    Ok(())
}

fn validate_album_text(
    name: &str,
    description: Option<&str>,
    operation: &'static str,
) -> Result<(), ExecutorError> {
    if name.len() > 64 * 1024 || description.is_some_and(|value| value.len() > 128 * 1024) {
        return Err(ExecutorError::new(
            ExecutorErrorKind::InvalidInput,
            operation,
            "album text exceeds the bounded operation input",
        ));
    }
    Ok(())
}

fn validate_album_media_ids(
    media_ids: &[i64],
    operation: &'static str,
) -> Result<(), ExecutorError> {
    if media_ids.len() > 500 {
        return Err(ExecutorError::new(
            ExecutorErrorKind::InvalidInput,
            operation,
            "mediaIds must contain at most 500 IDs",
        ));
    }
    Ok(())
}

pub(crate) fn spawn_sqlite_workers(
    worker_count: usize,
    pool: DbPool,
    receiver: Receiver<SqliteCommand>,
    capacity_wake: std::sync::Arc<Notify>,
    space_budget: crate::io::space_budget::DataDirSpaceBudget,
    database_path: std::path::PathBuf,
    footprints: crate::database::result_footprint::SqliteFootprintRegistry,
) -> Result<Vec<JoinHandle<()>>, std::io::Error> {
    let mut workers = Vec::new();
    workers.try_reserve_exact(worker_count).map_err(|error| {
        std::io::Error::other(format!("failed to reserve SQLite worker handles: {error}"))
    })?;
    for worker_index in 0..worker_count {
        let pool = pool.clone();
        let receiver = receiver.clone();
        let capacity_wake = std::sync::Arc::clone(&capacity_wake);
        let space_budget = space_budget.clone();
        let database_path = database_path.clone();
        workers.push(
            std::thread::Builder::new()
                .name(format!("momento-sqlite-{worker_index}"))
                .stack_size(crate::runtime::WORKER_STACK_BYTES as usize)
                .spawn(move || {
                    run_worker(
                        pool,
                        receiver,
                        capacity_wake,
                        space_budget,
                        database_path,
                        footprints,
                    )
                })?,
        );
    }
    Ok(workers)
}

fn run_worker(
    pool: DbPool,
    receiver: Receiver<SqliteCommand>,
    capacity_wake: std::sync::Arc<Notify>,
    space_budget: crate::io::space_budget::DataDirSpaceBudget,
    database_path: std::path::PathBuf,
    footprints: crate::database::result_footprint::SqliteFootprintRegistry,
) {
    while let Ok(command) = receiver.recv() {
        capacity_wake.notify_one();
        let operation_result = execute(
            &pool,
            command.operation,
            &space_budget,
            &database_path,
            &footprints,
        );
        let _ = command.reply.send(operation_result);
    }
}

fn execute(
    pool: &DbPool,
    operation: SqliteOperation,
    space_budget: &crate::io::space_budget::DataDirSpaceBudget,
    database_path: &std::path::Path,
    footprints: &crate::database::result_footprint::SqliteFootprintRegistry,
) -> Result<SqliteOutput, ExecutorError> {
    let operation_spec = operation.spec(footprints)?;
    let operation_name = operation.name();
    let durable_parent_job_id = operation.durable_parent_job_id().map(str::to_string);
    let mut capacity_token = match operation_spec.capacity {
        SqliteCapacitySource::ReadOnly | SqliteCapacitySource::DurableParent { .. } => None,
        SqliteCapacitySource::Fresh { max_growth_bytes }
        | SqliteCapacitySource::ProvisionalParent { max_growth_bytes } => {
            let reservation_id = format!("sqlite-{}", uuid::Uuid::new_v4().simple());
            let admission = space_budget
                .reserve_sqlite(reservation_id, max_growth_bytes)
                .map_err(|error| {
                    ExecutorError::new(
                        ExecutorErrorKind::Internal,
                        operation_name,
                        error.to_string(),
                    )
                })?;
            match admission {
                crate::io::space_budget::SpaceAdmission::Fits(token) => Some(token),
                crate::io::space_budget::SpaceAdmission::TemporarilyUnavailable {
                    required_bytes,
                    available_bytes,
                } => {
                    return Err(ExecutorError::new(
                        ExecutorErrorKind::DatabaseBusy,
                        operation_name,
                        format!(
                            "SQLite capacity is temporarily unavailable: required {required_bytes} bytes, available {available_bytes} bytes"
                        ),
                    ));
                }
                crate::io::space_budget::SpaceAdmission::ExceedsHardLimit {
                    required_bytes,
                    class_limit_bytes,
                } => {
                    return Err(ExecutorError::new(
                        ExecutorErrorKind::DatabasePermanent,
                        operation_name,
                        format!(
                            "SQLite operation growth {required_bytes} exceeds class limit {class_limit_bytes}"
                        ),
                    ));
                }
            }
        }
    };
    let _resources = operation_spec.resources;
    let deadline = Instant::now() + SQLITE_OPERATION_TIMEOUT;
    let mut connection = pool
        .get_timeout(SQLITE_CONNECTION_TIMEOUT)
        .map_err(|error| {
            ExecutorError::new(
                ExecutorErrorKind::DatabaseTimeout,
                operation_name,
                error.to_string(),
            )
        })?;
    let durable_capacity = match operation_spec.capacity {
        SqliteCapacitySource::DurableParent { max_growth_bytes } => {
            let job_id = durable_parent_job_id.as_deref().ok_or_else(|| {
                ExecutorError::new(
                    ExecutorErrorKind::Internal,
                    operation_name,
                    "SQLite durable-parent operation has no result owner",
                )
            })?;
            let record = load_result_sqlite_reservation(&connection, job_id, operation_name)?;
            if max_growth_bytes > record.reserved_peak_additional_bytes {
                return Err(ExecutorError::new(
                    ExecutorErrorKind::DatabasePermanent,
                    operation_name,
                    "SQLite child footprint exceeds its durable result parent",
                ));
            }
            let checkout = space_budget.reacquire_durable(&record).map_err(|error| {
                ExecutorError::new(
                    ExecutorErrorKind::DatabaseBusy,
                    operation_name,
                    error.to_string(),
                )
            })?;
            Some((record, checkout))
        }
        SqliteCapacitySource::ReadOnly
        | SqliteCapacitySource::Fresh { .. }
        | SqliteCapacitySource::ProvisionalParent { .. } => None,
    };
    connection.progress_handler(
        SQLITE_PROGRESS_HANDLER_OPS,
        Some(move || Instant::now() >= deadline),
    );
    let result = catch_unwind(AssertUnwindSafe(|| {
        if let SqliteCapacitySource::Fresh { max_growth_bytes }
        | SqliteCapacitySource::ProvisionalParent { max_growth_bytes }
        | SqliteCapacitySource::DurableParent { max_growth_bytes } = operation_spec.capacity
        {
            ensure_sqlite_wal_capacity(
                &mut connection,
                database_path,
                space_budget.sqlite_wal_limit_bytes(),
                max_growth_bytes,
                operation_name,
            )?;
        }
        execute_with_connection(
            &mut connection,
            operation,
            space_budget,
            database_path,
            &mut capacity_token,
            durable_capacity
                .as_ref()
                .map(|(record, _)| operations::SqliteResultCapacityChild {
                    reservation_id: record.reservation_id.clone(),
                    expected_version: record.version,
                    max_growth_bytes: match operation_spec.capacity {
                        SqliteCapacitySource::DurableParent { max_growth_bytes } => {
                            max_growth_bytes
                        }
                        _ => unreachable!(),
                    },
                    cleanup_remaining_bytes: footprints.result_cleanup_recovery_max_growth_bytes,
                }),
        )
    }));
    connection.progress_handler(0, None::<fn() -> bool>);
    let mut operation_result = match result {
        Ok(result) => result,
        Err(_) => Err(ExecutorError::new(
            ExecutorErrorKind::WorkerPanic,
            operation_name,
            "SQLite operation panicked",
        )),
    };
    if let Some((record, checkout)) = durable_capacity {
        if operation_result.is_ok() {
            let refreshed =
                load_result_sqlite_reservation(&connection, &record.owner_id, operation_name)?;
            let allocated = crate::io::space_budget::measure_sqlite_allocation(database_path)
                .map_err(|error| {
                    ExecutorError::new(
                        ExecutorErrorKind::Internal,
                        operation_name,
                        error.to_string(),
                    )
                })?;
            if let Err(error) = checkout.publish_sqlite_child(&refreshed, allocated) {
                operation_result = Err(ExecutorError::new(
                    ExecutorErrorKind::Internal,
                    operation_name,
                    error.to_string(),
                ));
            }
        }
    }
    let capacity_result = match operation_spec.capacity {
        SqliteCapacitySource::ReadOnly
        | SqliteCapacitySource::ProvisionalParent { .. }
        | SqliteCapacitySource::DurableParent { .. } => None,
        SqliteCapacitySource::Fresh { .. } => capacity_token.map(|token| {
            crate::io::space_budget::measure_sqlite_allocation(database_path)
                .and_then(|allocated| token.publish_ephemeral_sqlite_allocation(allocated))
                .map_err(|error| {
                    ExecutorError::new(
                        ExecutorErrorKind::Internal,
                        operation_name,
                        error.to_string(),
                    )
                })
        }),
    };
    match capacity_result {
        Some(Err(error)) => Err(error),
        Some(Ok(())) | None => operation_result,
    }
}

fn ensure_sqlite_wal_capacity(
    connection: &mut rusqlite::Connection,
    database_path: &std::path::Path,
    wal_limit_bytes: u64,
    max_growth_bytes: u64,
    operation: &'static str,
) -> Result<(), ExecutorError> {
    let wal_path = database_path.with_extension("sqlite-wal");
    let allocated = allocated_regular_file_bytes(&wal_path, operation)?;
    if allocated
        .checked_add(max_growth_bytes)
        .is_some_and(|peak| peak <= wal_limit_bytes)
    {
        return Ok(());
    }
    let (busy, _, _) = connection
        .query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|error| map_sqlite_error(operation, error))?;
    if busy != 0 {
        return Err(ExecutorError::new(
            ExecutorErrorKind::DatabaseBusy,
            operation,
            "SQLite WAL checkpoint is blocked by an active reader",
        ));
    }
    let allocated = allocated_regular_file_bytes(&wal_path, operation)?;
    let Some(peak) = allocated.checked_add(max_growth_bytes) else {
        return Err(ExecutorError::new(
            ExecutorErrorKind::DatabasePermanent,
            operation,
            "SQLite WAL growth bound overflowed",
        ));
    };
    if peak > wal_limit_bytes {
        return Err(ExecutorError::new(
            ExecutorErrorKind::DatabaseBusy,
            operation,
            format!("SQLite WAL needs {peak} bytes but its limit is {wal_limit_bytes} bytes"),
        ));
    }
    Ok(())
}

fn allocated_regular_file_bytes(
    path: &std::path::Path,
    operation: &'static str,
) -> Result<u64, ExecutorError> {
    use std::os::unix::fs::MetadataExt;

    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(ExecutorError::new(
                ExecutorErrorKind::DatabasePermanent,
                operation,
                format!(
                    "SQLite capacity path is not a regular file: {}",
                    path.display()
                ),
            ))
        }
        Ok(metadata) => metadata.blocks().checked_mul(512).ok_or_else(|| {
            ExecutorError::new(
                ExecutorErrorKind::DatabasePermanent,
                operation,
                "SQLite allocated byte count overflowed",
            )
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(ExecutorError::new(
            ExecutorErrorKind::Database,
            operation,
            format!(
                "could not measure SQLite capacity path {}: {error}",
                path.display()
            ),
        )),
    }
}

fn load_result_sqlite_reservation(
    connection: &rusqlite::Connection,
    job_id: &str,
    operation: &'static str,
) -> Result<crate::io::space_budget::DurableSpaceReservationRecord, ExecutorError> {
    connection
        .query_row(
            crate::database::queries::file_operations::SELECT_ACTIVE_SQLITE_RESULT_RESERVATION,
            [job_id],
            |row| {
                let class = row.get::<_, String>(1)?;
                let class =
                    crate::io::space_budget::SpaceReservationClass::try_from(class.as_str())
                        .map_err(|_| rusqlite::Error::InvalidQuery)?;
                Ok(crate::io::space_budget::DurableSpaceReservationRecord {
                    reservation_id: row.get(0)?,
                    class,
                    owner_kind: row.get(2)?,
                    owner_id: row.get(3)?,
                    journal_group_id: row.get(4)?,
                    filesystem_id: row.get(5)?,
                    reserved_peak_additional_bytes: row.get(6)?,
                    newly_allocated_blocks: row.get(7)?,
                    version: row.get(8)?,
                })
            },
        )
        .map_err(|error| map_sqlite_error(operation, error))
}

fn execute_with_connection(
    connection: &mut rusqlite::Connection,
    operation: SqliteOperation,
    space_budget: &crate::io::space_budget::DataDirSpaceBudget,
    database_path: &std::path::Path,
    capacity_token: &mut Option<crate::io::space_budget::ProvisionalSpaceToken>,
    durable_capacity: Option<operations::SqliteResultCapacityChild>,
) -> Result<SqliteOutput, ExecutorError> {
    let operation_name = operation.name();
    match operation {
        SqliteOperation::Probe { sequence } => {
            connection
                .query_row("SELECT 1", [], |_| Ok(()))
                .map_err(|error| map_sqlite_error(operation_name, error))?;
            Ok(SqliteOutput::Probe {
                sequence,
                thread_name: std::thread::current()
                    .name()
                    .unwrap_or("unnamed")
                    .to_string(),
            })
        }
        SqliteOperation::RegisterAuthAttempt(request) => {
            operations::register_auth_attempt(connection, request)
                .map(SqliteOutput::AuthAttempt)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::ClearAuthAttempts(request) => {
            operations::clear_auth_attempts(connection, request)
                .map(|()| SqliteOutput::AuthAttemptsCleared)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadUserForToken { user_id } => {
            operations::load_user_for_token(connection, user_id)
                .map(SqliteOutput::UserForToken)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadUserForAuthentication(identifier) => {
            operations::load_user_for_authentication(connection, identifier)
                .map(SqliteOutput::UserForAuthentication)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::InsertRefreshToken(request) => {
            operations::insert_refresh_token(connection, request)
                .map(|()| SqliteOutput::RefreshTokenInserted)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::RotateRefreshToken(request) => {
            operations::rotate_refresh_token(connection, request)
                .map(SqliteOutput::RefreshTokenRotated)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::RevokeRefreshToken { token_hash } => {
            operations::revoke_refresh_token(connection, token_hash)
                .map(|()| SqliteOutput::RefreshTokenRevoked)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadPasswordHash { user_id } => {
            operations::load_password_hash(connection, user_id)
                .map(SqliteOutput::PasswordHash)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::ReplacePassword(request) => {
            operations::replace_password(connection, request)
                .map(SqliteOutput::PasswordReplaced)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadAdminId => operations::load_admin_id(connection)
            .map(SqliteOutput::AdminId)
            .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::InsertDefaultAdmin { password_hash } => {
            operations::insert_default_admin_if_missing(connection, password_hash)
                .map(SqliteOutput::DefaultAdminInserted)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::PrepareAdminPasswordReset { admin_id } => {
            operations::prepare_admin_password_reset(connection, admin_id)
                .map(SqliteOutput::AdminPasswordResetPrepared)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::CleanupRefreshTokens => operations::cleanup_refresh_tokens(connection)
            .map(SqliteOutput::RefreshTokensCleaned)
            .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::InitializeDatabase => crate::database::init_database(connection)
            .map(|()| SqliteOutput::DatabaseInitialized)
            .map_err(|error| {
                ExecutorError::new(
                    ExecutorErrorKind::Database,
                    operation_name,
                    error.to_string(),
                )
            }),
        SqliteOperation::CreateUser(request) => operations::create_user(connection, request)
            .map(SqliteOutput::UserCreated)
            .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::ListUsers => operations::list_users(connection)
            .map(SqliteOutput::Users)
            .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::LoadUserRecord { user_id } => {
            operations::load_user_record(connection, user_id)
                .map(SqliteOutput::UserRecord)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::UpdateUser(request) => operations::update_user(connection, request)
            .map(SqliteOutput::UserUpdated)
            .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::DeleteUser { user_id } => operations::delete_user(connection, user_id)
            .map(SqliteOutput::UserDeleted)
            .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::LoadMapClusters(request) => {
            operations::load_map_clusters(connection, request)
                .map(SqliteOutput::MapClusters)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadMapMedia(request) => operations::load_map_media(connection, request)
            .map(SqliteOutput::MapMedia)
            .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::LoadDuplicateGroups(request) => {
            operations::load_duplicate_groups(connection, request)
                .map(SqliteOutput::DuplicateGroups)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadPlaceCover(request) => {
            operations::load_place_cover(connection, request)
                .map(SqliteOutput::PlaceCover)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadPlacesPage(request) => {
            operations::load_places_page(connection, request)
                .map(SqliteOutput::Places)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadPlaceMediaPage(request) => {
            operations::load_place_media_page(connection, request)
                .map(SqliteOutput::PlaceMediaPage)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::CreateAlbum(request) => operations::create_album(connection, request)
            .map(SqliteOutput::AlbumCreated)
            .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::ListAlbums { user_id } => operations::list_albums(connection, user_id)
            .map(SqliteOutput::Albums)
            .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::LoadAlbum(request) => operations::load_album(connection, request)
            .map(SqliteOutput::Album)
            .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::UpdateAlbum(request) => operations::update_album(connection, request)
            .map(SqliteOutput::AlbumUpdated)
            .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::DeleteAlbum(request) => {
            operations::delete_album_access(connection, request)
                .map(SqliteOutput::AlbumMutated)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::AddAlbumMedia(request) => operations::add_album_media(connection, request)
            .map(SqliteOutput::AlbumMutated)
            .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::RemoveAlbumMedia(request) => {
            operations::remove_album_media(connection, request)
                .map(SqliteOutput::AlbumMutated)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::ReorderAlbumMedia(request) => {
            operations::reorder_album_media(connection, request)
                .map(SqliteOutput::AlbumMutated)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::CreateShareLink(request) => {
            operations::create_share_link(connection, request)
                .map(SqliteOutput::ShareLinkCreated)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::ListShareLinks { user_id } => {
            operations::list_share_links(connection, user_id)
                .map(SqliteOutput::ShareLinks)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::DeleteShareLink { user_id, share_id } => {
            operations::delete_share_link(connection, user_id, share_id)
                .map(SqliteOutput::ShareLinkDeleted)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::GrantShareAccess(request) => {
            operations::grant_share_access(connection, request)
                .map(SqliteOutput::ShareAccessGranted)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadActiveShare { token } => {
            operations::load_active_share(connection, token)
                .map(SqliteOutput::ActiveShare)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadPublicShareContent(share) => {
            operations::load_public_share_content(connection, share)
                .map(SqliteOutput::PublicShareContent)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadPublicSharedFile(request) => {
            operations::load_public_shared_file(connection, request)
                .map(SqliteOutput::PublicSharedFile)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadPublicSharedThumbnail(request) => {
            operations::load_public_shared_thumbnail(connection, request)
                .map(SqliteOutput::PublicSharedThumbnail)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadMediaBatch(request) => {
            operations::load_media_batch(connection, request)
                .map(SqliteOutput::MediaBatch)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadTimelinePage(request) => {
            operations::load_timeline_page(connection, request)
                .map(SqliteOutput::TimelinePage)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadTimelineMarkers(request) => {
            operations::load_timeline_markers(connection, request)
                .map(SqliteOutput::TimelineMarkers)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::MoveMediaToTrash(request) => {
            operations::move_media_to_trash(connection, request)
                .map(SqliteOutput::MediaMovedToTrash)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::QueueIncompleteMetadata => {
            operations::queue_incomplete_metadata(connection)
                .map(SqliteOutput::IncompleteMetadataQueued)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::ResetMetadataPage { cleanup_group_id } => {
            operations::reset_metadata_page(connection, cleanup_group_id.as_deref())
                .map(SqliteOutput::MetadataResetStep)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadMetadataJobStatus => operations::load_metadata_job_status(connection)
            .map(SqliteOutput::MetadataJobStatus)
            .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::ClaimNextMetadataJob { claim_token } => {
            operations::claim_next_metadata_job(connection, &claim_token)
                .map(SqliteOutput::MetadataJobClaimed)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadNextMetadataJobDelay => {
            operations::next_metadata_job_delay_seconds(connection)
                .map(SqliteOutput::NextMetadataJobDelay)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::FinishMetadataJob(request) => {
            operations::finish_metadata_job(connection, request)
                .map(|()| SqliteOutput::MetadataJobFinished)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::RecoverMetadataClaims => operations::recover_metadata_claims(connection)
            .map(SqliteOutput::MetadataClaimsRecovered)
            .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::LoadMetadataGenerationMedia { media_id } => {
            operations::load_metadata_generation_media(connection, media_id)
                .map(SqliteOutput::MetadataGenerationMedia)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::PersistMetadataGeneration(request) => {
            operations::persist_metadata_generation(connection, *request)
                .map_err(|error| map_sqlite_error(operation_name, error))?;
            Ok(SqliteOutput::MetadataGenerationPersisted)
        }
        SqliteOperation::PrepareLlmSubmissionCycle => {
            operations::prepare_llm_submission_cycle(connection)
                .map(|()| SqliteOutput::LlmSubmissionCyclePrepared)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::ClaimLlmSubmissionJobs { limit } => {
            operations::claim_llm_submission_jobs(connection, limit)
                .map(SqliteOutput::LlmSubmissionJobs)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadNextLlmSubmissionDelay => {
            operations::next_llm_submission_delay_seconds(connection)
                .map(SqliteOutput::NextLlmSubmissionDelay)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadLlmPreparedInputs { job_id } => {
            operations::load_llm_prepared_inputs(connection, &job_id)
                .map(SqliteOutput::LlmPreparedInputs)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::FinishLlmSubmission(request) => {
            operations::finish_llm_submission(connection, request)
                .map(|()| SqliteOutput::LlmSubmissionFinished)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadLlmCancellationBatch { limit } => {
            operations::load_llm_cancellation_batch(connection, limit)
                .map(SqliteOutput::LlmCancellationBatch)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::AcknowledgeLlmCancellation(request) => {
            operations::acknowledge_llm_cancellation(connection, request)
                .map(|()| SqliteOutput::LlmCancellationAcknowledged)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::PrepareLlmResultReceipt(request) => {
            operations::prepare_llm_result_receipt(connection, request)
                .map(SqliteOutput::LlmResultReceiptPrepared)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::CreateLlmResultReceipt(request) => {
            let reservation = capacity_token.take().ok_or_else(|| {
                ExecutorError::new(
                    ExecutorErrorKind::Internal,
                    operation_name,
                    "LLM result receipt is missing its provisional SQLite reservation",
                )
            })?;
            operations::create_llm_result_receipt(connection, *request, reservation)
                .map(SqliteOutput::LlmResultReceiptCreated)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::CommitLlmResultReceipt(request) => {
            operations::commit_llm_result_receipt(connection, *request)
                .map(SqliteOutput::LlmResultReceiptCommitted)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::StageLlmResultPage(request) => {
            let capacity = durable_capacity.as_ref().ok_or_else(|| {
                ExecutorError::new(
                    ExecutorErrorKind::Internal,
                    operation_name,
                    "LLM staging is missing its durable SQLite capacity child",
                )
            })?;
            operations::stage_llm_result_page(connection, *request, capacity)
                .map(SqliteOutput::LlmResultPageStaged)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::SelectLlmResultStagingCleanup { limit } => {
            operations::select_llm_result_staging_cleanup(connection, i64::from(limit))
                .map(SqliteOutput::LlmResultStagingCleanup)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::CleanupLlmResultStagingPage { job_id, limit } => {
            operations::cleanup_llm_result_staging_page(connection, &job_id, i64::from(limit))
                .map(SqliteOutput::LlmResultStagingCleaned)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::FinalizeLlmResultCleanup { job_id } => {
            let reservation_id = operations::finalize_llm_result_cleanup(connection, &job_id)
                .map_err(|error| map_sqlite_error(operation_name, error))?;
            let Some(reservation_id) = reservation_id else {
                return Ok(SqliteOutput::LlmResultCleanupFinalized(false));
            };
            let allocated = crate::io::space_budget::measure_sqlite_allocation(database_path)
                .map_err(|error| {
                    ExecutorError::new(
                        ExecutorErrorKind::Internal,
                        operation_name,
                        error.to_string(),
                    )
                })?;
            space_budget
                .release_sqlite_after_terminal_commit(&reservation_id, allocated)
                .map_err(|error| {
                    ExecutorError::new(
                        ExecutorErrorKind::Internal,
                        operation_name,
                        error.to_string(),
                    )
                })?;
            Ok(SqliteOutput::LlmResultCleanupFinalized(true))
        }
        SqliteOperation::LoadLlmResultStagingPage {
            job_id,
            attempt,
            after_record_sequence,
            claim_token,
            limit,
        } => crate::processor::ai::result::load_staging_page(
            connection,
            &job_id,
            attempt,
            after_record_sequence,
            &claim_token,
            i64::from(limit),
        )
        .map(SqliteOutput::LlmResultStagingPage)
        .map_err(|error| map_result_app_error(operation_name, error)),
        SqliteOperation::ReleaseLlmResultClaim {
            job_id,
            claim_token,
        } => operations::release_llm_result_claim(connection, &job_id, &claim_token)
            .map(SqliteOutput::LlmResultClaimReleased)
            .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::RecoverLlmResultState => {
            let outcome = operations::recover_llm_result_state(connection)
                .map_err(|error| map_sqlite_error(operation_name, error))?;
            if !outcome.released_active_reservation_ids.is_empty() {
                let allocated = crate::io::space_budget::measure_sqlite_allocation(database_path)
                    .map_err(|error| {
                    ExecutorError::new(
                        ExecutorErrorKind::Internal,
                        operation_name,
                        error.to_string(),
                    )
                })?;
                for reservation_id in &outcome.released_active_reservation_ids {
                    space_budget
                        .release_sqlite_after_terminal_commit(reservation_id, allocated)
                        .map_err(|error| {
                            ExecutorError::new(
                                ExecutorErrorKind::Internal,
                                operation_name,
                                error.to_string(),
                            )
                        })?;
                }
            }
            Ok(SqliteOutput::LlmResultStateRecovered(outcome))
        }
        SqliteOperation::RejectLlmResultReceipt(request) => {
            operations::reject_llm_result_receipt(connection, request)
                .map(SqliteOutput::LlmResultReceiptRejected)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadFacePreparationContext { job_id, media_id } => {
            crate::processor::face_detection::load_preparation_context_on_connection(
                connection, &job_id, media_id,
            )
            .map(SqliteOutput::FacePreparationContext)
            .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::SelectLlmResultCandidates { limit } => {
            crate::processor::ai::result::select_result_candidates(connection, i64::from(limit))
                .map(SqliteOutput::LlmResultCandidates)
                .map_err(|error| map_result_app_error(operation_name, error))
        }
        SqliteOperation::PersistPreparedLlmResult(prepared) => {
            crate::processor::ai::result::persist_prepared_result(
                connection,
                prepared,
                durable_capacity.as_ref(),
            )
            .map(SqliteOutput::PreparedLlmResultPersisted)
            .map_err(|error| map_result_app_error(operation_name, error))
        }
        SqliteOperation::LoadMetadataAiInputVerification { media_id } => {
            operations::load_metadata_ai_input_verification(connection, media_id)
                .map(SqliteOutput::MetadataAiInputVerification)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadBinaryMedia(request) => {
            operations::load_binary_media(connection, request)
                .map(SqliteOutput::BinaryMedia)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::PrepareMediaUpdate(request) => {
            operations::prepare_media_update(connection, request)
                .map(SqliteOutput::MediaUpdatePrepared)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::FinalizeMediaUpdate(request) => {
            operations::finalize_media_update(connection, request)
                .map(Box::new)
                .map(SqliteOutput::MediaUpdateFinalized)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadTrash { user_id } => operations::load_trash(connection, user_id)
            .map(SqliteOutput::Trash)
            .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::RestoreTrash(request) => operations::restore_trash(connection, request)
            .map(SqliteOutput::TrashRestored)
            .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::DeleteTrashMedia(request) => {
            operations::delete_trash_media(connection, request)
                .map(SqliteOutput::TrashDeleted)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::DeleteTrashPage(request) => {
            operations::delete_trash_page(connection, request)
                .map(SqliteOutput::TrashDeleted)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::DeleteExpiredTrashPage(request) => {
            operations::delete_expired_trash_page(connection, request)
                .map(SqliteOutput::TrashDeleted)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadFaceGroupsPage(request) => {
            operations::load_face_groups_page(connection, request)
                .map(SqliteOutput::FaceGroupsPage)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadFaceGroup(request) => operations::load_face_group(connection, request)
            .map(SqliteOutput::FaceGroup)
            .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::LoadVisibleFaceRepresentative {
            face_group_id,
            user_id,
            config,
        } => crate::processor::face_detection::visible_representative_crop(
            connection,
            face_group_id,
            user_id,
            &config,
        )
        .map(SqliteOutput::VisibleFaceRepresentative)
        .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::MergeFaceGroups { group_ids, config } => {
            crate::processor::face_detection::merge_groups(connection, group_ids, &config)
                .map(SqliteOutput::FaceGroupsMerged)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadAiStatus { config, schedules } => {
            crate::processor::ai::operation::status_on_connection(&config, connection, schedules)
                .map(Box::new)
                .map(SqliteOutput::AiStatus)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::StartAiFeature {
            feature,
            trigger,
            scheduled_for,
        } => crate::processor::ai::operation::start_feature_on_connection(
            connection,
            feature,
            &trigger,
            scheduled_for.as_deref(),
        )
        .map(SqliteOutput::AiFeatureStarted)
        .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::CancelAiFeature { feature } => {
            crate::processor::ai::operation::cancel_feature_on_connection(connection, feature)
                .map(SqliteOutput::AiFeatureCancelled)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::CancelAllAiFeatures => {
            crate::processor::ai::operation::cancel_all_on_connection(connection)
                .map(SqliteOutput::AllAiFeaturesCancelled)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::CleanAiFeature {
            feature,
            cleanup_group_id,
        } => crate::processor::ai::operation::clean_feature_on_connection(
            connection,
            feature,
            &cleanup_group_id,
        )
        .map(SqliteOutput::AiFeatureCleaned)
        .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::LoadDeduplicateScheduleState => {
            operations::load_deduplicate_schedule_state(connection)
                .map(SqliteOutput::DeduplicateScheduleState)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::RecoverDeduplicateRuns => operations::recover_deduplicate_runs(connection)
            .map(|()| SqliteOutput::DeduplicateRunsRecovered)
            .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::LoadDeduplicateFinalizationWork => {
            crate::processor::deduplicator::load_finalization_work(connection)
                .map(SqliteOutput::DeduplicateFinalizationWork)
                .map_err(|error| map_result_app_error(operation_name, error))
        }
        SqliteOperation::CommitDeduplicateCpuResult(result) => {
            crate::processor::deduplicator::commit_cpu_result(connection, result)
                .map(|()| SqliteOutput::DeduplicateCpuResultCommitted)
                .map_err(|error| map_result_app_error(operation_name, error))
        }
        SqliteOperation::RecoverFaceGroupingRuns => {
            operations::recover_face_grouping_runs(connection)
                .map(|()| SqliteOutput::FaceGroupingRunsRecovered)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadFaceGroupFinalizationWork(config) => {
            crate::processor::face_detection::load_finalization_work(connection, &config)
                .map(SqliteOutput::FaceGroupFinalizationWork)
                .map_err(|error| map_result_app_error(operation_name, error))
        }
        SqliteOperation::CommitFaceGroupCpuResult(result) => {
            crate::processor::face_detection::commit_cpu_result(connection, result)
                .map(|()| SqliteOutput::FaceGroupCpuResultCommitted)
                .map_err(|error| map_result_app_error(operation_name, error))
        }
        SqliteOperation::LoadFaceRepresentativeGroupPage(request) => {
            operations::load_face_representative_group_page(connection, request)
                .map(SqliteOutput::FaceRepresentativeGroupPage)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadFaceRepresentativeCandidatePage(request) => {
            operations::load_face_representative_candidate_page(connection, request)
                .map(SqliteOutput::FaceRepresentativeCandidatePage)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::UpdateFaceRepresentative(request) => {
            operations::update_face_representative(connection, request)
                .map(|()| SqliteOutput::FaceRepresentativeUpdated)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::InvalidateWebdavReadiness(request) => {
            operations::invalidate_webdav_readiness(connection, request)
                .map(|()| SqliteOutput::WebdavReadinessInvalidated)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::MarkWebdavReady(request) => {
            operations::mark_webdav_ready(connection, request)
                .map(|()| SqliteOutput::WebdavReadyMarked)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::RegisterBackupDevice(request) => {
            operations::register_backup_device(connection, request)
                .map(|()| SqliteOutput::BackupDeviceRegistered)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::CreateBackupUpload(request) => {
            operations::create_backup_upload(connection, request)
                .map(SqliteOutput::BackupUploadCreated)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadBackupUpload(request) => {
            operations::load_backup_upload(connection, request)
                .map(SqliteOutput::BackupUpload)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::PrepareBackupCompletion(request) => {
            operations::prepare_backup_completion(connection, request)
                .map(SqliteOutput::BackupCompletionPrepared)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::QueueBackupCompletion(request) => {
            operations::queue_backup_completion(connection, request)
                .map(SqliteOutput::BackupCompletionQueued)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::ClaimBackupChunk(request) => {
            operations::claim_backup_chunk(connection, request)
                .map(SqliteOutput::BackupChunkClaimed)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::FinishBackupChunk(request) => {
            operations::finish_backup_chunk(connection, request)
                .map(SqliteOutput::BackupChunkFinished)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::AbandonBackupChunk(request) => {
            operations::abandon_backup_chunk(connection, request)
                .map(|()| SqliteOutput::BackupChunkAbandoned)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::CancelBackupUpload(request) => {
            operations::cancel_backup_upload(connection, request)
                .map(SqliteOutput::BackupUploadCancelled)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::RecoverBackupWritingSessions => {
            operations::recover_backup_writing_sessions(connection)
                .map(SqliteOutput::BackupWritingSessionsRecovered)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadBackupResumablePage(request) => {
            operations::load_backup_resumable_page(connection, request)
                .map(SqliteOutput::BackupResumablePage)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadBackupProcessingPage(request) => {
            operations::load_backup_processing_page(connection, request)
                .map(SqliteOutput::BackupProcessingPage)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::MaintainBackupSessions => operations::maintain_backup_sessions(connection)
            .map(SqliteOutput::BackupSessionsMaintained)
            .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::ClaimBackupAsset => operations::claim_backup_asset(connection)
            .map(SqliteOutput::BackupAssetClaimed)
            .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::LoadRecoveredBackupMedia {
            content_hash,
            user_id,
        } => operations::load_recovered_backup_media(connection, &content_hash, user_id)
            .map(SqliteOutput::RecoveredBackupMedia)
            .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::StoreBackupContentHash(request) => {
            operations::store_backup_content_hash(connection, request)
                .map(SqliteOutput::BackupContentHashStored)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::TransitionBackupProcessing(request) => {
            operations::transition_backup_processing(connection, request)
                .map(SqliteOutput::BackupProcessingTransitioned)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::CreateImportJob { source } => {
            crate::processor::import::create_import_job_on_connection(connection, source)
                .map(SqliteOutput::ImportJobCreated)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadImportStatus { source } => {
            crate::processor::import::get_import_status_on_connection(connection, source)
                .map(SqliteOutput::ImportStatus)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::SetImportJobTotal {
            job_id,
            total_files,
        } => crate::processor::import::set_import_job_total_on_connection(
            connection,
            job_id,
            total_files,
        )
        .map(SqliteOutput::ImportJobTotalSet)
        .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::RecordImportProgress {
            job_id,
            success,
            error_message,
        } => crate::processor::import::record_import_progress_on_connection(
            connection,
            job_id,
            success,
            &error_message,
        )
        .map(SqliteOutput::ImportProgressRecorded)
        .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::CompleteImportJob { job_id } => {
            crate::processor::import::complete_import_job_on_connection(connection, job_id)
                .map(SqliteOutput::ImportJobCompleted)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::AllocateImportMedia(request) => {
            crate::processor::import::allocate_import_media_on_connection(connection, request)
                .map(SqliteOutput::ImportMediaAllocated)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::FinalizeImportMedia(request) => {
            crate::processor::import::finalize_import_media_on_connection(connection, request)
                .map(SqliteOutput::ImportMediaFinalized)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::MarkImportMediaFailed { media_id, error } => {
            crate::processor::import::mark_import_media_failed_on_connection(
                connection, media_id, &error,
            )
            .map(SqliteOutput::ImportMediaFailed)
            .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::AbsorbExistingMedia(request) => {
            crate::processor::import::absorb_existing_media_on_connection(connection, request)
                .map(|()| SqliteOutput::ExistingMediaAbsorbed)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::RecoverInterruptedImportPage {
            after_media_id,
            limit,
        } => crate::processor::import::recover_interrupted_import_page_on_connection(
            connection,
            after_media_id,
            limit,
        )
        .map(SqliteOutput::InterruptedImportPage)
        .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::LoadWebdavReadyPage {
            after_user_id,
            after_file_path,
            limit,
        } => crate::processor::import::load_webdav_ready_page_on_connection(
            connection,
            after_user_id,
            &after_file_path,
            limit,
        )
        .map(SqliteOutput::WebdavReadyPage)
        .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::CheckWebdavReady { user_id, file_path } => {
            crate::processor::import::webdav_file_is_ready_on_connection(
                connection, user_id, &file_path,
            )
            .map(SqliteOutput::WebdavReadyChecked)
            .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::UpdateWebdavReadyPaths(request) => {
            crate::processor::import::update_webdav_ready_paths_on_connection(connection, request)
                .map(|()| SqliteOutput::WebdavReadyPathsUpdated)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::AcquireImportContentHashClaim {
            content_hash,
            claim_token,
            source,
        } => crate::processor::import::acquire_content_hash_claim_on_connection(
            connection,
            &content_hash,
            &claim_token,
            source,
        )
        .map(SqliteOutput::ImportContentHashClaimed)
        .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::ReleaseImportContentHashClaim {
            content_hash,
            claim_token,
        } => crate::processor::import::release_content_hash_claim_on_connection(
            connection,
            &content_hash,
            &claim_token,
        )
        .map(SqliteOutput::ImportContentHashClaimReleased)
        .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::RecoverImportContentHashClaims => {
            crate::processor::import::recover_content_hash_claims_on_connection(connection)
                .map(SqliteOutput::ImportContentHashClaimsRecovered)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::PrepareFileOperation(plan) => {
            crate::io::journal::prepare_file_operation(connection, plan)
                .map(SqliteOutput::FileOperationPrepared)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::PrepareDirectoryCopyOperation(request) => {
            let (plan, construction) = *request;
            crate::io::journal::prepare_directory_copy_operation(connection, plan, construction)
                .map(SqliteOutput::DirectoryCopyPrepared)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadDirectoryCopy { group_id } => {
            crate::io::journal::load_directory_copy(connection, group_id.as_deref())
                .map(SqliteOutput::NextDirectoryCopy)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::CheckpointDirectoryCopyEntry(checkpoint) => {
            crate::io::journal::checkpoint_directory_copy_entry(connection, checkpoint)
                .map(SqliteOutput::DirectoryCopyEntryCheckpointed)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::CheckpointDirectoryCopyFinished(checkpoint) => {
            crate::io::journal::checkpoint_directory_copy_finished(connection, checkpoint)
                .map(SqliteOutput::DirectoryCopyFinishedCheckpointed)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::BeginFileOperationPublication {
            group_id,
            expected_version,
        } => crate::io::journal::begin_file_operation_publication(
            connection,
            &group_id,
            expected_version,
        )
        .map(SqliteOutput::FileOperationPublicationBegun)
        .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::VerifyFileOperationPublication {
            group_id,
            expected_version,
        } => crate::io::journal::verify_file_operation_publication(
            connection,
            &group_id,
            expected_version,
        )
        .map(SqliteOutput::FileOperationPublicationVerified)
        .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::RecordFileEntryPublished {
            group_id,
            expected_version,
            sequence,
        } => crate::io::journal::record_file_entry_published(
            connection,
            &group_id,
            expected_version,
            sequence,
        )
        .map(SqliteOutput::FileEntryPublished)
        .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::CompleteFileOperation {
            group_id,
            expected_version,
        } => {
            let outcome = crate::io::journal::complete_file_operation(
                connection,
                &group_id,
                expected_version,
            )
            .map_err(|error| map_sqlite_error(operation_name, error))?;
            if matches!(outcome, JournalCheckpointOutcome::Advanced { .. }) {
                release_journal_space(space_budget, &group_id, operation_name)?;
            }
            Ok(SqliteOutput::FileOperationCompleted(outcome))
        }
        SqliteOperation::VerifyFileOperationCleanup {
            group_id,
            expected_version,
        } => crate::io::journal::verify_file_operation_cleanup(
            connection,
            &group_id,
            expected_version,
        )
        .map(SqliteOutput::FileOperationCleanupVerified)
        .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::RecordFileEntryCleaned {
            group_id,
            expected_version,
            sequence,
        } => {
            let checkpoint = crate::io::journal::record_file_entry_cleaned(
                connection,
                &group_id,
                expected_version,
                sequence,
            )
            .map_err(|error| map_sqlite_error(operation_name, error))?;
            if checkpoint.is_some_and(|value| value.phase_complete) {
                release_journal_space(space_budget, &group_id, operation_name)?;
            }
            Ok(SqliteOutput::FileEntryCleaned(checkpoint))
        }
        SqliteOperation::LoadNextGenericFileOperationRecovery => {
            crate::io::journal::load_next_generic_recovery_group(connection)
                .map(SqliteOutput::GenericFileOperationRecovery)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::YieldFileOperationProgress {
            group_id,
            expected_version,
        } => crate::io::journal::yield_file_operation_progress(
            connection,
            &group_id,
            expected_version,
        )
        .map(SqliteOutput::FileOperationProgressYielded)
        .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::RecordFileOperationFailure {
            group_id,
            expected_version,
            sequence,
            stage,
            error_kind,
            error,
        } => crate::io::journal::record_file_operation_failure(
            connection,
            &group_id,
            expected_version,
            sequence,
            stage,
            &error_kind,
            &error,
        )
        .map(SqliteOutput::FileOperationFailureRecorded)
        .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::RecordFileOperationFinalizationFailure {
            group_id,
            expected_version,
            error_kind,
            error,
        } => crate::io::journal::record_file_operation_finalization_failure(
            connection,
            &group_id,
            expected_version,
            &error_kind,
            &error,
        )
        .map(SqliteOutput::FileOperationFailureRecorded)
        .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::RetryFileOperation {
            retry_request_id,
            group_id,
            expected_version,
            request_hash,
        } => crate::io::journal::retry_file_operation(
            connection,
            &retry_request_id,
            &group_id,
            expected_version,
            request_hash,
        )
        .map(SqliteOutput::FileOperationRetried)
        .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::ListFileOperations {
            states,
            cursor,
            limit,
        } => crate::io::journal::list_file_operations(connection, states, cursor, limit)
            .map(SqliteOutput::FileOperationsListed)
            .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::LoadFileOperationDetail { group_id } => {
            crate::io::journal::load_file_operation_detail(connection, &group_id)
                .map(Box::new)
                .map(SqliteOutput::FileOperationDetail)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::MaintainFileOperationJournal => {
            crate::io::journal::maintain_file_operation_journal(connection)
                .map(SqliteOutput::FileOperationJournalMaintained)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::LoadFileOperationCancellationStatus { group_id } => {
            crate::io::journal::load_file_operation_cancellation_status(connection, &group_id)
                .map(SqliteOutput::FileOperationCancellationStatus)
                .map_err(|error| map_sqlite_error(operation_name, error))
        }
        SqliteOperation::RequestFileOperationCancellation {
            group_id,
            expected_version,
        } => {
            let outcome = crate::io::journal::request_file_operation_cancellation(
                connection,
                &group_id,
                expected_version,
            )
            .map_err(|error| map_sqlite_error(operation_name, error))?;
            if matches!(
                &outcome,
                JournalCancellationOutcome::Requested { state, .. } if state == "rolled_back"
            ) {
                release_journal_space(space_budget, &group_id, operation_name)?;
                release_rolled_back_sqlite_result_space(
                    connection,
                    space_budget,
                    database_path,
                    &group_id,
                    operation_name,
                )?;
            }
            Ok(SqliteOutput::FileOperationCancellationRequested(outcome))
        }
        SqliteOperation::VerifyFileOperationRollback {
            group_id,
            expected_version,
        } => crate::io::journal::verify_file_operation_rollback(
            connection,
            &group_id,
            expected_version,
        )
        .map(SqliteOutput::FileOperationRollbackVerified)
        .map_err(|error| map_sqlite_error(operation_name, error)),
        SqliteOperation::RecordFileEntryRolledBack {
            group_id,
            expected_version,
            sequence,
        } => {
            let checkpoint = crate::io::journal::record_file_entry_rolled_back(
                connection,
                &group_id,
                expected_version,
                sequence,
            )
            .map_err(|error| map_sqlite_error(operation_name, error))?;
            if checkpoint.is_some_and(|value| value.phase_complete) {
                release_journal_space(space_budget, &group_id, operation_name)?;
                release_rolled_back_sqlite_result_space(
                    connection,
                    space_budget,
                    database_path,
                    &group_id,
                    operation_name,
                )?;
            }
            Ok(SqliteOutput::FileEntryRolledBack(checkpoint))
        }
    }
}

fn release_journal_space(
    budget: &crate::io::space_budget::DataDirSpaceBudget,
    group_id: &str,
    operation: &'static str,
) -> Result<(), ExecutorError> {
    budget
        .release_journal_after_terminal_commit(group_id)
        .map(|_| ())
        .map_err(|error| {
            ExecutorError::new(ExecutorErrorKind::Internal, operation, error.to_string())
        })
}

fn release_rolled_back_sqlite_result_space(
    connection: &rusqlite::Connection,
    budget: &crate::io::space_budget::DataDirSpaceBudget,
    database_path: &std::path::Path,
    group_id: &str,
    operation: &'static str,
) -> Result<(), ExecutorError> {
    connection
        .execute(
            crate::database::queries::file_operations::RELEASE_ROLLED_BACK_SQLITE_RESULT_RESERVATION,
            [group_id],
        )
        .map_err(|error| map_sqlite_error(operation, error))?;
    let reservation_id = connection
        .query_row(
            crate::database::queries::file_operations::SELECT_LINKED_RELEASED_SQLITE_RESULT_RESERVATION,
            [group_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| map_sqlite_error(operation, error))?;
    let Some(reservation_id) = reservation_id else {
        return Ok(());
    };
    let allocated =
        crate::io::space_budget::measure_sqlite_allocation(database_path).map_err(|error| {
            ExecutorError::new(ExecutorErrorKind::Internal, operation, error.to_string())
        })?;
    budget
        .release_sqlite_after_terminal_commit(&reservation_id, allocated)
        .map_err(|error| {
            ExecutorError::new(ExecutorErrorKind::Internal, operation, error.to_string())
        })?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| map_sqlite_error(operation, error))?;
    let receipt_deleted = transaction
        .execute(
            crate::database::queries::file_operations::DELETE_REPLAYABLE_RESULT_RECEIPT_AFTER_TERMINATION,
            [group_id],
        )
        .map_err(|error| map_sqlite_error(operation, error))?;
    if receipt_deleted == 1
        && transaction
            .execute(
                crate::database::queries::file_operations::DELETE_RELEASED_RESULT_RESERVATION,
                [&reservation_id],
            )
            .map_err(|error| map_sqlite_error(operation, error))?
            != 1
    {
        return Err(ExecutorError::new(
            ExecutorErrorKind::Internal,
            operation,
            "replayable LLM result receipt released without retiring its SQLite reservation",
        ));
    }
    transaction
        .commit()
        .map_err(|error| map_sqlite_error(operation, error))?;
    Ok(())
}

fn map_result_app_error(operation: &'static str, error: crate::error::AppError) -> ExecutorError {
    match error {
        crate::error::AppError::Database(error) => map_sqlite_error(operation, error),
        crate::error::AppError::DatabaseBusy => ExecutorError::new(
            ExecutorErrorKind::DatabaseBusy,
            operation,
            "database is busy",
        ),
        crate::error::AppError::BadRequest(detail) => {
            ExecutorError::new(ExecutorErrorKind::BadRequest, operation, detail)
        }
        crate::error::AppError::Conflict(detail) => {
            ExecutorError::new(ExecutorErrorKind::Conflict, operation, detail)
        }
        crate::error::AppError::NotFound(detail) => {
            ExecutorError::new(ExecutorErrorKind::NotFound, operation, detail)
        }
        crate::error::AppError::Validation(detail) => {
            ExecutorError::new(ExecutorErrorKind::InvalidInput, operation, detail)
        }
        error => ExecutorError::new(ExecutorErrorKind::Internal, operation, error.to_string()),
    }
}

fn map_sqlite_error(operation: &'static str, error: rusqlite::Error) -> ExecutorError {
    let kind = match &error {
        rusqlite::Error::SqliteFailure(database_error, _)
            if database_error.code == ErrorCode::OperationInterrupted =>
        {
            ExecutorErrorKind::DatabaseTimeout
        }
        rusqlite::Error::SqliteFailure(database_error, _)
            if matches!(
                database_error.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) =>
        {
            ExecutorErrorKind::DatabaseBusy
        }
        rusqlite::Error::SqliteFailure(database_error, _)
            if matches!(
                database_error.code,
                ErrorCode::PermissionDenied
                    | ErrorCode::ReadOnly
                    | ErrorCode::DatabaseCorrupt
                    | ErrorCode::SchemaChanged
                    | ErrorCode::ConstraintViolation
                    | ErrorCode::TypeMismatch
                    | ErrorCode::AuthorizationForStatementDenied
                    | ErrorCode::NotADatabase
            ) =>
        {
            ExecutorErrorKind::DatabasePermanent
        }
        _ => ExecutorErrorKind::Database,
    };
    ExecutorError::new(kind, operation, error.to_string())
}
