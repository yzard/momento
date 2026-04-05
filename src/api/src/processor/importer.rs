use chrono::{DateTime, Utc};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::config::Config;
use crate::constants::{IMPORTS_DIR, SUPPORTED_EXTENSIONS, WEBDAV_DIR};
use crate::database::{fetch_one, DbPool};
use crate::processor::media_processor::{process_media_file, MediaProcessingContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportStatus {
    Idle,
    Running,
    Completed,
    Failed,
}

impl fmt::Display for ImportStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportStatus::Idle => write!(f, "idle"),
            ImportStatus::Running => write!(f, "running"),
            ImportStatus::Completed => write!(f, "completed"),
            ImportStatus::Failed => write!(f, "failed"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImportJob {
    pub status: ImportStatus,
    pub total_files: i64,
    pub processed_files: i64,
    pub successful_imports: i64,
    pub failed_imports: i64,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub errors: Vec<String>,
}

#[derive(Clone)]
pub struct ImportSettings {
    pub processing: MediaProcessingContext,
    pub delete_after_import: bool,
    pub concurrency: usize,
}

impl Default for ImportJob {
    fn default() -> Self {
        Self {
            status: ImportStatus::Idle,
            total_files: 0,
            processed_files: 0,
            successful_imports: 0,
            failed_imports: 0,
            started_at: None,
            completed_at: None,
            errors: Vec::new(),
        }
    }
}

/// Maximum number of errors to store in job state to prevent unbounded memory growth
const MAX_JOB_ERRORS: usize = 100;

lazy_static::lazy_static! {
    static ref CURRENT_JOB: RwLock<ImportJob> = RwLock::new(ImportJob::default());
}

fn push_job_error(errors: &mut Vec<String>, message: &str) {
    if errors.len() < MAX_JOB_ERRORS {
        errors.push(message.to_string());
    } else if errors.len() == MAX_JOB_ERRORS {
        errors.push("(additional errors truncated)".to_string());
    }
}

pub fn get_import_status() -> ImportJob {
    CURRENT_JOB.read().unwrap().clone()
}

pub fn is_import_running() -> bool {
    CURRENT_JOB.read().unwrap().status == ImportStatus::Running
}

fn start_import_job() {
    let mut job = CURRENT_JOB.write().unwrap();
    if job.status == ImportStatus::Running {
        return;
    }
    *job = ImportJob {
        status: ImportStatus::Running,
        started_at: Some(Utc::now()),
        ..Default::default()
    };
}

fn finalize_job_success() {
    let mut job = CURRENT_JOB.write().unwrap();
    job.status = ImportStatus::Completed;
    job.completed_at = Some(Utc::now());
}

#[allow(dead_code)]
fn finalize_job_failure(message: &str) {
    let mut job = CURRENT_JOB.write().unwrap();
    job.status = ImportStatus::Failed;
    job.completed_at = Some(Utc::now());
    push_job_error(&mut job.errors, message);
}

fn update_job_totals(total_files: i64) {
    let mut job = CURRENT_JOB.write().unwrap();
    job.total_files = total_files;
}

fn update_job_progress(success: bool, error_message: Option<&str>) {
    let mut job = CURRENT_JOB.write().unwrap();
    job.processed_files += 1;
    if success {
        job.successful_imports += 1;
    } else {
        job.failed_imports += 1;
        if let Some(msg) = error_message {
            push_job_error(&mut job.errors, msg);
        }
    }
}

fn collect_import_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    for ext in SUPPORTED_EXTENSIONS.iter() {
        // Collect files with both cases
        let patterns = vec![
            format!("**/*{}", ext),
            format!("**/*{}", ext.to_uppercase()),
            format!("*{}", ext),
            format!("*{}", ext.to_uppercase()),
        ];

        for pattern in patterns {
            let glob_pattern = root.join(&pattern);
            if let Ok(paths) = glob::glob(glob_pattern.to_str().unwrap_or("")) {
                for path in paths.filter_map(Result::ok) {
                    if path.is_file() && !files.contains(&path) {
                        files.push(path);
                    }
                }
            }
        }
    }

    files
}

use futures::stream::{self, StreamExt};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

pub async fn run_local_import(settings: ImportSettings) {
    start_import_job();

    let files_to_import = collect_import_files(&IMPORTS_DIR);
    update_job_totals(files_to_import.len() as i64);

    let effective_concurrency = if settings.concurrency > 0 {
        settings.concurrency
    } else {
        num_cpus::get()
    };
    let semaphore = Arc::new(Semaphore::new(effective_concurrency));
    let delete_after_import = settings.delete_after_import;
    let processing = settings.processing;

    let mut stream = stream::iter(files_to_import)
        .map(move |file_path| {
            let semaphore = semaphore.clone();
            let processing = processing.clone();

            async move {
                let _permit = semaphore.acquire().await.unwrap();

                if !file_path.exists() {
                    update_job_progress(
                        false,
                        Some(&format!("Missing file: {}", file_path.display())),
                    );
                    return;
                }

                let media_id = process_media_file(&file_path, &processing).await;

                if media_id.is_none() {
                    update_job_progress(
                        false,
                        Some(&format!("Failed to process: {}", file_path.display())),
                    );
                    return;
                }

                if delete_after_import {
                    if let Err(e) = tokio::fs::remove_file(&file_path).await {
                        update_job_progress(
                            false,
                            Some(&format!("Failed to delete {}: {}", file_path.display(), e)),
                        );
                        return;
                    }
                }

                update_job_progress(true, None);
            }
        })
        .buffer_unordered(effective_concurrency);

    while (stream.next().await).is_some() {}

    finalize_job_success();
}

pub async fn start_webdav_import_job(config: Arc<Config>, pool: DbPool) {
    if !config.webdav.enabled {
        info!("WebDAV import job disabled");
        return;
    }

    let poll_interval =
        std::time::Duration::from_secs(config.webdav.processing.poll_interval_seconds);

    info!(
        "Starting WebDAV import job: polling every {}s, root={}",
        config.webdav.processing.poll_interval_seconds,
        WEBDAV_DIR.display()
    );

    loop {
        run_webdav_import_cycle(&config, &pool).await;
        tokio::time::sleep(poll_interval).await;
    }
}

async fn run_webdav_import_cycle(config: &Config, pool: &DbPool) {
    if !WEBDAV_DIR.exists() {
        warn!(
            "WebDAV root directory missing, skipping import cycle: {}",
            WEBDAV_DIR.display()
        );
        return;
    }

    let Ok(entries) = std::fs::read_dir(&*WEBDAV_DIR) else {
        error!(
            "Failed to read WebDAV root directory: {}",
            WEBDAV_DIR.display()
        );
        return;
    };

    let semaphore = Arc::new(Semaphore::new(
        config.webdav.processing.max_concurrent_processing,
    ));

    let mut user_dir_count = 0usize;
    let mut skipped_user_dirs = 0usize;
    let mut queued_files = 0usize;
    let mut tasks: JoinSet<()> = JoinSet::new();

    for entry in entries.filter_map(|e| e.ok()) {
        let user_dir = entry.path();
        if !user_dir.is_dir() {
            continue;
        }

        let username = match user_dir.file_name().and_then(|n| n.to_str()) {
            Some(name) if !name.starts_with('.') => name.to_string(),
            _ => continue,
        };

        user_dir_count += 1;

        let user_id = match lookup_user_id(pool, &username) {
            Some(id) => id,
            None => {
                warn!("WebDAV directory for unknown user: {}", username);
                skipped_user_dirs += 1;
                continue;
            }
        };

        let stable_age = config.webdav.processing.stable_file_age_seconds;
        let files = collect_stable_webdav_files(&user_dir, stable_age);

        if files.is_empty() {
            debug!(
                "WebDAV import: no stable files for user {} (id={})",
                username, user_id
            );
            continue;
        }

        queued_files += files.len();
        info!(
            "WebDAV import: found {} stable files for user {} (id={})",
            files.len(),
            username,
            user_id
        );

        for file_path in files {
            let semaphore = semaphore.clone();
            let config = config.clone();
            let pool = pool.clone();
            let user_dir = user_dir.clone();

            tasks.spawn(async move {
                let _permit = semaphore.acquire().await.unwrap();

                process_webdav_file(&file_path, user_id, &user_dir, &config, &pool).await;
            });
        }
    }

    while tasks.join_next().await.is_some() {}

    if user_dir_count == 0 {
        debug!(
            "WebDAV import: no user directories found under {}",
            WEBDAV_DIR.display()
        );
        return;
    }

    debug!(
        "WebDAV import cycle complete: users_scanned={}, queued_files={}, skipped_users={}",
        user_dir_count, queued_files, skipped_user_dirs
    );
}

async fn process_webdav_file(
    file_path: &Path,
    user_id: i64,
    user_dir: &Path,
    config: &Config,
    pool: &DbPool,
) {
    let filename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    info!("WebDAV processing: {} for user {}", filename, user_id);

    let processing_dir = user_dir.join(".processing");
    if let Err(e) = std::fs::create_dir_all(&processing_dir) {
        error!(
            "Failed to create processing dir {}: {}",
            processing_dir.display(),
            e
        );
        return;
    }

    let processing_path = processing_dir.join(filename);
    if let Err(e) = std::fs::rename(file_path, &processing_path) {
        error!(
            "Failed to move file to processing: {} ({})",
            file_path.display(),
            e
        );
        return;
    }

    debug!(
        "WebDAV file moved to processing: {}",
        processing_path.display()
    );

    let processing = MediaProcessingContext {
        user_id,
        thumbnails: config.thumbnails.clone(),
        reverse_geocoding: Some(config.reverse_geocoding.clone()),
        pool: pool.clone(),
    };
    let result = process_media_file(&processing_path, &processing).await;

    match result {
        Some(media_id) => {
            info!(
                "WebDAV import success: {} -> media_id={} (thumbnails + metadata generated)",
                filename, media_id
            );
            match tokio::fs::remove_file(&processing_path).await {
                Ok(()) => {
                    debug!(
                        "WebDAV cleaned up processed file: {}",
                        processing_path.display()
                    );
                }
                Err(e) => {
                    warn!("Failed to cleanup processed file: {}", e);
                }
            }
        }
        None => {
            error!("WebDAV import failed: {}", filename);
            move_to_failed(&processing_path, user_dir).await;
        }
    }
}

async fn move_to_failed(processing_path: &Path, user_dir: &Path) {
    let failed_dir = user_dir.join(".failed");
    if let Err(e) = std::fs::create_dir_all(&failed_dir) {
        error!(
            "Failed to create failed dir {}: {}",
            failed_dir.display(),
            e
        );
        return;
    }

    let filename = processing_path.file_name().unwrap_or_default();

    let failed_path = failed_dir.join(filename);
    let error_sidecar = failed_dir.join(format!("{}.error.txt", filename.to_string_lossy()));

    if let Err(e) = std::fs::rename(processing_path, &failed_path) {
        error!(
            "Failed to move file to failed dir {}: {}",
            failed_path.display(),
            e
        );
        return;
    }

    debug!("WebDAV moved failed file to {}", failed_path.display());

    let error_content = format!(
        "Import failed at: {}\nOriginal path: {}",
        chrono::Utc::now().to_rfc3339(),
        processing_path.display()
    );

    match std::fs::write(&error_sidecar, error_content) {
        Ok(()) => {
            debug!("WebDAV wrote error sidecar: {}", error_sidecar.display());
        }
        Err(e) => {
            warn!("Failed to write error sidecar: {}", e);
        }
    }
}

fn lookup_user_id(pool: &DbPool, username: &str) -> Option<i64> {
    let conn = pool.get().ok()?;
    fetch_one(
        &conn,
        "SELECT id FROM users WHERE username = ? AND is_active = 1",
        &[&username],
        |row| row.get(0),
    )
    .ok()
    .flatten()
}

fn collect_stable_webdav_files(dir: &Path, stable_age_seconds: u64) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let now = std::time::SystemTime::now();
    let min_age = std::time::Duration::from_secs(stable_age_seconds);

    collect_stable_files_recursive(dir, &mut files, now, min_age);
    files
}

fn collect_stable_files_recursive(
    dir: &Path,
    files: &mut Vec<PathBuf>,
    now: std::time::SystemTime,
    min_age: std::time::Duration,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();

        if name.starts_with('.') {
            continue;
        }

        if path.is_dir() {
            collect_stable_files_recursive(&path, files, now, min_age);
        } else if path.is_file() {
            if let Ok(metadata) = path.metadata() {
                if let Ok(modified) = metadata.modified() {
                    if let Ok(age) = now.duration_since(modified) {
                        let supported = is_supported_extension(&path);
                        if age >= min_age && supported {
                            debug!("WebDAV file stable: {}", path.display());
                            files.push(path);
                        } else if age < min_age {
                            debug!(
                                "WebDAV file not stable yet: {} (age {}s < {}s)",
                                path.display(),
                                age.as_secs(),
                                min_age.as_secs()
                            );
                        } else if !supported {
                            debug!(
                                "WebDAV file skipped (unsupported extension): {}",
                                path.display()
                            );
                        }
                    }
                }
            }
        }
    }
}

fn is_supported_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_lowercase()))
        .map(|ext| SUPPORTED_EXTENSIONS.contains(ext.as_str()))
        .unwrap_or(false)
}
