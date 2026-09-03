use futures::stream::{self, StreamExt};
use rusqlite::OptionalExtension;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::warn;

use crate::config::Config;
use crate::constants::{image_mime_type, video_mime_type, IMAGE_EXTENSIONS, VIDEO_EXTENSIONS};
use crate::database::queries;
use crate::error::{AppError, AppResult};
use crate::executor::process::bounded_error_detail;
use crate::executor::{ExecutorErrorKind, SqliteExecutorHandle, StorageDirectoryEntryKind};
use crate::io::file::{NormalizedStoragePath, StorageRootId};
use crate::io::file::{PathClaimMode, PathClaimScope};
use crate::io::journal::{
    FileEntryAction, FileEntryPlan, FileOperationPlan, FilePathClaimPlan, JournalCheckpointOutcome,
    JournalSpaceReservationPlan, PrepareJournalOutcome,
};
use crate::io::{StorageFileSession, StorageFileSnapshot};
use crate::processor::media_processor::build_original_filename;
use crate::processor::metadata::supplemental_metadata_candidates;
use crate::runtime::{DurableAdmission, DurableSourceId, SchedulerAdmissionKind};

#[derive(Debug, Clone)]
pub(crate) struct ExistingMedia {
    pub(crate) id: i64,
    pub(crate) file_path: String,
    pub(crate) import_state: String,
}

#[derive(Debug, Clone)]
pub(crate) enum ImportContentHashClaimOutcome {
    Acquired,
    Busy,
    Existing(ExistingMedia),
}

#[derive(Debug)]
pub enum ImportStagedFileOutcome {
    Completed(i64),
    Deferred(Box<PreparedStagedImport>),
}

impl ImportStagedFileOutcome {
    pub fn completed_media_id(self) -> Option<i64> {
        match self {
            Self::Completed(media_id) => Some(media_id),
            Self::Deferred(_) => None,
        }
    }
}

#[derive(Debug)]
pub struct PreparedStagedImport {
    source: StagedImportFile,
    source_session: StorageFileSession,
    source_snapshot: StorageFileSnapshot,
    import_source: ImportSource,
    user_id: i64,
    cleanup: StagedImportCleanup,
    media_type: &'static str,
    original_filename: String,
    mime_type: String,
    source_size: u64,
    source_modified_seconds: Option<i64>,
    content_hash: String,
    supplemental_metadata: Option<PreparedSupplementalMetadata>,
}

