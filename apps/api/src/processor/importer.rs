use chrono::{DateTime, Utc};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::config::ReverseGeocodingConfig;
use crate::constants::{IMPORTS_DIR, SUPPORTED_EXTENSIONS};
use crate::database::DbPool;
use crate::processor::media_processor::process_media_file;

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

lazy_static::lazy_static! {
    static ref CURRENT_JOB: RwLock<ImportJob> = RwLock::new(ImportJob::default());
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

fn finalize_job_failure(message: &str) {
    let mut job = CURRENT_JOB.write().unwrap();
    job.status = ImportStatus::Failed;
    job.completed_at = Some(Utc::now());
    job.errors.push(message.to_string());
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
            job.errors.push(msg.to_string());
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

pub async fn run_local_import(
    user_id: i64,
    thumbnail_max_size: u32,
    tiny_thumbnail_size: u32,
    thumbnail_quality: u8,
    video_frame_quality: u8,
    delete_after_import: bool,
    reverse_geo_config: Option<&ReverseGeocodingConfig>,
    pool: &DbPool,
    concurrency: usize,
) {
    start_import_job();

    let files_to_import = collect_import_files(&IMPORTS_DIR);
    update_job_totals(files_to_import.len() as i64);

    let effective_concurrency = if concurrency > 0 {
        concurrency
    } else {
        num_cpus::get()
    };
    let semaphore = Arc::new(Semaphore::new(effective_concurrency));
    let config_rev_geo = reverse_geo_config.cloned();
    let config_rev_geo = config_rev_geo.map(Arc::new);
    let pool = pool.clone();

    let mut stream = stream::iter(files_to_import)
        .map(|file_path| {
            let semaphore = semaphore.clone();
            let config_rev_geo = config_rev_geo.clone();
            let pool = pool.clone();

            async move {
                let _permit = semaphore.acquire().await.unwrap();

                if !file_path.exists() {
                    update_job_progress(
                        false,
                        Some(&format!("Missing file: {}", file_path.display())),
                    );
                    return;
                }

                let media_id = process_media_file(
                    &file_path,
                    user_id,
                    thumbnail_max_size,
                    tiny_thumbnail_size,
                    thumbnail_quality,
                    video_frame_quality,
                    config_rev_geo.as_deref(),
                    &pool,
                )
                .await;

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

// Note: WebDAV import would require a WebDAV client crate
// For now, we'll provide a stub that can be implemented later
pub fn run_webdav_import(
    _user_id: i64,
    _hostname: &str,
    _username: &str,
    _password: &str,
    _remote_path: &str,
    _thumbnail_max_size: u32,
    _thumbnail_quality: u8,
    _video_frame_quality: u8,
    _reverse_geo_config: Option<&ReverseGeocodingConfig>,
    _pool: &DbPool,
) {
    start_import_job();
    finalize_job_failure("WebDAV import not yet implemented in Rust version");
}
