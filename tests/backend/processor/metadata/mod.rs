use crate::test_utils::{
    create_test_db, create_test_media, test_executor_handles_with_data_directory, QOI_FIXTURE,
};
use chrono::{TimeZone, Utc};
use momento_api::config::Config;
use momento_api::executor::ParsedSupplementalMetadata;
use momento_api::io::file::{NormalizedStoragePath, StorageRootId};
use momento_api::processor::metadata::{
    apply_supplemental_metadata, load_supplemental_metadata_storage, MediaMetadata,
};
use std::fs;

mod reset;
mod reverse_geocoding;

async fn claim_metadata_job(
    pool: &momento_api::database::DbPool,
    executors: &momento_api::runtime::ExecutorHandles,
    media_id: i64,
) -> String {
    pool.get()
        .expect("database connection")
        .execute(
            "INSERT INTO media_metadata_jobs (media_id, status) VALUES (?, 'queued')",
            [media_id],
        )
        .expect("metadata job");
    let claim = executors
        .sqlite
        .claim_next_metadata_job_durable()
        .await
        .expect("metadata claim")
        .expect("claimed metadata job");
    assert_eq!(claim.media_id, media_id);
    claim.claim_token
}

#[tokio::test]
async fn qoi_original_is_preserved_for_every_photo_inference_task() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "inference-source.qoi");
    let (executors, data_directory) = test_executor_handles_with_data_directory(pool.clone());
    let relative_path = format!("inference-source-{media_id}.qoi");
    let original_path = data_directory.join("originals").join(&relative_path);
    fs::create_dir_all(original_path.parent().expect("original parent")).expect("original parent");
    fs::write(&original_path, QOI_FIXTURE).expect("QOI original");
    pool.get()
        .expect("database connection")
        .execute(
            "UPDATE media SET file_path = ?, mime_type = 'image/qoi', import_state = 'imported' WHERE id = ?",
            rusqlite::params![relative_path, media_id],
        )
        .expect("QOI media");
    let claim_token = claim_metadata_job(&pool, &executors, media_id).await;
    let config = Config::default();
    momento_api::processor::metadata::generate_media_metadata(
        &executors,
        media_id,
        &claim_token,
        &config,
    )
    .await
    .expect("QOI metadata generation");

    let connection = pool.get().expect("database connection");
    let inputs = connection
        .prepare("SELECT storage_root, file_path, mime_type FROM media_ai_inputs WHERE media_id = ? ORDER BY task")
        .expect("QOI input query")
        .query_map([media_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .expect("QOI input rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("QOI inputs");
    assert_eq!(inputs.len(), 7);
    for (storage_root, input_path, mime_type) in inputs {
        assert_eq!(storage_root, "originals");
        assert_eq!(input_path, relative_path);
        assert_eq!(mime_type, "image/qoi");
    }
    let (thumbnail_path, preview_path, artifact_version): (String, String, i64) = connection
        .query_row(
            "SELECT thumbnail_path, preview_path, artifact_version FROM media_metadata WHERE media_id = ?",
            [media_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("metadata artifact generation");
    assert_eq!(artifact_version, 1);
    assert!(thumbnail_path.contains(&format!("v1-{claim_token}")));
    assert!(preview_path.contains(&format!("v1-{claim_token}")));
    for (root, path) in [
        ("thumbnails", thumbnail_path.as_str()),
        ("thumbnails_tiny", thumbnail_path.as_str()),
        ("thumbnails_places", thumbnail_path.as_str()),
        ("previews", preview_path.as_str()),
    ] {
        assert!(
            data_directory.join(root).join(path).is_file(),
            "published metadata artifact {root}/{path}"
        );
    }
    let product_group: (String, Option<String>, i64) = connection
        .query_row(
            "SELECT state, product_target, entry_count FROM file_operation_groups WHERE kind = 'metadata_artifacts'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("metadata product group");
    assert_eq!(product_group, ("cleanup_pending".to_string(), None, 4));
    let reserved_artifact_bytes: i64 = connection
        .query_row(
            "SELECT r.reserved_peak_additional_bytes FROM data_dir_space_reservations AS r JOIN file_operation_groups AS g ON g.id = r.journal_group_id WHERE g.kind = 'metadata_artifacts'",
            [],
            |row| row.get(0),
        )
        .expect("metadata artifact reservation");
    let thumbnail_size = i64::from(config.metadata.thumbnails_max_size);
    let tiny_thumbnail_size = i64::from(config.metadata.thumbnails_tiny_size);
    let thumbnail_bound = thumbnail_size * thumbnail_size * 8 + 1_048_576;
    let tiny_thumbnail_bound = tiny_thumbnail_size * tiny_thumbnail_size * 8 + 1_048_576;
    let web_preview_bound = 2_048_i64 * 2_048 * 8 + 1_048_576;
    assert_eq!(
        reserved_artifact_bytes,
        thumbnail_bound * 2 + tiny_thumbnail_bound + web_preview_bound
    );
    assert!(reserved_artifact_bytes < 512 * 1024 * 1024);
    drop(connection);
    momento_api::io::recovery::recover_generic_file_operations(&executors)
        .await
        .expect("metadata product cleanup");
    let terminal_state: String = pool
        .get()
        .expect("database after cleanup")
        .query_row(
            "SELECT state FROM file_operation_groups WHERE kind = 'metadata_artifacts'",
            [],
            |row| row.get(0),
        )
        .expect("terminal metadata product");
    assert_eq!(terminal_state, "cleaned");
}

#[tokio::test]
async fn metadata_references_the_canonical_original_for_every_photo_ai_task() {
    let pool = create_test_db();
    let photo_id = create_test_media(&pool, "classifier-preparation.jpg");
    let (executors, data_directory) = test_executor_handles_with_data_directory(pool.clone());
    let relative_path = format!("classifier-preparation-{photo_id}.jpg");
    let original_path = data_directory.join("originals").join(&relative_path);
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
    let claim_token = claim_metadata_job(&pool, &executors, photo_id).await;
    momento_api::processor::metadata::generate_media_metadata(
        &executors,
        photo_id,
        &claim_token,
        &Config::default(),
    )
    .await
    .expect("metadata generation");

    let connection = pool.get().expect("database connection");
    let metadata_sources = connection
        .prepare("SELECT source_type, payload_json FROM media_metadata_sources WHERE media_id = ? ORDER BY source_type")
        .expect("metadata source query")
        .query_map([photo_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("metadata source rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("metadata sources");
    assert_eq!(metadata_sources.len(), 2);
    assert_eq!(metadata_sources[0].0, "exiftool");
    assert_eq!(metadata_sources[1].0, "supplemental_sidecar");
    assert!(metadata_sources[1].1.contains("retained"));
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
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "full-resolution-frame.mp4");
    let (executors, data_directory) = test_executor_handles_with_data_directory(pool.clone());
    let ai_directory = data_directory
        .join("previews")
        .join("ai")
        .join(media_id.to_string());
    if ai_directory.exists() {
        fs::remove_dir_all(&ai_directory).expect("remove stale AI fixture directory");
    }
    let relative_path = format!("full-resolution-frame-{media_id}.mp4");
    let original_path = data_directory.join("originals").join(&relative_path);
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
    let claim_token = claim_metadata_job(&pool, &executors, media_id).await;
    momento_api::processor::metadata::generate_media_metadata(
        &executors,
        media_id,
        &claim_token,
        &Config::default(),
    )
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

    momento_api::processor::metadata::generate_media_metadata(
        &executors,
        media_id,
        &claim_token,
        &Config::default(),
    )
    .await
    .expect("repeated video metadata generation");

    let connection = pool.get().expect("database connection");
    let ffprobe_source_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_metadata_sources WHERE media_id = ? AND source_type = 'ffprobe' AND json_valid(payload_json)",
            [media_id],
            |row| row.get(0),
        )
        .expect("ffprobe source count");
    assert_eq!(ffprobe_source_count, 1);
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
    let frame = image::open(data_directory.join("previews").join(expected_relative_path))
        .expect("video frame");
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
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "unsupported-original.jpg");
    let (executors, data_directory) = test_executor_handles_with_data_directory(pool.clone());
    let relative_path = format!("unsupported-original-{media_id}.jpg");
    let original_path = data_directory.join("originals").join(&relative_path);
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
    let claim_token = claim_metadata_job(&pool, &executors, media_id).await;
    let error = momento_api::processor::metadata::generate_media_metadata(
        &executors,
        media_id,
        &claim_token,
        &Config::default(),
    )
    .await
    .expect_err("unsupported original should fail");

    assert!(error.contains("supported image MIME type"), "{error}");
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
    let persisted_metadata: (Option<i32>, Option<String>) = pool
        .get()
        .expect("database connection")
        .query_row(
            "SELECT width, thumbnail_path FROM media_metadata WHERE media_id = ?",
            [media_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("persisted metadata");
    let source_count: i64 = pool
        .get()
        .expect("database connection")
        .query_row(
            "SELECT COUNT(*) FROM media_metadata_sources WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("metadata source count");
    assert_eq!(persisted_metadata, (Some(1920), None));
    assert_eq!(source_count, 0, "sources must not commit without AI inputs");
}

async fn load_supplemental_fixture(
    media_path: &str,
    sidecars: &[(&str, &str)],
) -> Option<ParsedSupplementalMetadata> {
    let pool = create_test_db();
    momento_api::database::init_database(&pool.get().expect("schema connection")).expect("schema");
    let (executors, data_directory) = test_executor_handles_with_data_directory(pool);
    for (path, contents) in sidecars {
        let absolute = data_directory.join("originals").join(path);
        if let Some(parent) = absolute.parent() {
            fs::create_dir_all(parent).expect("sidecar parent");
        }
        fs::write(absolute, contents).expect("sidecar fixture");
    }
    load_supplemental_metadata_storage(
        &executors,
        StorageRootId::Originals,
        &NormalizedStoragePath::parse(media_path).expect("media path"),
    )
    .await
    .expect("load supplemental metadata")
}

#[tokio::test]
async fn loads_google_photos_supplemental_metadata() {
    let data = load_supplemental_fixture(
        "IMG_2373.HEIC",
        &[(
            "IMG_2373.HEIC.supplemental-metadata.json",
            r#"{
            "photoTakenTime": {"timestamp": "1530569813"},
            "geoData": {"latitude": 40.759, "longitude": -73.9859, "altitude": 303.0}
        }"#,
        )],
    )
    .await
    .expect("Sidecar should load");
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

#[tokio::test]
async fn finds_numbered_supplemental_metadata_sidecar() {
    assert!(load_supplemental_fixture(
        "Snapseed(10).heic",
        &[("Snapseed.heic.supplemental-metadata(10).json", "{}")],
    )
    .await
    .is_some());
}

#[tokio::test]
async fn unnumbered_media_does_not_claim_a_numbered_sidecar() {
    assert!(load_supplemental_fixture(
        "Snapseed.heic",
        &[("Snapseed.heic.supplemental-metadata(10).json", "{}")],
    )
    .await
    .is_none());
}

#[tokio::test]
async fn numbered_and_unnumbered_media_load_their_own_sidecars() {
    let sidecars = [
        (
            "photo.jpg.supplemental-metadata.json",
            r#"{"description":"first"}"#,
        ),
        (
            "photo.jpg.supplemental-metadata(2).json",
            r#"{"description":"second"}"#,
        ),
    ];
    assert_eq!(
        load_supplemental_fixture("photo.jpg", &sidecars)
            .await
            .and_then(|value| value.description),
        Some("first".to_string())
    );
    assert_eq!(
        load_supplemental_fixture("photo(2).jpg", &sidecars)
            .await
            .and_then(|value| value.description),
        Some("second".to_string())
    );
}

#[tokio::test]
async fn finds_takeout_truncated_numbered_sidecar() {
    assert!(load_supplemental_fixture(
        "1234567890123456789012345678901234567890(2).jpg",
        &[(
            "1234567890123456789012345678901234567890.jpg.s(2).json",
            "{}",
        )],
    )
    .await
    .is_some());
}

#[tokio::test]
async fn does_not_find_sidecar_outside_media_directory() {
    assert!(load_supplemental_fixture(
        ".processing/IMG_2373.HEIC",
        &[("IMG_2373.HEIC.supplemental-metadata.json", "{}")],
    )
    .await
    .is_none());
}

#[test]
fn supplemental_metadata_overrides_present_embedded_values() {
    let data = supplemental_metadata(
        Utc.timestamp_opt(1530569813, 0).single(),
        Some(40.759),
        Some(-73.9859),
        Some(303.0),
        Some("updated keywords"),
    );
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
    let data = supplemental_metadata(None, Some(40.759), Some(-73.9859), None, None);
    let mut metadata = MediaMetadata::default();

    apply_supplemental_metadata(&mut metadata, &data);

    assert_eq!(metadata.gps_latitude, Some(40.759));
    assert_eq!(metadata.gps_longitude, Some(-73.9859));
}

#[test]
fn supplemental_metadata_replaces_zero_embedded_coordinates() {
    let data = supplemental_metadata(None, Some(40.759), Some(-73.9859), None, None);
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
    let data = supplemental_metadata(None, None, None, None, None);
    let mut metadata = MediaMetadata::default();

    apply_supplemental_metadata(&mut metadata, &data);

    assert_eq!(metadata.gps_latitude, None);
    assert_eq!(metadata.gps_longitude, None);
}

fn supplemental_metadata(
    date_taken: Option<chrono::DateTime<Utc>>,
    gps_latitude: Option<f64>,
    gps_longitude: Option<f64>,
    gps_altitude: Option<f64>,
    description: Option<&str>,
) -> ParsedSupplementalMetadata {
    ParsedSupplementalMetadata {
        payload_json: "{}".to_string(),
        date_taken,
        gps_latitude,
        gps_longitude,
        gps_altitude,
        description: description.map(str::to_string),
    }
}

#[tokio::test]
async fn metadata_claims_are_exclusive_without_live_time_based_reclaim() {
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
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    assert_eq!(
        executors
            .sqlite
            .claim_next_metadata_job_durable()
            .await
            .expect("first claim")
            .map(|claim| claim.media_id),
        Some(media_id)
    );
    assert_eq!(
        executors
            .sqlite
            .claim_next_metadata_job_durable()
            .await
            .expect("second claim"),
        None
    );
    assert_eq!(
        executors
            .sqlite
            .claim_next_metadata_job_durable()
            .await
            .expect("still claimed"),
        None
    );
}

#[tokio::test]
async fn stale_metadata_claim_cannot_finish_a_new_owner() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "stale-claim.jpg");
    pool.get()
        .expect("connection")
        .execute(
            "INSERT INTO media_metadata_jobs (media_id, status) VALUES (?, 'queued')",
            [media_id],
        )
        .expect("job");
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    let first_claim = executors
        .sqlite
        .claim_next_metadata_job_durable()
        .await
        .expect("first claim")
        .expect("claimed job");
    assert_eq!(
        executors
            .sqlite
            .recover_metadata_claims_durable()
            .await
            .expect("recover claim"),
        1
    );
    let second_claim = executors
        .sqlite
        .claim_next_metadata_job_durable()
        .await
        .expect("second claim")
        .expect("reclaimed job");

    let stale = executors
        .sqlite
        .finish_metadata_job_durable(momento_api::database::operations::FinishMetadataJob {
            media_id,
            claim_token: first_claim.claim_token,
            error: None,
        })
        .await;

    assert!(stale.is_err());
    let active_token: String = pool
        .get()
        .expect("connection")
        .query_row(
            "SELECT claim_token FROM media_metadata_jobs WHERE media_id = ? AND status = 'processing'",
            [media_id],
            |row| row.get(0),
        )
        .expect("active token");
    assert_eq!(active_token, second_claim.claim_token);
}

#[tokio::test]
async fn metadata_rerun_requested_during_processing_runs_after_current_attempt() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "rerun-request.jpg");
    let connection = pool.get().expect("connection");
    connection
        .execute(
            "INSERT INTO media_metadata_jobs (media_id, status) VALUES (?, 'queued')",
            [media_id],
        )
        .expect("queued job");
    drop(connection);
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    let claim = executors
        .sqlite
        .claim_next_metadata_job_durable()
        .await
        .expect("claim")
        .expect("claimed job");
    let connection = pool.get().expect("connection");
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

    executors
        .sqlite
        .finish_metadata_job_durable(momento_api::database::operations::FinishMetadataJob {
            media_id,
            claim_token: claim.claim_token,
            error: None,
        })
        .await
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

#[tokio::test]
async fn metadata_claims_drain_the_entire_eligible_queue() {
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
    let executors = crate::test_utils::test_executor_handles(pool.clone());

    let mut claimed_ids = Vec::new();
    while let Some(media_id) = executors
        .sqlite
        .claim_next_metadata_job_durable()
        .await
        .expect("claim")
    {
        claimed_ids.push(media_id.media_id);
    }

    assert_eq!(claimed_ids, media_ids);
}

#[tokio::test]
async fn transient_metadata_failures_retry_without_attempt_limit() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "retry.jpg");
    let connection = pool.get().expect("connection");
    connection
        .execute(
            "INSERT INTO media_metadata_jobs (media_id, status) VALUES (?, 'queued')",
            [media_id],
        )
        .expect("job");
    drop(connection);
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    let first_claim = executors
        .sqlite
        .claim_next_metadata_job_durable()
        .await
        .expect("claim")
        .expect("claimed job");
    executors
        .sqlite
        .finish_metadata_job_durable(momento_api::database::operations::FinishMetadataJob {
            media_id,
            claim_token: first_claim.claim_token,
            error: Some("temporary failure".to_string()),
        })
        .await
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
    let retry_delay = executors
        .sqlite
        .load_next_metadata_job_delay_durable()
        .await
        .expect("metadata retry deadline")
        .expect("future metadata retry");
    assert!(retry_delay <= std::time::Duration::from_secs(30));
    assert!(retry_delay >= std::time::Duration::from_secs(1));
    connection
        .execute(
            "UPDATE media_metadata_jobs SET available_at = datetime('now') WHERE media_id = ?",
            [media_id],
        )
        .expect("make retry available");
    drop(connection);
    let second_claim = executors
        .sqlite
        .claim_next_metadata_job_durable()
        .await
        .expect("claim")
        .expect("claimed retry");
    executors
        .sqlite
        .finish_metadata_job_durable(momento_api::database::operations::FinishMetadataJob {
            media_id,
            claim_token: second_claim.claim_token,
            error: Some("another temporary failure".to_string()),
        })
        .await
        .expect("terminal");
    let connection = pool.get().expect("connection");
    let final_status: String = connection
        .query_row(
            "SELECT status FROM media_metadata_jobs WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("status");
    assert_eq!(final_status, "queued");
}
