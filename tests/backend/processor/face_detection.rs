use base64::Engine;
use momento_api::database::queries;
use momento_api::models::LlmInputResult;
use momento_api::processor::face_detection;

use crate::test_utils::{create_test_db, create_test_media, init_test_paths};

fn embedding() -> String {
    let values = (0..512)
        .flat_map(|index| (if index == 0 { 1.0_f32 } else { 0.0_f32 }).to_le_bytes())
        .collect::<Vec<_>>();
    base64::engine::general_purpose::STANDARD.encode(values)
}

#[test]
fn portrait_crop_includes_head_and_shoulders_within_image_bounds() {
    let (crop_x, crop_y, crop_width, crop_height) =
        face_detection::portrait_crop_box(1000, 1000, 0.4, 0.2, 0.2, 0.2);

    assert!(crop_x <= 400);
    assert!(crop_y < 200);
    assert!(crop_x + crop_width >= 600);
    assert!(crop_y + crop_height > 500);
    assert_eq!(crop_width, crop_height);

    let (edge_x, edge_y, edge_width, edge_height) =
        face_detection::portrait_crop_box(1000, 600, 0.85, 0.7, 0.15, 0.25);
    assert!(edge_x + edge_width <= 1000);
    assert!(edge_y + edge_height <= 600);
}

#[test]
fn face_callback_rejects_invalid_embedding_before_persistence() {
    init_test_paths();
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "face.jpg");
    let connection = pool.get().expect("connection");
    connection.execute("INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, 'face_detection', 0, 'image', 'missing.jpg', 'missing.jpg', 'image/jpeg', 1, 'hash')", [media_id]).expect("input");
    connection
        .execute(
            "INSERT INTO face_grouping_runs (id, status) VALUES (1, 'running')",
            [],
        )
        .expect("run");
    connection.execute("INSERT INTO llm_jobs (id, media_id, face_grouping_run_id, task, status) VALUES ('face-job', ?, 1, 'face_detection', 'submitted')", [media_id]).expect("job");
    connection.execute("INSERT INTO llm_job_inputs (job_id, sequence, input_kind, file_path, filename, mime_type, byte_size, content_hash) VALUES ('face-job', 0, 'image', 'missing.jpg', 'missing.jpg', 'image/jpeg', 1, 'hash')", []).expect("job input");
    let transaction = connection.unchecked_transaction().expect("transaction");
    let results = vec![LlmInputResult {
        sequence: 0,
        frame_timestamp_ms: None,
        result: serde_json::json!({ "task": "face_detection", "modelType": "face_detection", "modelVersion": "buffalo_l", "faces": [{ "index": 0, "boundingBox": { "x": 0.0, "y": 0.0, "width": 1.0, "height": 1.0 }, "confidence": 1.0, "qualityScore": 1.0, "embedding": "bad", "embeddingEncoding": "float32_le", "embeddingDimensions": 512 }] }),
    }];
    assert!(face_detection::persist_callback(
        &transaction,
        "face-job",
        media_id,
        "face_detection",
        "buffalo_l",
        Some(&results)
    )
    .is_err());
    transaction.rollback().expect("rollback");
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM media_faces", [], |row| row.get(0))
        .expect("count");
    assert_eq!(count, 0);
}

#[test]
fn face_callback_records_success_when_no_faces_are_detected() {
    init_test_paths();
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "no-face.jpg");
    let connection = pool.get().expect("connection");
    connection
        .execute(
            "INSERT INTO face_grouping_runs (id, status) VALUES (1, 'running')",
            [],
        )
        .expect("run");
    connection.execute("INSERT INTO llm_jobs (id, media_id, face_grouping_run_id, task, status) VALUES ('no-face-job', ?, 1, 'face_detection', 'submitted')", [media_id]).expect("job");
    connection.execute("INSERT INTO llm_job_inputs (job_id, sequence, input_kind, file_path, filename, mime_type, byte_size, content_hash) VALUES ('no-face-job', 0, 'image', 'missing.jpg', 'missing.jpg', 'image/jpeg', 1, 'hash')", []).expect("job input");
    let transaction = connection.unchecked_transaction().expect("transaction");
    let results = vec![LlmInputResult {
        sequence: 0,
        frame_timestamp_ms: None,
        result: serde_json::json!({
            "task": "face_detection",
            "modelType": "face_detection",
            "modelVersion": "buffalo_l",
            "faces": []
        }),
    }];
    let changes = face_detection::persist_callback(
        &transaction,
        "no-face-job",
        media_id,
        "face_detection",
        "buffalo_l",
        Some(&results),
    )
    .expect("empty face callback");
    transaction.commit().expect("commit");
    changes.commit();

    let result_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_face_detection_results WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("result count");
    let face_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_faces WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("face count");
    assert_eq!(result_count, 1);
    assert_eq!(face_count, 0);
}

#[test]
fn face_grouping_creates_deterministic_group_for_matching_embeddings() {
    init_test_paths();
    let pool = create_test_db();
    let first_media_id = create_test_media(&pool, "first.jpg");
    let second_media_id = create_test_media(&pool, "second.jpg");
    let connection = pool.get().expect("connection");
    let embedding = base64::engine::general_purpose::STANDARD
        .decode(embedding())
        .expect("embedding");
    for media_id in [first_media_id, second_media_id] {
        connection
            .execute(
                queries::faces::INSERT_FACE,
                rusqlite::params![
                    media_id,
                    0_i64,
                    0_i64,
                    0.0_f64,
                    0.0_f64,
                    1.0_f64,
                    1.0_f64,
                    1.0_f64,
                    1.0_f64,
                    embedding.clone(),
                    "faces/test.jpg"
                ],
            )
            .expect("face");
    }
    connection
        .execute(queries::faces::INSERT_GROUPING_RUN, [])
        .expect("run");
    drop(connection);
    face_detection::finalize_ready_runs(&pool).expect("finalize");
    let connection = pool.get().expect("connection");
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM face_group_members", [], |row| {
            row.get(0)
        })
        .expect("members");
    assert_eq!(count, 2);
}

