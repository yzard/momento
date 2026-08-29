use futures::stream::{self, StreamExt};
use rusqlite::OptionalExtension;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tracing::warn;

use crate::config::Config;
use crate::constants::{
    image_mime_type, paths, video_mime_type, IMAGE_EXTENSIONS, VIDEO_EXTENSIONS,
};
use crate::database::{execute_query, fetch_one, queries, DbPool};
use crate::error::{AppError, AppResult};
use crate::processor::media_processor::{
    apply_file_times, build_original_filename, capture_file_times,
};
use crate::processor::metadata::supplemental_metadata_path as find_supplemental_metadata_path;
use crate::utils::hash::calculate_file_hash;

static CONTENT_HASH_LOCKS: OnceLock<Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>> =
    OnceLock::new();

struct ExistingMedia {
    id: i64,
    file_path: String,
    import_state: String,
}

struct WebDavImportCandidate {
    source_path: PathBuf,
    supplemental_metadata_path: Option<PathBuf>,
    ready_file_path: String,
    supplemental_ready_file_path: Option<String>,
}

enum ImportTarget {
    New {
        media_id: i64,
        temporary_relative_path: PathBuf,
    },
    Existing(ExistingMedia),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportSource {
    Local,
    Webdav,
    MobileBackup,
}

impl ImportSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Webdav => "webdav",
            Self::MobileBackup => "mobile_backup",
        }
    }
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
    pub pool: DbPool,
    pub concurrency: usize,
}

pub fn create_import_job(pool: &DbPool, source: ImportSource) -> AppResult<i64> {
    let connection = pool.get().map_err(AppError::Pool)?;
    connection
        .execute(queries::import::INSERT_JOB, [source.as_str()])
        .map_err(|error| match error {
            rusqlite::Error::SqliteFailure(_, _) => {
                AppError::Conflict("Import already in progress".to_string())
            }
            other => AppError::Database(other),
        })?;
    Ok(connection.last_insert_rowid())
}

pub fn get_import_status(pool: &DbPool, source: ImportSource) -> AppResult<ImportJob> {
    let connection = pool.get().map_err(AppError::Pool)?;
    let job = connection
        .query_row(
            queries::import::SELECT_LATEST_JOB_FOR_SOURCE,
            [source.as_str()],
            |row| {
                Ok(ImportJob {
                    status: row.get(0)?,
                    total_files: row.get(1)?,
                    processed_files: row.get(2)?,
                    successful_imports: row.get(3)?,
                    failed_imports: row.get(4)?,
                    started_at: row.get(5)?,
                    completed_at: row.get(6)?,
                    errors: row.get::<_, Option<String>>(7)?.into_iter().collect(),
                })
            },
        )
        .optional()?;
    Ok(job.unwrap_or(ImportJob {
        status: "idle".to_string(),
        total_files: 0,
        processed_files: 0,
        successful_imports: 0,
        failed_imports: 0,
        started_at: None,
        completed_at: None,
        errors: Vec::new(),
    }))
}

pub async fn run_local_import(settings: ImportSettings, job_id: i64) {
    if let Err(error) = recover_import_claims(&paths().imports) {
        warn!("local import claim recovery failed: {error}");
    }
    let source_files = collect_import_files(&paths().imports);
    if let Ok(connection) = settings.pool.get() {
        let _ = connection.execute(
            queries::import::SET_JOB_TOTAL,
            rusqlite::params![source_files.len() as i64, job_id],
        );
    }

    let concurrency = settings
        .concurrency
        .max(1)
        .min(settings.pool.max_size() as usize);
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let mut imports = stream::iter(source_files)
        .map(|source_path| {
            let settings = settings.clone();
            let semaphore = Arc::clone(&semaphore);
            async move {
                let permit = semaphore.acquire().await;
                if permit.is_err() {
                    update_import_progress(&settings.pool, job_id, false, Some("local import worker stopped".to_string()));
                    return;
                }
                let source_supplemental_metadata_path =
                    find_supplemental_metadata_path(&source_path);
                let claimed_path = match claim_source(
                    &source_path,
                    source_supplemental_metadata_path.as_deref(),
                )
                .await
                {
                    Ok(claimed_path) => claimed_path,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
                    Err(error) => {
                        update_import_progress(&settings.pool, job_id, false, Some(error.to_string()));
                        return;
                    }
                };
                let import_result = import_staged_file(
                    &claimed_path,
                    ImportSource::Local,
                    settings.user_id,
                    &settings.pool,
                    true,
                )
                .await;
                match import_result {
                    Ok(_) => {
                        if let Err(error) = tokio::fs::remove_file(&claimed_path).await {
                            if error.kind() != std::io::ErrorKind::NotFound {
                                warn!(path = %source_path.display(), "local imported file cleanup failed: {error}");
                            }
                        }
                        let _ = remove_claim_directory(&claimed_path).await;
                        update_import_progress(&settings.pool, job_id, true, None);
                    }
                    Err(error) => {
                        let _ = restore_claim(&claimed_path, &source_path).await;
                        update_import_progress(&settings.pool, job_id, false, Some(error.to_string()));
                    }
                }
            }
        })
        .buffer_unordered(concurrency);

    while imports.next().await.is_some() {}

    if let Ok(connection) = settings.pool.get() {
        let _ = connection.execute(queries::import::COMPLETE_JOB, [job_id]);
    }
}

