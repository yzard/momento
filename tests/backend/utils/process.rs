use std::ffi::OsString;
use std::time::{Duration, Instant};

use momento_api::{
    config::MediaProcessConfig,
    utils::process::{
        validate_image_dimensions, ExternalProcess, ExternalProcessError, ImageDimensionError,
    },
};

use crate::test_utils::QOI_FIXTURE;

#[test]
fn external_process_enforces_timeout() {
    let process = ExternalProcess::new(
        "sh",
        vec!["-c".into(), "sleep 30".into()],
        Duration::from_millis(100),
        Duration::from_millis(100),
        1024,
        1024,
    );
    let started_at = Instant::now();

    let error = process
        .run_blocking()
        .expect_err("sleeping process must time out");

    assert!(matches!(error, ExternalProcessError::Timeout { .. }));
    assert!(started_at.elapsed() < Duration::from_secs(2));
}

#[test]
fn external_process_truncates_output_without_blocking_the_child() {
    let process = ExternalProcess::new(
        "sh",
        vec![
            OsString::from("-c"),
            OsString::from("printf '123456789'; printf 'abcdefghi' >&2"),
        ],
        Duration::from_secs(2),
        Duration::from_millis(100),
        4,
        5,
    );

    let output = process.run_blocking().expect("process output");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"1234");
    assert_eq!(output.stderr, b"abcde");
    assert!(output.stdout_truncated);
    assert!(output.stderr_truncated);
}

#[tokio::test]
async fn image_dimension_validation_enforces_the_total_pixel_limit() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let image_path = directory.path().join("image.qoi");
    std::fs::write(&image_path, QOI_FIXTURE).expect("QOI image");
    let config = MediaProcessConfig {
        maximum_decoded_image_pixels: 5,
        ..MediaProcessConfig::default()
    };

    let error = validate_image_dimensions(&image_path, &config)
        .await
        .expect_err("3x2 image must exceed five pixels");
    assert!(matches!(
        error,
        ImageDimensionError::PixelLimitExceeded {
            actual_pixels: 6,
            maximum_pixels: 5
        }
    ));
}
