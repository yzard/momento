use crate::test_utils::QOI_FIXTURE;
use momento_api::config::MediaProcessConfig;
use momento_api::processor::thumbnails::{generate_image_preview, generate_video_preview};

#[tokio::test]
async fn image_preview_preserves_aspect_ratio_within_the_size_bound() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let source = directory.path().join("source.png");
    let output = directory.path().join("nested/place.jpg");
    image::RgbImage::from_pixel(600, 400, image::Rgb([40, 80, 120]))
        .save(&source)
        .expect("source image");
    let process_config = MediaProcessConfig::default();

    generate_image_preview(&source, &output, 300, 85, &process_config)
        .await
        .expect("image preview");

    let preview = image::open(output).expect("generated preview");
    assert_eq!((preview.width(), preview.height()), (300, 200));
}

#[tokio::test]
async fn image_preview_decodes_avif_without_changing_the_original() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let source_png = directory.path().join("source.png");
    let source_avif = directory.path().join("source.avif");
    let output = directory.path().join("preview.jpg");
    image::RgbImage::from_pixel(120, 80, image::Rgb([20, 40, 60]))
        .save(&source_png)
        .expect("source PNG");
    let conversion = std::process::Command::new("convert")
        .arg(&source_png)
        .arg(&source_avif)
        .output()
        .expect("AVIF fixture conversion");
    assert!(
        conversion.status.success(),
        "AVIF fixture conversion: {}",
        String::from_utf8_lossy(&conversion.stderr)
    );
    let original_bytes = std::fs::read(&source_avif).expect("AVIF original bytes");

    generate_image_preview(
        &source_avif,
        &output,
        60,
        85,
        &MediaProcessConfig::default(),
    )
    .await
    .expect("AVIF preview");

    let preview = image::open(output).expect("generated AVIF preview");
    assert_eq!((preview.width(), preview.height()), (60, 40));
    assert_eq!(
        std::fs::read(source_avif).expect("unchanged AVIF original"),
        original_bytes
    );
}

#[tokio::test]
async fn image_preview_decodes_qoi_without_changing_the_original() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let source = directory.path().join("source.qoi");
    let output = directory.path().join("preview.jpg");
    std::fs::write(&source, QOI_FIXTURE).expect("QOI fixture");

    generate_image_preview(&source, &output, 2, 85, &MediaProcessConfig::default())
        .await
        .expect("QOI preview");

    let preview = image::open(output).expect("generated QOI preview");
    assert_eq!((preview.width(), preview.height()), (2, 1));
    assert_eq!(std::fs::read(source).expect("QOI original"), QOI_FIXTURE);
}

#[tokio::test]
async fn video_preview_waits_for_ffmpeg_and_generates_output() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let source = directory.path().join("source.mp4");
    let output = directory.path().join("preview.jpg");
    let ffmpeg = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=64x32:d=1",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&source)
        .output()
        .expect("video fixture command");
    assert!(
        ffmpeg.status.success(),
        "video fixture: {}",
        String::from_utf8_lossy(&ffmpeg.stderr)
    );
    let process_config = MediaProcessConfig::default();

    generate_video_preview(&source, &output, 32, 85, 85, &process_config)
        .await
        .expect("video preview");

    let preview = image::open(output).expect("generated video preview");
    assert_eq!((preview.width(), preview.height()), (32, 16));
}

#[tokio::test]
async fn invalid_image_preview_reports_the_tool_input_and_cause() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let source = directory.path().join("broken-input.heic");
    let output = directory.path().join("preview.jpg");
    std::fs::write(&source, b"not an image").expect("invalid image fixture");

    let error = generate_image_preview(&source, &output, 300, 85, &MediaProcessConfig::default())
        .await
        .expect_err("invalid image must fail");

    assert!(error.contains("identify"), "{error}");
    assert!(error.contains("broken-input.heic"), "{error}");
    assert!(error.contains("exiftool"), "{error}");
}
