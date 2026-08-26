use base64::Engine;
use momento_api::config::FaceGroupRepresentativeConfig;
use momento_api::database::queries;
use momento_api::processor::face_detection;
use momento_common::llm::JobInputResult;

use crate::test_utils::{create_test_db, create_test_media, init_test_paths};

fn embedding() -> String {
    let values = (0..512)
        .flat_map(|index| (if index == 0 { 1.0_f32 } else { 0.0_f32 }).to_le_bytes())
        .collect::<Vec<_>>();
    base64::engine::general_purpose::STANDARD.encode(values)
}

fn insert_face(
    connection: &rusqlite::Connection,
    media_id: i64,
    bounding_box: (f64, f64, f64, f64),
    frontality: f64,
    embedding: &[u8],
) -> i64 {
    connection
        .execute(
            queries::faces::INSERT_FACE,
            rusqlite::params![
                media_id,
                0_i64,
                0_i64,
                bounding_box.0,
                bounding_box.1,
                bounding_box.2,
                bounding_box.3,
                1.0_f64,
                1.0_f64,
                frontality,
                1.0_f64,
                1.0_f64,
                embedding,
                "faces/test.jpg"
            ],
        )
        .expect("face");
    connection.last_insert_rowid()
}

#[test]
fn portrait_crop_includes_head_and_shoulders_within_image_bounds() {
    let (crop_x, crop_y, crop_width, crop_height) =
        face_detection::portrait_crop_box(1000, 1000, 0.5, 0.3, 0.2, 0.2);

    assert!(crop_x <= 400);
    assert!(crop_y < 300);
    assert!(crop_x + crop_width >= 600);
    assert!(crop_y + crop_height > 300);
    assert_eq!(crop_width, crop_height);
    assert!((i64::from(crop_x + (crop_width / 2)) - 500).abs() <= 1);
    assert!((i64::from(crop_y + (crop_height / 2)) - 300).abs() <= 1);

    let (edge_x, edge_y, edge_width, edge_height) =
        face_detection::portrait_crop_box(1000, 600, 0.9, 0.75, 0.15, 0.25);
    assert!(edge_x + edge_width <= 1000);
    assert!(edge_y + edge_height <= 600);
    assert_eq!(edge_width, edge_height);

    let (wide_x, wide_y, wide_width, wide_height) =
        face_detection::portrait_crop_box(1600, 400, 0.5, 0.5, 0.4, 0.8);
    assert!(wide_x + wide_width <= 1600);
    assert!(wide_y + wide_height <= 400);
    assert_eq!(wide_width, wide_height);
    assert_eq!(wide_height, 400);
}

