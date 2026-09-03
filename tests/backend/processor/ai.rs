use crate::test_utils::{
    create_test_db, create_test_media, test_data_directory, test_executor_handles,
};
use momento_api::constants::SCREENSHOT_DETECTION_MODEL_TYPE;
use momento_api::io::file::{NormalizedStoragePath, StorageRootId};
use momento_api::processor::ai::operation::AiFeature;
use momento_api::processor::ai::{open_verified_input, verify_prepared_input};
use sha2::{Digest, Sha256};

mod input;
mod operation;
mod transport;

fn complete_metadata_with_input(pool: &momento_api::database::DbPool, media_id: i64, task: &str) {
    let connection = pool.get().expect("database connection");
    connection
        .execute(
            "INSERT INTO media_metadata_jobs (media_id, status) VALUES (?, 'completed')",
            [media_id],
        )
        .expect("metadata job");
    connection
        .execute(
            "INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, ?, 0, 'image', 'previews', 'ai/classifier.jpg', 'classifier.jpg', 'image/jpeg', 4, 'hash')",
            rusqlite::params![media_id, task],
        )
        .expect("classifier input");
}

#[tokio::test]
async fn classifier_queueing_is_image_only_and_snapshots_inputs() {
    let pool = create_test_db();
    let photo_id = create_test_media(&pool, "classifier-photo.jpg");
    let video_id = create_test_media(&pool, "classifier-video.mp4");
    complete_metadata_with_input(&pool, photo_id, SCREENSHOT_DETECTION_MODEL_TYPE);
    complete_metadata_with_input(&pool, video_id, SCREENSHOT_DETECTION_MODEL_TYPE);
    let connection = pool.get().expect("database connection");
    connection
        .execute(
            "UPDATE media SET media_type = 'video', mime_type = 'video/mp4' WHERE id = ?",
            [video_id],
        )
        .expect("video media type");

    assert_eq!(
        crate::test_utils::test_executor_handles(pool.clone())
            .sqlite
            .start_ai_feature_request(AiFeature::ScreenshotDetection, "manual".to_string(), None,)
            .await
            .expect("queue screenshots"),
        1
    );
    let connection = pool.get().expect("database connection");
    let queued_media_id: i64 = connection
        .query_row(
            "SELECT media_id FROM llm_jobs WHERE task = 'screenshot_detection'",
            [],
            |row| row.get(0),
        )
        .expect("queued classifier job");
    let snapshot_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM llm_job_inputs", [], |row| row.get(0))
        .expect("input snapshot count");
    assert_eq!(queued_media_id, photo_id);
    assert_eq!(snapshot_count, 1);
}

#[tokio::test]
async fn classifier_queueing_allows_overlap_and_skips_completed_results() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "classifier-overlap.jpg");
    complete_metadata_with_input(&pool, media_id, SCREENSHOT_DETECTION_MODEL_TYPE);
    let connection = pool.get().expect("database connection");
    connection
        .execute(
            "INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, 'document_detection', 0, 'image', 'previews', 'ai/document.jpg', 'document.jpg', 'image/jpeg', 4, 'hash')",
            [media_id],
        )
        .expect("document input");
    drop(connection);

    let executors = crate::test_utils::test_executor_handles(pool.clone());
    assert_eq!(
        executors
            .sqlite
            .start_ai_feature_request(AiFeature::ScreenshotDetection, "manual".to_string(), None,)
            .await
            .expect("start screenshot classifier")
            + executors
                .sqlite
                .start_ai_feature_request(AiFeature::DocumentDetection, "manual".to_string(), None,)
                .await
                .expect("start document classifier"),
        2
    );
    let connection = pool.get().expect("database connection");
    connection
        .execute(
            "UPDATE llm_jobs SET status = 'completed' WHERE task = 'screenshot_detection'",
            [],
        )
        .expect("complete screenshot job");
    connection
        .execute(
            "INSERT INTO media_screenshot_classifications (media_id, model_type, model_version, is_screenshot, confidence) VALUES (?, 'screenshot_detection', 'test', 1, 0.9)",
            [media_id],
        )
        .expect("screenshot result");
    drop(connection);

    assert_eq!(
        executors
            .sqlite
            .start_ai_feature_request(AiFeature::ScreenshotDetection, "manual".to_string(), None,)
            .await
            .expect("do not requeue screenshots"),
        0
    );
    assert_eq!(
        executors
            .sqlite
            .start_ai_feature_request(AiFeature::DocumentDetection, "manual".to_string(), None,)
            .await
            .expect("active document remains unique"),
        0
    );
}

