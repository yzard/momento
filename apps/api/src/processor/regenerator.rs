use chrono::{DateTime, Utc};
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

use crate::config::Config;
use crate::constants::{ORIGINALS_DIR, THUMBNAILS_DIR};
use crate::database::{fetch_all, DbPool};
use crate::processor::media_processor::generate_complete_metadata;
use crate::processor::thumbnails::{generate_image_thumbnail, generate_video_thumbnail};

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
    job.errors.push(message.to_string());
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
        job.errors.push(msg.to_string());
    }
}

fn merge_keyword_tags(conn: &rusqlite::Connection, media_id: i64, keywords: Option<&str>) -> i64 {
    let keywords = match keywords {
        Some(kw) if !kw.is_empty() => kw,
        _ => return 0,
    };

    let tags: Vec<&str> = keywords.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if tags.is_empty() {
        return 0;
    }

    let mut inserted_count = 0;
    for tag in tags {
        // Check if tag exists
        let existing: Option<i64> = conn
            .query_row("SELECT id FROM tags WHERE name = ?", [tag], |row| row.get(0))
            .ok();

        let tag_id = match existing {
            Some(id) => id,
            None => {
                conn.execute("INSERT INTO tags (name) VALUES (?)", [tag]).ok();
                conn.last_insert_rowid()
            }
        };

        conn.execute(
            "INSERT OR IGNORE INTO media_tags (media_id, tag_id) VALUES (?, ?)",
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
    let rows: Vec<(i64, Option<String>)> = fetch_all(
        &conn,
        "SELECT id, thumbnail_path FROM media",
        &[],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .unwrap_or_default();

    let mut cleared_count = 0;

    for (id, thumbnail_path) in rows {
        // Delete thumbnail file
        if let Some(thumb_path) = thumbnail_path {
            let thumb_file = THUMBNAILS_DIR.join(&thumb_path);
            let _ = std::fs::remove_file(&thumb_file);
        }

        // Clear metadata
        let _ = conn.execute(
            r#"
            UPDATE media SET
                thumbnail_path = NULL,
                width = NULL,
                height = NULL,
                duration_seconds = NULL,
                date_taken = NULL,
                gps_latitude = NULL,
                gps_longitude = NULL,
                gps_altitude = NULL,
                camera_make = NULL,
                camera_model = NULL,
                iso = NULL,
                exposure_time = NULL,
                f_number = NULL,
                focal_length = NULL,
                location_state = NULL,
                location_country = NULL,
                keywords = NULL
            WHERE id = ?
            "#,
            [id],
        );
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
    iso: Option<i32>,
    exposure_time: Option<String>,
    f_number: Option<f64>,
    focal_length: Option<f64>,
    location_state: Option<String>,
    location_country: Option<String>,
    keywords: Option<String>,
}

pub fn run_regeneration(missing_only: bool, config: &Config, pool: &DbPool) {
    clear_cancel_request();
    start_job();

    let conn = match pool.get() {
        Ok(c) => c,
        Err(e) => {
            finalize_job_failure(&format!("Failed to get connection: {}", e));
            return;
        }
    };

    let rows: Vec<MediaRow> = match fetch_all(
        &conn,
        r#"
        SELECT id, user_id, file_path, thumbnail_path, media_type, width, height,
               duration_seconds, date_taken, gps_latitude, gps_longitude, gps_altitude,
               camera_make, camera_model, iso, exposure_time, f_number, focal_length,
               location_state, location_country, keywords
        FROM media
        ORDER BY id
        "#,
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
                iso: row.get(14)?,
                exposure_time: row.get(15)?,
                f_number: row.get(16)?,
                focal_length: row.get(17)?,
                location_state: row.get(18)?,
                location_country: row.get(19)?,
                keywords: row.get(20)?,
            })
        },
    ) {
        Ok(r) => r,
        Err(e) => {
            finalize_job_failure(&format!("Failed to fetch media: {}", e));
            return;
        }
    };

    update_job_totals(rows.len() as i64);

    for row in rows {
        if is_cancel_requested() {
            finalize_job_cancelled();
            clear_cancel_request();
            return;
        }

        let original_path = ORIGINALS_DIR.join(&row.file_path);
        if !original_path.exists() {
            update_job_progress(false, false, 0, Some(&format!("Missing file: {}", row.file_path)));
            continue;
        }

        let thumbnail_file = row.thumbnail_path.as_ref().map(|p| THUMBNAILS_DIR.join(p));
        let thumbnail_missing = row.thumbnail_path.is_none()
            || thumbnail_file.as_ref().map(|f| !f.exists()).unwrap_or(true);

        let metadata_missing = row.width.is_none()
            || row.height.is_none()
            || row.date_taken.is_none()
            || row.gps_latitude.is_none()
            || row.camera_make.is_none()
            || row.iso.is_none();

        if missing_only && !metadata_missing && !thumbnail_missing {
            update_job_progress(false, false, 0, None);
            continue;
        }

        let geo_config = if missing_only { None } else { Some(&config.reverse_geocoding) };
        let metadata = generate_complete_metadata(&original_path, &row.media_type, geo_config);

        // Helper to choose existing or new value
        fn choose<T: Clone>(missing_only: bool, existing: Option<T>, new_value: Option<T>) -> Option<T> {
            if missing_only && existing.is_some() {
                existing
            } else {
                new_value.or(existing)
            }
        }

        let width = choose(missing_only, row.width, metadata.width);
        let height = choose(missing_only, row.height, metadata.height);
        let date_taken = metadata.date_taken.map(|dt| dt.to_rfc3339()).or(row.date_taken.clone());
        let gps_latitude = choose(missing_only, row.gps_latitude, metadata.gps_latitude);
        let gps_longitude = choose(missing_only, row.gps_longitude, metadata.gps_longitude);
        let gps_altitude = choose(missing_only, row.gps_altitude, metadata.gps_altitude);
        let camera_make = choose(missing_only, row.camera_make.clone(), metadata.camera_make);
        let camera_model = choose(missing_only, row.camera_model.clone(), metadata.camera_model);
        let iso = choose(missing_only, row.iso, metadata.iso);
        let exposure_time = choose(missing_only, row.exposure_time.clone(), metadata.exposure_time);
        let f_number = choose(missing_only, row.f_number, metadata.f_number);
        let focal_length = choose(missing_only, row.focal_length, metadata.focal_length);
        let location_state = choose(missing_only, row.location_state.clone(), metadata.location_state);
        let location_country = choose(missing_only, row.location_country.clone(), metadata.location_country);
        let keywords = choose(missing_only, row.keywords.clone(), metadata.keywords);
        let duration_seconds = choose(missing_only, row.duration_seconds, metadata.duration_seconds);

        // Update metadata
        let _ = conn.execute(
            r#"
            UPDATE media SET
                width = ?, height = ?, date_taken = ?,
                gps_latitude = ?, gps_longitude = ?, gps_altitude = ?,
                camera_make = ?, camera_model = ?, iso = ?,
                exposure_time = ?, f_number = ?, focal_length = ?,
                location_state = ?, location_country = ?, keywords = ?,
                duration_seconds = ?
            WHERE id = ?
            "#,
            rusqlite::params![
                width, height, date_taken,
                gps_latitude, gps_longitude, gps_altitude,
                camera_make, camera_model, iso,
                exposure_time, f_number, focal_length,
                location_state, location_country, keywords,
                duration_seconds, row.id
            ],
        );

        let metadata_updated = true;
        let mut thumbnail_generated = false;

        // Generate thumbnail if needed
        if !missing_only || thumbnail_missing {
            let thumbnail_relative = row.thumbnail_path.clone().unwrap_or_else(|| {
                PathBuf::from(row.user_id.to_string())
                    .join(format!("{}.jpg", PathBuf::from(&row.file_path).file_stem().unwrap().to_string_lossy()))
                    .to_string_lossy()
                    .to_string()
            });
            let thumbnail_output = THUMBNAILS_DIR.join(&thumbnail_relative);

            thumbnail_generated = if row.media_type == "image" {
                generate_image_thumbnail(
                    &original_path,
                    &thumbnail_output,
                    config.thumbnails.max_size,
                    config.thumbnails.quality,
                )
            } else {
                generate_video_thumbnail(
                    &original_path,
                    &thumbnail_output,
                    config.thumbnails.max_size,
                    config.thumbnails.quality,
                    config.thumbnails.video_frame_quality,
                )
            };

            if thumbnail_generated {
                let _ = conn.execute(
                    "UPDATE media SET thumbnail_path = ? WHERE id = ?",
                    rusqlite::params![thumbnail_relative, row.id],
                );
            }
        }

        let tags_updated = merge_keyword_tags(&conn, row.id, keywords.as_deref());

        update_job_progress(metadata_updated, thumbnail_generated, tags_updated, None);
    }

    finalize_job_success();
}