fn update_import_progress(
    pool: &DbPool,
    job_id: i64,
    success: bool,
    error_message: Option<String>,
) {
    let error_message = error_message.unwrap_or_default();
    if let Ok(connection) = pool.get() {
        let _ = connection.execute(
            queries::import::UPDATE_JOB_PROGRESS,
            rusqlite::params![success, success, error_message, error_message, job_id],
        );
    }
}

/// Imports one completed on-disk file after its source-specific claim and preparation.
pub async fn import_staged_file(
    source_path: &Path,
    import_source: ImportSource,
    user_id: i64,
    pool: &DbPool,
    delete_source: bool,
) -> AppResult<i64> {
    let source_metadata = tokio::fs::metadata(source_path).await?;
    if !source_metadata.is_file() {
        return Err(AppError::BadRequest(format!(
            "import source is not a file: {}",
            source_path.display()
        )));
    }
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
    let source_modified_seconds = source_metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64);
    let content_hash = calculate_file_hash(source_path).await?;
    let content_hash_lock = content_hash_lock(&content_hash);
    let _content_hash_guard = content_hash_lock.lock().await;

    let existing_media = {
        let connection = pool.get()?;
        select_existing_media(&connection, &content_hash)?
    };
    if let Some(existing_media) = existing_media {
        return absorb_existing_media(
            source_path,
            existing_media,
            user_id,
            pool,
            delete_source,
            source_modified_seconds,
        )
        .await;
    }

    let import_target = {
        let connection = pool.get()?;
        let temporary_filename = format!(".importing-{}", uuid::Uuid::new_v4());
        let temporary_relative_path = PathBuf::from(".importing").join(&temporary_filename);
        let rows = connection.execute(
            queries::import::INSERT_IMPORTING_MEDIA,
            rusqlite::params![
                user_id,
                temporary_filename,
                &original_filename,
                temporary_relative_path.to_string_lossy(),
                media_type,
                mime_type,
                source_metadata.len() as i64,
                &content_hash,
                source_modified_seconds,
                import_source.as_str(),
            ],
        )?;
        if rows == 0 {
            let existing_media =
                select_existing_media(&connection, &content_hash)?.ok_or_else(|| {
                    AppError::Conflict(
                        "content hash conflict did not identify existing media".to_string(),
                    )
                })?;
            ImportTarget::Existing(existing_media)
        } else if rows == 1 {
            ImportTarget::New {
                media_id: connection.last_insert_rowid(),
                temporary_relative_path,
            }
        } else {
            return Err(AppError::Internal(
                "failed to allocate media ID".to_string(),
            ));
        }
    };
    let (media_id, temporary_relative_path) = match import_target {
        ImportTarget::New {
            media_id,
            temporary_relative_path,
        } => (media_id, temporary_relative_path),
        ImportTarget::Existing(existing_media) => {
            return absorb_existing_media(
                source_path,
                existing_media,
                user_id,
                pool,
                delete_source,
                source_modified_seconds,
            )
            .await;
        }
    };

    let final_filename = build_original_filename(media_id, source_path);
    let final_relative_path = PathBuf::from(&final_filename);
    let temporary_path = paths().originals.join(&temporary_relative_path);
    let final_path = paths().originals.join(&final_relative_path);
    let source_supplemental_metadata_path = find_supplemental_metadata_path(source_path);
    let final_supplemental_metadata_path = canonical_supplemental_metadata_path(&final_path);
    let source_file_times = capture_file_times(source_path)?;

    let result = async {
        let temporary_parent = temporary_path.parent().ok_or_else(|| {
            AppError::Internal("temporary original path has no parent".to_string())
        })?;
        tokio::fs::create_dir_all(temporary_parent).await?;
        if let Some(source_supplemental_metadata_path) = &source_supplemental_metadata_path {
            tokio::fs::copy(
                source_supplemental_metadata_path,
                &final_supplemental_metadata_path,
            )
            .await?;
        }
        if delete_source {
            tokio::fs::rename(source_path, &final_path).await?;
        } else {
            tokio::fs::copy(source_path, &temporary_path).await?;
            apply_file_times(&temporary_path, source_file_times)?;
            tokio::fs::rename(&temporary_path, &final_path).await?;
        }

        let connection = pool.get()?;
        let transaction = connection.unchecked_transaction()?;
        let changed = transaction.execute(
            queries::import::MARK_IMPORTED,
            rusqlite::params![
                &final_filename,
                final_relative_path.to_string_lossy(),
                media_id,
            ],
        )?;
        if changed != 1 {
            return Err(AppError::Conflict(format!(
                "media {media_id} is no longer importing"
            )));
        }
        transaction.execute(
            queries::access::INSERT_MEDIA_ACCESS,
            rusqlite::params![media_id, user_id, 2],
        )?;
        transaction.execute(queries::metadata_jobs::INSERT_QUEUED, [media_id])?;
        transaction.commit()?;
        Ok(media_id)
    }
    .await;

    if let Err(error) = result {
        let _ = tokio::fs::remove_file(&temporary_path).await;
        if final_path.is_file() {
            if delete_source {
                let _ = tokio::fs::rename(&final_path, source_path).await;
            } else {
                let _ = tokio::fs::remove_file(&final_path).await;
            }
        }
        let _ = tokio::fs::remove_file(&final_supplemental_metadata_path).await;
        let connection = pool.get()?;
        let _ = execute_query(
            &connection,
            queries::import::MARK_FAILED,
            &[&error.to_string(), &media_id],
        );
        return Err(error);
    }
    if let Some(source_supplemental_metadata_path) = source_supplemental_metadata_path {
        if let Err(error) = tokio::fs::remove_file(&source_supplemental_metadata_path).await {
            if error.kind() != std::io::ErrorKind::NotFound {
                warn!(path = %source_supplemental_metadata_path.display(), "imported sidecar cleanup failed: {error}");
            }
        }
    }
    Ok(media_id)
}

