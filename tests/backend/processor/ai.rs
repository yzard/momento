use crate::test_utils::{create_test_db, create_test_media};
use momento_api::processor::ai::{cancel_active_jobs, verify_prepared_input};
use sha2::{Digest, Sha256};

mod result;
mod transport;

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
