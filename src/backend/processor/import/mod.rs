use futures::stream::{self, StreamExt};
use rusqlite::OptionalExtension;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tracing::warn;

use crate::config::Config;
use crate::constants::{paths, IMAGE_EXTENSIONS, VIDEO_EXTENSIONS};
use crate::database::{execute_query, fetch_one, queries, DbPool};
use crate::error::{AppError, AppResult};
use crate::processor::media_processor::build_original_filename;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportSource {
    Local,
    Webdav,
}

impl ImportSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Webdav => "webdav",
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
    pub delete_after_import: bool,
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

pub fn get_import_status(pool: &DbPool) -> AppResult<ImportJob> {
    let connection = pool.get().map_err(AppError::Pool)?;
    let job = connection
        .query_row(queries::import::SELECT_LATEST_JOB, [], |row| {
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
        })
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
                let claimed_path = match claim_source(&source_path).await {
                    Ok(claimed_path) => claimed_path,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
                    Err(error) => {
                        update_import_progress(&settings.pool, job_id, false, Some(error.to_string()));
                        return;
                    }
                };
                let import_result = finalize_staged_original(
                    &claimed_path,
                    ImportSource::Local,
                    settings.user_id,
                    &settings.pool,
                )
                .await;
                match import_result {
                    Ok(_) if settings.delete_after_import => {
                        if let Err(error) = tokio::fs::remove_file(&claimed_path).await {
                            warn!(path = %source_path.display(), "local imported file cleanup failed: {error}");
                        }
                        let _ = remove_claim_directory(&claimed_path).await;
                        update_import_progress(&settings.pool, job_id, true, None);
                    }
                    Ok(_) => {
                        if let Err(error) = restore_claim(&claimed_path, &source_path).await {
                            update_import_progress(&settings.pool, job_id, false, Some(error.to_string()));
                            return;
                        }
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

pub async fn finalize_staged_original(
    source_path: &Path,
    import_source: ImportSource,
    user_id: i64,
    pool: &DbPool,
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
    let mime_type = mime_guess::from_path(source_path)
        .first_raw()
        .unwrap_or("application/octet-stream")
        .to_string();

    let (media_id, temporary_relative_path) = {
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
                import_source.as_str(),
            ],
        )?;
        if rows != 1 {
            return Err(AppError::Internal(
                "failed to allocate media ID".to_string(),
            ));
        }
        (connection.last_insert_rowid(), temporary_relative_path)
    };

    let final_filename = build_original_filename(media_id, source_path);
    let final_relative_path = PathBuf::from(&final_filename);
    let temporary_path = paths().originals.join(&temporary_relative_path);
    let final_path = paths().originals.join(&final_relative_path);
    let final_supplemental_metadata_path = supplemental_metadata_path(&final_path);

    let result = async {
        let temporary_parent = temporary_path.parent().ok_or_else(|| {
            AppError::Internal("temporary original path has no parent".to_string())
        })?;
        tokio::fs::create_dir_all(temporary_parent).await?;
        tokio::fs::copy(source_path, &temporary_path).await?;
        tokio::fs::rename(&temporary_path, &final_path).await?;
        move_supplemental_metadata(source_path, &final_supplemental_metadata_path).await?;

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
            let _ = tokio::fs::rename(&final_path, source_path).await;
        }
        let source_supplemental_metadata_path = supplemental_metadata_path(source_path);
        if final_supplemental_metadata_path.is_file() {
            let _ = tokio::fs::rename(
                &final_supplemental_metadata_path,
                source_supplemental_metadata_path,
            )
            .await;
        }
        let connection = pool.get()?;
        let _ = execute_query(
            &connection,
            queries::import::MARK_FAILED,
            &[&error.to_string(), &media_id],
        );
        return Err(error);
    }
    Ok(media_id)
}

async fn move_supplemental_metadata(source_path: &Path, destination_path: &Path) -> AppResult<()> {
    let source_supplemental_metadata_path = supplemental_metadata_path(source_path);
    if !source_supplemental_metadata_path.is_file() {
        return Ok(());
    }
    tokio::fs::rename(&source_supplemental_metadata_path, destination_path).await?;
    Ok(())
}

fn supplemental_metadata_path(media_path: &Path) -> PathBuf {
    let media_filename = media_path
        .file_name()
        .and_then(|filename| filename.to_str())
        .unwrap_or_default();
    media_path.with_file_name(format!("{media_filename}.supplemental-metadata.json"))
}

