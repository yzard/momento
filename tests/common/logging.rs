use chrono::{TimeZone, Timelike, Utc};
use momento_common::logging::{format_log_prefix, init_logging};
use tracing::Level;

#[test]
fn formats_one_space_between_timestamp_level_and_application() {
    let timestamp = Utc
        .with_ymd_and_hms(2026, 8, 16, 1, 22, 38)
        .single()
        .expect("valid timestamp")
        .with_nanosecond(879_416_000)
        .expect("valid microseconds");

    let prefix = format_log_prefix(timestamp, &Level::WARN, "llm-service", 12345, false);

    assert_eq!(
        prefix,
        "2026-08-16T01:22:38.879416Z WARN llm-service[12345]"
    );
    assert!(!prefix.contains("  "));
}

#[test]
fn rejects_application_names_with_whitespace() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let error = match init_logging(directory.path(), "llm service", "info") {
        Ok(_) => panic!("invalid application name must fail"),
        Err(error) => error,
    };

    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn writes_application_and_process_id_before_event_fields() {
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
    assert!(output.contains(&format!(
        " WARN momento-api[{}] POST /api/v1/map/clusters 401 00.93ms",
        std::process::id()
    )));
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
        let prefix = format_log_prefix(timestamp, &level, "momento-api", 12345, true);
        assert!(prefix.starts_with("\u{1b}[2m2026-08-16T01:22:38.000000Z\u{1b}[0m "));
        assert!(prefix.ends_with(&format!(
            "\u{1b}[{color}m{level} momento-api[12345]\u{1b}[0m"
        )));
    }
}