#[tokio::test]
async fn prepared_input_verification_streams_size_and_hash_validation() {
    let pool = create_test_db();
    let executors = test_executor_handles(pool.clone());
    let relative_path = "ai-tests/prepared.jpg";
    let path = test_data_directory(&pool)
        .join("originals")
        .join(relative_path);
    std::fs::create_dir_all(path.parent().expect("input parent")).expect("input directory");
    let bytes = vec![42_u8; 256 * 1024];
    std::fs::write(&path, &bytes).expect("prepared input");
    let content_hash = format!("{:x}", Sha256::digest(&bytes));

    verify_prepared_input(
        &executors.file_io,
        &executors.cpu,
        StorageRootId::Originals,
        NormalizedStoragePath::parse(relative_path).expect("relative input path"),
        bytes.len() as u64,
        &content_hash,
    )
    .await
    .expect("matching descriptor");
    assert!(verify_prepared_input(
        &executors.file_io,
        &executors.cpu,
        StorageRootId::Originals,
        NormalizedStoragePath::parse(relative_path).expect("relative input path"),
        bytes.len() as u64 - 1,
        &content_hash,
    )
    .await
    .is_err());
}

#[tokio::test]
async fn verified_input_handle_survives_source_path_removal() {
    let pool = create_test_db();
    let executors = test_executor_handles(pool.clone());
    let relative_path = "ai-tests/canonical-original.jpg";
    let path = test_data_directory(&pool)
        .join("originals")
        .join(relative_path);
    std::fs::create_dir_all(path.parent().expect("input parent")).expect("input directory");
    let bytes = b"canonical original bytes";
    std::fs::write(&path, bytes).expect("canonical original");
    let content_hash = format!("{:x}", Sha256::digest(bytes));

    let session = open_verified_input(
        &executors.file_io,
        &executors.cpu,
        StorageRootId::Originals,
        NormalizedStoragePath::parse(relative_path).expect("relative input path"),
        bytes.len() as u64,
        &content_hash,
    )
    .await
    .expect("verified handle");
    std::fs::remove_file(&path).expect("remove original path after verification");
    let (session, streamed) = executors
        .file_io
        .read_storage_session_durable(session, 1024)
        .await
        .expect("stream handle");
    executors
        .file_io
        .close_storage_session_durable(session)
        .await
        .expect("close stream handle");

    assert_eq!(streamed, bytes);
}

