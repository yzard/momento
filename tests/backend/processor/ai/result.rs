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

fn completed_text_result(job_id: &str, media_id: i64, task: &str, text: &str) -> JobResult {
    let mut result = completed_result(job_id, media_id, 1);
    result.task = task.to_string();
    result.model_type = Some(task.to_string());
    result.result = Some(serde_json::json!({ "text": text }));
    result
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

fn insert_job_input(pool: &DbPool, job_id: &str, sequence: u32, frame_timestamp_ms: Option<i64>) {
    pool.get()
        .expect("connection")
        .execute(
            "INSERT INTO llm_job_inputs (job_id, sequence, input_kind, file_path, filename, mime_type, byte_size, content_hash, frame_timestamp_ms) VALUES (?, ?, 'image', 'ai/input.jpg', 'input.jpg', 'image/jpeg', 1, 'hash', ?)",
            rusqlite::params![job_id, sequence, frame_timestamp_ms],
        )
        .expect("job input");
}

fn classification_result(
    job_id: &str,
    media_id: i64,
    task: &str,
    detected: bool,
    confidence: f64,
) -> JobResult {
    let classification = serde_json::json!({
        "detected": detected,
        "confidence": confidence
    });
    JobResult {
        job_id: job_id.to_string(),
        media_id,
        task: task.to_string(),
        attempt: 1,
        status: "completed".to_string(),
        model_type: Some(task.to_string()),
        model_version: Some("classifier-v1".to_string()),
        result: Some(classification.clone()),
        input_results: Some(vec![JobInputResult {
            sequence: 0,
            frame_timestamp_ms: None,
            result: classification,
        }]),
        error: None,
    }
}

#[test]
fn classifier_results_persist_overlapping_aggregate_and_input_rows() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "classifier-result.jpg");
    for (job_id, task) in [
        ("result-screenshot", "screenshot_detection"),
        ("result-document", "document_detection"),
    ] {
        insert_submitted_job(&pool, job_id, media_id, task, 1);
        insert_job_input(&pool, job_id, 0, None);
        process_result(
            &pool,
            classification_result(job_id, media_id, task, true, 0.91),
        )
        .expect("classification result");
    }

    let connection = pool.get().expect("database connection");
    let screenshot: (bool, f64) = connection
        .query_row(
            "SELECT is_screenshot, confidence FROM media_screenshot_classifications WHERE media_id = ?",
            [media_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("screenshot aggregate");
    let document: (bool, f64) = connection
        .query_row(
            "SELECT is_document, confidence FROM media_document_classifications WHERE media_id = ?",
            [media_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("document aggregate");
    let input_count: i64 = connection
        .query_row(
            "SELECT (SELECT COUNT(*) FROM media_screenshot_classification_inputs) + (SELECT COUNT(*) FROM media_document_classification_inputs)",
            [],
            |row| row.get(0),
        )
        .expect("classification input count");
    assert_eq!(screenshot, (true, 0.91));
    assert_eq!(document, (true, 0.91));
    assert_eq!(input_count, 2);
}

#[test]
fn classifier_results_reject_invalid_payloads_and_correlation() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "invalid-classifier-result.jpg");
    insert_submitted_job(
        &pool,
        "invalid-classifier",
        media_id,
        "screenshot_detection",
        1,
    );
    insert_job_input(&pool, "invalid-classifier", 0, None);

    let invalid_confidence = classification_result(
        "invalid-classifier",
        media_id,
        "screenshot_detection",
        true,
        1.1,
    );
    assert!(matches!(
        process_result(&pool, invalid_confidence),
        Err(AppError::BadRequest(_))
    ));

    let mut mismatched_model = classification_result(
        "invalid-classifier",
        media_id,
        "screenshot_detection",
        true,
        0.8,
    );
    mismatched_model.model_type = Some("document_detection".to_string());
    assert!(matches!(
        process_result(&pool, mismatched_model),
        Err(AppError::BadRequest(_))
    ));

    let mut mismatched_input = classification_result(
        "invalid-classifier",
        media_id,
        "screenshot_detection",
        true,
        0.8,
    );
    mismatched_input
        .input_results
        .as_mut()
        .expect("input results")[0]
        .sequence = 1;
    assert!(matches!(
        process_result(&pool, mismatched_input),
        Err(AppError::BadRequest(_))
    ));

    let mut mismatched_aggregate = classification_result(
        "invalid-classifier",
        media_id,
        "screenshot_detection",
        true,
        0.8,
    );
    mismatched_aggregate.result = Some(serde_json::json!({
        "detected": false,
        "confidence": 0.8
    }));
    assert!(matches!(
        process_result(&pool, mismatched_aggregate),
        Err(AppError::BadRequest(_))
    ));
    let persisted: i64 = pool
        .get()
        .expect("database connection")
        .query_row(
            "SELECT COUNT(*) FROM media_screenshot_classifications WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("classification count");
    assert_eq!(persisted, 0);
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
fn different_task_results_complete_independently_in_arrival_order() {
    let pool = create_test_db();
    let ocr_media_id = create_test_media(&pool, "result-ocr-independent.jpg");
    let tagging_media_id = create_test_media(&pool, "result-tagging-independent.jpg");
    insert_submitted_job(&pool, "result-ocr", ocr_media_id, "ocr", 1);
    insert_submitted_job(
        &pool,
        "result-tagging",
        tagging_media_id,
        "image_tagging",
        1,
    );

    process_result(
        &pool,
        completed_text_result(
            "result-tagging",
            tagging_media_id,
            "image_tagging",
            "mountain, lake",
        ),
    )
    .expect("tagging result may arrive before OCR");

    let connection = pool.get().expect("database connection");
    let states: (String, String) = connection
        .query_row(
            "SELECT (SELECT status FROM llm_jobs WHERE id = 'result-ocr'), (SELECT status FROM llm_jobs WHERE id = 'result-tagging')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("independent job states");
    assert_eq!(states, ("submitted".to_string(), "completed".to_string()));
    drop(connection);

    process_result(
        &pool,
        completed_text_result("result-ocr", ocr_media_id, "ocr", "receipt text"),
    )
    .expect("OCR result completes later");

    let connection = pool.get().expect("database connection");
    let completed_jobs: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM llm_jobs WHERE id IN ('result-ocr', 'result-tagging') AND status = 'completed'",
            [],
            |row| row.get(0),
        )
        .expect("completed job count");
    let persisted_results: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_text WHERE media_id IN (?, ?)",
            rusqlite::params![ocr_media_id, tagging_media_id],
            |row| row.get(0),
        )
        .expect("persisted result count");
    assert_eq!(completed_jobs, 2);
    assert_eq!(persisted_results, 2);
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
fn aesthetics_result_persists_aggregate_and_input_scores() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "result-aesthetics.jpg");
    insert_submitted_job(&pool, "result-aesthetics", media_id, "image_aesthetics", 1);
    insert_job_input(&pool, "result-aesthetics", 0, None);
    let scores = serde_json::json!({
        "aestheticScore": 0.81,
        "scenicScore": 0.72,
        "simplicityScore": 0.63,
        "landscapeScore": 0.54,
        "technicalQualityScore": 0.45
    });
    let result = JobResult {
        job_id: "result-aesthetics".to_string(),
        media_id,
        task: "image_aesthetics".to_string(),
        attempt: 1,
        status: "completed".to_string(),
        model_type: Some("image_aesthetics".to_string()),
        model_version: Some("clip-vit-b-32-laion-v1".to_string()),
        result: Some(scores.clone()),
        input_results: Some(vec![JobInputResult {
            sequence: 0,
            frame_timestamp_ms: None,
            result: scores,
        }]),
        error: None,
    };

    process_result(&pool, result).expect("aesthetics result");

    let connection = pool.get().expect("connection");
    let aggregate: (f64, f64) = connection
        .query_row(
            "SELECT aesthetic_score, scenic_score FROM media_aesthetics WHERE media_id = ?",
            [media_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("aggregate aesthetics");
    let input_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_aesthetic_inputs WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("input aesthetics");
    assert_eq!(aggregate, (0.81, 0.72));
    assert_eq!(input_count, 1);
}

#[test]
fn aesthetics_result_rejects_missing_or_out_of_range_scores() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "result-invalid-aesthetics.jpg");
    insert_submitted_job(
        &pool,
        "result-invalid-aesthetics",
        media_id,
        "image_aesthetics",
        1,
    );
    let scores = serde_json::json!({
        "aestheticScore": 1.1,
        "scenicScore": 0.7,
        "simplicityScore": 0.6,
        "landscapeScore": 0.5,
        "technicalQualityScore": 0.4
    });
    let result = JobResult {
        job_id: "result-invalid-aesthetics".to_string(),
        media_id,
        task: "image_aesthetics".to_string(),
        attempt: 1,
        status: "completed".to_string(),
        model_type: Some("image_aesthetics".to_string()),
        model_version: Some("test".to_string()),
        result: Some(scores.clone()),
        input_results: Some(vec![JobInputResult {
            sequence: 0,
            frame_timestamp_ms: None,
            result: scores,
        }]),
        error: None,
    };

    let error = process_result(&pool, result).expect_err("invalid score must fail");
    assert!(matches!(error, AppError::BadRequest(_)));
}

#[test]
fn aesthetics_result_must_match_submitted_inputs_and_first_input_aggregate() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "result-correlated-aesthetics.jpg");
    insert_submitted_job(
        &pool,
        "result-correlated-aesthetics",
        media_id,
        "image_aesthetics",
        1,
    );
    insert_job_input(&pool, "result-correlated-aesthetics", 0, Some(100));
    let aggregate = serde_json::json!({
        "aestheticScore": 0.8,
        "scenicScore": 0.8,
        "simplicityScore": 0.8,
        "landscapeScore": 0.8,
        "technicalQualityScore": 0.8
    });
    let input = serde_json::json!({
        "aestheticScore": 0.7,
        "scenicScore": 0.7,
        "simplicityScore": 0.7,
        "landscapeScore": 0.7,
        "technicalQualityScore": 0.7
    });
    let result = JobResult {
        job_id: "result-correlated-aesthetics".to_string(),
        media_id,
        task: "image_aesthetics".to_string(),
        attempt: 1,
        status: "completed".to_string(),
        model_type: Some("image_aesthetics".to_string()),
        model_version: Some("test".to_string()),
        result: Some(aggregate),
        input_results: Some(vec![JobInputResult {
            sequence: 0,
            frame_timestamp_ms: Some(100),
            result: input,
        }]),
        error: None,
    };

    let mut wrong_correlation = result.clone();
    wrong_correlation.result = wrong_correlation
        .input_results
        .as_ref()
        .and_then(|results| results.first())
        .map(|input| input.result.clone());
    wrong_correlation
        .input_results
        .as_mut()
        .expect("input results")[0]
        .sequence = 1;
    let error =
        process_result(&pool, wrong_correlation).expect_err("input correlation mismatch must fail");
    assert!(matches!(error, AppError::BadRequest(_)));

    let error = process_result(&pool, result).expect_err("aggregate mismatch must fail");
    assert!(matches!(error, AppError::BadRequest(_)));
    let persisted: i64 = pool
        .get()
        .expect("connection")
        .query_row(
            "SELECT COUNT(*) FROM media_aesthetics WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("aesthetics count");
    assert_eq!(persisted, 0);
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
