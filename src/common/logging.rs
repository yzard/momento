use std::fmt;
use std::io;

use chrono::{DateTime, SecondsFormat, Utc};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::fmt::format::{FormatEvent, FormatFields, Writer};
use tracing_subscriber::fmt::FmtContext;
use tracing_subscriber::registry::LookupSpan;

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

pub fn validate_log_filename_prefix(log_filename_prefix: &str) -> io::Result<()> {
    if log_filename_prefix.is_empty() || log_filename_prefix.chars().any(char::is_whitespace) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "log filename prefix must be non-empty and contain no whitespace",
        ));
    }
    Ok(())
}

pub struct EventFormatter {
    colorize: bool,
}

impl EventFormatter {
    pub fn new(colorize: bool) -> Self {
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
