use chrono::{DateTime, NaiveDateTime, Utc};

pub fn parse_datetime(dt_str: &str) -> Option<DateTime<Utc>> {
    // Try ISO 8601 format first
    if let Ok(dt) = DateTime::parse_from_rfc3339(dt_str) {
        return Some(dt.with_timezone(&Utc));
    }

    // Try common formats
    let formats = [
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
        "%Y:%m:%d %H:%M:%S",
        "%Y-%m-%d",
    ];

    let clean_str = dt_str.replace("Z", "");
    for fmt in &formats {
        if let Ok(naive) = NaiveDateTime::parse_from_str(&clean_str, fmt) {
            return Some(DateTime::from_naive_utc_and_offset(naive, Utc));
        }
    }

    None
}

pub fn format_datetime(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339()
}
