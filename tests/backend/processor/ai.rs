use crate::test_utils::{create_test_db, create_test_media};
use momento_api::config::Config;
use momento_api::constants::{DOCUMENT_DETECTION_MODEL_TYPE, SCREENSHOT_DETECTION_MODEL_TYPE};
use momento_api::processor::ai::operation::{start_all_features, AiStartSource};
use momento_api::processor::ai::{
    cancel_active_jobs, open_verified_input, queue_task, verify_prepared_input,
};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

mod input;
mod operation;
mod result;
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

#[test]
fn classifier_queueing_is_image_only_and_snapshots_inputs() {
    let pool = create_test_db();
    let photo_id = create_test_media(&pool, "classifier-photo.jpg");
    let video_id = create_test_media(&pool, "classifier-video.mp4");
    complete_metadata_with_input(&pool, photo_id, SCREENSHOT_DETECTION_MODEL_TYPE);
    complete_metadata_with_input(&pool, video_id, SCREENSHOT_DETECTION_MODEL_TYPE);
    pool.get()
        .expect("database connection")
        .execute(
            "UPDATE media SET media_type = 'video', mime_type = 'video/mp4' WHERE id = ?",
            [video_id],
        )
        .expect("video media type");

    assert_eq!(
        queue_task(&pool, SCREENSHOT_DETECTION_MODEL_TYPE, true).expect("queue screenshots"),
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

#[test]
fn classifier_queueing_allows_overlap_and_skips_completed_results() {
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

    let mut config = Config::default();
    config.llm.enabled = true;
    assert_eq!(
        start_all_features(&config, &pool, AiStartSource::Manual).expect("start classifiers"),
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
        queue_task(&pool, SCREENSHOT_DETECTION_MODEL_TYPE, true)
            .expect("do not requeue screenshots"),
        0
    );
    assert_eq!(
        queue_task(&pool, DOCUMENT_DETECTION_MODEL_TYPE, true)
            .expect("active document remains unique"),
        0
    );
}

#[tokio::test]
async fn prepared_input_verification_streams_size_and_hash_validation() {
    let directory = tempfile::TempDir::new().expect("temporary directory");
    let path = directory.path().join("prepared.jpg");
    let bytes = vec![42_u8; 256 * 1024];
    std::fs::write(&path, &bytes).expect("prepared input");
    let content_hash = format!("{:x}", Sha256::digest(&bytes));

    verify_prepared_input(&path, bytes.len() as u64, &content_hash)
        .await
        .expect("matching descriptor");
    assert!(
        verify_prepared_input(&path, bytes.len() as u64 - 1, &content_hash)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn verified_input_handle_survives_source_path_removal() {
    let directory = tempfile::tempdir().expect("input directory");
    let path = directory.path().join("canonical-original.jpg");
    let bytes = b"canonical original bytes";
    std::fs::write(&path, bytes).expect("canonical original");
    let content_hash = format!("{:x}", Sha256::digest(bytes));

    let mut file = open_verified_input(&path, bytes.len() as u64, &content_hash)
        .await
        .expect("verified handle");
    std::fs::remove_file(&path).expect("remove original path after verification");
    let mut streamed = Vec::new();
    file.read_to_end(&mut streamed)
        .await
        .expect("stream handle");

    assert_eq!(streamed, bytes);
}

#[test]
fn cancelling_a_submitting_job_preserves_its_in_flight_attempt() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "submitting-cancel.jpg");
    pool.get()
        .expect("database connection")
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status, attempts) VALUES ('2123456789abcdef0123456789abcdef', ?, 'ocr', 'submitting', 4)",
            [media_id],
        )
        .expect("submitting job");

    cancel_active_jobs(&pool, Some("ocr")).expect("local cancellation");

    let (status, attempts): (String, i64) = pool
        .get()
        .expect("database connection")
        .query_row(
            "SELECT status, attempts FROM llm_jobs WHERE id = '2123456789abcdef0123456789abcdef'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("cancelled job");
    assert_eq!(status, "cancelled");
    assert_eq!(attempts, 5);
}

#[test]
fn queued_replacements_wait_for_task_cancellation_acknowledgement() {
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
    cancel_active_jobs(&pool, Some("image_aesthetics")).expect("local cancellation");
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
