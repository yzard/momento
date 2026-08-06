use chrono::{DateTime, Utc};
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

use crate::config::Config;
use crate::constants::paths;
use crate::database::execute_query;
use crate::database::{fetch_all, queries, DbConn, DbPool};
use crate::llm_client::generate_missing_batches;
use crate::processor::media_processor::{
    calculate_geohash, delete_from_rtree, generate_complete_metadata, insert_into_rtree,
};
use crate::processor::metadata::{normalize_gps_coordinates, MediaMetadata};
use crate::processor::thumbnails::{generate_image_thumbnail, generate_video_thumbnail};
use crate::utils::hash::calculate_file_hash;
use futures::stream::{self, StreamExt};
use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegenerationStatus {
    Idle,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl fmt::Display for RegenerationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegenerationStatus::Idle => write!(f, "idle"),
            RegenerationStatus::Running => write!(f, "running"),
            RegenerationStatus::Completed => write!(f, "completed"),
            RegenerationStatus::Failed => write!(f, "failed"),
            RegenerationStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RegenerationJob {
    pub status: RegenerationStatus,
    pub total_jobs: i64,
    pub completed_jobs: i64,
    pub metadata_jobs: i64,
    pub metadata_completed: i64,
    pub thumbnail_jobs: i64,
    pub thumbnails_completed: i64,
    pub image_text_jobs: i64,
    pub image_text_completed: i64,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub errors: Vec<String>,
}

impl Default for RegenerationJob {
    fn default() -> Self {
        Self {
            status: RegenerationStatus::Idle,
            total_jobs: 0,
            completed_jobs: 0,
            metadata_jobs: 0,
            metadata_completed: 0,
            thumbnail_jobs: 0,
            thumbnails_completed: 0,
            image_text_jobs: 0,
            image_text_completed: 0,
            started_at: None,
            completed_at: None,
            errors: Vec::new(),
        }
    }
}

/// Maximum number of errors to store in job state to prevent unbounded memory growth
const MAX_JOB_ERRORS: usize = 100;

lazy_static::lazy_static! {
    static ref CURRENT_JOB: RwLock<RegenerationJob> = RwLock::new(RegenerationJob::default());
}

static CANCEL_REQUESTED: AtomicBool = AtomicBool::new(false);

pub fn get_regeneration_status() -> RegenerationJob {
    CURRENT_JOB.read().unwrap().clone()
}

pub fn is_regeneration_running() -> bool {
    CURRENT_JOB.read().unwrap().status == RegenerationStatus::Running
}

pub fn cancel_regeneration() -> bool {
    let job = CURRENT_JOB.read().unwrap();
    if job.status != RegenerationStatus::Running {
        return false;
    }
    CANCEL_REQUESTED.store(true, Ordering::SeqCst);
    true
}

pub(crate) fn is_cancel_requested() -> bool {
    CANCEL_REQUESTED.load(Ordering::SeqCst)
}

fn clear_cancel_request() {
    CANCEL_REQUESTED.store(false, Ordering::SeqCst);
}

fn start_job() {
    let mut job = CURRENT_JOB.write().unwrap();
    if job.status == RegenerationStatus::Running {
        return;
    }
    *job = RegenerationJob {
        status: RegenerationStatus::Running,
        started_at: Some(Utc::now()),
        ..Default::default()
    };
}

fn finalize_job_success() {
    let mut job = CURRENT_JOB.write().unwrap();
    job.status = RegenerationStatus::Completed;
    job.completed_at = Some(Utc::now());
}

fn finalize_job_failure(message: &str) {
    let mut job = CURRENT_JOB.write().unwrap();
    job.status = RegenerationStatus::Failed;
    job.completed_at = Some(Utc::now());
    push_job_error(&mut job.errors, message);
}

fn push_job_error(errors: &mut Vec<String>, message: &str) {
    if errors.len() < MAX_JOB_ERRORS {
        errors.push(message.to_string());
    } else if errors.len() == MAX_JOB_ERRORS {
        errors.push("(additional errors truncated)".to_string());
    }
}

pub(crate) fn record_regeneration_error(message: &str) {
    let mut job = CURRENT_JOB.write().unwrap();
    push_job_error(&mut job.errors, message);
}

pub(crate) fn record_image_text_job_completed() {
    let mut job = CURRENT_JOB.write().unwrap();
    job.image_text_completed += 1;
    job.completed_jobs += 1;
}

fn finalize_job_cancelled() {
    let mut job = CURRENT_JOB.write().unwrap();
    job.status = RegenerationStatus::Cancelled;
    job.completed_at = Some(Utc::now());
}

async fn generate_missing_llm_metadata(config: &Config, pool: &DbPool) {
    if is_cancel_requested() {
        return;
    }
    generate_missing_batches(config, pool).await;
}

fn update_job_totals(metadata_jobs: i64, thumbnail_jobs: i64, image_text_jobs: i64) {
    let mut job = CURRENT_JOB.write().unwrap();
    job.metadata_jobs = metadata_jobs;
    job.thumbnail_jobs = thumbnail_jobs;
    job.image_text_jobs = image_text_jobs;
    job.total_jobs = metadata_jobs + thumbnail_jobs + image_text_jobs;
}

fn update_job_progress(
    metadata_job_completed: bool,
    thumbnail_job_completed: bool,
    error: Option<&str>,
) {
    let mut job = CURRENT_JOB.write().unwrap();
    if metadata_job_completed {
        job.metadata_completed += 1;
        job.completed_jobs += 1;
    }
    if thumbnail_job_completed {
        job.thumbnails_completed += 1;
        job.completed_jobs += 1;
    }
    if let Some(msg) = error {
        push_job_error(&mut job.errors, msg);
    }
}

fn count_missing_image_text_jobs(conn: &DbConn, config: &Config) -> i64 {
    if !config.llm.enabled {
        return 0;
    }

    let mut total = 0;
    for model_type in [
        crate::constants::OCR_MODEL_TYPE,
        crate::constants::IMAGE_TAGGING_MODEL_TYPE,
    ] {
        if model_type == crate::constants::IMAGE_TAGGING_MODEL_TYPE
            && !config.llm.image_tagging_enabled
        {
            continue;
        }
        total += fetch_all(
            conn,
            queries::image_text::SELECT_MISSING_FOR_MODEL_TYPE,
            &[&model_type],
            |row| row.get::<_, i64>(0),
        )
        .map(|rows| rows.len() as i64)
        .unwrap_or(0);
    }
    total
}

pub fn clear_all_metadata_and_thumbnails(pool: &DbPool) -> i64 {
    let conn = match pool.get() {
        Ok(c) => c,
        Err(_) => return 0,
    };

    // Get all media with thumbnails
    let rows: Vec<(i64, Option<String>)> =
        fetch_all(&conn, queries::regenerator::SELECT_THUMBNAILS, &[], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap_or_default();

    let mut cleared_count = 0;

    for (id, thumbnail_path) in rows {
        if let Some(thumb_path) = thumbnail_path {
            let thumb_file = paths().thumbnails.join(&thumb_path);
            let _ = std::fs::remove_file(&thumb_file);
        }

        let _ = conn.execute(queries::regenerator::CLEAR_METADATA, [id]);
        let _ = conn.execute(queries::image_text::DELETE_BY_IMAGE_ID, [id]);
        cleared_count += 1;
    }

    cleared_count
}

#[derive(Debug)]
struct MediaRow {
    id: i64,
    user_id: i64,
    file_path: String,
    thumbnail_path: Option<String>,
    media_type: String,
    width: Option<i32>,
    height: Option<i32>,
    duration_seconds: Option<f64>,
    date_taken: Option<String>,
    gps_latitude: Option<f64>,
    gps_longitude: Option<f64>,
    gps_altitude: Option<f64>,
    camera_make: Option<String>,
    camera_model: Option<String>,
    lens_make: Option<String>,
    lens_model: Option<String>,
    iso: Option<i32>,
    exposure_time: Option<String>,
    f_number: Option<f64>,
    focal_length: Option<f64>,
    focal_length_35mm: Option<f64>,
    location_city: Option<String>,
    location_state: Option<String>,
    location_country: Option<String>,
    video_codec: Option<String>,
    keywords: Option<String>,
}

use tracing::{error, info};

pub async fn generate_missing_metadata(config: &Config, pool: &DbPool) {
    clear_cancel_request();
    start_job();

    let conn = match pool.get() {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("Failed to get connection: {}", e);
            error!("{}", msg);
            finalize_job_failure(&msg);
            return;
        }
    };

    // Backfill missing hashes
    let hash_rows: Vec<(i64, String)> =
        fetch_all(&conn, queries::media::SELECT_WITHOUT_HASH, &[], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap_or_default();

    if !hash_rows.is_empty() {
        info!("Backfilling hashes for {} items", hash_rows.len());
        let hash_semaphore = Arc::new(Semaphore::new(if config.regenerate.num_cpus > 0 {
            config.regenerate.num_cpus
        } else {
            num_cpus::get()
        }));
        let pool_hash = pool.clone();

        stream::iter(hash_rows)
            .for_each_concurrent(Some(num_cpus::get()), |(id, path)| {
                let pool = pool_hash.clone();
                let sem = hash_semaphore.clone();
                async move {
                    let _permit = sem.acquire().await.unwrap();
                    let full_path = paths().originals.join(&path);
                    if let Ok(hash) = calculate_file_hash(&full_path).await {
                        let _ = tokio::task::spawn_blocking(move || {
                            if let Ok(c) = pool.get() {
                                let _ = execute_query(
                                    &c,
                                    queries::media::UPDATE_CONTENT_HASH,
                                    &[&hash, &id],
                                );
                            }
                        })
                        .await;
                    }
                }
            })
            .await;
    }

    let rows: Vec<MediaRow> = match fetch_all(
        &conn,
        queries::regenerator::SELECT_MISSING_METADATA,
        &[],
        |row| {
            Ok(MediaRow {
                id: row.get(0)?,
                user_id: row.get(1)?,
                file_path: row.get(2)?,
                thumbnail_path: row.get(3)?,
                media_type: row.get(4)?,
                width: row.get(5)?,
                height: row.get(6)?,
                duration_seconds: row.get(7)?,
                date_taken: row.get(8)?,
                gps_latitude: row.get(9)?,
                gps_longitude: row.get(10)?,
                gps_altitude: row.get(11)?,
                camera_make: row.get(12)?,
                camera_model: row.get(13)?,
                lens_make: row.get(14)?,
                lens_model: row.get(15)?,
                iso: row.get(16)?,
                exposure_time: row.get(17)?,
                f_number: row.get(18)?,
                focal_length: row.get(19)?,
                focal_length_35mm: row.get(20)?,
                location_city: row.get(21)?,
                location_state: row.get(22)?,
                location_country: row.get(23)?,
                video_codec: row.get(24)?,
                keywords: row.get(25)?,
            })
        },
    ) {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("Failed to fetch media: {}", e);
            error!("{}", msg);
            finalize_job_failure(&msg);
            return;
        }
    };

    let count = rows.len();
    let missing_metadata = rows
        .iter()
        .filter(|row| {
            row.width.is_none()
                || row.height.is_none()
                || matches!(
                    (row.gps_latitude, row.gps_longitude),
                    (Some(latitude), Some(longitude)) if latitude == 0.0 && longitude == 0.0
                )
        })
        .count();
    let missing_thumbnails = rows
        .iter()
        .filter(|row| row.thumbnail_path.is_none())
        .count();
    info!(
        "Starting metadata/thumbnail generation for {} items (missing metadata: {}, missing thumbnails: {})",
        count,
        missing_metadata,
        missing_thumbnails
    );
    let metadata_jobs = missing_metadata as i64;
    let thumbnail_jobs = missing_thumbnails as i64;
    let image_text_jobs = count_missing_image_text_jobs(&conn, config);
    update_job_totals(metadata_jobs, thumbnail_jobs, image_text_jobs);

    if count == 0 {
        generate_missing_llm_metadata(config, pool).await;
        finalize_job_success();
        return;
    }

    // Limit concurrency to avoid overloading the system
    let concurrency = if config.regenerate.num_cpus > 0 {
        config.regenerate.num_cpus
    } else {
        num_cpus::get()
    };
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let config = Arc::new(config.clone());
    let pool = pool.clone();

    let mut stream = stream::iter(rows)
        .map(|row| {
            let semaphore = semaphore.clone();
            let config = config.clone();
            let pool = pool.clone();

            async move {
                let _permit = semaphore.acquire().await.unwrap();

                if is_cancel_requested() {
                    return None;
                }

                let original_path = paths().originals.join(&row.file_path);
                if !original_path.exists() {
                    let msg = format!("Missing file: {}", row.file_path);
                    error!("{}", msg);
                    update_job_progress(false, false, Some(&msg));
                    return Some(());
                }

                // Since we filtered by NULLs, we know we need to generate things.
                // But we still check specifically what's missing for the 'choose' logic.

                let geo_config = Some(&config.reverse_geocoding);

                // Always generate complete metadata as we are in "fill missing" mode
                let metadata =
                    generate_complete_metadata(&original_path, &row.media_type, geo_config).await;

                // Choose logic: If DB has value, keep it (unless we want to overwrite, but this function is 'generate missing')
                // Wait, if we came from "Clean & Regenerate", the DB values are NULL, so we take new metadata.
                // If we came from "Generate Info" (missing only), existing valid values are kept.

                fn choose<T: Clone>(existing: Option<T>, new_value: Option<T>) -> Option<T> {
                    existing.or(new_value)
                }

                let width = choose(row.width, metadata.width);
                let height = choose(row.height, metadata.height);
                let date_taken = row
                    .date_taken
                    .clone()
                    .or(metadata.date_taken.map(|dt| dt.to_rfc3339()));
                let mut gps_metadata = MediaMetadata {
                    gps_latitude: metadata.gps_latitude.or(row.gps_latitude),
                    gps_longitude: metadata.gps_longitude.or(row.gps_longitude),
                    ..MediaMetadata::default()
                };
                normalize_gps_coordinates(&mut gps_metadata);
                let gps_latitude = gps_metadata.gps_latitude;
                let gps_longitude = gps_metadata.gps_longitude;
                let gps_altitude = metadata.gps_altitude.or(row.gps_altitude);
                let camera_make = choose(row.camera_make.clone(), metadata.camera_make);
                let camera_model = choose(row.camera_model.clone(), metadata.camera_model);
                let lens_make = choose(row.lens_make.clone(), metadata.lens_make);
                let lens_model = choose(row.lens_model.clone(), metadata.lens_model);
                let iso = choose(row.iso, metadata.iso);
                let exposure_time = choose(row.exposure_time.clone(), metadata.exposure_time);
                let f_number = choose(row.f_number, metadata.f_number);
                let focal_length = choose(row.focal_length, metadata.focal_length);
                let location_city = choose(row.location_city.clone(), metadata.location_city);
                let location_state = choose(row.location_state.clone(), metadata.location_state);
                let location_country =
                    choose(row.location_country.clone(), metadata.location_country);
                let keywords = choose(row.keywords.clone(), metadata.keywords);
                let duration_seconds = choose(row.duration_seconds, metadata.duration_seconds);
                let focal_length_35mm = choose(row.focal_length_35mm, metadata.focal_length_35mm);
                let video_codec = choose(row.video_codec.clone(), metadata.video_codec);

                let pool_clone = pool.clone();
                let row_id = row.id;

                let update_keywords = keywords.clone();
                let update_result = tokio::task::spawn_blocking(move || {
                    if let Ok(conn) = pool_clone.get() {
                        let _ = conn.execute(
                            queries::regenerator::UPDATE_METADATA,
                            rusqlite::params![
                                row_id,
                                width,
                                height,
                                date_taken,
                                gps_latitude,
                                gps_longitude,
                                gps_altitude,
                                camera_make,
                                camera_model,
                                lens_make,
                                lens_model,
                                iso,
                                exposure_time,
                                f_number,
                                focal_length,
                                focal_length_35mm,
                                location_city,
                                location_state,
                                location_country,
                                video_codec,
                                update_keywords,
                                duration_seconds
                            ],
                        );

                        let geohash = match (gps_latitude, gps_longitude) {
                            (Some(lat), Some(lon)) => calculate_geohash(lat, lon),
                            _ => None,
                        };

                        if let Err(err) = conn.execute(
                            "INSERT INTO media_metadata (media_id, geohash) VALUES (?, ?) ON CONFLICT(media_id) DO UPDATE SET geohash = excluded.geohash",
                            rusqlite::params![row_id, geohash],
                        ) {
                            error!("Failed to update geohash for {}: {}", row_id, err);
                        }

                        if let Err(err) = delete_from_rtree(&conn, row_id) {
                            error!("Failed to clear rtree for {}: {}", row_id, err);
                        }

                        if let (Some(lat), Some(lon)) = (gps_latitude, gps_longitude) {
                            if let Err(err) = insert_into_rtree(&conn, row_id, lat, lon) {
                                error!("Failed to insert rtree for {}: {}", row_id, err);
                            }
                        }
                    }
                })
                .await;

                if let Err(e) = update_result {
                    error!("Failed to update metadata DB for {}: {}", row_id, e);
                }

                let metadata_job = row.width.is_none() || row.height.is_none();
                let thumbnail_missing = row.thumbnail_path.is_none()
                    || row
                        .thumbnail_path
                        .as_ref()
                        .map(|p| !paths().thumbnails.join(p).exists())
                        .unwrap_or(true);

                if thumbnail_missing {
                    let thumbnail_relative = row.thumbnail_path.clone().unwrap_or_else(|| {
                        PathBuf::from(row.user_id.to_string())
                            .join(format!(
                                "{}.jpg",
                                PathBuf::from(&row.file_path)
                                    .file_stem()
                                    .unwrap()
                                    .to_string_lossy()
                            ))
                            .to_string_lossy()
                            .to_string()
                    });

                    let thumbnail_output = paths().thumbnails.join(&thumbnail_relative);
                    let tiny_thumbnail_output = paths().thumbnails_tiny.join(&thumbnail_relative);

                    if let Some(parent) = tiny_thumbnail_output.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }

                    let thumbnail_generated = if row.media_type == "image" {
                        let normal_ok = generate_image_thumbnail(
                            &original_path,
                            &thumbnail_output,
                            config.thumbnails.max_size,
                            config.thumbnails.quality,
                        )
                        .await;

                        let _ = generate_image_thumbnail(
                            &original_path,
                            &tiny_thumbnail_output,
                            config.thumbnails.tiny_size,
                            config.thumbnails.quality,
                        )
                        .await;

                        normal_ok
                    } else {
                        let normal_ok = generate_video_thumbnail(
                            &original_path,
                            &thumbnail_output,
                            config.thumbnails.max_size,
                            config.thumbnails.quality,
                            config.thumbnails.video_frame_quality,
                        )
                        .await;

                        let _ = generate_video_thumbnail(
                            &original_path,
                            &tiny_thumbnail_output,
                            config.thumbnails.tiny_size,
                            config.thumbnails.quality,
                            config.thumbnails.video_frame_quality,
                        )
                        .await;

                        normal_ok
                    };

                    if thumbnail_generated {
                        let pool_clone = pool.clone();
                        let row_id = row.id;
                        let thumb_path = thumbnail_relative.clone();

                        let _ = tokio::task::spawn_blocking(move || {
                            if let Ok(conn) = pool_clone.get() {
                                let _ = conn.execute(
                                    queries::regenerator::UPDATE_THUMBNAIL,
                                    rusqlite::params![thumb_path, row_id],
                                );
                            }
                        })
                        .await;
                    }
                }

                update_job_progress(metadata_job, thumbnail_missing, None);
                Some(())
            }
        })
        .buffer_unordered(concurrency);

    while (stream.next().await).is_some() {}

    generate_missing_llm_metadata(&config, &pool).await;

    let job = get_regeneration_status();
    info!(
        "Generation completed. Metadata updated: {}, Thumbnails generated: {}, image text updated: {}",
        job.metadata_completed, job.thumbnails_completed, job.image_text_completed
    );

    if is_cancel_requested() {
        finalize_job_cancelled();
    } else if !job.errors.is_empty() {
        finalize_job_failure("Regeneration completed with errors");
    } else {
        finalize_job_success();
    }
}
