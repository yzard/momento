use crate::test_utils::{create_test_db, create_test_media};
use chrono::{TimeZone, Utc};
use momento_api::processor::metadata::{
    apply_supplemental_metadata, delete_supplemental_metadata, load_supplemental_metadata,
    supplemental_metadata_path, MediaMetadata,
};
use std::fs;

mod reset;

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
fn supplemental_metadata_does_not_replace_embedded_values() {
    let data = serde_json::json!({
        "photoTakenTime": {"timestamp": "1530569813"},
        "geoData": {"latitude": 40.759, "longitude": -73.9859}
    });
    let embedded_date = Utc.timestamp_opt(1, 0).single();
    let mut metadata = MediaMetadata {
        date_taken: embedded_date,
        gps_latitude: Some(1.0),
        ..MediaMetadata::default()
    };

    apply_supplemental_metadata(&mut metadata, &data);

    assert_eq!(metadata.date_taken, embedded_date);
    assert_eq!(metadata.gps_latitude, Some(1.0));
    assert_eq!(metadata.gps_longitude, Some(-73.9859));
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
fn deletes_consumed_supplemental_metadata() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let media_path = directory.path().join("camera.jpg");
    let sidecar_path = directory
        .path()
        .join("camera.jpg.supplemental-metadata.json");
    fs::write(&media_path, "image").expect("media");
    fs::write(&sidecar_path, "{}\n").expect("sidecar");
    delete_supplemental_metadata(&media_path).expect("delete sidecar");
    assert!(!sidecar_path.exists());
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
        momento_api::processor::metadata_worker::claim_jobs(&pool, 1).expect("first claim"),
        vec![media_id]
    );
    assert!(
        momento_api::processor::metadata_worker::claim_jobs(&pool, 1)
            .expect("second claim")
            .is_empty()
    );
    let connection = pool.get().expect("connection");
    connection.execute("UPDATE media_metadata_jobs SET claimed_at = datetime('now', '-10 minutes') WHERE media_id = ?", [media_id]).expect("expire lease");
    drop(connection);
    momento_api::processor::metadata_worker::reclaim_expired_leases(&pool, 30).expect("reclaim");
    assert_eq!(
        momento_api::processor::metadata_worker::claim_jobs(&pool, 1).expect("reclaimed claim"),
        vec![media_id]
    );
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
