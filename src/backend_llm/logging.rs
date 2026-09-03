use std::fs;
use std::io;
use std::path::Path;

use momento_common::logging::{validate_log_filename_prefix, EventFormatter};
use tracing::Level;
use tracing_appender::non_blocking::{NonBlockingBuilder, WorkerGuard};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::writer::MakeWriterExt;
use tracing_subscriber::prelude::*;

pub struct LoggingGuard {
    _file_guard: WorkerGuard,
}

pub fn init_logging(data_dir: &Path, log_filename_prefix: &str) -> io::Result<LoggingGuard> {
    validate_log_filename_prefix(log_filename_prefix)?;
    let log_dir = data_dir.join("logs");
    fs::create_dir_all(&log_dir)?;
    let file = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(log_filename_prefix)
        .filename_suffix("log")
        .build(log_dir)
        .map_err(|error| io::Error::other(error.to_string()))?;
    let (file_writer, file_guard) = NonBlockingBuilder::default().lossy(false).finish(file);
    let console_writer = std::io::stderr
        .with_max_level(Level::WARN)
        .or_else(std::io::stdout);
    let console_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .event_format(EventFormatter::new(true))
        .with_writer(console_writer);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .event_format(EventFormatter::new(false))
        .with_writer(file_writer);

    tracing_subscriber::registry()
        .with(LevelFilter::INFO)
        .with(console_layer)
        .with(file_layer)
        .try_init()
        .map_err(|error| io::Error::other(error.to_string()))?;
    Ok(LoggingGuard {
        _file_guard: file_guard,
    })
}
