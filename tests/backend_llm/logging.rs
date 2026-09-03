#[test]
fn llm_service_owns_its_daily_non_ansi_file_sink() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let guard = llm_service::logging::init_logging(directory.path(), "llm-service")
        .expect("logging initialization");

    tracing::debug!("filtered debug event");
    tracing::info!("accepted info event");
    tracing::warn!(job_id = "abc123", "result delivery retry");
    drop(guard);

    let log_path = std::fs::read_dir(directory.path().join("logs"))
        .expect("log directory")
        .map(|entry| entry.expect("log entry").path())
        .find(|path| {
            path.file_name().is_some_and(|name| {
                let name = name.to_string_lossy();
                name.starts_with("llm-service.") && name.ends_with(".log")
            })
        })
        .expect("daily log file");
    let output = std::fs::read_to_string(log_path).expect("log output");
    assert!(output.contains(" WARN result delivery retry job_id=\"abc123\""));
    assert!(output.contains(" INFO accepted info event"));
    assert!(!output.contains("filtered debug event"));
    assert!(!output.contains("llm-service["));
    assert!(!output.contains('\u{1b}'));
}
