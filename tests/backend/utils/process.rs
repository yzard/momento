use std::ffi::OsString;

use momento_api::{
    config::MediaProcessConfig,
    utils::process::{
        image_magick_resource_arguments, validate_image_dimensions, ExternalProcess,
        ImageDimensionError,
    },
};

use crate::test_utils::QOI_FIXTURE;

#[test]
fn external_process_truncates_output_without_blocking_the_child() {
    let process = ExternalProcess::new(
        "sh",
        vec![
            OsString::from("-c"),
            OsString::from("printf '123456789'; printf 'abcdefghi' >&2"),
        ],
        4,
        5,
    );

    let output = process.run_blocking().expect("process output");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"1234");
    assert_eq!(output.stderr, b"abcde");
    assert!(output.stdout_truncated);
    assert!(output.stderr_truncated);
    let detail = output.failure_detail("fixture");
    assert!(detail.contains("fixture exited with"), "{detail}");
    assert!(detail.contains("stderr: abcde"), "{detail}");
    assert!(detail.contains("stderr capture truncated"), "{detail}");
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

#[test]
fn external_process_waits_until_the_command_completes() {
    let process = ExternalProcess::new(
        "sh",
        vec!["-c".into(), "sleep 0.1; printf complete".into()],
        1024,
        1024,
    );

    let output = process.run_blocking().expect("process must complete");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"complete");
}

#[test]
fn image_magick_arguments_omit_the_time_limit() {
    let config = MediaProcessConfig::default();
    let arguments = image_magick_resource_arguments(&config);
    let arguments = arguments
        .iter()
        .map(|argument| argument.to_string_lossy())
        .collect::<Vec<_>>();

    assert!(!arguments.iter().any(|argument| argument == "time"));
    assert!(arguments.iter().any(|argument| argument == "memory"));
    assert!(arguments.iter().any(|argument| argument == "disk"));
}