fn content_hash_lock(content_hash: &str) -> Arc<tokio::sync::Mutex<()>> {
    let locks = CONTENT_HASH_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut locks = locks
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(content_hash).and_then(Weak::upgrade) {
        return lock;
    }

    let lock = Arc::new(tokio::sync::Mutex::new(()));
    locks.insert(content_hash.to_string(), Arc::downgrade(&lock));
    lock
}

fn select_existing_media(
    connection: &rusqlite::Connection,
    content_hash: &str,
) -> AppResult<Option<ExistingMedia>> {
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
        .map_err(AppError::Database)
}

async fn absorb_existing_media(
    source_path: &Path,
    existing_media: ExistingMedia,
    user_id: i64,
    pool: &DbPool,
    delete_source: bool,
    source_modified_seconds: Option<i64>,
) -> AppResult<i64> {
    if existing_media.import_state != "imported" {
        return Err(AppError::Conflict(format!(
            "matching media {} is still {}",
            existing_media.id, existing_media.import_state
        )));
    }

    let existing_original_path = crate::utils::path::resolve_existing_storage_path(
        &paths().originals,
        &existing_media.file_path,
    )
    .await
    .map_err(|_| {
        AppError::Conflict(format!(
            "matching media {} has no canonical original",
            existing_media.id
        ))
    })?;
    if !existing_original_path.is_file() {
        return Err(AppError::Conflict(format!(
            "matching media {} has no canonical original",
            existing_media.id
        )));
    }

    let source_sidecar_path = find_supplemental_metadata_path(source_path);
    let destination_sidecar_path = canonical_supplemental_metadata_path(&existing_original_path);
    let pending_sidecar_path =
        destination_sidecar_path.with_extension(format!("pending-{}", uuid::Uuid::new_v4()));
    if let Some(source_sidecar_path) = &source_sidecar_path {
        tokio::fs::copy(source_sidecar_path, &pending_sidecar_path).await?;
    }

    let database_result = (|| -> AppResult<()> {
        let connection = pool.get()?;
        let transaction = connection.unchecked_transaction()?;
        if let Some(modified_seconds) = source_modified_seconds {
            transaction.execute(
                queries::import::UPDATE_EARLIER_CREATED_AT,
                rusqlite::params![modified_seconds, existing_media.id, modified_seconds],
            )?;
        }
        transaction.execute(
            queries::access::INSERT_MEDIA_ACCESS,
            rusqlite::params![existing_media.id, user_id, 2],
        )?;
        transaction.execute(
            queries::access::RESTORE_MEDIA_ACCESS,
            rusqlite::params![existing_media.id, user_id],
        )?;
        if source_sidecar_path.is_some() {
            transaction.execute(queries::metadata_jobs::REQUEST_RERUN, [existing_media.id])?;
        }
        transaction.commit()?;
        Ok(())
    })();

    if let Err(error) = database_result {
        let _ = std::fs::remove_file(&pending_sidecar_path);
        return Err(error);
    }
    if source_sidecar_path.is_some() {
        if let Err(error) = std::fs::rename(&pending_sidecar_path, &destination_sidecar_path) {
            let _ = std::fs::remove_file(&pending_sidecar_path);
            return Err(AppError::Io(error));
        }
        let connection = pool.get()?;
        connection.execute(queries::metadata_jobs::REQUEST_RERUN, [existing_media.id])?;
    }
    if let Some(source_sidecar_path) = source_sidecar_path {
        if let Err(error) = tokio::fs::remove_file(&source_sidecar_path).await {
            if error.kind() != std::io::ErrorKind::NotFound {
                warn!(path = %source_sidecar_path.display(), "absorbed sidecar cleanup failed: {error}");
            }
        }
    }
    if delete_source {
        if let Err(error) = tokio::fs::remove_file(source_path).await {
            if error.kind() != std::io::ErrorKind::NotFound {
                warn!(path = %source_path.display(), "duplicate import source cleanup failed: {error}");
            }
        }
    }
    tracing::info!(
        media_id = existing_media.id,
        content_path = %source_path.display(),
        "absorbed duplicate import into existing media"
    );
    Ok(existing_media.id)
}

