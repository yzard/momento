use crate::test_utils::{create_test_db, test_executor_handles_with_data_directory, QOI_FIXTURE};
use momento_api::config::MediaProcessConfig;
use momento_api::io::file::{NormalizedStoragePath, StorageRootId};
use momento_api::processor::thumbnails::{
    generate_image_preview, generate_video_preview, ArtifactPublicationOwner, StorageMediaFile,
};

fn test_media_runtime() -> (momento_api::runtime::ExecutorHandles, std::path::PathBuf) {
    let pool = create_test_db();
    momento_api::database::init_database(&pool.get().expect("schema connection")).expect("schema");
    test_executor_handles_with_data_directory(pool)
}

fn storage_file(storage_root: StorageRootId, path: &str) -> StorageMediaFile {
    StorageMediaFile {
        storage_root,
        path: NormalizedStoragePath::parse(path).expect("normalized storage path"),
    }
}

#[tokio::test]
async fn image_preview_preserves_aspect_ratio_within_the_size_bound() {
    let (executors, data_directory) = test_media_runtime();
    let source_file = storage_file(StorageRootId::Originals, "source.png");
    let output_file = storage_file(StorageRootId::Previews, "nested/place.jpg");
    let source = data_directory.join("originals/source.png");
    let output = data_directory.join("previews/nested/place.jpg");
    image::RgbImage::from_pixel(600, 400, image::Rgb([40, 80, 120]))
        .save(&source)
        .expect("source image");
    let process_config = MediaProcessConfig::default();

    generate_image_preview(
        &executors,
        &source_file,
        &output_file,
        300,
        85,
        &process_config,
        ArtifactPublicationOwner::JournalGroup,
    )
    .await
    .expect("image preview");

    let preview = image::open(output).expect("generated preview");
    assert_eq!((preview.width(), preview.height()), (300, 200));
}

#[tokio::test]
async fn image_preview_decodes_avif_without_changing_the_original() {
    let (executors, data_directory) = test_media_runtime();
    let source_png = data_directory.join("originals/source.png");
    let source_avif = data_directory.join("originals/source.avif");
    let output = data_directory.join("previews/preview.jpg");
    let source_file = storage_file(StorageRootId::Originals, "source.avif");
    let output_file = storage_file(StorageRootId::Previews, "preview.jpg");
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
        &executors,
        &source_file,
        &output_file,
        60,
        85,
        &MediaProcessConfig::default(),
        ArtifactPublicationOwner::JournalGroup,
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
    let (executors, data_directory) = test_media_runtime();
    let source = data_directory.join("originals/source.qoi");
    let output = data_directory.join("previews/preview.jpg");
    let source_file = storage_file(StorageRootId::Originals, "source.qoi");
    let output_file = storage_file(StorageRootId::Previews, "preview.jpg");
    std::fs::write(&source, QOI_FIXTURE).expect("QOI fixture");

    generate_image_preview(
        &executors,
        &source_file,
        &output_file,
        2,
        85,
        &MediaProcessConfig::default(),
        ArtifactPublicationOwner::JournalGroup,
    )
    .await
    .expect("QOI preview");

    let preview = image::open(output).expect("generated QOI preview");
    assert_eq!((preview.width(), preview.height()), (2, 1));
    assert_eq!(std::fs::read(source).expect("QOI original"), QOI_FIXTURE);
}

#[tokio::test]
async fn video_preview_waits_for_ffmpeg_and_generates_output() {
    let (executors, data_directory) = test_media_runtime();
    let source = data_directory.join("originals/source.mp4");
    let output = data_directory.join("previews/preview.jpg");
    let source_file = storage_file(StorageRootId::Originals, "source.mp4");
    let output_file = storage_file(StorageRootId::Previews, "preview.jpg");
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

    generate_video_preview(
        &executors,
        &source_file,
        &output_file,
        32,
        85,
        &process_config,
        ArtifactPublicationOwner::JournalGroup,
    )
    .await
    .expect("video preview");

    let preview = image::open(output).expect("generated video preview");
    assert_eq!((preview.width(), preview.height()), (32, 16));
}

#[tokio::test]
async fn invalid_image_preview_reports_the_tool_input_and_cause() {
    let (executors, data_directory) = test_media_runtime();
    let source = data_directory.join("originals/broken-input.heic");
    let source_file = storage_file(StorageRootId::Originals, "broken-input.heic");
    let output_file = storage_file(StorageRootId::Previews, "preview.jpg");
    std::fs::write(&source, b"not an image").expect("invalid image fixture");

    let error = generate_image_preview(
        &executors,
        &source_file,
        &output_file,
        300,
        85,
        &MediaProcessConfig::default(),
        ArtifactPublicationOwner::JournalGroup,
    )
    .await
    .expect_err("invalid image must fail");

    assert!(error.contains("identify"), "{error}");
    assert!(error.contains("broken-input.heic"), "{error}");
    assert!(error.contains("exiftool"), "{error}");
}

#[tokio::test]
async fn image_preview_aborts_when_the_generated_file_exceeds_its_bound() {
    let (executors, data_directory) = test_media_runtime();
    let source_file = storage_file(StorageRootId::Originals, "bounded-source.png");
    let output_file = storage_file(StorageRootId::Previews, "bounded-preview.jpg");
    let source = data_directory.join("originals/bounded-source.png");
    let output = data_directory.join("previews/bounded-preview.jpg");
    image::RgbImage::from_pixel(600, 400, image::Rgb([40, 80, 120]))
        .save(&source)
        .expect("source image");
    let process_config = MediaProcessConfig {
        maximum_normalized_image_output_bytes: 64,
        ..MediaProcessConfig::default()
    };

    let error = generate_image_preview(
        &executors,
        &source_file,
        &output_file,
        300,
        85,
        &process_config,
        ArtifactPublicationOwner::JournalGroup,
    )
    .await
    .expect_err("oversized generated preview must fail");

    assert!(error.contains("convert"), "{error}");
    assert!(!output.exists(), "oversized output must not be published");
}
