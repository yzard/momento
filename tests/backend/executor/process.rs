use crate::test_utils::{create_test_db, test_executor_handles_with_data_directory};
use momento_api::{
    config::MediaProcessConfig,
    executor::process::{
        ffmpeg_single_thread_arguments, ffprobe_single_thread_arguments,
        image_magick_resource_arguments, validate_image_geometry,
        validate_storage_image_dimensions, ImageDimensionError,
    },
};

use crate::test_utils::QOI_FIXTURE;

#[tokio::test]
async fn image_dimension_validation_enforces_the_total_pixel_limit() {
    let pool = create_test_db();
    momento_api::database::init_database(&pool.get().expect("schema connection")).expect("schema");
    let (executors, data_directory) = test_executor_handles_with_data_directory(pool);
    let relative_path = momento_api::io::file::NormalizedStoragePath::parse("fixtures/limit.qoi")
        .expect("normalized fixture path");
    let image_path = data_directory
        .join("originals")
        .join(relative_path.relative_path());
    std::fs::create_dir_all(image_path.parent().expect("fixture parent")).expect("fixture parent");
    std::fs::write(&image_path, QOI_FIXTURE).expect("QOI image");
    let config = MediaProcessConfig {
        maximum_decoded_image_pixels: 5,
        ..MediaProcessConfig::default()
    };

    let error = validate_storage_image_dimensions(
        &executors.cpu,
        &executors.file_io,
        momento_api::io::file::StorageRootId::Originals,
        relative_path,
        &config,
    )
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

#[tokio::test]
async fn storage_image_validation_hands_a_pinned_descriptor_to_the_cpu_child() {
    let pool = create_test_db();
    momento_api::database::init_database(&pool.get().expect("schema connection")).expect("schema");
    let (executors, data_directory) = test_executor_handles_with_data_directory(pool);
    let relative_path = momento_api::io::file::NormalizedStoragePath::parse("fixtures/image.qoi")
        .expect("normalized fixture path");
    let absolute_path = data_directory
        .join("originals")
        .join(relative_path.relative_path());
    std::fs::create_dir_all(absolute_path.parent().expect("fixture parent"))
        .expect("fixture parent");
    std::fs::write(&absolute_path, QOI_FIXTURE).expect("QOI fixture");

    validate_storage_image_dimensions(
        &executors.cpu,
        &executors.file_io,
        momento_api::io::file::StorageRootId::Originals,
        relative_path,
        &MediaProcessConfig::default(),
    )
    .await
    .expect("descriptor-backed dimension validation");
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
    assert_eq!(argument_value(&arguments, "memory"), "256MiB");
    assert_eq!(argument_value(&arguments, "map"), "0MiB");
    assert_eq!(argument_value(&arguments, "disk"), "4096MiB");
    assert_eq!(argument_value(&arguments, "thread"), "1");
}

#[test]
fn image_geometry_is_checked_before_decode_allocation() {
    assert_eq!(
        validate_image_geometry(262_144, 1, 1_000_000_000).expect("maximum dimension"),
        (262_144, 1)
    );
    assert!(matches!(
        validate_image_geometry(262_145, 1, 1_000_000_000),
        Err(ImageDimensionError::DimensionLimitExceeded { .. })
    ));
    assert!(matches!(
        validate_image_geometry(100_000, 100_000, 1_000_000_000),
        Err(ImageDimensionError::PixelLimitExceeded { .. })
    ));
    assert!(matches!(
        validate_image_geometry(u64::MAX, 2, u64::MAX),
        Err(ImageDimensionError::DimensionLimitExceeded { .. })
    ));
    assert!(matches!(
        validate_image_geometry(0, 1, 1_000_000_000),
        Err(ImageDimensionError::InvalidOutput)
    ));
}

#[test]
fn ffmpeg_and_ffprobe_arguments_limit_internal_threads() {
    let ffmpeg_arguments = ffmpeg_single_thread_arguments();
    let ffmpeg_arguments = ffmpeg_arguments
        .iter()
        .map(|argument| argument.to_string_lossy())
        .collect::<Vec<_>>();
    assert_eq!(argument_value(&ffmpeg_arguments, "-threads"), "1");
    assert_eq!(argument_value(&ffmpeg_arguments, "-filter_threads"), "1");
    assert_eq!(
        argument_value(&ffmpeg_arguments, "-filter_complex_threads"),
        "1"
    );

    let ffprobe_arguments = ffprobe_single_thread_arguments();
    let ffprobe_arguments = ffprobe_arguments
        .iter()
        .map(|argument| argument.to_string_lossy())
        .collect::<Vec<_>>();
    assert_eq!(argument_value(&ffprobe_arguments, "-threads"), "1");
}

fn argument_value<'a>(arguments: &'a [std::borrow::Cow<'a, str>], option: &str) -> &'a str {
    let option_index = arguments
        .iter()
        .position(|argument| argument == option)
        .expect("command option");
    arguments
        .get(option_index + 1)
        .expect("command option value")
        .as_ref()
}
