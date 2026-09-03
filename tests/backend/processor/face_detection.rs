use base64::Engine;
use momento_api::config::{FaceGroupConfig, MediaProcessConfig};
use momento_api::database::queries;
use momento_api::processor::face_detection;
use momento_common::llm::result_payload::FacePayload;
use momento_common::llm::result_stream::{ValidatedResultInput, ValidatedResultValue};

use crate::test_utils::{create_test_db, create_test_media};

fn embedding() -> String {
    let values = (0..512)
        .flat_map(|index| (if index == 0 { 1.0_f32 } else { 0.0_f32 }).to_le_bytes())
        .collect::<Vec<_>>();
    base64::engine::general_purpose::STANDARD.encode(values)
}

fn face_group_config(similarity_threshold: f32) -> FaceGroupConfig {
    FaceGroupConfig {
        similarity_threshold,
        ..FaceGroupConfig::default()
    }
}

async fn finalize_face_groups(pool: &momento_api::database::DbPool, config: &FaceGroupConfig) {
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    face_detection::finalize_ready_runs(&executors, config)
        .await
        .expect("face-group finalization");
}

fn advance_face_finalization_once(
    connection: &rusqlite::Connection,
    config: &FaceGroupConfig,
) -> bool {
    match face_detection::load_finalization_work(connection, config)
        .expect("load face-group finalization work")
    {
        face_detection::FaceGroupFinalizationWork::Idle => false,
        face_detection::FaceGroupFinalizationWork::Progressed => true,
        face_detection::FaceGroupFinalizationWork::Compare(page) => {
            let result = face_detection::compare_group_page(page).expect("compare face page");
            face_detection::commit_cpu_result(connection, result)
                .expect("commit face comparison page");
            true
        }
        face_detection::FaceGroupFinalizationWork::ReduceRepresentative(page) => {
            let result = face_detection::reduce_representative_page(page)
                .expect("reduce representative page");
            face_detection::commit_cpu_result(connection, result)
                .expect("commit representative page");
            true
        }
    }
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

#[tokio::test]
async fn face_callback_rejects_invalid_embedding_before_persistence() {
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
    let results = vec![ValidatedResultInput {
        sequence: 0,
        frame_timestamp_ms: None,
        value: ValidatedResultValue::Faces(vec![FacePayload {
            index: 0,
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
            eye_center_x: 0.5,
            eye_center_y: 0.3,
            confidence: 1.0,
            face_size_score: 1.0,
            frontality_score: 1.0,
            visibility_score: 1.0,
            feature_clarity_score: 1.0,
            embedding: vec![1.0],
        }]),
    }];
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    let process_config = MediaProcessConfig::default();
    let claim_token = uuid::Uuid::new_v4().to_string();
    assert!(face_detection::prepare_typed_result(
        &executors,
        face_detection::TypedFaceResultPreparationRequest {
            context: face_detection::load_preparation_context_on_connection(
                &connection,
                "face-job",
                media_id,
            )
            .expect("face preparation context"),
            job_id: "face-job",
            media_id,
            model_type: "face_detection",
            model_version: "buffalo_l",
            input_results: &results,
            claim_token: &claim_token,
            product_version: 1,
            process_config: &process_config,
        },
    )
    .await
    .is_err());
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM media_faces", [], |row| row.get(0))
        .expect("count");
    assert_eq!(count, 0);
}

#[tokio::test]
async fn face_callback_records_success_when_no_faces_are_detected() {
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
    let results = vec![ValidatedResultInput {
        sequence: 0,
        frame_timestamp_ms: None,
        value: ValidatedResultValue::Faces(Vec::new()),
    }];
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    let process_config = MediaProcessConfig::default();
    let claim_token = uuid::Uuid::new_v4().to_string();
    let prepared = face_detection::prepare_typed_result(
        &executors,
        face_detection::TypedFaceResultPreparationRequest {
            context: face_detection::load_preparation_context_on_connection(
                &connection,
                "no-face-job",
                media_id,
            )
            .expect("face preparation context"),
            job_id: "no-face-job",
            media_id,
            model_type: "face_detection",
            model_version: "buffalo_l",
            input_results: &results,
            claim_token: &claim_token,
            product_version: 1,
            process_config: &process_config,
        },
    )
    .await
    .expect("empty face callback");
    let transaction = connection.unchecked_transaction().expect("transaction");
    let replaced_paths = face_detection::persist_prepared_result(&transaction, prepared)
        .expect("persist empty face callback");
    transaction.commit().expect("commit");
    assert!(replaced_paths.is_empty());

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

#[tokio::test]
async fn failed_face_jobs_still_group_successful_results() {
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
    finalize_face_groups(&pool, &face_group_config(0.55)).await;
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

#[tokio::test]
async fn face_group_similarity_threshold_controls_matching_tolerance() {
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

    finalize_face_groups(&pool, &face_group_config(0.7)).await;
    let connection = pool.get().expect("connection");
    let strict_groups: i64 = connection
        .query_row("SELECT COUNT(*) FROM face_groups", [], |row| row.get(0))
        .expect("strict group count");
    assert_eq!(strict_groups, 2);
    connection
        .execute(queries::faces::INSERT_GROUPING_RUN, [])
        .expect("tolerant grouping run");
    drop(connection);

    finalize_face_groups(&pool, &face_group_config(0.55)).await;
    let tolerant_groups: i64 = pool
        .get()
        .expect("connection")
        .query_row("SELECT COUNT(*) FROM face_groups", [], |row| row.get(0))
        .expect("tolerant group count");
    assert_eq!(tolerant_groups, 1);
}

#[tokio::test]
async fn face_group_representative_weights_frontality_over_center_proximity() {
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

    finalize_face_groups(&pool, &face_group_config(0.55)).await;
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

#[tokio::test]
async fn face_group_representative_recomputes_from_configured_visibility_and_clarity_weights() {
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
    let visibility_config = FaceGroupConfig {
        confidence_weight: 0.0,
        face_size_weight: 0.0,
        center_proximity_weight: 0.0,
        frontality_weight: 0.0,
        visibility_weight: 1.0,
        feature_clarity_weight: 0.0,
        ..FaceGroupConfig::default()
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

    let clarity_config = FaceGroupConfig {
        confidence_weight: 0.0,
        face_size_weight: 0.0,
        center_proximity_weight: 0.0,
        frontality_weight: 0.0,
        visibility_weight: 0.0,
        feature_clarity_weight: 1.0,
        ..FaceGroupConfig::default()
    };
    let handles = crate::test_utils::test_executor_handles(pool.clone());
    face_detection::recompute_face_representatives(&handles, &clarity_config)
        .await
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

    assert_eq!(
        face_detection::start(&pool.get().expect("start connection"), true).expect("start"),
        1
    );

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

#[tokio::test]
async fn restart_recovery_resumes_running_face_jobs_and_finishes_cancellation() {
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

    let executors = crate::test_utils::test_executor_handles(pool.clone());
    executors
        .sqlite
        .recover_face_grouping_runs_durable()
        .await
        .expect("resume running run");
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

    executors
        .sqlite
        .recover_face_grouping_runs_durable()
        .await
        .expect("recover cancellation");
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

#[tokio::test]
async fn automatic_regrouping_attaches_new_faces_to_any_matching_manual_anchor() {
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

    finalize_face_groups(&pool, &face_group_config(0.55)).await;

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

#[tokio::test]
async fn building_face_generation_is_invisible_and_resumes_from_durable_cursors() {
    let pool = create_test_db();
    let first_media_id = create_test_media(&pool, "visible-generation.jpg");
    let first_embedding = [1.0_f32]
        .into_iter()
        .chain(std::iter::repeat_n(0.0_f32, 511))
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    let connection = pool.get().expect("connection");
    insert_face(
        &connection,
        first_media_id,
        (0.0, 0.0, 1.0, 1.0),
        1.0,
        &first_embedding,
    );
    connection
        .execute(queries::faces::INSERT_GROUPING_RUN, [])
        .expect("first run");
    drop(connection);
    let config = face_group_config(0.7);
    finalize_face_groups(&pool, &config).await;

    let second_media_id = create_test_media(&pool, "building-generation.jpg");
    let second_embedding = [0.0_f32, 1.0_f32]
        .into_iter()
        .chain(std::iter::repeat_n(0.0_f32, 510))
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    let connection = pool.get().expect("connection");
    insert_face(
        &connection,
        second_media_id,
        (0.0, 0.0, 1.0, 1.0),
        1.0,
        &second_embedding,
    );
    connection
        .execute(queries::faces::INSERT_GROUPING_RUN, [])
        .expect("second run");

    for _ in 0..32 {
        assert!(advance_face_finalization_once(&connection, &config));
        let staged_groups: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM face_groups WHERE automatic_generation_id = (SELECT id FROM face_group_generations WHERE status = 'building')",
                [],
                |row| row.get(0),
            )
            .expect("staged groups");
        if staged_groups > 0 {
            break;
        }
    }
    let visible_groups: i64 = connection
        .query_row(queries::faces::COUNT_GROUPS, [], |row| row.get(0))
        .expect("visible group count");
    let total_groups: i64 = connection
        .query_row("SELECT COUNT(*) FROM face_groups", [], |row| row.get(0))
        .expect("total group count");
    assert_eq!(visible_groups, 1);
    assert!(total_groups > visible_groups);
    drop(connection);

    finalize_face_groups(&pool, &config).await;
    let connection = pool.get().expect("connection");
    let visible_groups: i64 = connection
        .query_row(queries::faces::COUNT_GROUPS, [], |row| row.get(0))
        .expect("published group count");
    let finalization_rows: i64 = connection
        .query_row("SELECT COUNT(*) FROM face_group_finalizations", [], |row| {
            row.get(0)
        })
        .expect("finalization rows");
    assert_eq!(visible_groups, 2);
    assert_eq!(finalization_rows, 0);
}

#[tokio::test]
async fn face_grouping_pages_more_than_sixty_four_faces_and_cleans_staging() {
    let pool = create_test_db();
    let embedding = [1.0_f32]
        .into_iter()
        .chain(std::iter::repeat_n(0.0_f32, 511))
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    let connection = pool.get().expect("connection");
    for index in 0..70 {
        let media_id = create_test_media(&pool, &format!("paged-face-{index}.jpg"));
        insert_face(&connection, media_id, (0.0, 0.0, 1.0, 1.0), 1.0, &embedding);
    }
    connection
        .execute(queries::faces::INSERT_GROUPING_RUN, [])
        .expect("run");
    drop(connection);

    finalize_face_groups(&pool, &face_group_config(0.7)).await;
    let connection = pool.get().expect("connection");
    let visible_groups: i64 = connection
        .query_row(queries::faces::COUNT_GROUPS, [], |row| row.get(0))
        .expect("visible groups");
    let visible_members: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM face_group_members WHERE automatic_generation_id = (SELECT active_generation_id FROM face_group_generation_state WHERE id = 1)",
            [],
            |row| row.get(0),
        )
        .expect("visible members");
    let staging_rows: i64 = connection
        .query_row(
            "SELECT (SELECT COUNT(*) FROM face_group_finalization_faces) + (SELECT COUNT(*) FROM face_group_finalization_manual_anchors) + (SELECT COUNT(*) FROM face_group_finalization_groups)",
            [],
            |row| row.get(0),
        )
        .expect("staging rows");
    assert_eq!(visible_groups, 1);
    assert_eq!(visible_members, 70);
    assert_eq!(staging_rows, 0);
}

#[tokio::test]
async fn manual_merge_during_build_restarts_and_preserves_the_target_group_identity() {
    let pool = create_test_db();
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
    for (index, embedding) in [&first_embedding, &second_embedding]
        .into_iter()
        .enumerate()
    {
        let media_id = create_test_media(&pool, &format!("manual-restart-{index}.jpg"));
        insert_face(&connection, media_id, (0.0, 0.0, 1.0, 1.0), 1.0, embedding);
    }
    connection
        .execute(queries::faces::INSERT_GROUPING_RUN, [])
        .expect("first run");
    drop(connection);
    let config = face_group_config(0.7);
    finalize_face_groups(&pool, &config).await;

    let new_media_id = create_test_media(&pool, "manual-restart-new.jpg");
    let connection = pool.get().expect("connection");
    let new_face_id = insert_face(
        &connection,
        new_media_id,
        (0.0, 0.0, 1.0, 1.0),
        1.0,
        &second_embedding,
    );
    connection
        .execute(queries::faces::INSERT_GROUPING_RUN, [])
        .expect("second run");
    assert!(advance_face_finalization_once(&connection, &config));
    let outcome = face_detection::merge_groups(&connection, vec![1, 2], &config)
        .expect("merge while finalization is building");
    assert!(matches!(
        outcome,
        face_detection::MergeFaceGroupsOutcome::Merged(_)
    ));
    drop(connection);

    finalize_face_groups(&pool, &config).await;
    let connection = pool.get().expect("connection");
    let (group_id, manual_anchor): (i64, i64) = connection
        .query_row(
            "SELECT face_group_id, manual_anchor FROM face_group_members WHERE face_id = ? AND (manual_anchor = 1 OR automatic_generation_id = (SELECT active_generation_id FROM face_group_generation_state WHERE id = 1)) ORDER BY manual_anchor DESC LIMIT 1",
            [new_face_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("new face membership");
    let source_group_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM face_groups WHERE id = 2", [], |row| {
            row.get(0)
        })
        .expect("source group count");
    assert_eq!(group_id, 1);
    assert_eq!(manual_anchor, 0);
    assert_eq!(source_group_count, 0);
}
