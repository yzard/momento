use chrono::{TimeZone, Timelike, Utc};
use momento_common::logging::{format_log_prefix, validate_log_filename_prefix};
use tracing::Level;

#[test]
fn formats_one_space_between_timestamp_and_level() {
    let timestamp = Utc
        .with_ymd_and_hms(2026, 8, 16, 1, 22, 38)
        .single()
        .expect("valid timestamp")
        .with_nanosecond(879_416_000)
        .expect("valid microseconds");

    let prefix = format_log_prefix(timestamp, &Level::WARN, false);

    assert_eq!(prefix, "2026-08-16T01:22:38.879416Z WARN");
    assert!(!prefix.contains("  "));
}

#[test]
fn rejects_log_filename_prefixes_with_whitespace() {
    let error = validate_log_filename_prefix("llm service")
        .expect_err("invalid application name must fail");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn console_levels_use_requested_ansi_colors() {
    let timestamp = Utc
        .with_ymd_and_hms(2026, 8, 16, 1, 22, 38)
        .single()
        .expect("valid timestamp");

    for (level, color) in [
        (Level::DEBUG, 37),
        (Level::INFO, 37),
        (Level::WARN, 33),
        (Level::ERROR, 31),
    ] {
        let prefix = format_log_prefix(timestamp, &level, true);
        assert!(prefix.starts_with("\u{1b}[2m2026-08-16T01:22:38.000000Z\u{1b}[0m "));
        assert!(prefix.ends_with(&format!("\u{1b}[{color}m{level}\u{1b}[0m")));
        assert!(!prefix.contains("momento-api"));
        assert!(!prefix.contains("12345"));
    }
}
