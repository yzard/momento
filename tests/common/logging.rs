use chrono::{TimeZone, Timelike, Utc};
use momento_common::logging::{format_log_prefix, init_logging};
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
    let directory = tempfile::tempdir().expect("temporary directory");
    let error = match init_logging(directory.path(), "llm service", "info") {
        Ok(_) => panic!("invalid application name must fail"),
        Err(error) => error,
    };

    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn writes_level_before_event_fields_without_application_or_process_id() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let guard =
        init_logging(directory.path(), "momento-api", "info").expect("logging initialization");

    tracing::warn!(job_id = "abc123", "POST /api/v1/map/clusters 401 00.93ms");
    drop(guard);

    let log_path = std::fs::read_dir(directory.path().join("logs"))
        .expect("log directory")
        .map(|entry| entry.expect("log entry").path())
        .find(|path| {
            path.file_name().is_some_and(|name| {
                let name = name.to_string_lossy();
                name.starts_with("momento-api.") && name.ends_with(".log")
            })
        })
        .expect("daily log file");
    let output = std::fs::read_to_string(log_path).expect("log output");
    assert!(output.contains(" WARN POST /api/v1/map/clusters 401 00.93ms"));
    assert!(!output.contains("momento-api["));
    assert!(!output.contains("  WARN"));
    assert!(!output.contains('\u{1b}'));
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
