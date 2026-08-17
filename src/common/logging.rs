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
    log_filename_prefix: &str,
    default_filter: &str,
) -> io::Result<LoggingGuard> {
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
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));
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
        .with(filter)
        .with(console_layer)
        .with(file_layer)
        .try_init()
        .map_err(|error| io::Error::other(error.to_string()))?;
    Ok(LoggingGuard {
        _file_guard: file_guard,
    })
}

pub fn format_log_prefix(timestamp: DateTime<Utc>, level: &Level, colorize: bool) -> String {
    let timestamp = timestamp.to_rfc3339_opts(SecondsFormat::Micros, true);
    if !colorize {
        return format!("{timestamp} {level}");
    }
    let color = level_color(level);
    format!("\u{1b}[2m{timestamp}\u{1b}[0m \u{1b}[{color}m{level}\u{1b}[0m")
}

fn level_color(level: &Level) -> u8 {
    match *level {
        Level::WARN => 33,
        Level::ERROR => 31,
        _ => 37,
    }
}

fn validate_log_filename_prefix(log_filename_prefix: &str) -> io::Result<()> {
    if log_filename_prefix.is_empty() || log_filename_prefix.chars().any(char::is_whitespace) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "log filename prefix must be non-empty and contain no whitespace",
        ));
    }
    Ok(())
}

struct EventFormatter {
    colorize: bool,
}

impl EventFormatter {
    fn new(colorize: bool) -> Self {
        Self { colorize }
    }
}

impl<S, N> FormatEvent<S, N> for EventFormatter
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
        let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true);
        let level = event.metadata().level();
        if self.colorize {
            write!(
                writer,
                "\u{1b}[2m{timestamp}\u{1b}[0m \u{1b}[{}m{level} ",
                level_color(level)
            )?;
        } else {
            write!(writer, "{timestamp} {level} ")?;
        }
        context.format_fields(writer.by_ref(), event)?;
        if self.colorize {
            writeln!(writer, "\u{1b}[0m")
        } else {
            writeln!(writer)
        }
    }
}