fn canonical_supplemental_metadata_path(media_path: &Path) -> PathBuf {
    let media_filename = media_path
        .file_name()
        .and_then(|filename| filename.to_str())
        .unwrap_or_default();
    media_path.with_file_name(format!("{media_filename}.supplemental-metadata.json"))
}

pub fn recover_interrupted_imports(pool: &DbPool) -> AppResult<()> {
    let connection = pool.get()?;
    connection.execute(queries::import::FAIL_INTERRUPTED_JOBS, [])?;
    let interrupted_imports = connection
        .prepare(queries::import::SELECT_INTERRUPTED)?
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (media_id, temporary_file_path, original_filename, user_id) in interrupted_imports {
        let final_filename = build_original_filename(media_id, Path::new(&original_filename));
        let final_relative_path = PathBuf::from(&final_filename);
        let final_path = paths().originals.join(&final_relative_path);
        let temporary_path = paths().originals.join(temporary_file_path);
        if final_path.is_file() {
            let transaction = connection.unchecked_transaction()?;
            transaction.execute(
                queries::import::MARK_IMPORTED,
                rusqlite::params![
                    final_filename,
                    final_relative_path.to_string_lossy(),
                    media_id
                ],
            )?;
            transaction.execute(
                queries::access::INSERT_MEDIA_ACCESS,
                rusqlite::params![media_id, user_id, 2],
            )?;
            transaction.execute(queries::metadata_jobs::INSERT_QUEUED, [media_id])?;
            transaction.commit()?;
            let _ = std::fs::remove_file(&temporary_path);
        } else {
            let _ = std::fs::remove_file(&temporary_path);
            let _ = std::fs::remove_file(canonical_supplemental_metadata_path(&final_path));
            connection.execute(
                queries::import::MARK_FAILED,
                rusqlite::params!["original file was not finalized", media_id],
            )?;
        }
    }
    Ok(())
}

pub fn recover_import_claims(root: &Path) -> std::io::Result<()> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.file_name().is_some_and(|name| name == ".processing") {
            restore_webdav_claim_directory(&path)?;
            continue;
        }
        recover_import_claims(&path)?;
    }
    Ok(())
}

