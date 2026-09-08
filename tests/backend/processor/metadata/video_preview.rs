use super::*;

#[test]
fn preview_reservation_is_duration_bounded_and_rejects_invalid_metadata() {
    for duration in [
        None,
        Some(0.0),
        Some(-1.0),
        Some(f64::NAN),
        Some(f64::INFINITY),
    ] {
        assert!(output_limit(duration).is_err());
    }
    assert_eq!(
        output_limit(Some(1.5)).unwrap(),
        6_000_000 + 16 * 1024 * 1024
    );
    assert_eq!(
        output_limit(Some(24.0 * 3600.0)).unwrap(),
        32 * 1024 * 1024 * 1024
    );
    assert!(output_limit(Some(f64::MAX)).is_err());
}

#[test]
fn mov_detection_accepts_quicktime_mime_and_case_insensitive_extension() {
    use crate::constants::requires_mp4_preview;
    use std::path::Path;
    assert!(requires_mp4_preview(Path::new("photo.MOV"), None));
    assert!(requires_mp4_preview(
        Path::new("media"),
        Some("video/quicktime")
    ));
    assert!(!requires_mp4_preview(
        Path::new("video.mp4"),
        Some("video/mp4")
    ));
}