#[derive(Debug)]
struct PreparedSupplementalMetadata {
    path: NormalizedStoragePath,
    snapshot: StorageFileSnapshot,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct StagedImportFile {
    pub storage_root: StorageRootId,
    pub path: NormalizedStoragePath,
}

impl StagedImportFile {
    pub fn new(storage_root: StorageRootId, path: NormalizedStoragePath) -> AppResult<Self> {
        validate_staged_import_root(storage_root)?;
        Ok(Self { storage_root, path })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StagedImportCleanup {
    pub source: bool,
    pub supplemental_metadata: bool,
}

struct WebDavImportCandidate {
    source: StagedImportFile,
    ready_file_path: String,
    supplemental_ready_file_path: Option<String>,
}

pub(crate) enum ImportTarget {
    New {
        media_id: i64,
        temporary_relative_path: PathBuf,
    },
    Existing(ExistingMedia),
}

#[derive(Debug)]
pub(crate) struct AllocateImportMedia {
    pub user_id: i64,
    pub temporary_filename: String,
    pub original_filename: String,
    pub temporary_relative_path: String,
    pub media_type: String,
    pub mime_type: String,
    pub source_size: i64,
    pub content_hash: String,
    pub source_modified_seconds: Option<i64>,
    pub import_source: ImportSource,
}

#[derive(Debug)]
pub(crate) struct FinalizeImportMedia {
    pub media_id: i64,
    pub user_id: i64,
    pub final_filename: String,
    pub final_relative_path: String,
    pub product_group_id: String,
    pub product_group_version: i64,
    pub claim_token: String,
    pub source_cleanup: Option<FileOperationPlan>,
}

struct CommittedImportProduct {
    group_id: String,
    version: i64,
}

struct ImportProductPublication<'a> {
    source: StorageFileSession,
    temporary_path: NormalizedStoragePath,
    destination_path: NormalizedStoragePath,
    source_snapshot: StorageFileSnapshot,
    media_id: i64,
    claim_token: &'a str,
    content_hash: [u8; 32],
    supplemental_metadata: Option<&'a PreparedSupplementalMetadata>,
}

#[derive(Debug)]
pub(crate) struct AbsorbExistingMediaDatabase {
    pub media_id: i64,
    pub user_id: i64,
    pub source_modified_seconds: Option<i64>,
    pub request_metadata_rerun: bool,
    pub source_cleanup: Option<FileOperationPlan>,
}

#[derive(Debug)]
pub(crate) struct InterruptedImport {
    pub media_id: i64,
}

#[derive(Debug)]
pub(crate) struct WebdavReadyFile {
    pub user_id: i64,
    pub username: String,
    pub file_path: String,
}

#[derive(Debug)]
pub(crate) struct UpdateWebdavReadyPaths {
    pub user_id: i64,
    pub remove: Vec<String>,
    pub add: Vec<String>,
}

pub(crate) fn set_import_job_total_on_connection(
    connection: &rusqlite::Connection,
    job_id: i64,
    total_files: i64,
) -> rusqlite::Result<bool> {
    connection
        .execute(
            queries::import::SET_JOB_TOTAL,
            rusqlite::params![total_files, job_id],
        )
        .map(|updated| updated == 1)
}

pub(crate) fn record_import_progress_on_connection(
    connection: &rusqlite::Connection,
    job_id: i64,
    success: bool,
    error_message: &str,
) -> rusqlite::Result<bool> {
    let transaction = connection.unchecked_transaction()?;
    let updated = transaction.execute(
        queries::import::UPDATE_JOB_PROGRESS,
        rusqlite::params![success, success, error_message, error_message, job_id],
    )?;
    if updated == 1 && !success && !error_message.is_empty() {
        transaction.execute(
            queries::import::INSERT_JOB_ERROR,
            rusqlite::params![job_id, error_message],
        )?;
    }
    transaction.commit()?;
    Ok(updated == 1)
}

pub(crate) fn complete_import_job_on_connection(
    connection: &rusqlite::Connection,
    job_id: i64,
) -> rusqlite::Result<bool> {
    connection
        .execute(queries::import::COMPLETE_JOB, [job_id])
        .map(|updated| updated == 1)
}

pub(crate) fn allocate_import_media_on_connection(
    connection: &rusqlite::Connection,
    request: AllocateImportMedia,
) -> rusqlite::Result<ImportTarget> {
    let rows = connection.execute(
        queries::import::INSERT_IMPORTING_MEDIA,
        rusqlite::params![
            request.user_id,
            request.temporary_filename,
            request.original_filename,
            request.temporary_relative_path,
            request.media_type,
            request.mime_type,
            request.source_size,
            request.content_hash,
            request.source_modified_seconds,
            request.import_source.as_str(),
        ],
    )?;
    if rows == 0 {
        return select_existing_media(connection, &request.content_hash)?.map_or_else(
            || Err(rusqlite::Error::InvalidQuery),
            |existing| Ok(ImportTarget::Existing(existing)),
        );
    }
    if rows != 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(ImportTarget::New {
        media_id: connection.last_insert_rowid(),
        temporary_relative_path: PathBuf::from(request.temporary_relative_path),
    })
}

pub(crate) fn finalize_import_media_on_connection(
    connection: &rusqlite::Connection,
    request: FinalizeImportMedia,
) -> rusqlite::Result<bool> {
    let transaction = connection.unchecked_transaction()?;
    if transaction.execute(
        queries::import::FINALIZE_PRODUCT,
        rusqlite::params![
            &request.product_group_id,
            request.product_group_version,
            request.media_id,
            request.claim_token,
        ],
    )? != 1
    {
        transaction.rollback()?;
        return Ok(false);
    }
    let changed = transaction.execute(
        queries::import::MARK_IMPORTED,
        rusqlite::params![
            request.final_filename,
            request.final_relative_path,
            request.media_id,
        ],
    )?;
    if changed != 1 {
        transaction.rollback()?;
        return Ok(false);
    }
    transaction.execute(
        queries::access::INSERT_MEDIA_ACCESS,
        rusqlite::params![request.media_id, request.user_id, 2],
    )?;
    transaction.execute(queries::metadata_jobs::INSERT_QUEUED, [request.media_id])?;
    transaction.execute(
        queries::file_operations::RELEASE_GROUP_CLAIMS,
        [&request.product_group_id],
    )?;
    if let Some(source_cleanup) = request.source_cleanup {
        if crate::io::journal::prepare_committed_cleanup(&transaction, source_cleanup)?
            == PrepareJournalOutcome::PathConflict
        {
            transaction.rollback()?;
            return Ok(false);
        }
    }
    transaction.commit()?;
    Ok(true)
}

pub(crate) fn mark_import_media_failed_on_connection(
    connection: &rusqlite::Connection,
    media_id: i64,
    error: &str,
) -> rusqlite::Result<bool> {
    connection
        .execute(
            queries::import::MARK_FAILED,
            rusqlite::params![error, media_id],
        )
        .map(|updated| updated == 1)
}

pub(crate) fn absorb_existing_media_on_connection(
    connection: &rusqlite::Connection,
    request: AbsorbExistingMediaDatabase,
) -> rusqlite::Result<()> {
    let transaction = connection.unchecked_transaction()?;
    if let Some(modified_seconds) = request.source_modified_seconds {
        transaction.execute(
            queries::import::UPDATE_EARLIER_CREATED_AT,
            rusqlite::params![modified_seconds, request.media_id, modified_seconds],
        )?;
    }
    transaction.execute(
        queries::access::INSERT_MEDIA_ACCESS,
        rusqlite::params![request.media_id, request.user_id, 2],
    )?;
    transaction.execute(
        queries::access::RESTORE_MEDIA_ACCESS,
        rusqlite::params![request.media_id, request.user_id],
    )?;
    if request.request_metadata_rerun {
        transaction.execute(queries::metadata_jobs::REQUEST_RERUN, [request.media_id])?;
    }
    if let Some(source_cleanup) = request.source_cleanup {
        if crate::io::journal::prepare_committed_cleanup(&transaction, source_cleanup)?
            == PrepareJournalOutcome::PathConflict
        {
            return Err(rusqlite::Error::StatementChangedRows(0));
        }
    }
    transaction.commit()
}

pub(crate) fn recover_interrupted_import_page_on_connection(
    connection: &rusqlite::Connection,
    after_media_id: i64,
    limit: u16,
) -> rusqlite::Result<Vec<InterruptedImport>> {
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(queries::import::RECORD_INTERRUPTED_JOB_ERRORS, [])?;
    transaction.execute(queries::import::FAIL_INTERRUPTED_JOBS, [])?;
    let imports = transaction
        .prepare(queries::import::SELECT_INTERRUPTED_PAGE)?
        .query_map(rusqlite::params![after_media_id, limit], |row| {
            Ok(InterruptedImport {
                media_id: row.get(0)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    transaction.commit()?;
    Ok(imports)
}

pub(crate) fn load_webdav_ready_page_on_connection(
    connection: &rusqlite::Connection,
    after_user_id: i64,
    after_file_path: &str,
    limit: u16,
) -> rusqlite::Result<Vec<WebdavReadyFile>> {
    connection
        .prepare(queries::webdav_ready::SELECT_IMPORT_PAGE)?
        .query_map(
            rusqlite::params![after_user_id, after_user_id, after_file_path, limit],
            |row| {
                Ok(WebdavReadyFile {
                    user_id: row.get(0)?,
                    username: row.get(1)?,
                    file_path: row.get(2)?,
                })
            },
        )?
        .collect()
}

pub(crate) fn webdav_file_is_ready_on_connection(
    connection: &rusqlite::Connection,
    user_id: i64,
    file_path: &str,
) -> rusqlite::Result<bool> {
    connection.query_row(
        queries::webdav_ready::EXISTS,
        rusqlite::params![user_id, file_path],
        |row| row.get(0),
    )
}

pub(crate) fn update_webdav_ready_paths_on_connection(
    connection: &rusqlite::Connection,
    request: UpdateWebdavReadyPaths,
) -> rusqlite::Result<()> {
    let transaction = connection.unchecked_transaction()?;
    for path in request.remove {
        transaction.execute(
            queries::webdav_ready::DELETE,
            rusqlite::params![request.user_id, path],
        )?;
    }
    for path in request.add {
        transaction.execute(
            queries::webdav_ready::UPSERT,
            rusqlite::params![request.user_id, path],
        )?;
    }
    transaction.commit()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportSource {
    Local,
    Webdav,
    MobileBackup,
}

impl ImportSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Webdav => "webdav",
            Self::MobileBackup => "mobile_backup",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum CreateImportJobOutcome {
    AlreadyRunning,
    Created(i64),
}

#[derive(Debug)]
pub struct ImportStatusSnapshot {
    pub job: ImportJob,
    pub total_media: i64,
}

#[derive(Debug, Clone)]
pub struct ImportJob {
    pub status: String,
    pub total_files: i64,
    pub processed_files: i64,
    pub successful_imports: i64,
    pub failed_imports: i64,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub errors: Vec<String>,
}

#[derive(Clone)]
pub struct ImportSettings {
    pub user_id: i64,
    pub executors: crate::runtime::ExecutorHandles,
    pub scheduler: crate::runtime::SchedulerHandle,
}

pub(crate) fn create_import_job_on_connection(
    connection: &rusqlite::Connection,
    source: ImportSource,
) -> rusqlite::Result<CreateImportJobOutcome> {
    match connection.execute(queries::import::INSERT_JOB, [source.as_str()]) {
        Ok(_) => Ok(CreateImportJobOutcome::Created(
            connection.last_insert_rowid(),
        )),
        Err(rusqlite::Error::SqliteFailure(database_error, _))
            if database_error.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            Ok(CreateImportJobOutcome::AlreadyRunning)
        }
        Err(error) => Err(error),
    }
}

pub(crate) fn get_import_status_on_connection(
    connection: &rusqlite::Connection,
    source: ImportSource,
) -> rusqlite::Result<ImportStatusSnapshot> {
    let job_row = connection
        .query_row(
            queries::import::SELECT_LATEST_JOB_FOR_SOURCE,
            [source.as_str()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    ImportJob {
                        status: row.get(1)?,
                        total_files: row.get(2)?,
                        processed_files: row.get(3)?,
                        successful_imports: row.get(4)?,
                        failed_imports: row.get(5)?,
                        started_at: row.get(6)?,
                        completed_at: row.get(7)?,
                        errors: Vec::new(),
                    },
                    row.get::<_, Option<String>>(8)?,
                ))
            },
        )
        .optional()?;
    let Some((job_id, mut job, last_error)) = job_row else {
        return Ok(ImportStatusSnapshot {
            job: ImportJob {
                status: "idle".to_string(),
                total_files: 0,
                processed_files: 0,
                successful_imports: 0,
                failed_imports: 0,
                started_at: None,
                completed_at: None,
                errors: Vec::new(),
            },
            total_media: connection.query_row(
                queries::import::COUNT_IMPORTED_MEDIA,
                [],
                |row| row.get(0),
            )?,
        });
    };
    let mut statement = connection.prepare(queries::import::SELECT_JOB_ERRORS)?;
    let mut rows = statement.query([job_id])?;
    let mut error_bytes = 0usize;
    while let Some(row) = rows.next()? {
        if job.errors.len() == 4096 {
            return Err(rusqlite::Error::InvalidParameterName(
                "import errors exceed 4096 rows".to_string(),
            ));
        }
        let error = row.get::<_, String>(0)?;
        error_bytes = error_bytes.checked_add(error.len()).ok_or_else(|| {
            rusqlite::Error::InvalidParameterName("import error size overflow".to_string())
        })?;
        if error_bytes > 1024 * 1024 {
            return Err(rusqlite::Error::InvalidParameterName(
                "import errors exceed one mebibyte".to_string(),
            ));
        }
        job.errors.push(error);
    }
    if job.errors.is_empty() {
        job.errors.extend(last_error);
    }
    let total_media =
        connection.query_row(queries::import::COUNT_IMPORTED_MEDIA, [], |row| row.get(0))?;
    Ok(ImportStatusSnapshot { job, total_media })
}

pub async fn run_local_import(settings: ImportSettings, job_id: i64) {
    let Ok(initial_worker) = settings
        .scheduler
        .acquire_durable(
            DurableSourceId::LocalImport,
            SchedulerAdmissionKind::NewClaim,
        )
        .await
    else {
        return;
    };
    let source_files = match collect_import_files(&settings.executors, StorageRootId::Imports).await
    {
        Ok(source_files) => source_files,
        Err(error) => {
            warn!(error = %error, "local import scan failed");
            let _ = settings
                .executors
                .sqlite
                .complete_import_job_durable(job_id)
                .await;
            return;
        }
    };
    if let Err(error) = settings
        .executors
        .sqlite
        .set_import_job_total_durable(job_id, source_files.len() as i64)
        .await
    {
        warn!(job_id, error = %error, "failed to persist local import total");
    }
    drop(initial_worker);

    let concurrency = settings.scheduler.durable_capacity();
    let mut imports = stream::iter(source_files)
        .map(|staged_source| {
            let settings = settings.clone();
            async move {
                let worker_permit = settings
                    .scheduler
                    .acquire_durable(
                        DurableSourceId::LocalImport,
                        SchedulerAdmissionKind::NewClaim,
                    )
                    .await;
                if worker_permit.is_err() {
                    let detail = format!(
                        "local import worker stopped before processing {}",
                        staged_source.path.relative_path()
                    );
                    warn!(path = %staged_source.path.relative_path(), error = %detail, "Local import failed");
                    update_import_progress(&settings.executors.sqlite, job_id, false, Some(detail)).await;
                    return;
                }
                let mut worker_permit = worker_permit.expect("worker permit was checked");
                let source_label = staged_source.path.relative_path().to_string();
                let mut attempt = import_staged_file(
                    staged_source,
                    ImportSource::Local,
                    settings.user_id,
                    &settings.executors,
                    StagedImportCleanup {
                        source: true,
                        supplemental_metadata: true,
                    },
                    &worker_permit,
                )
                .await;
                let import_result = loop {
                    match attempt {
                        Ok(ImportStagedFileOutcome::Deferred(prepared)) => {
                            drop(worker_permit);
                            tokio::task::yield_now().await;
                            worker_permit = match settings
                                .scheduler
                                .acquire_durable(
                                    DurableSourceId::LocalImport,
                                    SchedulerAdmissionKind::ExistingClaimCompletion,
                                )
                                .await
                            {
                                Ok(worker_permit) => worker_permit,
                                Err(error) => {
                                    break Err(AppError::Unavailable(error));
                                }
                            };
                            attempt = resume_staged_file_import(
                                *prepared,
                                &settings.executors,
                                &worker_permit,
                            )
                            .await;
                        }
                        Ok(ImportStagedFileOutcome::Completed(media_id)) => break Ok(media_id),
                        Err(error) => break Err(error),
                    }
                };
                match import_result {
                    Ok(_) => {
                        settings.scheduler.wake_metadata();
                        update_import_progress(&settings.executors.sqlite, job_id, true, None).await;
                    }
                    Err(error) => {
                        let detail = format!(
                            "local import failed for {}: {error}",
                            source_label
                        );
                        let detail = bounded_error_detail(&detail);
                        warn!(path = %source_label, error = %detail, "Local import failed");
                        update_import_progress(&settings.executors.sqlite, job_id, false, Some(detail)).await;
                    }
                }
            }
        })
        .buffer_unordered(concurrency);

    while imports.next().await.is_some() {}

    if let Err(error) = settings
        .executors
        .sqlite
        .complete_import_job_durable(job_id)
        .await
    {
        warn!(job_id, error = %error, "failed to complete local import job");
    }
}

async fn update_import_progress(
    sqlite: &SqliteExecutorHandle,
    job_id: i64,
    success: bool,
    error_message: Option<String>,
) {
    let error_message = error_message
        .map(|error| bounded_error_detail(&error))
        .unwrap_or_default();
    if let Err(error) = sqlite
        .record_import_progress_durable(job_id, success, error_message)
        .await
    {
        warn!(job_id, error = %error, "failed to persist import progress");
    }
}

/// Imports one completed on-disk file after its source-specific claim and preparation.
pub async fn import_staged_file(
    source: StagedImportFile,
    import_source: ImportSource,
    user_id: i64,
    executors: &crate::runtime::ExecutorHandles,
    cleanup: StagedImportCleanup,
    admission: &DurableAdmission,
) -> AppResult<ImportStagedFileOutcome> {
    validate_staged_import_root(source.storage_root)?;
    let source_path = Path::new(source.path.relative_path());
    let media_type = media_type(source_path).ok_or_else(|| {
        AppError::BadRequest(format!("unsupported media file: {}", source_path.display()))
    })?;
    let original_filename = source_path
        .file_name()
        .and_then(|filename| filename.to_str())
        .ok_or_else(|| AppError::BadRequest("media filename is not valid UTF-8".to_string()))?
        .to_string();
    let mime_type = match media_type {
        "image" => image_mime_type(source_path),
        "video" => video_mime_type(source_path),
        _ => None,
    }
    .ok_or_else(|| {
        AppError::BadRequest(format!("unsupported media file: {}", source_path.display()))
    })?
    .to_string();
    let (source_session, source_snapshot, content_hash) =
        open_and_hash_staged_file(executors, &source).await?;
    let supplemental_metadata = read_staged_supplemental_metadata(executors, &source).await?;
    let source_modified_seconds = Some(source_snapshot.modified_seconds);
    attempt_prepared_staged_import(
        PreparedStagedImport {
            source,
            source_session,
            source_snapshot,
            import_source,
            user_id,
            cleanup,
            media_type,
            original_filename,
            mime_type,
            source_size: source_snapshot.byte_size,
            source_modified_seconds,
            content_hash,
            supplemental_metadata,
        },
        executors,
        admission,
    )
    .await
}

pub async fn resume_staged_file_import(
    prepared: PreparedStagedImport,
    executors: &crate::runtime::ExecutorHandles,
    admission: &DurableAdmission,
) -> AppResult<ImportStagedFileOutcome> {
    attempt_prepared_staged_import(prepared, executors, admission).await
}

fn validate_staged_import_root(storage_root: StorageRootId) -> AppResult<()> {
    if matches!(
        storage_root,
        StorageRootId::Imports | StorageRootId::WebDav | StorageRootId::Backups
    ) {
        Ok(())
    } else {
        Err(AppError::Validation(
            "staged import source must be below Imports, WebDav, or Backups".to_string(),
        ))
    }
}

fn staged_source_cleanup_plan(
    source: &StagedImportFile,
    source_snapshot: StorageFileSnapshot,
    supplemental_metadata: Option<&PreparedSupplementalMetadata>,
    cleanup_source: bool,
    owner_id: String,
) -> FileOperationPlan {
    let group_id = format!("import-source-cleanup-{}", uuid::Uuid::new_v4());
    let mut entries = Vec::new();
    let mut claims = Vec::new();
    if cleanup_source {
        entries.push(FileEntryPlan {
            action: FileEntryAction::Cleanup,
            storage_root: source.storage_root,
            source_path: Some(source.path.clone()),
            temporary_path: None,
            destination_path: None,
            tombstone_path: None,
            expected_size: Some(source_snapshot.byte_size),
            expected_sha256: None,
            expected_version: Some(source_snapshot.identity_version()),
        });
        claims.push(FilePathClaimPlan {
            storage_root: source.storage_root,
            path: source.path.clone(),
            mode: PathClaimMode::Write,
            scope: PathClaimScope::Exact,
            role: "staged_import_source".to_string(),
            expected_version: Some(source_snapshot.identity_version()),
        });
    }
    if let Some(supplemental_metadata) = supplemental_metadata {
        entries.push(FileEntryPlan {
            action: FileEntryAction::Cleanup,
            storage_root: source.storage_root,
            source_path: Some(supplemental_metadata.path.clone()),
            temporary_path: None,
            destination_path: None,
            tombstone_path: None,
            expected_size: Some(supplemental_metadata.snapshot.byte_size),
            expected_sha256: None,
            expected_version: Some(supplemental_metadata.snapshot.identity_version()),
        });
        claims.push(FilePathClaimPlan {
            storage_root: source.storage_root,
            path: supplemental_metadata.path.clone(),
            mode: PathClaimMode::Write,
            scope: PathClaimScope::Exact,
            role: "staged_import_sidecar".to_string(),
            expected_version: Some(supplemental_metadata.snapshot.identity_version()),
        });
    }
    FileOperationPlan {
        group_id,
        kind: "import_source_cleanup".to_string(),
        owner_kind: "import".to_string(),
        owner_id,
        claim_token: None,
        product_target: None,
        product_version: None,
        entries,
        claims,
        space_reservation: None,
    }
}

async fn open_and_hash_staged_file(
    executors: &crate::runtime::ExecutorHandles,
    source: &StagedImportFile,
) -> AppResult<(StorageFileSession, StorageFileSnapshot, String)> {
    let (opened_file, snapshot) = executors
        .file_io
        .open_storage_read_session_durable(source.storage_root, source.path.clone())
        .await?;
    let mut file = Some(opened_file);
    let mut hasher = Some(executors.cpu.start_sha256_session_durable().await?);
    let mut byte_count = 0_u64;
    loop {
        let (returned_file, bytes) = executors
            .file_io
            .read_storage_session_durable(
                file.take().ok_or_else(|| {
                    AppError::Internal("import source session is unavailable".to_string())
                })?,
                crate::runtime::FILE_IO_CHUNK_BYTES as usize,
            )
            .await?;
        file = Some(returned_file);
        if bytes.is_empty() {
            break;
        }
        byte_count = byte_count
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| AppError::ResourceLimit("import source is too large".to_string()))?;
        if byte_count > snapshot.byte_size {
            return Err(AppError::Conflict(
                "import source changed while hashing".to_string(),
            ));
        }
        let (returned_hasher, _) = executors
            .cpu
            .update_sha256_session_durable(
                hasher.take().ok_or_else(|| {
                    AppError::Internal("import hash session is unavailable".to_string())
                })?,
                bytes,
            )
            .await?;
        hasher = Some(returned_hasher);
    }
    if byte_count != snapshot.byte_size {
        return Err(AppError::Conflict(
            "import source changed while hashing".to_string(),
        ));
    }
    let content_hash =
        executors
            .cpu
            .finish_sha256_session_durable(hasher.take().ok_or_else(|| {
                AppError::Internal("import hash session is unavailable".to_string())
            })?)
            .await?;
    let file = executors
        .file_io
        .seek_storage_read_session_durable(
            file.take().ok_or_else(|| {
                AppError::Internal("import source session is unavailable".to_string())
            })?,
            0,
        )
        .await?;
    Ok((file, snapshot, content_hash))
}

async fn read_staged_supplemental_metadata(
    executors: &crate::runtime::ExecutorHandles,
    source: &StagedImportFile,
) -> AppResult<Option<PreparedSupplementalMetadata>> {
    const MAX_SUPPLEMENTAL_METADATA_BYTES: u64 = 4 * 1024 * 1024;
    for candidate in supplemental_metadata_candidates(Path::new(source.path.relative_path())) {
        let Some(relative) = candidate.to_str() else {
            continue;
        };
        let path = NormalizedStoragePath::parse(relative)
            .map_err(|error| AppError::Validation(error.to_string()))?;
        let (session, snapshot) = match executors
            .file_io
            .open_storage_read_session_durable(source.storage_root, path.clone())
            .await
        {
            Ok(opened) => opened,
            Err(error) if error.kind == ExecutorErrorKind::FileNotFound => continue,
            Err(error) => return Err(error.into()),
        };
        if snapshot.byte_size > MAX_SUPPLEMENTAL_METADATA_BYTES {
            executors
                .file_io
                .close_storage_session_durable(session)
                .await?;
            return Err(AppError::ResourceLimit(format!(
                "supplemental metadata exceeds {MAX_SUPPLEMENTAL_METADATA_BYTES} bytes"
            )));
        }
        let mut session = Some(session);
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(snapshot.byte_size as usize)
            .map_err(|_| {
                AppError::ResourceLimit("supplemental metadata allocation failed".to_string())
            })?;
        loop {
            let remaining = snapshot.byte_size.saturating_sub(bytes.len() as u64);
            if remaining == 0 {
                break;
            }
            let maximum_bytes = usize::try_from(remaining.min(crate::runtime::FILE_IO_CHUNK_BYTES))
                .map_err(|_| {
                    AppError::ResourceLimit("supplemental metadata size overflow".to_string())
                })?;
            let (returned_session, chunk) = executors
                .file_io
                .read_storage_session_durable(
                    session.take().ok_or_else(|| {
                        AppError::Internal(
                            "supplemental metadata session is unavailable".to_string(),
                        )
                    })?,
                    maximum_bytes,
                )
                .await?;
            session = Some(returned_session);
            if chunk.is_empty() {
                return Err(AppError::Conflict(
                    "supplemental metadata changed while reading".to_string(),
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        let (returned_session, trailing) = executors
            .file_io
            .read_storage_session_durable(
                session.take().ok_or_else(|| {
                    AppError::Internal("supplemental metadata session is unavailable".to_string())
                })?,
                1,
            )
            .await?;
        if !trailing.is_empty() {
            return Err(AppError::Conflict(
                "supplemental metadata changed while reading".to_string(),
            ));
        }
        executors
            .file_io
            .close_storage_session_durable(returned_session)
            .await?;
        return Ok(Some(PreparedSupplementalMetadata {
            path,
            snapshot,
            bytes,
        }));
    }
    Ok(None)
}

fn canonical_supplemental_metadata_relative_path(
    original_path: &NormalizedStoragePath,
) -> AppResult<NormalizedStoragePath> {
    NormalizedStoragePath::parse(&format!(
        "{}.supplemental-metadata.json",
        original_path.relative_path()
    ))
    .map_err(|error| AppError::Validation(error.to_string()))
}

fn decode_content_hash(value: &str) -> AppResult<[u8; 32]> {
    if value.len() != 64 || !value.is_ascii() {
        return Err(AppError::Internal(
            "import content hash is not a SHA-256 digest".to_string(),
        ));
    }
    let mut decoded = [0_u8; 32];
    for (index, output) in decoded.iter_mut().enumerate() {
        let offset = index * 2;
        *output = u8::from_str_radix(&value[offset..offset + 2], 16).map_err(|_| {
            AppError::Internal("import content hash is not hexadecimal".to_string())
        })?;
    }
    Ok(decoded)
}

async fn publish_supplemental_metadata(
    executors: &crate::runtime::ExecutorHandles,
    destination_path: NormalizedStoragePath,
    supplemental_metadata: &PreparedSupplementalMetadata,
) -> AppResult<()> {
    let existing_snapshot = match executors
        .file_io
        .open_storage_read_session_durable(StorageRootId::Originals, destination_path.clone())
        .await
    {
        Ok((session, snapshot)) => {
            executors
                .file_io
                .close_storage_session_durable(session)
                .await?;
            Some(snapshot)
        }
        Err(error) if error.kind == ExecutorErrorKind::FileNotFound => None,
        Err(error) => return Err(error.into()),
    };
    let group_id = format!("import-sidecar-{}", uuid::Uuid::new_v4());
    let temporary_path =
        NormalizedStoragePath::parse(&format!(".importing/sidecar-{}", uuid::Uuid::new_v4()))
            .map_err(|error| AppError::Internal(error.to_string()))?;
    let tombstone_path = existing_snapshot
        .map(|_| {
            NormalizedStoragePath::parse(&format!(
                ".importing/replaced-sidecar-{}",
                uuid::Uuid::new_v4()
            ))
            .map_err(|error| AppError::Internal(error.to_string()))
        })
        .transpose()?;
    let mut entries = Vec::new();
    let mut claims = vec![
        FilePathClaimPlan {
            storage_root: StorageRootId::Originals,
            path: temporary_path.clone(),
            mode: PathClaimMode::Write,
            scope: PathClaimScope::Exact,
            role: "temporary_sidecar".to_string(),
            expected_version: None,
        },
        FilePathClaimPlan {
            storage_root: StorageRootId::Originals,
            path: destination_path.clone(),
            mode: PathClaimMode::Write,
            scope: PathClaimScope::Exact,
            role: "canonical_sidecar".to_string(),
            expected_version: existing_snapshot.map(|snapshot| snapshot.identity_version()),
        },
    ];
    if let (Some(snapshot), Some(tombstone_path)) = (existing_snapshot, tombstone_path.as_ref()) {
        entries.push(FileEntryPlan {
            action: FileEntryAction::Move,
            storage_root: StorageRootId::Originals,
            source_path: Some(destination_path.clone()),
            temporary_path: None,
            destination_path: Some(tombstone_path.clone()),
            tombstone_path: None,
            expected_size: Some(snapshot.byte_size),
            expected_sha256: None,
            expected_version: Some(snapshot.identity_version()),
        });
        claims.push(FilePathClaimPlan {
            storage_root: StorageRootId::Originals,
            path: tombstone_path.clone(),
            mode: PathClaimMode::Write,
            scope: PathClaimScope::Exact,
            role: "replaced_sidecar".to_string(),
            expected_version: None,
        });
    }
    entries.push(FileEntryPlan {
        action: FileEntryAction::Publish,
        storage_root: StorageRootId::Originals,
        source_path: None,
        temporary_path: Some(temporary_path.clone()),
        destination_path: Some(destination_path),
        tombstone_path: None,
        expected_size: Some(supplemental_metadata.snapshot.byte_size),
        expected_sha256: None,
        expected_version: None,
    });
    if let (Some(snapshot), Some(tombstone_path)) = (existing_snapshot, tombstone_path) {
        entries.push(FileEntryPlan {
            action: FileEntryAction::Cleanup,
            storage_root: StorageRootId::Originals,
            source_path: Some(tombstone_path),
            temporary_path: None,
            destination_path: None,
            tombstone_path: None,
            expected_size: Some(snapshot.byte_size),
            expected_sha256: None,
            expected_version: Some(snapshot.identity_version()),
        });
    }
    let reservation_bytes = supplemental_metadata
        .snapshot
        .byte_size
        .checked_add(4096)
        .ok_or_else(|| AppError::ResourceLimit("sidecar reservation overflow".to_string()))?;
    let reservation = executors
        .file_io
        .reserve_journal_space(group_id.clone(), reservation_bytes)
        .map_err(|error| AppError::ResourceLimit(error.to_string()))?
        .into_result()
        .map_err(|error| AppError::ResourceLimit(error.to_string()))?;
    let plan = FileOperationPlan {
        group_id: group_id.clone(),
        kind: "import_sidecar_publication".to_string(),
        owner_kind: "import".to_string(),
        owner_id: group_id.clone(),
        claim_token: None,
        product_target: None,
        product_version: None,
        entries,
        claims,
        space_reservation: Some(
            JournalSpaceReservationPlan::new(reservation)
                .map_err(|error| AppError::ResourceLimit(error.to_string()))?,
        ),
    };
    if executors
        .sqlite
        .prepare_file_operation_durable(plan)
        .await?
        == PrepareJournalOutcome::PathConflict
    {
        return Err(AppError::Conflict(
            "supplemental metadata paths are owned by another operation".to_string(),
        ));
    }
    let result = publish_supplemental_metadata_group(
        executors,
        &group_id,
        temporary_path,
        supplemental_metadata.bytes.clone(),
        existing_snapshot.is_some(),
    )
    .await;
    if result.is_err() {
        if let Ok(Some(status)) = executors
            .sqlite
            .load_file_operation_cancellation_status_durable(group_id.clone())
            .await
        {
            let _ = crate::io::recovery::cancel_generic_file_operation(
                executors,
                group_id,
                status.version,
            )
            .await;
        }
    }
    result
}

async fn publish_supplemental_metadata_group(
    executors: &crate::runtime::ExecutorHandles,
    group_id: &str,
    temporary_path: NormalizedStoragePath,
    bytes: Vec<u8>,
    replaces_existing: bool,
) -> AppResult<()> {
    let session = executors
        .file_io
        .open_storage_write_session_durable(StorageRootId::Originals, temporary_path, 0)
        .await?;
    let expected_bytes = bytes.len();
    let (session, written) = executors
        .file_io
        .write_storage_session_durable(session, bytes)
        .await?;
    if written != expected_bytes {
        return Err(AppError::Internal(
            "supplemental metadata write was partial".to_string(),
        ));
    }
    executors
        .file_io
        .commit_storage_session_durable(session)
        .await?;
    let publication_entries = if replaces_existing { 2_u16 } else { 1_u16 };
    let ticket = executors
        .file_io
        .reserve_journal_mutation(group_id, 2)
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let grant = executors
        .sqlite
        .begin_file_operation_publication_durable(&ticket, 1)
        .await?
        .ok_or_else(|| AppError::Conflict("sidecar publication changed".to_string()))?;
    let mut lease =
        crate::io::recovery::acquire_verified_journal_mutation(executors, ticket, grant).await?;
    let mut version = 2_i64;
    for sequence in 0..publication_entries {
        if replaces_existing && sequence == 0 {
            executors
                .file_io
                .rename_journal_entry_durable(&mut lease, sequence)
                .await?;
        } else {
            executors
                .file_io
                .publish_journal_entry_durable(&mut lease, sequence)
                .await?;
        }
        let checkpoint = executors
            .sqlite
            .record_file_entry_published_durable(group_id.to_string(), version, sequence)
            .await?
            .ok_or_else(|| AppError::Conflict("sidecar checkpoint changed".to_string()))?;
        version = checkpoint.version;
        if sequence + 1 == publication_entries && !checkpoint.phase_complete {
            return Err(AppError::Internal(
                "sidecar publication did not complete".to_string(),
            ));
        }
    }
    drop(lease);
    if executors
        .sqlite
        .complete_no_product_file_operation_durable(group_id.to_string(), version)
        .await?
        != (JournalCheckpointOutcome::Advanced {
            version: version + 1,
        })
    {
        return Err(AppError::Conflict(
            "sidecar publication changed before completion".to_string(),
        ));
    }
    if replaces_existing {
        executors.scheduler.wake_journal_recovery();
    }
    Ok(())
}

async fn copy_staged_source_to_original(
    executors: &crate::runtime::ExecutorHandles,
    source: StorageFileSession,
    destination_path: NormalizedStoragePath,
) -> AppResult<()> {
    let mut source = Some(source);
    let mut destination = Some(
        executors
            .file_io
            .open_storage_write_session_durable(StorageRootId::Originals, destination_path, 0)
            .await?,
    );
    loop {
        let (returned_source, bytes) = executors
            .file_io
            .read_storage_session_durable(
                source.take().ok_or_else(|| {
                    AppError::Internal("import source session is unavailable".to_string())
                })?,
                crate::runtime::FILE_IO_CHUNK_BYTES as usize,
            )
            .await?;
        source = Some(returned_source);
        if bytes.is_empty() {
            break;
        }
        let expected = bytes.len();
        let (returned_destination, written) = executors
            .file_io
            .write_storage_session_durable(
                destination.take().ok_or_else(|| {
                    AppError::Internal("import destination session is unavailable".to_string())
                })?,
                bytes,
            )
            .await?;
        destination = Some(returned_destination);
        if written != expected {
            return Err(AppError::Internal(
                "import destination accepted a partial chunk".to_string(),
            ));
        }
    }
    executors
        .file_io
        .close_storage_session_durable(source.take().ok_or_else(|| {
            AppError::Internal("import source session is unavailable".to_string())
        })?)
        .await?;
    executors
        .file_io
        .commit_storage_session_durable(destination.take().ok_or_else(|| {
            AppError::Internal("import destination session is unavailable".to_string())
        })?)
        .await?;
    Ok(())
}

async fn publish_import_original(
    executors: &crate::runtime::ExecutorHandles,
    publication: ImportProductPublication<'_>,
) -> AppResult<CommittedImportProduct> {
    let ImportProductPublication {
        source,
        temporary_path,
        destination_path,
        source_snapshot,
        media_id,
        claim_token,
        content_hash,
        supplemental_metadata,
    } = publication;
    let group_id = format!("import-{media_id}-{}", uuid::Uuid::new_v4());
    let supplemental_size = supplemental_metadata
        .map(|metadata| metadata.snapshot.byte_size)
        .unwrap_or(0);
    let reservation_bytes = source_snapshot
        .byte_size
        .checked_add(supplemental_size)
        .and_then(|size| size.checked_add(8192))
        .ok_or_else(|| AppError::ResourceLimit("import reservation size overflow".to_string()))?;
    let reservation = executors
        .file_io
        .reserve_journal_space(group_id.clone(), reservation_bytes)
        .map_err(|error| AppError::ResourceLimit(error.to_string()))?
        .into_result()
        .map_err(|error| AppError::ResourceLimit(error.to_string()))?;
    let reservation = JournalSpaceReservationPlan::new(reservation)
        .map_err(|error| AppError::ResourceLimit(error.to_string()))?;
    let supplemental_hash = match supplemental_metadata {
        Some(metadata) => Some(executors.cpu.sha256_durable(metadata.bytes.clone()).await?),
        None => None,
    };
    let supplemental_paths = supplemental_metadata
        .map(|_| {
            let destination = canonical_supplemental_metadata_relative_path(&destination_path)?;
            let temporary = NormalizedStoragePath::parse(&format!(
                ".importing/sidecar-{}",
                uuid::Uuid::new_v4()
            ))
            .map_err(|error| AppError::Internal(error.to_string()))?;
            Ok::<_, AppError>((temporary, destination))
        })
        .transpose()?;
    let mut entries = vec![FileEntryPlan {
        action: FileEntryAction::Publish,
        storage_root: StorageRootId::Originals,
        source_path: None,
        temporary_path: Some(temporary_path.clone()),
        destination_path: Some(destination_path.clone()),
        tombstone_path: None,
        expected_size: Some(source_snapshot.byte_size),
        expected_sha256: Some(content_hash),
        expected_version: None,
    }];
    let mut claims = vec![
        FilePathClaimPlan {
            storage_root: StorageRootId::Originals,
            path: temporary_path.clone(),
            mode: PathClaimMode::Write,
            scope: PathClaimScope::Exact,
            role: "temporary_original".to_string(),
            expected_version: None,
        },
        FilePathClaimPlan {
            storage_root: StorageRootId::Originals,
            path: destination_path,
            mode: PathClaimMode::Write,
            scope: PathClaimScope::Exact,
            role: "canonical_original".to_string(),
            expected_version: None,
        },
    ];
    if let (Some(metadata), Some((temporary, destination))) =
        (supplemental_metadata, supplemental_paths.as_ref())
    {
        entries.push(FileEntryPlan {
            action: FileEntryAction::Publish,
            storage_root: StorageRootId::Originals,
            source_path: None,
            temporary_path: Some(temporary.clone()),
            destination_path: Some(destination.clone()),
            tombstone_path: None,
            expected_size: Some(metadata.snapshot.byte_size),
            expected_sha256: supplemental_hash,
            expected_version: None,
        });
        claims.push(FilePathClaimPlan {
            storage_root: StorageRootId::Originals,
            path: temporary.clone(),
            mode: PathClaimMode::Write,
            scope: PathClaimScope::Exact,
            role: "temporary_sidecar".to_string(),
            expected_version: None,
        });
        claims.push(FilePathClaimPlan {
            storage_root: StorageRootId::Originals,
            path: destination.clone(),
            mode: PathClaimMode::Write,
            scope: PathClaimScope::Exact,
            role: "canonical_sidecar".to_string(),
            expected_version: None,
        });
    }
    let plan = FileOperationPlan {
        group_id: group_id.clone(),
        kind: "import_media_publication".to_string(),
        owner_kind: "import".to_string(),
        owner_id: media_id.to_string(),
        claim_token: Some(claim_token.to_string()),
        product_target: Some("import_media".to_string()),
        product_version: Some(1),
        entries,
        claims,
        space_reservation: Some(reservation),
    };
    if executors
        .sqlite
        .prepare_file_operation_durable(plan)
        .await?
        == PrepareJournalOutcome::PathConflict
    {
        return Err(AppError::Conflict(
            "import original paths are owned by another operation".to_string(),
        ));
    }

    let result = async {
        copy_staged_source_to_original(executors, source, temporary_path.clone()).await?;
        executors
            .file_io
            .set_storage_modified_time_durable(
                StorageRootId::Originals,
                temporary_path,
                source_snapshot.modified_seconds,
                source_snapshot.modified_nanoseconds,
            )
            .await?;
        if let (Some(metadata), Some((temporary, _))) =
            (supplemental_metadata, supplemental_paths.as_ref())
        {
            let session = executors
                .file_io
                .open_storage_write_session_durable(StorageRootId::Originals, temporary.clone(), 0)
                .await?;
            let expected = metadata.bytes.len();
            let (session, written) = executors
                .file_io
                .write_storage_session_durable(session, metadata.bytes.clone())
                .await?;
            if written != expected {
                return Err(AppError::Internal(
                    "supplemental metadata write was partial".to_string(),
                ));
            }
            executors
                .file_io
                .commit_storage_session_durable(session)
                .await?;
        }
        let ticket = executors
            .file_io
            .reserve_journal_mutation(&group_id, 2)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        let grant = executors
            .sqlite
            .begin_file_operation_publication_durable(&ticket, 1)
            .await?
            .ok_or_else(|| {
                AppError::Conflict("import publication changed before it began".to_string())
            })?;
        let mut lease =
            crate::io::recovery::acquire_verified_journal_mutation(executors, ticket, grant)
                .await?;
        let mut version = 2_i64;
        let mut checkpoint = None;
        for sequence in 0..if supplemental_metadata.is_some() {
            2_u16
        } else {
            1_u16
        } {
            executors
                .file_io
                .publish_journal_entry_durable(&mut lease, sequence)
                .await?;
            let current = executors
                .sqlite
                .record_file_entry_published_durable(group_id.clone(), version, sequence)
                .await?
                .ok_or_else(|| {
                    AppError::Conflict("import publication changed before checkpoint".to_string())
                })?;
            version = current.version;
            checkpoint = Some(current);
        }
        drop(lease);
        let checkpoint = checkpoint.ok_or_else(|| {
            AppError::Internal("import product has no publication entries".to_string())
        })?;
        if !checkpoint.phase_complete {
            return Err(AppError::Internal(
                "import product publication did not complete".to_string(),
            ));
        }
        Ok(CommittedImportProduct {
            group_id: group_id.clone(),
            version: checkpoint.version,
        })
    }
    .await;
    if result.is_err() {
        if let Ok(Some(status)) = executors
            .sqlite
            .load_file_operation_cancellation_status_durable(group_id.clone())
            .await
        {
            let _ = crate::io::recovery::cancel_generic_file_operation(
                executors,
                group_id,
                status.version,
            )
            .await;
        }
    }
    result
}

async fn attempt_prepared_staged_import(
    prepared: PreparedStagedImport,
    executors: &crate::runtime::ExecutorHandles,
    admission: &DurableAdmission,
) -> AppResult<ImportStagedFileOutcome> {
    let PreparedStagedImport {
        source,
        source_session,
        source_snapshot,
        import_source,
        user_id,
        cleanup,
        media_type,
        original_filename,
        mime_type,
        source_size,
        source_modified_seconds,
        content_hash,
        supplemental_metadata,
    } = prepared;
    let mut source_session = Some(source_session);
    let sqlite = &executors.sqlite;
    let source_path = Path::new(source.path.relative_path());
    let claim_token = uuid::Uuid::new_v4().to_string();
    let claim_outcome = sqlite
        .acquire_import_content_hash_claim_durable(
            content_hash.clone(),
            claim_token.clone(),
            import_source,
        )
        .await?;
    if matches!(claim_outcome, ImportContentHashClaimOutcome::Busy) {
        return Ok(ImportStagedFileOutcome::Deferred(Box::new(
            PreparedStagedImport {
                source,
                source_session: source_session.take().ok_or_else(|| {
                    AppError::Internal("import source session is unavailable".to_string())
                })?,
                source_snapshot,
                import_source,
                user_id,
                cleanup,
                media_type,
                original_filename,
                mime_type,
                source_size,
                source_modified_seconds,
                content_hash,
                supplemental_metadata,
            },
        )));
    }
    let claim_guard =
        ImportContentHashClaimGuard::new(sqlite.clone(), content_hash.clone(), claim_token.clone());
    match claim_outcome {
        ImportContentHashClaimOutcome::Acquired => {}
        ImportContentHashClaimOutcome::Existing(existing_media) => {
            executors
                .file_io
                .close_storage_session_durable(source_session.take().ok_or_else(|| {
                    AppError::Internal("import source session is unavailable".to_string())
                })?)
                .await?;
            let result = absorb_existing_media(
                ExistingStagedImport {
                    source: &source,
                    source_snapshot,
                    supplemental_metadata: supplemental_metadata.as_ref(),
                    existing_media,
                    user_id,
                    cleanup,
                    source_modified_seconds,
                },
                executors,
            )
            .await;
            claim_guard.release().await?;
            return result.map(ImportStagedFileOutcome::Completed);
        }
        ImportContentHashClaimOutcome::Busy => unreachable!(),
    };
    let _claim_registration = executors
        .scheduler
        .register_durable_claim(admission, claim_token.clone())
        .map_err(AppError::Internal)?;
    let content_hash_bytes = decode_content_hash(&content_hash)?;

    let temporary_filename = format!(".importing-{}", uuid::Uuid::new_v4());
    let temporary_relative_path = PathBuf::from(".importing").join(&temporary_filename);
    let import_target = sqlite
        .allocate_import_media_durable(AllocateImportMedia {
            user_id,
            temporary_filename,
            original_filename,
            temporary_relative_path: temporary_relative_path.to_string_lossy().into_owned(),
            media_type: media_type.to_string(),
            mime_type,
            source_size: i64::try_from(source_size).map_err(|_| {
                AppError::ResourceLimit(
                    "import source size exceeds SQLite integer range".to_string(),
                )
            })?,
            content_hash: content_hash.clone(),
            source_modified_seconds,
            import_source,
        })
        .await?;
    let (media_id, temporary_relative_path) = match import_target {
        ImportTarget::New {
            media_id,
            temporary_relative_path,
        } => (media_id, temporary_relative_path),
        ImportTarget::Existing(existing_media) => {
            executors
                .file_io
                .close_storage_session_durable(source_session.take().ok_or_else(|| {
                    AppError::Internal("import source session is unavailable".to_string())
                })?)
                .await?;
            let result = absorb_existing_media(
                ExistingStagedImport {
                    source: &source,
                    source_snapshot,
                    supplemental_metadata: supplemental_metadata.as_ref(),
                    existing_media,
                    user_id,
                    cleanup,
                    source_modified_seconds,
                },
                executors,
            )
            .await
            .map(ImportStagedFileOutcome::Completed);
            claim_guard.release().await?;
            return result;
        }
    };

    let final_filename = build_original_filename(media_id, source_path);
    let final_relative_path = PathBuf::from(&final_filename);
    let temporary_storage_path =
        NormalizedStoragePath::parse(&temporary_relative_path.to_string_lossy())
            .map_err(|error| AppError::Internal(error.to_string()))?;

    let result = async {
        let final_storage_path =
            NormalizedStoragePath::parse(&final_relative_path.to_string_lossy())
                .map_err(|error| AppError::Internal(error.to_string()))?;
        let committed_product = publish_import_original(
            executors,
            ImportProductPublication {
                source: source_session.take().ok_or_else(|| {
                    AppError::Internal("import source session is unavailable".to_string())
                })?,
                temporary_path: temporary_storage_path.clone(),
                destination_path: final_storage_path.clone(),
                source_snapshot,
                media_id,
                claim_token: &claim_token,
                content_hash: content_hash_bytes,
                supplemental_metadata: supplemental_metadata.as_ref(),
            },
        )
        .await?;

        let product_group_id = committed_product.group_id.clone();
        let product_group_version = committed_product.version;
        let finalized = sqlite
            .finalize_import_media_durable(FinalizeImportMedia {
                media_id,
                user_id,
                final_filename: final_filename.clone(),
                final_relative_path: final_relative_path.to_string_lossy().into_owned(),
                product_group_id: committed_product.group_id,
                product_group_version,
                claim_token: claim_token.clone(),
                source_cleanup: (cleanup.source
                    || (cleanup.supplemental_metadata && supplemental_metadata.is_some()))
                .then(|| {
                    staged_source_cleanup_plan(
                        &source,
                        source_snapshot,
                        cleanup
                            .supplemental_metadata
                            .then_some(supplemental_metadata.as_ref())
                            .flatten(),
                        cleanup.source,
                        media_id.to_string(),
                    )
                }),
            })
            .await;
        let finalized = match finalized {
            Ok(finalized) => finalized,
            Err(error) => {
                if let Err(cancel_error) = crate::io::recovery::cancel_generic_file_operation(
                    executors,
                    product_group_id.clone(),
                    product_group_version,
                )
                .await
                {
                    tracing::warn!(
                        media_id,
                        error = %cancel_error,
                        "Failed to schedule import product rollback after database error"
                    );
                }
                return Err(error.into());
            }
        };
        if !finalized {
            if let Err(cancel_error) = crate::io::recovery::cancel_generic_file_operation(
                executors,
                product_group_id,
                product_group_version,
            )
            .await
            {
                tracing::warn!(
                    media_id,
                    error = %cancel_error,
                    "Failed to schedule changed import product rollback"
                );
            }
            return Err(AppError::Conflict(format!(
                "media {media_id} is no longer importing"
            )));
        }
        executors.scheduler.wake_journal_recovery();
        Ok(media_id)
    }
    .await;

    if let Err(error) = result {
        let _ = sqlite
            .mark_import_media_failed_durable(media_id, bounded_error_detail(&error.to_string()))
            .await;
        return Err(error);
    }
    claim_guard.release().await?;
    if cleanup.source || (cleanup.supplemental_metadata && supplemental_metadata.is_some()) {
        executors.scheduler.wake_journal_recovery();
    }
    Ok(ImportStagedFileOutcome::Completed(media_id))
}

struct ImportContentHashClaimGuard {
    sqlite: SqliteExecutorHandle,
    content_hash: String,
    claim_token: String,
    released: bool,
}

impl ImportContentHashClaimGuard {
    fn new(sqlite: SqliteExecutorHandle, content_hash: String, claim_token: String) -> Self {
        Self {
            sqlite,
            content_hash,
            claim_token,
            released: false,
        }
    }

    async fn release(mut self) -> AppResult<()> {
        if !self
            .sqlite
            .release_import_content_hash_claim_durable(
                self.content_hash.clone(),
                self.claim_token.clone(),
            )
            .await?
        {
            return Err(AppError::Conflict(
                "import content-hash claim changed before release".to_string(),
            ));
        }
        self.released = true;
        Ok(())
    }
}

impl Drop for ImportContentHashClaimGuard {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        if let Err(error) = self.sqlite.release_import_content_hash_claim_eventually(
            self.content_hash.clone(),
            self.claim_token.clone(),
        ) {
            tracing::warn!(error = %error, "failed to enqueue import content-hash claim release");
        }
    }
}

pub(crate) fn acquire_content_hash_claim_on_connection(
    connection: &mut rusqlite::Connection,
    content_hash: &str,
    claim_token: &str,
    source: ImportSource,
) -> rusqlite::Result<ImportContentHashClaimOutcome> {
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let inserted = transaction.execute(
        queries::import::INSERT_CONTENT_HASH_CLAIM,
        rusqlite::params![content_hash, claim_token, source.as_str()],
    )?;
    if inserted == 0 {
        transaction.rollback()?;
        return Ok(ImportContentHashClaimOutcome::Busy);
    }
    if let Some(existing) = select_existing_media(&transaction, content_hash)? {
        if existing.import_state != "imported" {
            transaction.rollback()?;
            return Ok(ImportContentHashClaimOutcome::Busy);
        }
        transaction.commit()?;
        return Ok(ImportContentHashClaimOutcome::Existing(existing));
    }
    transaction.commit()?;
    Ok(ImportContentHashClaimOutcome::Acquired)
}

pub(crate) fn release_content_hash_claim_on_connection(
    connection: &rusqlite::Connection,
    content_hash: &str,
    claim_token: &str,
) -> rusqlite::Result<bool> {
    connection
        .execute(
            queries::import::RELEASE_CONTENT_HASH_CLAIM,
            rusqlite::params![content_hash, claim_token],
        )
        .map(|changed| changed == 1)
}

pub(crate) fn recover_content_hash_claims_on_connection(
    connection: &rusqlite::Connection,
) -> rusqlite::Result<usize> {
    connection.execute(queries::import::RECOVER_CONTENT_HASH_CLAIMS, [])
}

fn select_existing_media(
    connection: &rusqlite::Connection,
    content_hash: &str,
) -> rusqlite::Result<Option<ExistingMedia>> {
    connection
        .query_row(
            queries::import::SELECT_BY_CONTENT_HASH,
            [content_hash],
            |row| {
                Ok(ExistingMedia {
                    id: row.get(0)?,
                    file_path: row.get(1)?,
                    import_state: row.get(2)?,
                })
            },
        )
        .optional()
}

struct ExistingStagedImport<'a> {
    source: &'a StagedImportFile,
    source_snapshot: StorageFileSnapshot,
    supplemental_metadata: Option<&'a PreparedSupplementalMetadata>,
    existing_media: ExistingMedia,
    user_id: i64,
    cleanup: StagedImportCleanup,
    source_modified_seconds: Option<i64>,
}

async fn absorb_existing_media(
    import: ExistingStagedImport<'_>,
    executors: &crate::runtime::ExecutorHandles,
) -> AppResult<i64> {
    let ExistingStagedImport {
        source,
        source_snapshot,
        supplemental_metadata,
        existing_media,
        user_id,
        cleanup,
        source_modified_seconds,
    } = import;
    if existing_media.import_state != "imported" {
        return Err(AppError::Conflict(format!(
            "matching media {} is still {}",
            existing_media.id, existing_media.import_state
        )));
    }

    let existing_original_path =
        NormalizedStoragePath::parse(&existing_media.file_path).map_err(|_| {
            AppError::Conflict(format!(
                "matching media {} has an invalid canonical original path",
                existing_media.id
            ))
        })?;
    let (existing_original_session, _) = executors
        .file_io
        .open_storage_read_session_durable(StorageRootId::Originals, existing_original_path.clone())
        .await
        .map_err(|_| {
            AppError::Conflict(format!(
                "matching media {} has no canonical original",
                existing_media.id
            ))
        })?;
    executors
        .file_io
        .close_storage_session_durable(existing_original_session)
        .await?;

    let sqlite = &executors.sqlite;
    if let Some(supplemental_metadata) = supplemental_metadata {
        publish_supplemental_metadata(
            executors,
            canonical_supplemental_metadata_relative_path(&existing_original_path)?,
            supplemental_metadata,
        )
        .await?;
    }

    sqlite
        .absorb_existing_media_durable(AbsorbExistingMediaDatabase {
            media_id: existing_media.id,
            user_id,
            source_modified_seconds,
            request_metadata_rerun: supplemental_metadata.is_some(),
            source_cleanup: (cleanup.source
                || (cleanup.supplemental_metadata && supplemental_metadata.is_some()))
            .then(|| {
                staged_source_cleanup_plan(
                    source,
                    source_snapshot,
                    cleanup
                        .supplemental_metadata
                        .then_some(supplemental_metadata)
                        .flatten(),
                    cleanup.source,
                    existing_media.id.to_string(),
                )
            }),
        })
        .await?;
    if cleanup.source || (cleanup.supplemental_metadata && supplemental_metadata.is_some()) {
        executors.scheduler.wake_journal_recovery();
    }
    tracing::info!(
        media_id = existing_media.id,
        content_path = %source.path.relative_path(),
        "absorbed duplicate import into existing media"
    );
    Ok(existing_media.id)
}

pub async fn recover_interrupted_imports(
    executors: &crate::runtime::ExecutorHandles,
) -> AppResult<()> {
    let sqlite = &executors.sqlite;
    let mut after_media_id = 0_i64;
    loop {
        let interrupted_imports = sqlite
            .recover_interrupted_import_page_durable(after_media_id, 256)
            .await?;
        if interrupted_imports.is_empty() {
            return Ok(());
        }
        for interrupted in interrupted_imports {
            let media_id = interrupted.media_id;
            after_media_id = media_id;
            let _ = sqlite
                .mark_import_media_failed_durable(
                    media_id,
                    "import product was not atomically finalized".to_string(),
                )
                .await?;
        }
    }
}

pub async fn start_webdav_import_job(
    config: Arc<Config>,
    executors: crate::runtime::ExecutorHandles,
    webdav_request_gate: crate::webdav::WebDAVRequestGate,
    scheduler: crate::runtime::SchedulerHandle,
) {
    loop {
        run_webdav_import_cycle(&config, &executors, &webdav_request_gate, &scheduler).await;
        scheduler.webdav_import_notified().await;
    }
}

pub async fn run_webdav_import_cycle(
    config: &Config,
    executors: &crate::runtime::ExecutorHandles,
    webdav_request_gate: &crate::webdav::WebDAVRequestGate,
    scheduler: &crate::runtime::SchedulerHandle,
) {
    let sqlite = &executors.sqlite;
    let Ok(cycle_worker) = scheduler
        .acquire_durable(
            DurableSourceId::WebDavImport,
            SchedulerAdmissionKind::NewClaim,
        )
        .await
    else {
        return;
    };
    let mut ready_by_user = BTreeMap::<(i64, String), Vec<String>>::new();
    let mut after_user_id = 0_i64;
    let mut after_file_path = String::new();
    loop {
        let page = match sqlite
            .load_webdav_ready_page_durable(after_user_id, after_file_path.clone(), 256)
            .await
        {
            Ok(page) => page,
            Err(error) => {
                warn!(error = %error, "failed to load WebDAV ready files");
                return;
            }
        };
        if page.is_empty() {
            break;
        }
        let page_is_full = page.len() == 256;
        for ready in page {
            after_user_id = ready.user_id;
            after_file_path.clone_from(&ready.file_path);
            ready_by_user
                .entry((ready.user_id, ready.username))
                .or_default()
                .push(ready.file_path);
        }
        if !page_is_full {
            break;
        }
    }
    let mut pending_imports = Vec::new();
    for ((user_id, username), ready_file_paths) in ready_by_user {
        match collect_ready_webdav_files(
            executors,
            &username,
            ready_file_paths,
            config.webdav.stable_file_age_seconds,
        )
        .await
        {
            Ok(candidates) => {
                pending_imports.extend(candidates.into_iter().map(|candidate| (candidate, user_id)))
            }
            Err(error) => {
                warn!(user = %username, error = %error, "failed to inspect WebDAV ready files")
            }
        }
    }
    if pending_imports.is_empty() {
        return;
    }
    let job_id = match sqlite.create_import_job_durable(ImportSource::Webdav).await {
        Ok(CreateImportJobOutcome::Created(job_id)) => job_id,
        Ok(CreateImportJobOutcome::AlreadyRunning) => return,
        Err(error) => {
            warn!(error = %error, "failed to create WebDAV import job");
            return;
        }
    };
    let upload_barrier = Arc::clone(webdav_request_gate).write_owned().await;
    let mut claimed_imports = Vec::with_capacity(pending_imports.len());
    for (candidate, user_id) in pending_imports {
        if !staged_file_is_stable(
            executors,
            &candidate.source,
            config.webdav.stable_file_age_seconds,
        )
        .await
        .unwrap_or(false)
        {
            continue;
        }
        if !sqlite
            .check_webdav_ready_durable(user_id, candidate.ready_file_path.clone())
            .await
            .unwrap_or(false)
        {
            continue;
        }
        claimed_imports.push((
            candidate.source,
            user_id,
            candidate.ready_file_path,
            candidate.supplemental_ready_file_path,
        ));
    }
    drop(upload_barrier);
    drop(cycle_worker);
    if let Err(error) = sqlite
        .set_import_job_total_durable(job_id, claimed_imports.len() as i64)
        .await
    {
        warn!(job_id, error = %error, "failed to persist WebDAV import total");
    }
    stream::iter(claimed_imports)
        .for_each_concurrent(
            scheduler.durable_capacity(),
            |(staged_source, user_id, ready_file_path, supplemental_ready_file_path)| async move {
                let Ok(mut worker_permit) = scheduler
                    .acquire_durable(
                        DurableSourceId::WebDavImport,
                        SchedulerAdmissionKind::ExistingClaimCompletion,
                    )
                    .await
                else {
                    return;
                };
                let source_label = staged_source.path.relative_path().to_string();
                let mut attempt = import_staged_file(
                    staged_source,
                    ImportSource::Webdav,
                    user_id,
                    executors,
                    StagedImportCleanup {
                        source: true,
                        supplemental_metadata: true,
                    },
                    &worker_permit,
                )
                .await;
                let import_result = loop {
                    match attempt {
                        Ok(ImportStagedFileOutcome::Deferred(prepared)) => {
                            drop(worker_permit);
                            tokio::task::yield_now().await;
                            worker_permit = match scheduler
                                .acquire_durable(
                                    DurableSourceId::WebDavImport,
                                    SchedulerAdmissionKind::ExistingClaimCompletion,
                                )
                                .await
                            {
                                Ok(worker_permit) => worker_permit,
                                Err(error) => break Err(AppError::Unavailable(error)),
                            };
                            attempt =
                                resume_staged_file_import(*prepared, executors, &worker_permit)
                                    .await;
                        }
                        Ok(ImportStagedFileOutcome::Completed(media_id)) => break Ok(media_id),
                        Err(error) => break Err(error),
                    }
                };
                if let Err(error) = import_result {
                    let detail = bounded_error_detail(&format!(
                        "WebDAV import failed for {}: {error}",
                        source_label
                    ));
                    warn!(path = %source_label, error = %detail, "WebDAV import failed");
                    update_import_progress(sqlite, job_id, false, Some(detail)).await;
                } else {
                    delete_ready_paths(
                        sqlite,
                        user_id,
                        &ready_file_path,
                        supplemental_ready_file_path.as_deref(),
                    )
                    .await;
                    update_import_progress(sqlite, job_id, true, None).await;
                    scheduler.wake_metadata();
                }
            },
        )
        .await;
    if let Err(error) = sqlite.complete_import_job_durable(job_id).await {
        warn!(job_id, error = %error, "failed to complete WebDAV import job");
    }
}

async fn collect_import_files(
    executors: &crate::runtime::ExecutorHandles,
    storage_root: StorageRootId,
) -> AppResult<Vec<StagedImportFile>> {
    validate_staged_import_root(storage_root)?;
    let mut directories = VecDeque::from([None]);
    let mut source_files = Vec::new();
    while let Some(directory) = directories.pop_front() {
        let mut session = Some(
            executors
                .file_io
                .open_storage_directory_session_durable(storage_root, directory.clone())
                .await?,
        );
        loop {
            let (returned_session, entries, finished) = executors
                .file_io
                .read_storage_directory_session_durable(session.take().ok_or_else(|| {
                    AppError::Internal("import directory session is unavailable".to_string())
                })?)
                .await?;
            session = Some(returned_session);
            for entry in entries {
                if entry.name.starts_with('.') {
                    continue;
                }
                let relative = directory.as_ref().map_or_else(
                    || entry.name.clone(),
                    |parent| format!("{}/{}", parent.relative_path(), entry.name),
                );
                let path = NormalizedStoragePath::parse(&relative)
                    .map_err(|error| AppError::Validation(error.to_string()))?;
                match entry.kind {
                    StorageDirectoryEntryKind::Directory => directories.push_back(Some(path)),
                    StorageDirectoryEntryKind::File
                        if media_type(Path::new(&relative)).is_some() =>
                    {
                        source_files.push(StagedImportFile { storage_root, path });
                    }
                    StorageDirectoryEntryKind::File => {}
                }
            }
            if finished {
                executors
                    .file_io
                    .close_storage_session_durable(session.take().ok_or_else(|| {
                        AppError::Internal("import directory session is unavailable".to_string())
                    })?)
                    .await?;
                break;
            }
        }
    }
    Ok(source_files)
}

async fn collect_ready_webdav_files(
    executors: &crate::runtime::ExecutorHandles,
    username: &str,
    ready_file_paths: Vec<String>,
    stable_age_seconds: u64,
) -> AppResult<Vec<WebDavImportCandidate>> {
    let mut candidates = Vec::new();
    let ready_file_path_set = ready_file_paths.iter().cloned().collect::<HashSet<_>>();
    for ready_file_path in ready_file_paths {
        let relative_path = Path::new(&ready_file_path);
        if !safe_webdav_relative_path(relative_path)
            || relative_path.components().any(|component| {
                component
                    .as_os_str()
                    .to_str()
                    .is_some_and(|segment| segment.starts_with('.'))
            })
        {
            continue;
        }
        if media_type(relative_path).is_none() {
            continue;
        }
        let path = NormalizedStoragePath::parse(&format!("{username}/{ready_file_path}"))
            .map_err(|error| AppError::Validation(error.to_string()))?;
        let source = StagedImportFile {
            storage_root: StorageRootId::WebDav,
            path,
        };
        if !staged_file_is_stable(executors, &source, stable_age_seconds).await? {
            continue;
        }
        let supplemental_ready_file_path = supplemental_metadata_candidates(relative_path)
            .into_iter()
            .find_map(|candidate_path| {
                let relative_candidate_path = candidate_path.to_str()?.to_string();
                ready_file_path_set
                    .contains(&relative_candidate_path)
                    .then_some(relative_candidate_path)
            });
        candidates.push(WebDavImportCandidate {
            source,
            ready_file_path,
            supplemental_ready_file_path,
        });
    }
    Ok(candidates)
}

fn safe_webdav_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

async fn delete_ready_paths(
    sqlite: &SqliteExecutorHandle,
    user_id: i64,
    ready_file_path: &str,
    supplemental_ready_file_path: Option<&str>,
) {
    let mut remove = vec![ready_file_path.to_string()];
    remove.extend(supplemental_ready_file_path.map(str::to_string));
    if let Err(error) = sqlite
        .update_webdav_ready_paths_durable(UpdateWebdavReadyPaths {
            user_id,
            remove,
            add: Vec::new(),
        })
        .await
    {
        warn!("failed to remove imported WebDAV readiness: {error}");
    }
}

async fn staged_file_is_stable(
    executors: &crate::runtime::ExecutorHandles,
    source: &StagedImportFile,
    stable_age_seconds: u64,
) -> AppResult<bool> {
    let (session, snapshot) = match executors
        .file_io
        .open_storage_read_session_durable(source.storage_root, source.path.clone())
        .await
    {
        Ok(opened) => opened,
        Err(error) if error.kind == ExecutorErrorKind::FileNotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    executors
        .file_io
        .close_storage_session_durable(session)
        .await?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| AppError::Internal(error.to_string()))?
        .as_secs();
    let modified = u64::try_from(snapshot.modified_seconds).unwrap_or(0);
    Ok(now.saturating_sub(modified) >= stable_age_seconds)
}

fn media_type(source_path: &Path) -> Option<&'static str> {
    let extension = source_path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| format!(".{}", extension.to_ascii_lowercase()))?;
    if IMAGE_EXTENSIONS.contains(extension.as_str()) {
        Some("image")
    } else if VIDEO_EXTENSIONS.contains(extension.as_str()) {
        Some("video")
    } else {
        None
    }
}
