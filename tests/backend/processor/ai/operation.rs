use momento_api::database::DbPool;
use momento_api::processor::ai::operation::AiFeature;

use crate::test_utils::{create_test_db, create_test_media};

fn prepare_input(pool: &DbPool, media_id: i64, task: &str) {
    let connection = pool.get().expect("database connection");
    connection
        .execute(
            "INSERT OR IGNORE INTO media_metadata_jobs (media_id, status) VALUES (?, 'completed')",
            [media_id],
        )
        .expect("metadata job");
    connection
        .execute(
            "INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, ?, 0, 'image', 'previews', ?, ?, 'image/jpeg', 4, 'hash')",
            rusqlite::params![media_id, task, format!("ai/{task}.jpg"), format!("{task}.jpg")],
        )
        .expect("prepared AI input");
}

#[tokio::test]
async fn a_scheduled_feature_starts_while_another_feature_is_submitted() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "independent-features.jpg");
    prepare_input(&pool, media_id, "ocr");
    prepare_input(&pool, media_id, "image_tagging");
    let executors = crate::test_utils::test_executor_handles(pool.clone());

    assert_eq!(
        executors
            .sqlite
            .start_ai_feature_request(AiFeature::Ocr, "manual".to_string(), None)
            .await
            .expect("manual OCR start"),
        1
    );
    pool.get()
        .expect("database connection")
        .execute(
            "UPDATE llm_jobs SET status = 'submitted' WHERE task = 'ocr'",
            [],
        )
        .expect("submitted OCR job");

    assert_eq!(
        executors
            .sqlite
            .start_ai_feature_durable(
                AiFeature::ImageTagging,
                "scheduled".to_string(),
                Some("2026-08-25T12:00:00Z".to_string()),
            )
            .await
            .expect("scheduled tagging start"),
        1
    );

    let connection = pool.get().expect("database connection");
    let mut statement = connection
        .prepare("SELECT task, status FROM llm_jobs ORDER BY task")
        .expect("job query");
    let jobs = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("job rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("jobs");
    assert_eq!(
        jobs,
        [
            ("image_tagging".to_string(), "queued".to_string()),
            ("ocr".to_string(), "submitted".to_string()),
        ]
    );
}

#[tokio::test]
async fn deduplicate_manual_and_scheduled_starts_share_the_operation() {
    let manual_pool = create_test_db();
    let manual_executors = crate::test_utils::test_executor_handles(manual_pool.clone());
    manual_executors
        .sqlite
        .start_ai_feature_request(AiFeature::Deduplicate, "manual".to_string(), None)
        .await
        .expect("manual deduplicate start");
    let manual_trigger: (String, Option<String>) = manual_pool
        .get()
        .expect("database connection")
        .query_row(
            "SELECT trigger, scheduled_for FROM media_similarity_runs",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("manual run");
    assert_eq!(manual_trigger, ("manual".to_string(), None));

    let scheduled_pool = create_test_db();
    let scheduled_executors = crate::test_utils::test_executor_handles(scheduled_pool.clone());
    scheduled_executors
        .sqlite
        .start_ai_feature_durable(
            AiFeature::Deduplicate,
            "scheduled".to_string(),
            Some("2026-08-25T13:00:00Z".to_string()),
        )
        .await
        .expect("scheduled deduplicate start");
    let scheduled_trigger: (String, Option<String>) = scheduled_pool
        .get()
        .expect("database connection")
        .query_row(
            "SELECT trigger, scheduled_for FROM media_similarity_runs",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("scheduled run");
    assert_eq!(
        scheduled_trigger,
        (
            "scheduled".to_string(),
            Some("2026-08-25T13:00:00Z".to_string()),
        )
    );
}

#[tokio::test]
async fn an_active_deduplicate_run_does_not_block_other_features() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "global-start.jpg");
    prepare_input(&pool, media_id, "ocr");
    prepare_input(&pool, media_id, "image_tagging");
    pool.get()
        .expect("database connection")
        .execute(
            "INSERT INTO media_similarity_runs (trigger, status) VALUES ('manual', 'running')",
            [],
        )
        .expect("active deduplicate run");
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    let mut queued = 0;
    for feature in AiFeature::ALL {
        queued += executors
            .sqlite
            .start_ai_feature_request(feature, "manual".to_string(), None)
            .await
            .expect("feature start");
    }
    assert_eq!(queued, 2);
    let queued_tasks: i64 = pool
        .get()
        .expect("database connection")
        .query_row("SELECT COUNT(DISTINCT task) FROM llm_jobs", [], |row| {
            row.get(0)
        })
        .expect("queued task count");
    assert_eq!(queued_tasks, 2);
}

#[tokio::test]
async fn one_start_failure_does_not_prevent_or_hide_other_queued_features() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "partially-independent-start.jpg");
    prepare_input(&pool, media_id, "ocr");
    prepare_input(&pool, media_id, "image_aesthetics");
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    pool.get()
        .expect("database connection")
        .execute("DROP TABLE media_aesthetics", [])
        .expect("break only image aesthetics persistence");
    assert_eq!(
        executors
            .sqlite
            .start_ai_feature_request(AiFeature::Ocr, "manual".to_string(), None)
            .await
            .expect("OCR remains independently startable"),
        1
    );
    let error = executors
        .sqlite
        .start_ai_feature_request(AiFeature::ImageAesthetics, "manual".to_string(), None)
        .await
        .expect_err("broken feature must fail independently");
    assert!(error.to_string().contains("media_aesthetics"));
    let ocr_status: String = pool
        .get()
        .expect("database connection")
        .query_row(
            "SELECT status FROM llm_jobs WHERE task = 'ocr'",
            [],
            |row| row.get(0),
        )
        .expect("queued OCR job");
    assert_eq!(ocr_status, "queued");
}

#[tokio::test]
async fn invalid_scheduled_start_is_rejected_before_sqlite_dispatch() {
    let pool = create_test_db();
    let error = crate::test_utils::test_executor_handles(pool)
        .sqlite
        .start_ai_feature_durable(AiFeature::Ocr, "scheduled".to_string(), None)
        .await
        .expect_err("scheduled starts require their occurrence");
    assert!(error.to_string().contains("AI start source is invalid"));
}
