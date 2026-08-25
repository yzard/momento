use chrono::{TimeZone, Utc};
use momento_api::config::{Config, CronjobConfig};
use momento_api::cronjob::{next_scheduled_at, run_scheduled_occurrence, ScheduledTask};
use momento_api::database::DbPool;

use crate::test_utils::create_test_db;

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
            "INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, ?, 0, 'image', ?, ?, 'image/jpeg', 4, 'hash')",
            rusqlite::params![media_id, task, filename, filename],
        )
        .expect("prepared input");
}

#[test]
fn schedule_uses_configured_iana_timezone() {
    let config = CronjobConfig {
        timezone: "America/New_York".to_string(),
        deduplicate_cron: "0 3 * * *".to_string(),
        ..CronjobConfig::default()
    };
    let after = Utc
        .with_ymd_and_hms(2026, 1, 10, 0, 0, 0)
        .single()
        .expect("Valid date");

    let next = next_scheduled_at(&config, &config.deduplicate_cron, "deduplicate", after)
        .expect("Schedule should resolve");

    assert_eq!(next, Utc.with_ymd_and_hms(2026, 1, 10, 8, 0, 0).unwrap());
}

#[test]
fn schedule_skips_nonexistent_spring_forward_time() {
    let config = CronjobConfig {
        timezone: "America/New_York".to_string(),
        deduplicate_cron: "30 2 * * *".to_string(),
        ..CronjobConfig::default()
    };
    let after = Utc
        .with_ymd_and_hms(2026, 3, 8, 5, 0, 0)
        .single()
        .expect("Valid date");

    let next = next_scheduled_at(&config, &config.deduplicate_cron, "deduplicate", after)
        .expect("Schedule should resolve");

    assert_eq!(next, Utc.with_ymd_and_hms(2026, 3, 9, 6, 30, 0).unwrap());
}

#[test]
fn schedules_dispatch_through_the_correct_run_abstractions() {
    let mut config = Config::default();
    config.llm.enabled = true;
    config.llm.image_tagging_enabled = true;
    config.llm.deduplicate_enabled = true;
    config.llm.face_detection_enabled = true;
    config.llm.image_aesthetics_enabled = true;
    config.llm.screenshot_detection_enabled = true;
    config.llm.document_detection_enabled = true;
    let scheduled_for = "2026-08-17T03:00:00Z";

    let text_pool = create_test_db();
    assert_eq!(
        run_scheduled_occurrence(&config, &text_pool, ScheduledTask::Ocr, scheduled_for).unwrap(),
        0
    );
    assert_eq!(
        run_scheduled_occurrence(
            &config,
            &text_pool,
            ScheduledTask::ImageAesthetics,
            scheduled_for,
        )
        .unwrap(),
        0
    );
    assert_eq!(
        run_scheduled_occurrence(
            &config,
            &text_pool,
            ScheduledTask::ScreenshotDetection,
            scheduled_for,
        )
        .unwrap(),
        0
    );
    assert_eq!(
        run_scheduled_occurrence(
            &config,
            &text_pool,
            ScheduledTask::DocumentDetection,
            scheduled_for,
        )
        .unwrap(),
        0
    );
    assert_eq!(
        run_scheduled_occurrence(
            &config,
            &text_pool,
            ScheduledTask::ImageTagging,
            scheduled_for,
        )
        .unwrap(),
        0
    );

    let deduplicate_pool = create_test_db();
    assert_eq!(
        run_scheduled_occurrence(
            &config,
            &deduplicate_pool,
            ScheduledTask::Deduplicate,
            scheduled_for,
        )
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
    assert_eq!(
        run_scheduled_occurrence(
            &config,
            &face_pool,
            ScheduledTask::FaceDetection,
            scheduled_for,
        )
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

#[test]
fn classification_schedules_queue_their_exact_durable_tasks() {
    let mut config = Config::default();
    config.llm.enabled = true;
    config.llm.screenshot_detection_enabled = true;
    config.llm.document_detection_enabled = true;
    let pool = create_test_db();
    prepare_task_input(&pool, "screenshot_detection", "screenshot.jpg");
    prepare_task_input(&pool, "document_detection", "document.jpg");

    assert_eq!(
        run_scheduled_occurrence(
            &config,
            &pool,
            ScheduledTask::ScreenshotDetection,
            "2026-08-17T06:00:00Z",
        )
        .expect("screenshot schedule"),
        1
    );
    assert_eq!(
        run_scheduled_occurrence(
            &config,
            &pool,
            ScheduledTask::DocumentDetection,
            "2026-08-17T07:00:00Z",
        )
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

#[test]
fn disabled_classification_features_do_not_queue_scheduled_tasks() {
    let mut config = Config::default();
    config.llm.enabled = true;
    let pool = create_test_db();
    prepare_task_input(&pool, "screenshot_detection", "screenshot.jpg");
    prepare_task_input(&pool, "document_detection", "document.jpg");

    for task in [
        ScheduledTask::ScreenshotDetection,
        ScheduledTask::DocumentDetection,
    ] {
        assert_eq!(
            run_scheduled_occurrence(&config, &pool, task, "2026-08-17T06:00:00Z")
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

#[test]
fn disabled_global_llm_prevents_every_scheduled_task() {
    let mut config = Config::default();
    config.llm.deduplicate_enabled = true;
    config.llm.face_detection_enabled = true;
    let pool = create_test_db();

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
            run_scheduled_occurrence(&config, &pool, task, "2026-08-17T03:00:00Z").unwrap(),
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

#[test]
fn one_scheduled_feature_does_not_wait_for_another_feature_result() {
    let mut config = Config::default();
    config.llm.enabled = true;
    config.llm.image_tagging_enabled = true;
    let pool = create_test_db();
    prepare_task_input(&pool, "ocr", "long-running-ocr.jpg");
    prepare_task_input(&pool, "image_tagging", "scheduled-tagging.jpg");

    assert_eq!(
        run_scheduled_occurrence(&config, &pool, ScheduledTask::Ocr, "2026-08-25T10:00:00Z",)
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
            &pool,
            ScheduledTask::ImageTagging,
            "2026-08-25T10:05:00Z",
        )
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
