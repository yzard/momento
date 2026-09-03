use chrono::{TimeZone, Timelike, Utc};
use momento_api::config::Config;
use momento_api::cronjob::{next_scheduled_at, run_scheduled_occurrence, ScheduledTask};
use momento_api::database::DbPool;

use crate::test_utils::{
    assert_failed_ai_job_restarted, create_test_db, create_test_media, prepare_failed_ai_job,
};

fn prepare_task_input(pool: &DbPool, task: &str, filename: &str) {
    let connection = pool.get().expect("database connection");
    connection
        .execute(
            "INSERT INTO media (filename, original_filename, file_path, media_type) VALUES (?, ?, ?, 'image')",
            [filename, filename, filename],
        )
        .expect("media");
    let media_id = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO media_metadata_jobs (media_id, status) VALUES (?, 'completed')",
            [media_id],
        )
        .expect("metadata job");
    connection
        .execute(
            "INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, ?, 0, 'image', 'previews', ?, ?, 'image/jpeg', 4, 'hash')",
            rusqlite::params![media_id, task, filename, filename],
        )
        .expect("prepared input");
}

#[test]
fn schedule_uses_the_explicit_immutable_timezone_snapshot() {
    let after = Utc
        .with_ymd_and_hms(2026, 1, 10, 0, 0, 0)
        .single()
        .expect("Valid date");

    let timezone = chrono_tz::America::New_York;
    let next = next_scheduled_at("0 3 * * *", "deduplicate", after, timezone)
        .expect("Schedule should resolve");
    let local = next.with_timezone(&timezone);

    assert!(next > after);
    assert_eq!((local.hour(), local.minute(), local.second()), (3, 0, 0));
}

