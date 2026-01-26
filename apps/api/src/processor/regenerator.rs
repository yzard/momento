use chrono::{DateTime, Utc};
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

use crate::config::Config;
use crate::constants::{ORIGINALS_DIR, THUMBNAILS_DIR, THUMBNAILS_TINY_DIR};
use crate::database::execute_query;
use crate::database::{fetch_all, queries, DbPool};
use crate::processor::media_processor::{
    calculate_geohash, delete_from_rtree, generate_complete_metadata, insert_into_rtree,
};
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
    pub total_media: i64,
    pub processed_media: i64,
    pub updated_metadata: i64,
    pub generated_thumbnails: i64,
    pub updated_tags: i64,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub errors: Vec<String>,
}

impl Default for RegenerationJob {
    fn default() -> Self {
        Self {
            status: RegenerationStatus::Idle,
            total_media: 0,
            processed_media: 0,
            updated_metadata: 0,
            generated_thumbnails: 0,
            updated_tags: 0,
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

fn is_cancel_requested() -> bool {
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

fn finalize_job_cancelled() {
    let mut job = CURRENT_JOB.write().unwrap();
    job.status = RegenerationStatus::Cancelled;
    job.completed_at = Some(Utc::now());
}

fn update_job_totals(total_media: i64) {
    let mut job = CURRENT_JOB.write().unwrap();
    job.total_media = total_media;
}

fn update_job_progress(
    metadata_updated: bool,
    thumbnail_generated: bool,
    tags_updated: i64,
    error: Option<&str>,
) {
    let mut job = CURRENT_JOB.write().unwrap();
    job.processed_media += 1;
    if metadata_updated {
        job.updated_metadata += 1;
    }
    if thumbnail_generated {
        job.generated_thumbnails += 1;
    }
    job.updated_tags += tags_updated;
    if let Some(msg) = error {
        push_job_error(&mut job.errors, msg);
    }
}

fn merge_keyword_tags(conn: &rusqlite::Connection, media_id: i64, keywords: Option<&str>) -> i64 {
    let keywords = match keywords {
        Some(kw) if !kw.is_empty() => kw,
        _ => return 0,
    };

    let tags: Vec<&str> = keywords
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if tags.is_empty() {
        return 0;
    }

    let mut inserted_count = 0;
    for tag in tags {
        // Check if tag exists
        let existing: Option<i64> = conn
            .query_row(queries::regenerator::SELECT_TAG_ID, [tag], |row| row.get(0))
            .ok();

        let tag_id = match existing {
            Some(id) => id,
            None => {
                conn.execute(queries::regenerator::INSERT_TAG, [tag]).ok();
                conn.last_insert_rowid()
            }
        };

        conn.execute(
            queries::regenerator::INSERT_MEDIA_TAG,
            rusqlite::params![media_id, tag_id],
        )
        .ok();
        inserted_count += 1;
    }

    inserted_count
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
            let thumb_file = THUMBNAILS_DIR.join(&thumb_path);
            let _ = std::fs::remove_file(&thumb_file);
        }

        let _ = conn.execute(queries::regenerator::CLEAR_METADATA, [id]);
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
                    let full_path = ORIGINALS_DIR.join(&path);
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
        .filter(|row| row.width.is_none() || row.height.is_none())
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
    update_job_totals(count as i64);

    if count == 0 {
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

                let original_path = ORIGINALS_DIR.join(&row.file_path);
                if !original_path.exists() {
                    let msg = format!("Missing file: {}", row.file_path);
                    error!("{}", msg);
                    update_job_progress(false, false, 0, Some(&msg));
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
                let gps_latitude = metadata.gps_latitude.or(row.gps_latitude);
                let gps_longitude = metadata.gps_longitude.or(row.gps_longitude);
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
                let kw_clone = keywords.clone();
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
                                duration_seconds,
                                row_id
                            ],
                        );

                        let geohash = match (gps_latitude, gps_longitude) {
                            (Some(lat), Some(lon)) => calculate_geohash(lat, lon),
                            _ => None,
                        };

                        if let Err(err) = conn.execute(
                            "UPDATE media SET geohash = ? WHERE id = ?",
                            rusqlite::params![geohash, row_id],
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

                let metadata_updated = row.width.is_none() || row.height.is_none();
                let mut thumbnail_generated = false;

                let thumbnail_missing = row.thumbnail_path.is_none()
                    || row
                        .thumbnail_path
                        .as_ref()
                        .map(|p| !THUMBNAILS_DIR.join(p).exists())
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

                    let thumbnail_output = THUMBNAILS_DIR.join(&thumbnail_relative);
                    let tiny_thumbnail_output = THUMBNAILS_TINY_DIR.join(&thumbnail_relative);

                    if let Some(parent) = tiny_thumbnail_output.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }

                    thumbnail_generated = if row.media_type == "image" {
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

                let pool_clone = pool.clone();
                let row_id = row.id;

                let tags_updated = tokio::task::spawn_blocking(move || {
                    if let Ok(conn) = pool_clone.get() {
                        merge_keyword_tags(&conn, row_id, kw_clone.as_deref())
                    } else {
                        0
                    }
                })
                .await
                .unwrap_or(0);

                update_job_progress(metadata_updated, thumbnail_generated, tags_updated, None);
                Some(())
            }
        })
        .buffer_unordered(concurrency);

    while (stream.next().await).is_some() {}

    let job = get_regeneration_status();
    info!(
        "Generation completed. Metadata updated: {}, Thumbnails generated: {}",
        job.updated_metadata, job.generated_thumbnails
    );

    let job = get_regeneration_status();
    info!(
        "Generation completed. Metadata updated: {}, Thumbnails generated: {}",
        job.updated_metadata, job.generated_thumbnails
    );

    if is_cancel_requested() {
        finalize_job_cancelled();
    } else {
        finalize_job_success();
    }
}