fn restore_webdav_claim_directory(processing_directory: &Path) -> std::io::Result<()> {
    let source_directory = processing_directory
        .parent()
        .ok_or_else(|| std::io::Error::other("WebDAV processing directory has no parent"))?;
    for claim_entry in std::fs::read_dir(processing_directory)? {
        let claim_entry = claim_entry?;
        let claim_directory = claim_entry.path();
        if !claim_directory.is_dir() {
            continue;
        }
        let claim_paths = std::fs::read_dir(&claim_directory)?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        let can_restore_original_paths = claim_paths.iter().all(|path| {
            path.file_name()
                .is_some_and(|filename| !source_directory.join(filename).exists())
        });
        if can_restore_original_paths {
            for path in claim_paths {
                let filename = path
                    .file_name()
                    .ok_or_else(|| std::io::Error::other("claimed path has no filename"))?;
                std::fs::rename(&path, source_directory.join(filename))?;
            }
            std::fs::remove_dir(&claim_directory)?;
        } else {
            let _ = expose_claim_directory(source_directory, &claim_directory)?;
        }
    }
    if std::fs::read_dir(processing_directory)?.next().is_none() {
        std::fs::remove_dir(processing_directory)?;
    }
    Ok(())
}

fn expose_claim_directory(
    source_directory: &Path,
    claim_directory: &Path,
) -> std::io::Result<PathBuf> {
    let claim_name = claim_directory
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("claim");
    let mut recovered_directory = source_directory.join(format!("recovered-{claim_name}"));
    let mut suffix = 1;
    while recovered_directory.exists() {
        recovered_directory = source_directory.join(format!("recovered-{claim_name}-{suffix}"));
        suffix += 1;
    }
    std::fs::rename(claim_directory, &recovered_directory)?;
    Ok(recovered_directory)
}

async fn expose_claim_for_retry(claimed_path: &Path) -> std::io::Result<PathBuf> {
    let claim_directory = claimed_path
        .parent()
        .ok_or_else(|| std::io::Error::other("claimed source has no parent"))?;
    let processing_directory = claim_directory
        .parent()
        .ok_or_else(|| std::io::Error::other("claimed source has no processing directory"))?;
    let source_directory = processing_directory
        .parent()
        .ok_or_else(|| std::io::Error::other("claimed source has no source directory"))?;
    let recovered_directory = expose_claim_directory(source_directory, claim_directory)?;
    if std::fs::read_dir(processing_directory)?.next().is_none() {
        tokio::fs::remove_dir(processing_directory).await?;
    }
    let filename = claimed_path
        .file_name()
        .ok_or_else(|| std::io::Error::other("claimed source has no filename"))?;
    Ok(recovered_directory.join(filename))
}

pub async fn start_webdav_import_job(
    config: Arc<Config>,
    pool: DbPool,
    webdav_request_gate: crate::webdav::WebDAVRequestGate,
) {
    let poll_interval = std::time::Duration::from_secs(config.webdav.poll_interval_seconds);
    let mut poll = tokio::time::interval(poll_interval);
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        poll.tick().await;
        run_webdav_import_cycle(&config, &pool, &webdav_request_gate).await;
    }
}

