use chrono::{TimeZone, Utc};
use momento_api::config::{Config, CronjobConfig};
use momento_api::cronjob::{next_scheduled_at, run_scheduled_occurrence, ScheduledTask};

use crate::test_utils::create_test_db;

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
fn disabled_global_llm_prevents_every_scheduled_task() {
    let mut config = Config::default();
    config.llm.deduplicate_enabled = true;
    config.llm.face_detection_enabled = true;
    let pool = create_test_db();

    for task in [
        ScheduledTask::Ocr,
        ScheduledTask::ImageTagging,
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