#[test]
fn face_callback_rejects_invalid_embedding_before_persistence() {
    init_test_paths();
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "face.jpg");
    let connection = pool.get().expect("connection");
    connection.execute("INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, 'face_detection', 0, 'image', 'previews', 'missing.jpg', 'missing.jpg', 'image/jpeg', 1, 'hash')", [media_id]).expect("input");
    connection
        .execute(
            "INSERT INTO face_grouping_runs (id, status) VALUES (1, 'running')",
            [],
        )
        .expect("run");
    connection.execute("INSERT INTO llm_jobs (id, media_id, face_grouping_run_id, task, status) VALUES ('face-job', ?, 1, 'face_detection', 'submitted')", [media_id]).expect("job");
    connection.execute("INSERT INTO llm_job_inputs (job_id, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES ('face-job', 0, 'image', 'previews', 'missing.jpg', 'missing.jpg', 'image/jpeg', 1, 'hash')", []).expect("job input");
    let results = vec![JobInputResult {
        sequence: 0,
        frame_timestamp_ms: None,
        result: serde_json::json!({ "task": "face_detection", "modelType": "face_detection", "modelVersion": "buffalo_l", "faces": [{ "index": 0, "boundingBox": { "x": 0.0, "y": 0.0, "width": 1.0, "height": 1.0 }, "eyeCenter": { "x": 0.5, "y": 0.3 }, "confidence": 1.0, "faceSizeScore": 1.0, "frontalityScore": 1.0, "visibilityScore": 1.0, "featureClarityScore": 1.0, "embedding": "bad", "embeddingEncoding": "float32_le", "embeddingDimensions": 512 }] }),
    }];
    assert!(face_detection::prepare_result(
        &connection,
        "face-job",
        media_id,
        "face_detection",
        "buffalo_l",
        Some(&results)
    )
    .is_err());
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
    connection.execute("INSERT INTO llm_job_inputs (job_id, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES ('no-face-job', 0, 'image', 'previews', 'missing.jpg', 'missing.jpg', 'image/jpeg', 1, 'hash')", []).expect("job input");
    let results = vec![JobInputResult {
        sequence: 0,
        frame_timestamp_ms: None,
        result: serde_json::json!({
            "task": "face_detection",
            "modelType": "face_detection",
            "modelVersion": "buffalo_l",
            "faces": []
        }),
    }];
    let prepared = face_detection::prepare_result(
        &connection,
        "no-face-job",
        media_id,
        "face_detection",
        "buffalo_l",
        Some(&results),
    )
    .expect("empty face callback");
    let transaction = connection.unchecked_transaction().expect("transaction");
    let changes = face_detection::persist_prepared_result(&transaction, prepared)
        .expect("persist empty face callback");
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
fn failed_face_jobs_still_group_successful_results() {
    init_test_paths();
    let pool = create_test_db();
    let first_media_id = create_test_media(&pool, "first.jpg");
    let second_media_id = create_test_media(&pool, "second.jpg");
    let failed_media_id = create_test_media(&pool, "failed.jpg");
    let connection = pool.get().expect("connection");
    let embedding = base64::engine::general_purpose::STANDARD
        .decode(embedding())
        .expect("embedding");
    for media_id in [first_media_id, second_media_id] {
        insert_face(&connection, media_id, (0.0, 0.0, 1.0, 1.0), 1.0, &embedding);
    }
    connection
        .execute(queries::faces::INSERT_GROUPING_RUN, [])
        .expect("run");
    let run_id = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO llm_jobs (id, media_id, face_grouping_run_id, task, status) VALUES ('failed-face-job', ?, ?, 'face_detection', 'failed')",
            rusqlite::params![failed_media_id, run_id],
        )
        .expect("failed face job");
    drop(connection);
    face_detection::finalize_ready_runs(&pool, 0.55, &FaceGroupRepresentativeConfig::default())
        .expect("finalize");
    let connection = pool.get().expect("connection");
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM face_group_members", [], |row| {
            row.get(0)
        })
        .expect("members");
    assert_eq!(count, 2);
    let (run_status, run_error): (String, Option<String>) = connection
        .query_row(
            "SELECT status, error FROM face_grouping_runs WHERE id = ?",
            [run_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("grouping run status");
    assert_eq!(run_status, "completed");
    assert_eq!(
        run_error.as_deref(),
        Some("1 face detection jobs failed; groups generated from successful results")
    );
}

#[test]
fn face_group_similarity_threshold_controls_matching_tolerance() {
    init_test_paths();
    let pool = create_test_db();
    let first_media_id = create_test_media(&pool, "first-threshold.jpg");
    let second_media_id = create_test_media(&pool, "second-threshold.jpg");
    let connection = pool.get().expect("connection");
    let first_embedding = [1.0_f32]
        .into_iter()
        .chain(std::iter::repeat_n(0.0_f32, 511))
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    let second_embedding = [0.6_f32, 0.8_f32]
        .into_iter()
        .chain(std::iter::repeat_n(0.0_f32, 510))
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    insert_face(
        &connection,
        first_media_id,
        (0.0, 0.0, 1.0, 1.0),
        1.0,
        &first_embedding,
    );
    insert_face(
        &connection,
        second_media_id,
        (0.0, 0.0, 1.0, 1.0),
        1.0,
        &second_embedding,
    );
    connection
        .execute(queries::faces::INSERT_GROUPING_RUN, [])
        .expect("strict grouping run");
    drop(connection);

    face_detection::finalize_ready_runs(&pool, 0.7, &FaceGroupRepresentativeConfig::default())
        .expect("strict grouping");
    let connection = pool.get().expect("connection");
    let strict_groups: i64 = connection
        .query_row("SELECT COUNT(*) FROM face_groups", [], |row| row.get(0))
        .expect("strict group count");
    assert_eq!(strict_groups, 2);
    connection
        .execute(queries::faces::INSERT_GROUPING_RUN, [])
        .expect("tolerant grouping run");
    drop(connection);

    face_detection::finalize_ready_runs(&pool, 0.55, &FaceGroupRepresentativeConfig::default())
        .expect("tolerant grouping");
    let tolerant_groups: i64 = pool
        .get()
        .expect("connection")
        .query_row("SELECT COUNT(*) FROM face_groups", [], |row| row.get(0))
        .expect("tolerant group count");
    assert_eq!(tolerant_groups, 1);
}

#[test]
fn face_group_representative_weights_frontality_over_center_proximity() {
    init_test_paths();
    let pool = create_test_db();
    let media_ids = [
        "center-low-frontality.jpg",
        "near-center-frontal.jpg",
        "center-frontal.jpg",
        "edge-more-frontal.jpg",
    ]
    .map(|filename| create_test_media(&pool, filename));
    let first_embedding = [1.0_f32]
        .into_iter()
        .chain(std::iter::repeat_n(0.0_f32, 511))
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    let second_embedding = [0.0_f32, 1.0_f32]
        .into_iter()
        .chain(std::iter::repeat_n(0.0_f32, 510))
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    let connection = pool.get().expect("connection");
    insert_face(
        &connection,
        media_ids[0],
        (0.4, 0.4, 0.2, 0.2),
        0.1,
        &first_embedding,
    );
    let near_center_frontal_face_id = insert_face(
        &connection,
        media_ids[1],
        (0.41, 0.4, 0.2, 0.2),
        1.0,
        &first_embedding,
    );
    let center_frontal_face_id = insert_face(
        &connection,
        media_ids[2],
        (0.4, 0.4, 0.2, 0.2),
        0.8,
        &second_embedding,
    );
    insert_face(
        &connection,
        media_ids[3],
        (0.0, 0.0, 0.2, 0.2),
        0.9,
        &second_embedding,
    );
    connection
        .execute(queries::faces::INSERT_GROUPING_RUN, [])
        .expect("grouping run");
    drop(connection);

    face_detection::finalize_ready_runs(&pool, 0.55, &FaceGroupRepresentativeConfig::default())
        .expect("finalize grouping");
    let connection = pool.get().expect("connection");
    let frontality_dominant_representative: i64 = connection
        .query_row(
            "SELECT face_groups.representative_face_id FROM face_groups JOIN face_group_members ON face_group_members.face_group_id = face_groups.id WHERE face_group_members.face_id = ?",
            [near_center_frontal_face_id],
            |row| row.get(0),
        )
        .expect("frontality-dominant group representative");
    let center_weighted_representative: i64 = connection
        .query_row(
            "SELECT face_groups.representative_face_id FROM face_groups JOIN face_group_members ON face_group_members.face_group_id = face_groups.id WHERE face_group_members.face_id = ?",
            [center_frontal_face_id],
            |row| row.get(0),
        )
        .expect("center-weighted group representative");

    assert_eq!(
        frontality_dominant_representative,
        near_center_frontal_face_id
    );
    assert_eq!(center_weighted_representative, center_frontal_face_id);
}

#[test]
fn face_group_representative_recomputes_from_configured_visibility_and_clarity_weights() {
    init_test_paths();
    let pool = create_test_db();
    let visibility_media_id = create_test_media(&pool, "visible-face.jpg");
    let clarity_media_id = create_test_media(&pool, "clear-face.jpg");
    let embedding = base64::engine::general_purpose::STANDARD
        .decode(embedding())
        .expect("embedding");
    let connection = pool.get().expect("connection");
    let mut face_ids = Vec::new();
    for (media_id, visibility_score, feature_clarity_score) in [
        (visibility_media_id, 1.0_f64, 0.0_f64),
        (clarity_media_id, 0.0_f64, 1.0_f64),
    ] {
        connection
            .execute(
                queries::faces::INSERT_FACE,
                rusqlite::params![
                    media_id,
                    0,
                    0,
                    0.4,
                    0.4,
                    0.2,
                    0.2,
                    1.0,
                    1.0,
                    1.0,
                    visibility_score,
                    feature_clarity_score,
                    &embedding,
                    "faces/test.jpg"
                ],
            )
            .expect("face");
        face_ids.push(connection.last_insert_rowid());
    }
    connection
        .execute(queries::faces::INSERT_GROUP, [])
        .expect("group");
    let group_id = connection.last_insert_rowid();
    for face_id in &face_ids {
        connection
            .execute(
                queries::faces::INSERT_AUTOMATIC_MEMBER,
                [group_id, *face_id],
            )
            .expect("member");
    }
    let visibility_config = FaceGroupRepresentativeConfig {
        confidence_weight: 0.0,
        face_size_weight: 0.0,
        center_proximity_weight: 0.0,
        frontality_weight: 0.0,
        visibility_weight: 1.0,
        feature_clarity_weight: 0.0,
    };
    face_detection::update_group_representative(&connection, group_id, &visibility_config)
        .expect("visibility representative");
    let representative_face_id: i64 = connection
        .query_row(
            "SELECT representative_face_id FROM face_groups WHERE id = ?",
            [group_id],
            |row| row.get(0),
        )
        .expect("representative");
    assert_eq!(representative_face_id, face_ids[0]);
    drop(connection);

    let clarity_config = FaceGroupRepresentativeConfig {
        confidence_weight: 0.0,
        face_size_weight: 0.0,
        center_proximity_weight: 0.0,
        frontality_weight: 0.0,
        visibility_weight: 0.0,
        feature_clarity_weight: 1.0,
    };
    face_detection::recompute_all_group_representatives(&pool, &clarity_config)
        .expect("clarity representative recomputation");
    let representative_face_id: i64 = pool
        .get()
        .expect("connection")
        .query_row(
            "SELECT representative_face_id FROM face_groups WHERE id = ?",
            [group_id],
            |row| row.get(0),
        )
        .expect("representative");
    assert_eq!(representative_face_id, face_ids[1]);
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
    connection.execute("INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, 'face_detection', 0, 'image', 'previews', 'ai/face.jpg', 'face.jpg', 'image/jpeg', 4, 'abcd')", [media_id]).expect("input");
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
    let job_id = "0123456789abcdef0123456789abcdef";
    connection.execute("INSERT INTO llm_jobs (id, media_id, face_grouping_run_id, task, status) VALUES (?, ?, 1, 'face_detection', 'submitted')", rusqlite::params![job_id, media_id]).expect("job");
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
    let queued_cancellation: String = connection
        .query_row(
            "SELECT job_id FROM llm_job_cancellations WHERE job_id = ?",
            [job_id],
            |row| row.get(0),
        )
        .expect("durable cancellation");
    assert_eq!(queued_cancellation, job_id);
}

#[test]
fn automatic_regrouping_attaches_new_faces_to_any_matching_manual_anchor() {
    init_test_paths();
    let pool = create_test_db();
    let first_anchor_embedding = [1.0_f32, 0.0_f32]
        .into_iter()
        .chain(std::iter::repeat_n(0.0_f32, 510))
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    let second_anchor_embedding = [0.0_f32, 1.0_f32]
        .into_iter()
        .chain(std::iter::repeat_n(0.0_f32, 510))
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    let media_ids = ["manual-a.jpg", "manual-b.jpg", "automatic.jpg"]
        .map(|filename| create_test_media(&pool, filename));
    let connection = pool.get().expect("connection");
    let mut face_ids = Vec::new();
    for (media_id, face_embedding) in media_ids.into_iter().zip([
        first_anchor_embedding,
        second_anchor_embedding.clone(),
        second_anchor_embedding,
    ]) {
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
                    1.0,
                    1.0,
                    1.0,
                    face_embedding,
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
            .execute(
                queries::faces::INSERT_MANUAL_MEMBER,
                [manual_group_id, *face_id],
            )
            .expect("manual member");
    }
    connection
        .execute(queries::faces::INSERT_GROUPING_RUN, [])
        .expect("run");
    drop(connection);

    face_detection::finalize_ready_runs(&pool, 0.55, &FaceGroupRepresentativeConfig::default())
        .expect("finalize");

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
    assert_eq!(group_count, 1);
    let (new_face_group_id, new_face_manual_anchor): (i64, i64) = connection
        .query_row(
            "SELECT face_group_id, manual_anchor FROM face_group_members WHERE face_id = ?",
            [face_ids[2]],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("new face membership");
    assert_eq!(new_face_group_id, manual_group_id);
    assert_eq!(new_face_manual_anchor, 0);
    let manual_anchor_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM face_group_members WHERE face_group_id = ? AND manual_anchor = 1",
            [manual_group_id],
            |row| row.get(0),
        )
        .expect("manual anchor count");
    assert_eq!(manual_anchor_count, 2);
}
