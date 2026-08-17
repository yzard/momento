use std::time::Duration;

use base64::Engine;
use momento_api::constants::paths;
use momento_api::database::{create_pool_at, init_database, DbPool};
use momento_api::error::AppError;
use momento_api::processor::ai::result::process_result;
use momento_common::llm::{JobInputResult, JobResult};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use crate::test_utils::{create_test_db, create_test_media, init_test_paths};

fn completed_result(job_id: &str, media_id: i64, attempt: u32) -> JobResult {
    JobResult {
        job_id: job_id.to_string(),
        media_id,
        task: "ocr".to_string(),
        attempt,
        status: "completed".to_string(),
        model_type: Some("ocr".to_string()),
        model_version: Some("test".to_string()),
        result: Some(serde_json::json!({ "text": "recognized" })),
        input_results: None,
        error: None,
    }
}

fn insert_submitted_job(pool: &DbPool, job_id: &str, media_id: i64, task: &str, attempt: u32) {
    pool.get()
        .expect("connection")
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES (?, ?, ?, 'submitted', ?)",
            rusqlite::params![job_id, media_id, task, attempt],
        )
        .expect("submitted job");
}

#[test]
fn result_rejects_stale_attempts_and_non_terminal_statuses() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "result-validation.jpg");
    insert_submitted_job(&pool, "stale-result", media_id, "ocr", 2);

    let error = process_result(&pool, completed_result("stale-result", media_id, 1))
        .expect_err("stale attempt must fail");
    assert!(matches!(error, AppError::Conflict(_)));

    let mut invalid = completed_result("stale-result", media_id, 2);
    invalid.status = "running".to_string();
    let error = process_result(&pool, invalid).expect_err("non-terminal status must fail");
    assert!(matches!(error, AppError::BadRequest(_)));
}

#[test]
fn terminal_result_is_idempotent() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "result-idempotent.jpg");
    insert_submitted_job(&pool, "result-idempotent", media_id, "ocr", 1);
    let result = completed_result("result-idempotent", media_id, 1);

    process_result(&pool, result.clone()).expect("first result");
    process_result(&pool, result).expect("duplicate result");

    let text_count: i64 = pool
        .get()
        .expect("connection")
        .query_row(
            "SELECT COUNT(*) FROM media_text WHERE media_id = ? AND model_type = 'ocr'",
            [media_id],
            |row| row.get(0),
        )
        .expect("text count");
    assert_eq!(text_count, 1);
}

#[test]
fn result_persists_every_video_frame() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "result-video.mp4");
    insert_submitted_job(&pool, "result-frames", media_id, "ocr", 1);
    let mut result = completed_result("result-frames", media_id, 1);
    result.result = Some(serde_json::json!({ "text": "first" }));
    result.input_results = Some(vec![
        JobInputResult {
            sequence: 0,
            frame_timestamp_ms: Some(0),
            result: serde_json::json!({ "text": "first" }),
        },
        JobInputResult {
            sequence: 1,
            frame_timestamp_ms: Some(1000),
            result: serde_json::json!({ "text": "second" }),
        },
    ]);

    process_result(&pool, result).expect("video frame result");

    let connection = pool.get().expect("connection");
    let frame_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_text_inputs WHERE media_id = ? AND model_type = 'ocr'",
            [media_id],
            |row| row.get(0),
        )
        .expect("frame count");
    let aggregate_text: String = connection
        .query_row(
            "SELECT string FROM media_text WHERE media_id = ? AND model_type = 'ocr'",
            [media_id],
            |row| row.get(0),
        )
        .expect("aggregate text");
    assert_eq!(frame_count, 2);
    assert_eq!(aggregate_text, "first\nsecond");
}

