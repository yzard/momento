use crate::test_utils::{create_test_db, create_test_media, init_test_paths};
use chrono::{TimeZone, Utc};
use momento_api::config::Config;
use momento_api::constants::paths;
use momento_api::processor::metadata::{
    apply_supplemental_metadata, load_supplemental_metadata, supplemental_metadata_path,
    MediaMetadata,
};
use std::fs;

mod reset;
mod reverse_geocoding;

#[tokio::test]
async fn metadata_references_the_canonical_original_for_every_photo_ai_task() {
    init_test_paths();
    let pool = create_test_db();
    let photo_id = create_test_media(&pool, "classifier-preparation.jpg");
    let relative_path = format!("classifier-preparation-{photo_id}.jpg");
    let original_path = paths().originals.join(&relative_path);
    fs::create_dir_all(original_path.parent().expect("original parent")).expect("original parent");
    image::RgbImage::from_pixel(3000, 1000, image::Rgb([20, 40, 60]))
        .save(&original_path)
        .expect("photo fixture");
    let supplemental_path = original_path.with_file_name(format!(
        "{}.supplemental-metadata.json",
        original_path
            .file_name()
            .expect("filename")
            .to_string_lossy()
    ));
    fs::write(&supplemental_path, r#"{"description":"retained"}"#).expect("supplemental fixture");
    pool.get()
        .expect("database connection")
        .execute(
            "UPDATE media SET file_path = ?, import_state = 'imported' WHERE id = ?",
            rusqlite::params![relative_path, photo_id],
        )
        .expect("photo path");
    momento_api::processor::metadata::generate_media_metadata(&pool, photo_id, &Config::default())
        .await
        .expect("metadata generation");

    let connection = pool.get().expect("database connection");
    let regenerated_content_hash: String = connection
        .query_row(
            "SELECT content_hash FROM media WHERE id = ?",
            [photo_id],
            |row| row.get(0),
        )
        .expect("regenerated content hash");
    assert_eq!(regenerated_content_hash.len(), 64);
    let ai_inputs = connection
        .prepare("SELECT task, storage_root, file_path, content_hash FROM media_ai_inputs WHERE media_id = ? ORDER BY task")
        .expect("AI input query")
        .query_map([photo_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .expect("AI input rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("AI inputs");
    assert_eq!(ai_inputs.len(), 7);
    for (task, storage_root, input_path, content_hash) in ai_inputs {
        assert_eq!(storage_root, "originals", "{task} storage root");
        assert_eq!(input_path, relative_path, "{task} original path");
        assert_eq!(content_hash, regenerated_content_hash, "{task} hash");
    }
    assert!(supplemental_path.is_file());
}

#[tokio::test]
async fn metadata_reuses_one_unscaled_full_resolution_video_frame_for_ai() {
    init_test_paths();
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "full-resolution-frame.mp4");
    let ai_directory = paths().previews.join("ai").join(media_id.to_string());
    if ai_directory.exists() {
        fs::remove_dir_all(&ai_directory).expect("remove stale AI fixture directory");
    }
    let relative_path = format!("full-resolution-frame-{media_id}.mp4");
    let original_path = paths().originals.join(&relative_path);
    fs::create_dir_all(original_path.parent().expect("original parent")).expect("original parent");
    let ffmpeg = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=64x32:d=1",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&original_path)
        .output()
        .expect("video fixture command");
    assert!(
        ffmpeg.status.success(),
        "video fixture: {}",
        String::from_utf8_lossy(&ffmpeg.stderr)
    );
    pool.get()
        .expect("database connection")
        .execute(
            "UPDATE media SET file_path = ?, media_type = 'video', mime_type = 'video/mp4', import_state = 'imported' WHERE id = ?",
            rusqlite::params![relative_path, media_id],
        )
        .expect("video media");

    momento_api::processor::metadata::generate_media_metadata(&pool, media_id, &Config::default())
        .await
        .expect("video metadata generation");
    let canonical_original_hash: String = pool
        .get()
        .expect("database connection")
        .query_row(
            "SELECT content_hash FROM media WHERE id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("video original hash");
    let video_dimensions: (i32, i32) = pool
        .get()
        .expect("database connection")
        .query_row(
            "SELECT width, height FROM media_metadata WHERE media_id = ?",
            [media_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("video metadata dimensions");
    assert_eq!(video_dimensions, (64, 32));

    momento_api::processor::metadata::generate_media_metadata(&pool, media_id, &Config::default())
        .await
        .expect("repeated video metadata generation");

    let connection = pool.get().expect("database connection");
    let inputs = connection
        .prepare("SELECT storage_root, file_path, mime_type, input_kind, frame_timestamp_ms FROM media_ai_inputs WHERE media_id = ? ORDER BY task")
        .expect("video AI input query")
        .query_map([media_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<i64>>(4)?,
            ))
        })
        .expect("video AI input rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("video AI inputs");
    assert_eq!(inputs.len(), 5);
    let expected_relative_path = inputs
        .first()
        .map(|(_, file_path, _, _, _)| file_path.clone())
        .expect("shared video frame path");
    assert_eq!(
        expected_relative_path,
        format!("ai/{media_id}/frames/{canonical_original_hash}.png")
    );
    for (storage_root, file_path, mime_type, input_kind, timestamp) in inputs {
        assert_eq!(storage_root, "previews");
        assert_eq!(file_path, expected_relative_path);
        assert_eq!(mime_type, "image/png");
        assert_eq!(input_kind, "video_frame");
        assert_eq!(timestamp, Some(0));
    }
    let frame = image::open(paths().previews.join(expected_relative_path)).expect("video frame");
    assert_eq!((frame.width(), frame.height()), (64, 32));
    let frame_count = fs::read_dir(ai_directory.join("frames"))
        .expect("video frame directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_file())
        .count();
    assert_eq!(frame_count, 1);
}

#[tokio::test]
async fn metadata_rejects_an_original_without_an_image_mime_type() {
    init_test_paths();
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "unsupported-original.jpg");
    let relative_path = format!("unsupported-original-{media_id}.jpg");
    let original_path = paths().originals.join(&relative_path);
    fs::create_dir_all(original_path.parent().expect("original parent")).expect("original parent");
    image::RgbImage::from_pixel(16, 16, image::Rgb([1, 2, 3]))
        .save(&original_path)
        .expect("photo fixture");
    pool.get()
        .expect("database connection")
        .execute(
            "UPDATE media SET file_path = ?, mime_type = 'application/octet-stream', import_state = 'imported' WHERE id = ?",
            rusqlite::params![relative_path, media_id],
        )
        .expect("unsupported media MIME");

    let error = momento_api::processor::metadata::generate_media_metadata(
        &pool,
        media_id,
        &Config::default(),
    )
    .await
    .expect_err("unsupported original should fail");

    assert!(error.contains("supported image MIME type"));
    let input_count: i64 = pool
        .get()
        .expect("database connection")
        .query_row(
            "SELECT COUNT(*) FROM media_ai_inputs WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("AI input count");
    assert_eq!(input_count, 0);
}

#[test]
fn loads_google_photos_supplemental_metadata() {
    let directory = tempfile::tempdir().expect("Failed to create temporary directory");
    let media_path = directory.path().join("IMG_2373.HEIC");
    let sidecar_path = directory
        .path()
        .join("IMG_2373.HEIC.supplemental-metadata.json");
    fs::write(&media_path, b"image").expect("Failed to write media fixture");
    fs::write(
        &sidecar_path,
        r#"{
            "photoTakenTime": {"timestamp": "1530569813"},
            "geoData": {"latitude": 40.759, "longitude": -73.9859, "altitude": 303.0}
        }"#,
    )
    .expect("Failed to write metadata fixture");

    assert_eq!(supplemental_metadata_path(&media_path), Some(sidecar_path));
    let data = load_supplemental_metadata(&media_path).expect("Sidecar should load");
    let mut metadata = MediaMetadata::default();
    apply_supplemental_metadata(&mut metadata, &data);

    assert_eq!(
        metadata.date_taken,
        Utc.timestamp_opt(1530569813, 0).single()
    );
    assert_eq!(metadata.gps_latitude, Some(40.759));
    assert_eq!(metadata.gps_longitude, Some(-73.9859));
    assert_eq!(metadata.gps_altitude, Some(303.0));
}

#[test]
fn finds_numbered_supplemental_metadata_sidecar() {
    let directory = tempfile::tempdir().expect("Failed to create temporary directory");
    let media_path = directory.path().join("Snapseed.heic");
    let sidecar_path = directory
        .path()
        .join("Snapseed.heic.supplemental-metadata(10).json");
    fs::write(&media_path, b"image").expect("Failed to write media fixture");
    fs::write(&sidecar_path, "{}").expect("Failed to write metadata fixture");

    assert_eq!(supplemental_metadata_path(&media_path), Some(sidecar_path));
}

#[test]
fn does_not_find_sidecar_outside_media_directory() {
    let directory = tempfile::tempdir().expect("Failed to create temporary directory");
    let processing_directory = directory.path().join(".processing");
    fs::create_dir(&processing_directory).expect("Failed to create processing directory");
    let media_path = processing_directory.join("IMG_2373.HEIC");
    let sidecar_path = directory
        .path()
        .join("IMG_2373.HEIC.supplemental-metadata.json");
    fs::write(&media_path, b"image").expect("Failed to write media fixture");
    fs::write(&sidecar_path, "{}").expect("Failed to write metadata fixture");

    assert_eq!(supplemental_metadata_path(&media_path), None);
}

#[test]
fn supplemental_metadata_overrides_present_embedded_values() {
    let data = serde_json::json!({
        "photoTakenTime": {"timestamp": "1530569813"},
        "geoData": {"latitude": 40.759, "longitude": -73.9859, "altitude": 303.0},
        "description": "updated keywords"
    });
    let mut metadata = MediaMetadata {
        date_taken: Utc.timestamp_opt(1, 0).single(),
        gps_latitude: Some(1.0),
        gps_longitude: Some(2.0),
        gps_altitude: Some(3.0),
        location_city: Some("Old city".to_string()),
        location_state: Some("Old state".to_string()),
        location_country: Some("Old country".to_string()),
        keywords: Some("old keywords".to_string()),
        ..MediaMetadata::default()
    };

    apply_supplemental_metadata(&mut metadata, &data);

    assert_eq!(
        metadata.date_taken,
        Utc.timestamp_opt(1530569813, 0).single()
    );
    assert_eq!(metadata.gps_latitude, Some(40.759));
    assert_eq!(metadata.gps_longitude, Some(-73.9859));
    assert_eq!(metadata.gps_altitude, Some(303.0));
    assert_eq!(metadata.location_city, None);
    assert_eq!(metadata.location_state, None);
    assert_eq!(metadata.location_country, None);
    assert_eq!(metadata.keywords.as_deref(), Some("updated keywords"));
}

#[test]
fn supplemental_metadata_uses_geo_data_when_exif_coordinates_are_zero() {
    let data = serde_json::json!({
        "geoDataExif": {"latitude": 0.0, "longitude": 0.0},
        "geoData": {"latitude": 40.759, "longitude": -73.9859}
    });
    let mut metadata = MediaMetadata::default();

    apply_supplemental_metadata(&mut metadata, &data);

    assert_eq!(metadata.gps_latitude, Some(40.759));
    assert_eq!(metadata.gps_longitude, Some(-73.9859));
}

#[test]
fn supplemental_metadata_replaces_zero_embedded_coordinates() {
    let data = serde_json::json!({
        "geoData": {"latitude": 40.759, "longitude": -73.9859}
    });
    let mut metadata = MediaMetadata {
        gps_latitude: Some(0.0),
        gps_longitude: Some(0.0),
        ..MediaMetadata::default()
    };

    apply_supplemental_metadata(&mut metadata, &data);

    assert_eq!(metadata.gps_latitude, Some(40.759));
    assert_eq!(metadata.gps_longitude, Some(-73.9859));
}

#[test]
fn zero_gps_coordinates_are_normalized_to_missing() {
    let mut metadata = MediaMetadata {
        gps_latitude: Some(0.0),
        gps_longitude: Some(0.0),
        ..MediaMetadata::default()
    };

    momento_api::processor::metadata::normalize_gps_coordinates(&mut metadata);

    assert_eq!(metadata.gps_latitude, None);
    assert_eq!(metadata.gps_longitude, None);
}

#[test]
fn supplemental_metadata_ignores_zero_coordinate_components() {
    let data = serde_json::json!({
        "geoData": {"latitude": 41.031669, "longitude": 0.0}
    });
    let mut metadata = MediaMetadata::default();

    apply_supplemental_metadata(&mut metadata, &data);

    assert_eq!(metadata.gps_latitude, None);
    assert_eq!(metadata.gps_longitude, None);
}

#[test]
fn metadata_claims_are_exclusive_and_expired_leases_are_recovered() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "claim.jpg");
    let connection = pool.get().expect("connection");
    connection
        .execute(
            "INSERT INTO media_metadata_jobs (media_id, status) VALUES (?, 'queued')",
            [media_id],
        )
        .expect("job");
    drop(connection);
    assert_eq!(
        momento_api::processor::metadata_worker::claim_next_job(&pool).expect("first claim"),
        Some(media_id)
    );
    assert_eq!(
        momento_api::processor::metadata_worker::claim_next_job(&pool).expect("second claim"),
        None
    );
    let connection = pool.get().expect("connection");
    connection.execute("UPDATE media_metadata_jobs SET claimed_at = datetime('now', '-10 minutes') WHERE media_id = ?", [media_id]).expect("expire lease");
    drop(connection);
    momento_api::processor::metadata_worker::reclaim_expired_leases(&pool, 30).expect("reclaim");
    assert_eq!(
        momento_api::processor::metadata_worker::claim_next_job(&pool).expect("reclaimed claim"),
        Some(media_id)
    );
}

#[test]
fn metadata_rerun_requested_during_processing_runs_after_current_attempt() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "rerun-request.jpg");
    let connection = pool.get().expect("connection");
    connection
        .execute(
            "INSERT INTO media_metadata_jobs (media_id, status, attempts, claimed_at) VALUES (?, 'processing', 1, datetime('now'))",
            [media_id],
        )
        .expect("processing job");
    connection
        .execute(
            momento_api::database::queries::metadata_jobs::REQUEST_RERUN,
            [media_id],
        )
        .expect("request rerun");
    let processing_state: (String, i64) = connection
        .query_row(
            "SELECT status, rerun_requested FROM media_metadata_jobs WHERE media_id = ?",
            [media_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("processing state");
    assert_eq!(processing_state, ("processing".to_string(), 1));
    drop(connection);

    momento_api::processor::metadata_worker::finish_job(&pool, media_id, Ok(()), 3)
        .expect("finish current attempt");
    let queued_state: (String, i64, i64) = pool
        .get()
        .expect("connection")
        .query_row(
            "SELECT status, rerun_requested, attempts FROM media_metadata_jobs WHERE media_id = ?",
            [media_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("queued state");
    assert_eq!(queued_state, ("queued".to_string(), 0, 0));
}

#[test]
fn metadata_claims_drain_the_entire_eligible_queue() {
    let pool = create_test_db();
    let media_ids = (0..65)
        .map(|index| create_test_media(&pool, &format!("queued-{index}.jpg")))
        .collect::<Vec<_>>();
    let connection = pool.get().expect("connection");
    for media_id in &media_ids {
        connection
            .execute(
                "INSERT INTO media_metadata_jobs (media_id, status) VALUES (?, 'queued')",
                [media_id],
            )
            .expect("job");
    }
    drop(connection);

    let mut claimed_ids = Vec::new();
    while let Some(media_id) =
        momento_api::processor::metadata_worker::claim_next_job(&pool).expect("claim")
    {
        claimed_ids.push(media_id);
    }

    assert_eq!(claimed_ids, media_ids);
}

#[test]
fn metadata_failures_back_off_then_become_terminal() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "retry.jpg");
    let connection = pool.get().expect("connection");
    connection.execute("INSERT INTO media_metadata_jobs (media_id, status, attempts) VALUES (?, 'processing', 1)", [media_id]).expect("job");
    drop(connection);
    momento_api::processor::metadata_worker::finish_job(
        &pool,
        media_id,
        Err("temporary failure".to_string()),
        2,
    )
    .expect("retry");
    let connection = pool.get().expect("connection");
    let first_status: String = connection
        .query_row(
            "SELECT status FROM media_metadata_jobs WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("status");
    assert_eq!(first_status, "queued");
    connection
        .execute(
            "UPDATE media_metadata_jobs SET status = 'processing', attempts = 2 WHERE media_id = ?",
            [media_id],
        )
        .expect("retry claim");
    drop(connection);
    momento_api::processor::metadata_worker::finish_job(
        &pool,
        media_id,
        Err("terminal failure".to_string()),
        2,
    )
    .expect("terminal");
    let connection = pool.get().expect("connection");
    let final_status: String = connection
        .query_row(
            "SELECT status FROM media_metadata_jobs WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("status");
    assert_eq!(final_status, "failed");
}
