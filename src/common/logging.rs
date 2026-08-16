use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use chrono::{DateTime, SecondsFormat, Utc};
use tracing::{Event, Level, Subscriber};
use tracing_appender::non_blocking::{NonBlockingBuilder, WorkerGuard};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::fmt::format::{FormatEvent, FormatFields, Writer};
use tracing_subscriber::fmt::writer::MakeWriterExt;
use tracing_subscriber::fmt::FmtContext;
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::EnvFilter;

pub struct LoggingGuard {
    _file_guard: WorkerGuard,
}

pub fn init_logging(
    data_dir: &Path,
    application_name: &str,
    default_filter: &str,
) -> io::Result<LoggingGuard> {
    validate_application_name(application_name)?;
    let log_dir = data_dir.join("logs");
    fs::create_dir_all(&log_dir)?;
    let file = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(application_name)
        .filename_suffix("log")
        .build(log_dir)
        .map_err(|error| io::Error::other(error.to_string()))?;
    let (file_writer, file_guard) = NonBlockingBuilder::default().lossy(false).finish(file);
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));
    let console_writer = std::io::stderr
        .with_max_level(Level::WARN)
        .or_else(std::io::stdout);
    let console_layer = tracing_subscriber::fmt::layer()
        .with_ansi(true)
        .event_format(ApplicationEventFormatter::new(
            application_name,
            std::process::id(),
            true,
        ))
        .with_writer(console_writer);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .event_format(ApplicationEventFormatter::new(
            application_name,
            std::process::id(),
            false,
        ))
        .with_writer(file_writer);

    tracing_subscriber::registry()
        .with(filter)
        .with(console_layer)
        .with(file_layer)
        .try_init()
        .map_err(|error| io::Error::other(error.to_string()))?;
    Ok(LoggingGuard {
        _file_guard: file_guard,
    })
}

pub fn format_log_prefix(
    timestamp: DateTime<Utc>,
    level: &Level,
    application_name: &str,
    process_id: u32,
    colorize: bool,
) -> String {
    let level = format_level(level, colorize);
    format!(
        "{} {} {}[{}]",
        timestamp.to_rfc3339_opts(SecondsFormat::Micros, true),
        level,
        application_name,
        process_id
    )
}

fn format_level(level: &Level, colorize: bool) -> String {
    if !colorize {
        return level.to_string();
    }
    let color = match *level {
        Level::WARN => 33,
        Level::ERROR => 31,
        _ => 37,
    };
    format!("\u{1b}[{color}m{level}\u{1b}[0m")
}

fn validate_application_name(application_name: &str) -> io::Result<()> {
    if application_name.is_empty() || application_name.chars().any(char::is_whitespace) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "application name must be non-empty and contain no whitespace",
        ));
    }
    Ok(())
}

struct ApplicationEventFormatter {
    application_name: Box<str>,
    process_id: u32,
    colorize: bool,
}

impl ApplicationEventFormatter {
    fn new(application_name: &str, process_id: u32, colorize: bool) -> Self {
        Self {
            application_name: application_name.into(),
            process_id,
            colorize,
        }
    }
}

impl<S, N> FormatEvent<S, N> for ApplicationEventFormatter
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(
        &self,
        context: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let prefix = format_log_prefix(
            Utc::now(),
            event.metadata().level(),
            &self.application_name,
            self.process_id,
            self.colorize,
        );
        write!(writer, "{prefix} ")?;
        context.format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}
