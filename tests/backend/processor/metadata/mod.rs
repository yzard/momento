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
async fn metadata_prepares_aspect_preserving_photo_only_classifier_inputs() {
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
    let expected_content_hash: String = pool
        .get()
        .expect("database connection")
        .query_row(
            "SELECT content_hash FROM media WHERE id = ?",
            [photo_id],
            |row| row.get(0),
        )
        .expect("stored content hash");

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
    assert_eq!(regenerated_content_hash, expected_content_hash);
    let classifier_inputs = connection
        .prepare("SELECT task, file_path FROM media_ai_inputs WHERE media_id = ? AND task IN ('screenshot_detection', 'document_detection') ORDER BY task")
        .expect("classifier input query")
        .query_map([photo_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("classifier input rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("classifier inputs");
    assert_eq!(classifier_inputs.len(), 2);
    for (task, input_path) in classifier_inputs {
        let prepared = image::open(paths().previews.join(input_path)).expect("prepared classifier");
        assert_eq!(prepared.width(), 2048, "{task} width");
        assert!((682..=683).contains(&prepared.height()), "{task} height");
    }
    assert!(supplemental_path.is_file());
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