pub async fn run_webdav_import_cycle(
    config: &Config,
    pool: &DbPool,
    webdav_request_gate: &crate::webdav::WebDAVRequestGate,
) {
    let Ok(entries) = std::fs::read_dir(&paths().webdav) else {
        return;
    };
    let mut pending_imports = Vec::new();
    for entry in entries.flatten() {
        let user_directory = entry.path();
        let Some(username) = user_directory.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(user_id) = lookup_user_id(pool, username) else {
            continue;
        };
        let ready_file_paths = match select_ready_webdav_files(pool, user_id) {
            Some(ready_file_paths) => ready_file_paths,
            None => continue,
        };
        pending_imports.extend(
            collect_ready_webdav_files(
                &user_directory,
                ready_file_paths,
                config.webdav.stable_file_age_seconds,
            )
            .into_iter()
            .map(|candidate| (candidate, user_id, user_directory.clone())),
        );
    }
    if pending_imports.is_empty() {
        return;
    }
    let Ok(job_id) = create_import_job(pool, ImportSource::Webdav) else {
        return;
    };
    let Ok(upload_barrier) = Arc::clone(webdav_request_gate)
        .acquire_many_owned(config.webdav.max_concurrent_requests as u32)
        .await
    else {
        return;
    };
    let mut claimed_imports = Vec::with_capacity(pending_imports.len());
    let mut claim_errors = Vec::new();
    for (candidate, user_id, user_directory) in pending_imports {
        let source_path = candidate.source_path;
        if !is_stable_webdav_file(&source_path, config.webdav.stable_file_age_seconds) {
            continue;
        }
        if !webdav_file_is_ready(pool, user_id, &candidate.ready_file_path) {
            continue;
        }
        match claim_source(
            &source_path,
            candidate.supplemental_metadata_path.as_deref(),
        )
        .await
        {
            Ok(claimed_path) => claimed_imports.push((
                source_path,
                claimed_path,
                user_id,
                user_directory,
                candidate.ready_file_path,
                candidate.supplemental_ready_file_path,
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => claim_errors.push(error.to_string()),
        }
    }
    drop(upload_barrier);
    if let Ok(connection) = pool.get() {
        let _ = connection.execute(
            queries::import::SET_WEBDAV_JOB_TOTAL,
            rusqlite::params![(claimed_imports.len() + claim_errors.len()) as i64, job_id],
        );
    }
    for error in claim_errors {
        update_import_progress(pool, job_id, false, Some(error));
    }
    let semaphore = Arc::new(Semaphore::new(
        config.webdav.max_concurrent_processing.max(1),
    ));
    let mut tasks = JoinSet::new();
    for (
        source_path,
        claimed_path,
        user_id,
        user_directory,
        ready_file_path,
        supplemental_ready_file_path,
    ) in claimed_imports
    {
        let pool = pool.clone();
        let semaphore = Arc::clone(&semaphore);
        tasks.spawn(async move {
                let Ok(_permit) = semaphore.acquire().await else {
                    return;
                };
                if let Err(error) = import_staged_file(&claimed_path, ImportSource::Webdav, user_id, &pool, true).await {
                    warn!(path = %source_path.display(), "WebDAV import failed: {error}");
                    if let Ok(recovered_path) = expose_claim_for_retry(&claimed_path).await {
                        if !update_recovered_ready_paths(
                            &pool,
                            user_id,
                            &user_directory,
                            &ready_file_path,
                            supplemental_ready_file_path.as_deref(),
                            &recovered_path,
                        ) {
                            let supplemental_source_path = supplemental_ready_file_path
                                .as_deref()
                                .map(|path| user_directory.join(path));
                            if let Err(restore_error) = restore_recovered_ready_files(
                                &recovered_path,
                                &source_path,
                                supplemental_source_path.as_deref(),
                            ) {
                                warn!(path = %recovered_path.display(), "WebDAV retry readiness recovery failed: {restore_error}");
                            }
                        }
                    }
                    update_import_progress(&pool, job_id, false, Some(error.to_string()));
                } else {
                    if let Err(error) = tokio::fs::remove_file(&claimed_path).await {
                        if error.kind() != std::io::ErrorKind::NotFound {
                            warn!(path = %source_path.display(), "WebDAV imported file cleanup failed: {error}");
                        }
                    }
                    let _ = remove_claim_directory(&claimed_path).await;
                    delete_ready_paths(
                        &pool,
                        user_id,
                        &ready_file_path,
                        supplemental_ready_file_path.as_deref(),
                    );
                    update_import_progress(&pool, job_id, true, None);
                }
            });
    }
    while tasks.join_next().await.is_some() {}
    if let Ok(connection) = pool.get() {
        let _ = connection.execute(queries::import::COMPLETE_JOB, [job_id]);
    }
}

async fn claim_source(
    source_path: &Path,
    source_supplemental_metadata_path: Option<&Path>,
) -> Result<PathBuf, std::io::Error> {
    let parent = source_path
        .parent()
        .ok_or_else(|| std::io::Error::other("import source has no parent"))?;
    let filename = source_path
        .file_name()
        .ok_or_else(|| std::io::Error::other("import source has no filename"))?;
    let claim_directory = parent
        .join(".processing")
        .join(uuid::Uuid::new_v4().to_string());
    tokio::fs::create_dir_all(&claim_directory).await?;
    let claimed_path = claim_directory.join(filename);
    tokio::fs::rename(source_path, &claimed_path).await?;
    if let Some(source_supplemental_metadata_path) = source_supplemental_metadata_path {
        let claimed_supplemental_metadata_path = source_supplemental_metadata_path
            .file_name()
            .map(|filename| claim_directory.join(filename))
            .ok_or_else(|| std::io::Error::other("supplemental metadata has no filename"))?;
        if let Err(error) = tokio::fs::rename(
            source_supplemental_metadata_path,
            &claimed_supplemental_metadata_path,
        )
        .await
        {
            let _ = tokio::fs::rename(&claimed_path, source_path).await;
            let _ = tokio::fs::remove_dir(&claim_directory).await;
            return Err(error);
        }
    }
    Ok(claimed_path)
}

async fn restore_claim(claimed_path: &Path, source_path: &Path) -> Result<(), std::io::Error> {
    tokio::fs::rename(claimed_path, source_path).await?;
    if let Some(claimed_supplemental_metadata_path) = find_supplemental_metadata_path(claimed_path)
    {
        let restored_supplemental_metadata_path = source_path
            .parent()
            .zip(claimed_supplemental_metadata_path.file_name())
            .map(|(parent, filename)| parent.join(filename))
            .ok_or_else(|| std::io::Error::other("supplemental metadata has no filename"))?;
        tokio::fs::rename(
            claimed_supplemental_metadata_path,
            restored_supplemental_metadata_path,
        )
        .await?;
    }
    remove_claim_directory(claimed_path).await
}

async fn remove_claim_directory(claimed_path: &Path) -> Result<(), std::io::Error> {
    let Some(claim_directory) = claimed_path.parent() else {
        return Ok(());
    };
    tokio::fs::remove_dir(claim_directory).await
}

fn collect_import_files(root: &Path) -> Vec<PathBuf> {
    let mut source_files = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return source_files;
    };
    collect_supported_files(entries, &mut source_files);
    source_files
}

fn collect_supported_files(entries: std::fs::ReadDir, source_files: &mut Vec<PathBuf>) {
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with('.'))
        {
            continue;
        }
        if path.is_dir() {
            if let Ok(children) = std::fs::read_dir(path) {
                collect_supported_files(children, source_files);
            }
        } else if path.is_file() && media_type(&path).is_some() {
            source_files.push(path);
        }
    }
}