#[test]
fn face_result_persists_crop_and_success_marker() {
    init_test_paths();
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "result-face.jpg");
    let input_relative = format!("ai/{media_id}/face_detection/input.jpg");
    let input_path = paths().previews.join(&input_relative);
    std::fs::create_dir_all(input_path.parent().expect("input parent")).expect("input parent");
    image::RgbImage::from_pixel(20, 20, image::Rgb([120, 80, 40]))
        .save(&input_path)
        .expect("prepared input");
    let input_bytes = std::fs::read(&input_path).expect("input bytes");
    let input_hash = format!("{:x}", Sha256::digest(&input_bytes));
    let connection = pool.get().expect("connection");
    connection
        .execute(
            "INSERT INTO face_grouping_runs (id, status) VALUES (1, 'running')",
            [],
        )
        .expect("run");
    connection.execute("INSERT INTO llm_jobs (id, media_id, face_grouping_run_id, task, status, attempts) VALUES ('result-face', ?, 1, 'face_detection', 'submitted', 1)", [media_id]).expect("job");
    connection.execute("INSERT INTO llm_job_inputs (job_id, sequence, input_kind, file_path, filename, mime_type, byte_size, content_hash) VALUES ('result-face', 0, 'image', ?, 'input.jpg', 'image/jpeg', ?, ?)", rusqlite::params![input_relative, input_bytes.len() as i64, input_hash]).expect("job input");
    drop(connection);
    let embedding = base64::engine::general_purpose::STANDARD.encode(
        (0..512)
            .flat_map(|index| (if index == 0 { 1.0_f32 } else { 0.0_f32 }).to_le_bytes())
            .collect::<Vec<_>>(),
    );
    let input_result = serde_json::json!({
        "task": "face_detection",
        "modelType": "face_detection",
        "modelVersion": "buffalo_l",
        "faces": [{
            "index": 0,
            "boundingBox": {"x": 0.9, "y": 0.8, "width": 0.10000003, "height": 0.20000003},
            "eyeCenter": {"x": 0.95, "y": 0.86},
            "confidence": 0.95,
            "qualityScore": 0.8,
            "frontalityScore": 0.9,
            "embedding": embedding,
            "embeddingEncoding": "float32_le",
            "embeddingDimensions": 512
        }]
    });
    let result = JobResult {
        job_id: "result-face".to_string(),
        media_id,
        task: "face_detection".to_string(),
        attempt: 1,
        status: "completed".to_string(),
        model_type: Some("face_detection".to_string()),
        model_version: Some("buffalo_l".to_string()),
        result: Some(input_result.clone()),
        input_results: Some(vec![JobInputResult {
            sequence: 0,
            frame_timestamp_ms: None,
            result: input_result,
        }]),
        error: None,
    };

    process_result(&pool, result).expect("face result");

    let connection = pool.get().expect("connection");
    let (crop_path, box_x, box_width, frontality): (String, f64, f64, f64) = connection
        .query_row(
            "SELECT crop_path, x, width, frontality FROM media_faces WHERE media_id = ?",
            [media_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("face row");
    let result_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_face_detection_results WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("result marker");
    assert_eq!(result_count, 1);
    assert!(box_x + box_width <= 1.0);
    assert_eq!(frontality, 0.9);
    let crop = image::open(paths().previews.join(crop_path)).expect("face crop image");
    assert_eq!((crop.width(), crop.height()), (256, 256));
}

#[test]
fn clustering_result_persists_integer_capture_timestamp() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "result-clustering.jpg");
    let connection = pool.get().expect("connection");
    let run_id = connection
        .query_row(
            "INSERT INTO media_similarity_runs (trigger, status) VALUES ('manual', 'running') RETURNING id",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("similarity run");
    connection.execute("INSERT INTO llm_jobs (id, media_id, deduplicate_run_id, task, status, attempts) VALUES ('result-clustering', ?, ?, 'image_clustering', 'submitted', 1)", [media_id, run_id]).expect("job");
    drop(connection);
    let embedding = base64::engine::general_purpose::STANDARD.encode(vec![0_u8; 384 * 4]);
    let result = JobResult {
        job_id: "result-clustering".to_string(),
        media_id,
        task: "image_clustering".to_string(),
        attempt: 1,
        status: "completed".to_string(),
        model_type: Some("image_clustering".to_string()),
        model_version: Some("dinov2-small".to_string()),
        result: Some(serde_json::json!({
            "embedding": embedding,
            "embeddingEncoding": "float32_le",
            "embeddingDimensions": 384,
            "perceptualHash": "0123456789abcdef"
        })),
        input_results: None,
        error: None,
    };

    process_result(&pool, result).expect("clustering result");

    let capture_time_seconds: i64 = pool
        .get()
        .expect("connection")
        .query_row(
            "SELECT capture_time_seconds FROM media_similarity_index WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("capture timestamp");
    assert_eq!(capture_time_seconds, 1_705_314_600);
}

#[test]
fn result_returns_internal_database_detail() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "result-database-error.jpg");
    insert_submitted_job(&pool, "result-database-error", media_id, "ocr", 1);
    pool.get()
        .expect("connection")
        .execute("DROP TABLE media_text", [])
        .expect("drop text table");

    let error = process_result(
        &pool,
        completed_result("result-database-error", media_id, 1),
    )
    .expect_err("missing result table must fail");
    assert!(error.to_string().contains("no such table: media_text"));
}

#[test]
fn result_waits_for_concurrent_writer() {
    init_test_paths();
    let directory = TempDir::new().expect("database directory");
    let database_path = directory.path().join("database.sqlite");
    let pool = create_pool_at(&database_path).expect("database pool");
    let connection = pool.get().expect("connection");
    init_database(&connection).expect("database schema");
    drop(connection);
    let media_id = create_test_media(&pool, "result-contention.jpg");
    insert_submitted_job(&pool, "result-contention", media_id, "ocr", 1);
    let writer = rusqlite::Connection::open(&database_path).expect("writer connection");
    writer
        .execute_batch("BEGIN IMMEDIATE")
        .expect("writer transaction");
    let result_pool = pool.clone();
    let result = std::thread::spawn(move || {
        process_result(
            &result_pool,
            completed_result("result-contention", media_id, 1),
        )
    });
    std::thread::sleep(Duration::from_millis(150));
    writer.execute_batch("ROLLBACK").expect("writer rollback");

    result
        .join()
        .expect("result thread")
        .expect("result persistence");
}

#[test]
fn cancelled_job_accepts_late_result_without_persisting() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "result-cancelled.jpg");
    pool.get()
        .expect("connection")
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES ('result-cancelled', ?, 'ocr', 'cancelled', 1)",
            [media_id],
        )
        .expect("cancelled job");

    process_result(&pool, completed_result("result-cancelled", media_id, 1)).expect("late result");

    let text_count: i64 = pool
        .get()
        .expect("connection")
        .query_row(
            "SELECT COUNT(*) FROM media_text WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("text count");
    assert_eq!(text_count, 0);
}