#[tokio::test]
async fn schedules_dispatch_through_the_correct_run_abstractions() {
    let mut config = Config::default();
    config.llm.enabled = true;
    let scheduled_for = "2026-08-17T03:00:00Z";

    let text_pool = create_test_db();
    let text_executors = crate::test_utils::test_executor_handles(text_pool.clone());
    assert_eq!(
        run_scheduled_occurrence(
            &config,
            &text_executors.sqlite,
            ScheduledTask::Ocr,
            scheduled_for,
        )
        .await
        .unwrap(),
        0
    );
    assert_eq!(
        run_scheduled_occurrence(
            &config,
            &text_executors.sqlite,
            ScheduledTask::ImageAesthetics,
            scheduled_for,
        )
        .await
        .unwrap(),
        0
    );
    assert_eq!(
        run_scheduled_occurrence(
            &config,
            &text_executors.sqlite,
            ScheduledTask::ScreenshotDetection,
            scheduled_for,
        )
        .await
        .unwrap(),
        0
    );
    assert_eq!(
        run_scheduled_occurrence(
            &config,
            &text_executors.sqlite,
            ScheduledTask::DocumentDetection,
            scheduled_for,
        )
        .await
        .unwrap(),
        0
    );
    assert_eq!(
        run_scheduled_occurrence(
            &config,
            &text_executors.sqlite,
            ScheduledTask::ImageTagging,
            scheduled_for,
        )
        .await
        .unwrap(),
        0
    );

    let deduplicate_pool = create_test_db();
    let deduplicate_executors = crate::test_utils::test_executor_handles(deduplicate_pool.clone());
    assert_eq!(
        run_scheduled_occurrence(
            &config,
            &deduplicate_executors.sqlite,
            ScheduledTask::Deduplicate,
            scheduled_for,
        )
        .await
        .unwrap(),
        0
    );
    let deduplicate_connection = deduplicate_pool.get().unwrap();
    let (trigger, stored_schedule): (String, String) = deduplicate_connection
        .query_row(
            "SELECT trigger, scheduled_for FROM media_similarity_runs",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(trigger, "scheduled");
    assert_eq!(stored_schedule, scheduled_for);

    let face_pool = create_test_db();
    let face_executors = crate::test_utils::test_executor_handles(face_pool.clone());
    assert_eq!(
        run_scheduled_occurrence(
            &config,
            &face_executors.sqlite,
            ScheduledTask::FaceDetection,
            scheduled_for,
        )
        .await
        .unwrap(),
        0
    );
    let face_run_count: i64 = face_pool
        .get()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM face_grouping_runs", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(face_run_count, 1);
}

#[tokio::test]
async fn classification_schedules_queue_their_exact_durable_tasks() {
    let mut config = Config::default();
    config.llm.enabled = true;
    let pool = create_test_db();
    prepare_task_input(&pool, "screenshot_detection", "screenshot.jpg");
    prepare_task_input(&pool, "document_detection", "document.jpg");
    let executors = crate::test_utils::test_executor_handles(pool.clone());

    assert_eq!(
        run_scheduled_occurrence(
            &config,
            &executors.sqlite,
            ScheduledTask::ScreenshotDetection,
            "2026-08-17T06:00:00Z",
        )
        .await
        .expect("screenshot schedule"),
        1
    );
    assert_eq!(
        run_scheduled_occurrence(
            &config,
            &executors.sqlite,
            ScheduledTask::DocumentDetection,
            "2026-08-17T07:00:00Z",
        )
        .await
        .expect("document schedule"),
        1
    );

    let connection = pool.get().expect("database connection");
    let mut statement = connection
        .prepare("SELECT task FROM llm_jobs ORDER BY task")
        .expect("task query");
    let tasks = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("queued tasks")
        .collect::<Result<Vec<_>, _>>()
        .expect("task rows");
    assert_eq!(tasks, ["document_detection", "screenshot_detection"]);
}

#[tokio::test]
async fn scheduled_occurrences_queue_a_new_attempt_after_failure_for_every_ai_feature() {
    let mut config = Config::default();
    config.llm.enabled = true;
    let cases = [
        (ScheduledTask::Ocr, "ocr"),
        (ScheduledTask::ImageTagging, "image_tagging"),
        (ScheduledTask::ImageAesthetics, "image_aesthetics"),
        (ScheduledTask::ScreenshotDetection, "screenshot_detection"),
        (ScheduledTask::DocumentDetection, "document_detection"),
        (ScheduledTask::FaceDetection, "face_detection"),
        (ScheduledTask::Deduplicate, "image_clustering"),
    ];
    assert_eq!(
        cases.map(|(_, task)| task),
        momento_api::processor::ai::operation::AiFeature::ALL
            .map(momento_api::processor::ai::operation::AiFeature::inference_task)
    );

    for (case_index, (scheduled_task, inference_task)) in cases.into_iter().enumerate() {
        let pool = create_test_db();
        let media_id =
            create_test_media(&pool, &format!("scheduled-failed-retry-{case_index}.jpg"));
        prepare_failed_ai_job(
            &pool,
            media_id,
            inference_task,
            &format!("{case_index:032x}"),
        );
        let executors = crate::test_utils::test_executor_handles(pool.clone());

        assert_eq!(
            run_scheduled_occurrence(
                &config,
                &executors.sqlite,
                scheduled_task,
                "2026-09-02T18:00:00Z",
            )
            .await
            .expect("scheduled failed-job retry"),
            1
        );

        assert_failed_ai_job_restarted(&pool, inference_task);
    }
}

#[tokio::test]
async fn disabled_global_llm_does_not_queue_classification_schedules() {
    let config = Config::default();
    let pool = create_test_db();
    prepare_task_input(&pool, "screenshot_detection", "screenshot.jpg");
    prepare_task_input(&pool, "document_detection", "document.jpg");
    let executors = crate::test_utils::test_executor_handles(pool.clone());

    for task in [
        ScheduledTask::ScreenshotDetection,
        ScheduledTask::DocumentDetection,
    ] {
        assert_eq!(
            run_scheduled_occurrence(&config, &executors.sqlite, task, "2026-08-17T06:00:00Z")
                .await
                .expect("disabled classification schedule"),
            0
        );
    }
    let queued_jobs: i64 = pool
        .get()
        .expect("database connection")
        .query_row("SELECT COUNT(*) FROM llm_jobs", [], |row| row.get(0))
        .expect("job count");
    assert_eq!(queued_jobs, 0);
}

#[tokio::test]
async fn disabled_global_llm_prevents_every_scheduled_task() {
    let config = Config::default();
    let pool = create_test_db();
    let executors = crate::test_utils::test_executor_handles(pool.clone());

    for task in [
        ScheduledTask::Ocr,
        ScheduledTask::ImageTagging,
        ScheduledTask::ImageAesthetics,
        ScheduledTask::ScreenshotDetection,
        ScheduledTask::DocumentDetection,
        ScheduledTask::Deduplicate,
        ScheduledTask::FaceDetection,
    ] {
        assert_eq!(
            run_scheduled_occurrence(&config, &executors.sqlite, task, "2026-08-17T03:00:00Z")
                .await
                .unwrap(),
            0
        );
    }
    let connection = pool.get().unwrap();
    let deduplicate_runs: i64 = connection
        .query_row("SELECT COUNT(*) FROM media_similarity_runs", [], |row| {
            row.get(0)
        })
        .unwrap();
    let face_runs: i64 = connection
        .query_row("SELECT COUNT(*) FROM face_grouping_runs", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(deduplicate_runs, 0);
    assert_eq!(face_runs, 0);
}

#[tokio::test]
async fn one_scheduled_feature_does_not_wait_for_another_feature_result() {
    let mut config = Config::default();
    config.llm.enabled = true;
    let pool = create_test_db();
    prepare_task_input(&pool, "ocr", "long-running-ocr.jpg");
    prepare_task_input(&pool, "image_tagging", "scheduled-tagging.jpg");
    let executors = crate::test_utils::test_executor_handles(pool.clone());

    assert_eq!(
        run_scheduled_occurrence(
            &config,
            &executors.sqlite,
            ScheduledTask::Ocr,
            "2026-08-25T10:00:00Z",
        )
        .await
        .expect("scheduled OCR"),
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
        run_scheduled_occurrence(
            &config,
            &executors.sqlite,
            ScheduledTask::ImageTagging,
            "2026-08-25T10:05:00Z",
        )
        .await
        .expect("scheduled tagging"),
        1
    );

    let connection = pool.get().expect("database connection");
    let active_task_count: i64 = connection
        .query_row(
            "SELECT COUNT(DISTINCT task) FROM llm_jobs WHERE status IN ('queued', 'submitted')",
            [],
            |row| row.get(0),
        )
        .expect("active task count");
    assert_eq!(active_task_count, 2);
}