fn lookup_user_id(pool: &DbPool, username: &str) -> Option<i64> {
    let connection = pool.get().ok()?;
    fetch_one(
        &connection,
        queries::users::SELECT_ID_BY_CREDENTIALS,
        &[&username, &username],
        |row| row.get(0),
    )
    .ok()
    .flatten()
}

fn select_ready_webdav_files(pool: &DbPool, user_id: i64) -> Option<Vec<String>> {
    let connection = pool.get().ok()?;
    let mut statement = connection
        .prepare(queries::webdav_ready::SELECT_FOR_USER)
        .ok()?;
    let ready_file_paths = statement
        .query_map([user_id], |row| row.get(0))
        .ok()?
        .collect::<Result<Vec<_>, _>>()
        .ok();
    ready_file_paths
}

fn webdav_file_is_ready(pool: &DbPool, user_id: i64, ready_file_path: &str) -> bool {
    pool.get()
        .ok()
        .and_then(|connection| {
            connection
                .query_row(
                    queries::webdav_ready::EXISTS,
                    rusqlite::params![user_id, ready_file_path],
                    |row| row.get::<_, bool>(0),
                )
                .ok()
        })
        .unwrap_or(false)
}

fn collect_ready_webdav_files(
    user_directory: &Path,
    ready_file_paths: Vec<String>,
    stable_age_seconds: u64,
) -> Vec<WebDavImportCandidate> {
    let mut candidates = Vec::new();
    let mut supplemental_metadata = HashMap::<String, (bool, String)>::new();
    for ready_file_path in &ready_file_paths {
        let relative_path = Path::new(ready_file_path);
        if !safe_webdav_relative_path(relative_path) {
            continue;
        }
        let Some(file_name) = relative_path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some((media_filename, is_exact)) = supplemental_media_filename(file_name) else {
            continue;
        };
        let media_ready_file_path = relative_path
            .with_file_name(media_filename)
            .to_string_lossy()
            .to_string();
        let should_insert = supplemental_metadata
            .get(&media_ready_file_path)
            .is_none_or(|(existing_is_exact, _)| is_exact && !existing_is_exact);
        if should_insert {
            supplemental_metadata
                .insert(media_ready_file_path, (is_exact, ready_file_path.clone()));
        }
    }
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
        let path = user_directory.join(relative_path);
        if !is_stable_webdav_file(&path, stable_age_seconds) || media_type(&path).is_none() {
            continue;
        }
        let supplemental_ready_file_path = supplemental_metadata
            .get(&ready_file_path)
            .map(|(_, path)| path.clone());
        let supplemental_metadata_path = supplemental_ready_file_path
            .as_deref()
            .map(|path| user_directory.join(path))
            .filter(|path| path.is_file());
        candidates.push(WebDavImportCandidate {
            source_path: path,
            supplemental_metadata_path,
            ready_file_path,
            supplemental_ready_file_path,
        });
    }
    candidates
}