#[tokio::test]
async fn cancelling_a_submitting_job_preserves_its_in_flight_attempt() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "submitting-cancel.jpg");
    let connection = pool.get().expect("database connection");
    connection
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES ('2123456789abcdef0123456789abcdef', ?, 'ocr', 'submitting', 4)",
            [media_id],
        )
        .expect("submitting job");
    connection
        .execute(
            "INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, state, product_target, product_version, entry_count) VALUES ('cancel-receipt-group', 'llm_result_receive', 'llm_result', '2123456789abcdef0123456789abcdef', 'prepared', 'llm_result_inbox', 1, 1)",
            [],
        )
        .expect("result journal group");
    connection
        .execute(
            "INSERT INTO data_dir_space_reservations (id, class, owner_kind, owner_id, filesystem_id, reserved_peak_additional_bytes, state) VALUES ('cancel-result-sqlite', 'sqlite', 'llm_result', '2123456789abcdef0123456789abcdef', 'test', 4096, 'released')",
            [],
        )
        .expect("result SQLite reservation");
    connection
        .execute(
            "INSERT INTO llm_result_receipts (job_id, attempt, job_version, media_id, task, result_status, model_type, model_version, encoding, record_count, byte_size, content_hash, journal_group_id, sqlite_reservation_id, inbox_path, receive_token, state, result_product_version) VALUES ('2123456789abcdef0123456789abcdef', 5, 1, ?, 'ocr', 'completed', 'ocr', 'test', 'momento-result-records-v1', 3, 72, ?, 'cancel-receipt-group', 'cancel-result-sqlite', 'llm-results/cancel.bin', '00000000-0000-0000-0000-000000000003', 'receiving', 1)",
            rusqlite::params![media_id, "0".repeat(64)],
        )
        .expect("active result receipt");
    drop(connection);

    let result = crate::test_utils::test_executor_handles(pool.clone())
        .sqlite
        .cancel_ai_feature_request(AiFeature::Ocr)
        .await
        .expect("local cancellation");
    assert_eq!(result.affected_jobs, 1);

    let connection = pool.get().expect("database connection");
    let (status, attempts): (String, i64) = connection
        .query_row(
            "SELECT status, attempts FROM llm_jobs WHERE id = '2123456789abcdef0123456789abcdef'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("cancelled job");
    assert_eq!(status, "cancelled");
    assert_eq!(attempts, 5);
    let receipt: (String, i64) = connection
        .query_row(
            "SELECT state, cancel_requested FROM llm_result_receipts WHERE job_id = '2123456789abcdef0123456789abcdef'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("discarded receipt");
    assert_eq!(receipt, ("discarded".to_string(), 1));
}

#[tokio::test]
async fn queued_replacements_wait_for_task_cancellation_acknowledgement() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "reset-replacement.jpg");
    let connection = pool.get().expect("database connection");
    connection
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status) VALUES ('3123456789abcdef0123456789abcdef', ?, 'image_aesthetics', 'queued')",
            [media_id],
        )
        .expect("active job");
    drop(connection);
    crate::test_utils::test_executor_handles(pool.clone())
        .sqlite
        .cancel_ai_feature_request(AiFeature::ImageAesthetics)
        .await
        .expect("local cancellation");
    let connection = pool.get().expect("database connection");
    connection
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status) VALUES ('4123456789abcdef0123456789abcdef', ?, 'image_aesthetics', 'queued')",
            [media_id],
        )
        .expect("replacement job");

    let blocked = connection
        .prepare(momento_api::database::queries::ai_jobs::SELECT_QUEUED)
        .expect("queued query")
        .query_map([10_i64], |row| row.get::<_, String>(0))
        .expect("blocked rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("blocked jobs");
    assert!(blocked.is_empty());

    connection
        .execute(
            momento_api::database::queries::ai_jobs::DELETE_CANCELLATION_SCOPE_FOR_TASK,
            ["image_aesthetics"],
        )
        .expect("acknowledged cancellation");
    let eligible = connection
        .prepare(momento_api::database::queries::ai_jobs::SELECT_QUEUED)
        .expect("queued query")
        .query_map([10_i64], |row| row.get::<_, String>(0))
        .expect("eligible rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("eligible jobs");
    assert_eq!(eligible, ["4123456789abcdef0123456789abcdef"]);
}

#[tokio::test]
async fn submission_retry_deadline_excludes_cancelled_scopes() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "submission-retry.jpg");
    let connection = pool.get().expect("database connection");
    connection
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status, available_at) VALUES ('5123456789abcdef0123456789abcdef', ?, 'ocr', 'queued', datetime('now', '+30 seconds'))",
            [media_id],
        )
        .expect("future submission");
    drop(connection);
    let executors = test_executor_handles(pool.clone());

    let retry_delay = executors
        .sqlite
        .load_next_llm_submission_delay_durable()
        .await
        .expect("submission retry deadline")
        .expect("future submission retry");
    assert!(retry_delay <= std::time::Duration::from_secs(30));
    assert!(retry_delay >= std::time::Duration::from_secs(1));

    pool.get()
        .expect("database connection")
        .execute(
            "INSERT INTO llm_cancellation_scopes (scope, task) VALUES ('task', 'ocr')",
            [],
        )
        .expect("cancellation scope");
    assert!(executors
        .sqlite
        .load_next_llm_submission_delay_durable()
        .await
        .expect("blocked submission retry deadline")
        .is_none());
}

#[tokio::test]
async fn submission_worker_deadline_includes_a_live_stale_claim_timer() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "stale-submission.jpg");
    pool.get()
        .expect("database connection")
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status, claimed_at) VALUES ('6123456789abcdef0123456789abcdef', ?, 'ocr', 'submitting', datetime('now', '+1 minute'))",
            [media_id],
        )
        .expect("submitting job");
    let executors = test_executor_handles(pool);

    let retry_delay = executors
        .sqlite
        .load_next_llm_submission_delay_durable()
        .await
        .expect("stale claim deadline")
        .expect("future stale claim");

    assert!(retry_delay <= std::time::Duration::from_secs(360));
    assert!(retry_delay >= std::time::Duration::from_secs(330));
}