pub fn recover_interrupted_imports(pool: &DbPool) -> AppResult<()> {
    let connection = pool.get()?;
    let interrupted_imports = connection
        .prepare(queries::import::SELECT_INTERRUPTED)?
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (media_id, file_path, original_filename) in interrupted_imports {
        if paths().originals.join(&file_path).is_file() {
            connection.execute(
                queries::import::MARK_IMPORTED,
                rusqlite::params![original_filename, file_path, media_id],
            )?;
            connection.execute(queries::metadata_jobs::INSERT_QUEUED, [media_id])?;
        } else {
            connection.execute(
                queries::import::MARK_FAILED,
                rusqlite::params!["original file was not finalized", media_id],
            )?;
        }
    }
    Ok(())
}

pub async fn start_webdav_import_job(config: Arc<Config>, pool: DbPool) {
    if !config.webdav.enabled {
        return;
    }
    let poll_interval = std::time::Duration::from_secs(config.webdav.poll_interval_seconds);
    loop {
        run_webdav_import_cycle(&config, &pool).await;
        tokio::time::sleep(poll_interval).await;
    }
}

async fn run_webdav_import_cycle(config: &Config, pool: &DbPool) {
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
        pending_imports.extend(
            collect_stable_webdav_files(&user_directory, config.webdav.stable_file_age_seconds)
                .into_iter()
                .map(|source_path| (source_path, user_id)),
        );
    }
    if pending_imports.is_empty() {
        return;
    }
    let Ok(job_id) = create_import_job(pool, ImportSource::Webdav) else {
        return;
    };
    if let Ok(connection) = pool.get() {
        let _ = connection.execute(
            queries::import::SET_WEBDAV_JOB_TOTAL,
            rusqlite::params![pending_imports.len() as i64, job_id],
        );
    }
    let semaphore = Arc::new(Semaphore::new(
        config.webdav.max_concurrent_processing.max(1),
    ));
    let mut tasks = JoinSet::new();
    for (source_path, user_id) in pending_imports {
        let pool = pool.clone();
        let semaphore = Arc::clone(&semaphore);
        tasks.spawn(async move {
                let Ok(_permit) = semaphore.acquire().await else {
                    return;
                };
                let claimed_path = match claim_source(&source_path).await {
                    Ok(claimed_path) => claimed_path,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
                    Err(error) => {
                        update_import_progress(&pool, job_id, false, Some(error.to_string()));
                        return;
                    }
                };
                if let Err(error) = finalize_staged_original(&claimed_path, ImportSource::Webdav, user_id, &pool).await {
                    warn!(path = %source_path.display(), "WebDAV import failed: {error}");
                    let _ = restore_claim(&claimed_path, &source_path).await;
                    update_import_progress(&pool, job_id, false, Some(error.to_string()));
                } else if let Err(error) = tokio::fs::remove_file(&claimed_path).await {
                    warn!(path = %source_path.display(), "WebDAV imported file cleanup failed: {error}");
                    let _ = remove_claim_directory(&claimed_path).await;
                    update_import_progress(&pool, job_id, true, None);
                } else {
                    let _ = remove_claim_directory(&claimed_path).await;
                    update_import_progress(&pool, job_id, true, None);
                }
            });
    }
    while tasks.join_next().await.is_some() {}
    if let Ok(connection) = pool.get() {
        let _ = connection.execute(queries::import::COMPLETE_JOB, [job_id]);
    }
}

async fn claim_source(source_path: &Path) -> Result<PathBuf, std::io::Error> {
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
    let source_supplemental_metadata_path = supplemental_metadata_path(source_path);
    if !source_supplemental_metadata_path.is_file() {
        return Ok(claimed_path);
    }
    let claimed_supplemental_metadata_path = supplemental_metadata_path(&claimed_path);
    if let Err(error) = tokio::fs::rename(
        &source_supplemental_metadata_path,
        &claimed_supplemental_metadata_path,
    )
    .await
    {
        let _ = tokio::fs::rename(&claimed_path, source_path).await;
        let _ = tokio::fs::remove_dir(&claim_directory).await;
        return Err(error);
    }
    Ok(claimed_path)
}

async fn restore_claim(claimed_path: &Path, source_path: &Path) -> Result<(), std::io::Error> {
    tokio::fs::rename(claimed_path, source_path).await?;
    let claimed_supplemental_metadata_path = supplemental_metadata_path(claimed_path);
    if claimed_supplemental_metadata_path.is_file() {
        tokio::fs::rename(
            claimed_supplemental_metadata_path,
            supplemental_metadata_path(source_path),
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

fn collect_stable_webdav_files(directory: &Path, stable_age_seconds: u64) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(directory) else {
        return files;
    };
    let minimum_age = std::time::Duration::from_secs(stable_age_seconds);
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        let hidden = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with('.'));
        if hidden {
            continue;
        }
        if path.is_dir() {
            files.extend(collect_stable_webdav_files(&path, stable_age_seconds));
            continue;
        }
        let is_stable = path
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= minimum_age);
        if is_stable && media_type(&path).is_some() {
            files.push(path);
        }
    }
    files
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