fn safe_webdav_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

fn delete_ready_paths(
    pool: &DbPool,
    user_id: i64,
    ready_file_path: &str,
    supplemental_ready_file_path: Option<&str>,
) {
    let Ok(connection) = pool.get() else {
        return;
    };
    let Ok(transaction) = connection.unchecked_transaction() else {
        return;
    };
    if transaction
        .execute(
            queries::webdav_ready::DELETE,
            rusqlite::params![user_id, ready_file_path],
        )
        .is_err()
    {
        return;
    }
    if let Some(supplemental_ready_file_path) = supplemental_ready_file_path {
        if transaction
            .execute(
                queries::webdav_ready::DELETE,
                rusqlite::params![user_id, supplemental_ready_file_path],
            )
            .is_err()
        {
            return;
        }
    }
    if let Err(error) = transaction.commit() {
        warn!("failed to remove imported WebDAV readiness: {error}");
    }
}

fn update_recovered_ready_paths(
    pool: &DbPool,
    user_id: i64,
    user_directory: &Path,
    ready_file_path: &str,
    supplemental_ready_file_path: Option<&str>,
    recovered_path: &Path,
) -> bool {
    let Some(recovered_ready_file_path) = recovered_path
        .strip_prefix(user_directory)
        .ok()
        .and_then(|path| path.to_str())
    else {
        return false;
    };
    let recovered_supplemental_path = find_supplemental_metadata_path(recovered_path);
    let recovered_supplemental_ready_file_path = recovered_supplemental_path
        .as_deref()
        .and_then(|path| path.strip_prefix(user_directory).ok())
        .and_then(|path| path.to_str());
    let Ok(connection) = pool.get() else {
        return false;
    };
    let Ok(transaction) = connection.unchecked_transaction() else {
        return false;
    };
    if transaction
        .execute(
            queries::webdav_ready::DELETE,
            rusqlite::params![user_id, ready_file_path],
        )
        .and_then(|_| {
            transaction.execute(
                queries::webdav_ready::UPSERT,
                rusqlite::params![user_id, recovered_ready_file_path],
            )
        })
        .is_err()
    {
        return false;
    }
    if let Some(supplemental_ready_file_path) = supplemental_ready_file_path {
        if transaction
            .execute(
                queries::webdav_ready::DELETE,
                rusqlite::params![user_id, supplemental_ready_file_path],
            )
            .is_err()
        {
            return false;
        }
    }
    if let Some(recovered_supplemental_ready_file_path) = recovered_supplemental_ready_file_path {
        if transaction
            .execute(
                queries::webdav_ready::UPSERT,
                rusqlite::params![user_id, recovered_supplemental_ready_file_path],
            )
            .is_err()
        {
            return false;
        }
    }
    transaction.commit().is_ok()
}

fn restore_recovered_ready_files(
    recovered_path: &Path,
    source_path: &Path,
    supplemental_source_path: Option<&Path>,
) -> std::io::Result<()> {
    if source_path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "WebDAV source path was replaced before retry recovery",
        ));
    }
    let recovered_supplemental_path = find_supplemental_metadata_path(recovered_path);
    std::fs::rename(recovered_path, source_path)?;
    if let Some(supplemental_source_path) = supplemental_source_path {
        if let Some(recovered_supplemental_path) = recovered_supplemental_path {
            std::fs::rename(recovered_supplemental_path, supplemental_source_path)?;
        }
    }
    let recovered_directory = recovered_path
        .parent()
        .ok_or_else(|| std::io::Error::other("recovered source has no parent"))?;
    if std::fs::read_dir(recovered_directory)?.next().is_none() {
        std::fs::remove_dir(recovered_directory)?;
    }
    Ok(())
}

fn is_stable_webdav_file(path: &Path, stable_age_seconds: u64) -> bool {
    let minimum_age = std::time::Duration::from_secs(stable_age_seconds);
    path.metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| std::time::SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age >= minimum_age)
}

fn supplemental_media_filename(file_name: &str) -> Option<(String, bool)> {
    const MARKER: &str = ".supplemental-metadata";

    let marker_index = file_name.rfind(MARKER)?;
    let suffix = &file_name[marker_index + MARKER.len()..];
    if !suffix.ends_with(".json") {
        return None;
    }
    let media_filename = &file_name[..marker_index];
    if media_filename.is_empty() {
        return None;
    }
    Some((media_filename.to_string(), suffix == ".json"))
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
