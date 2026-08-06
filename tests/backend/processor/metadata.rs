use chrono::{TimeZone, Utc};
use momento_api::processor::metadata::{
    apply_supplemental_metadata, load_supplemental_metadata, supplemental_metadata_path,
    MediaMetadata,
};
use std::fs;

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
