use chrono::{TimeZone, Utc};
use momento_api::config::CronjobConfig;
use momento_api::cronjob::next_scheduled_at;

#[test]
fn schedule_uses_configured_iana_timezone() {
    let config = CronjobConfig {
        timezone: "America/New_York".to_string(),
        deduplicate_cron: "0 3 * * *".to_string(),
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
    };
    let after = Utc
        .with_ymd_and_hms(2026, 3, 8, 5, 0, 0)
        .single()
        .expect("Valid date");

    let next = next_scheduled_at(&config, &config.deduplicate_cron, "deduplicate", after)
        .expect("Schedule should resolve");

    assert_eq!(next, Utc.with_ymd_and_hms(2026, 3, 9, 6, 30, 0).unwrap());
}