#[test]
fn face_start_associates_jobs_and_snapshots_self_contained_inputs() {
    init_test_paths();
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "queued-face.jpg");
    let connection = pool.get().expect("connection");
    connection
        .execute(
            "INSERT INTO media_metadata_jobs (media_id, status) VALUES (?, 'completed')",
            [media_id],
        )
        .expect("metadata job");
    connection.execute("INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, 'face_detection', 0, 'image', 'ai/face.jpg', 'face.jpg', 'image/jpeg', 4, 'abcd')", [media_id]).expect("input");
    drop(connection);

    assert_eq!(face_detection::start(&pool, true).expect("start"), 1);

    let connection = pool.get().expect("connection");
    let (run_id, snapshots): (i64, i64) = connection
        .query_row(
            "SELECT llm_jobs.face_grouping_run_id, COUNT(llm_job_inputs.sequence) FROM llm_jobs JOIN llm_job_inputs ON llm_job_inputs.job_id = llm_jobs.id WHERE llm_jobs.media_id = ? GROUP BY llm_jobs.id",
            [media_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("associated face job");
    assert!(run_id > 0);
    assert_eq!(snapshots, 1);
}

#[test]
fn restart_recovery_resumes_running_face_jobs_and_finishes_cancellation() {
    init_test_paths();
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "restart-face.jpg");
    let connection = pool.get().expect("connection");
    connection
        .execute(
            "INSERT INTO face_grouping_runs (id, status) VALUES (1, 'running')",
            [],
        )
        .expect("run");
    connection.execute("INSERT INTO llm_jobs (id, media_id, face_grouping_run_id, task, status) VALUES ('restart-face-job', ?, 1, 'face_detection', 'submitted')", [media_id]).expect("job");
    drop(connection);

    face_detection::recover_interrupted_runs(&pool).expect("resume running run");
    let connection = pool.get().expect("connection");
    let (run_status, job_status): (String, String) = connection
        .query_row(
            "SELECT face_grouping_runs.status, llm_jobs.status FROM face_grouping_runs JOIN llm_jobs ON llm_jobs.face_grouping_run_id = face_grouping_runs.id WHERE face_grouping_runs.id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("running state");
    assert_eq!(run_status, "running");
    assert_eq!(job_status, "submitted");
    connection
        .execute(
            "UPDATE face_grouping_runs SET status = 'cancelling' WHERE id = 1",
            [],
        )
        .expect("request cancellation");
    drop(connection);

    face_detection::recover_interrupted_runs(&pool).expect("recover cancellation");
    let connection = pool.get().expect("connection");
    let (run_status, job_status): (String, String) = connection
        .query_row(
            "SELECT face_grouping_runs.status, llm_jobs.status FROM face_grouping_runs JOIN llm_jobs ON llm_jobs.face_grouping_run_id = face_grouping_runs.id WHERE face_grouping_runs.id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("cancelled state");
    assert_eq!(run_status, "cancelled");
    assert_eq!(job_status, "cancelled");
}

#[test]
fn automatic_regrouping_does_not_duplicate_manually_merged_faces() {
    init_test_paths();
    let pool = create_test_db();
    let embedding = base64::engine::general_purpose::STANDARD
        .decode(embedding())
        .expect("embedding");
    let media_ids = ["manual-a.jpg", "manual-b.jpg", "automatic.jpg"]
        .map(|filename| create_test_media(&pool, filename));
    let connection = pool.get().expect("connection");
    let mut face_ids = Vec::new();
    for media_id in media_ids {
        connection
            .execute(
                queries::faces::INSERT_FACE,
                rusqlite::params![
                    media_id,
                    0,
                    0,
                    0.0,
                    0.0,
                    1.0,
                    1.0,
                    1.0,
                    1.0,
                    embedding.clone(),
                    "faces/test.jpg"
                ],
            )
            .expect("face");
        face_ids.push(connection.last_insert_rowid());
    }
    connection
        .execute(
            "INSERT INTO face_groups (representative_face_id, manual_curated) VALUES (?, 1)",
            [face_ids[0]],
        )
        .expect("manual group");
    let manual_group_id = connection.last_insert_rowid();
    for face_id in &face_ids[..2] {
        connection
            .execute(queries::faces::INSERT_MEMBER, [manual_group_id, *face_id])
            .expect("manual member");
    }
    connection
        .execute(queries::faces::INSERT_GROUPING_RUN, [])
        .expect("run");
    drop(connection);

    face_detection::finalize_ready_runs(&pool).expect("finalize");

    let connection = pool.get().expect("connection");
    for face_id in &face_ids[..2] {
        let memberships: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM face_group_members WHERE face_id = ?",
                [face_id],
                |row| row.get(0),
            )
            .expect("membership count");
        assert_eq!(memberships, 1);
    }
    let group_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM face_groups", [], |row| row.get(0))
        .expect("group count");
    assert_eq!(group_count, 2);
}
