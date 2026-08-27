use momento_api::processor::thumbnails::generate_image_preview;

#[tokio::test]
async fn image_preview_preserves_aspect_ratio_within_the_size_bound() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let source = directory.path().join("source.png");
    let output = directory.path().join("nested/place.jpg");
    image::RgbImage::from_pixel(600, 400, image::Rgb([40, 80, 120]))
        .save(&source)
        .expect("source image");

    assert!(generate_image_preview(&source, &output, 300, 85).await);

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

    assert!(generate_image_preview(&source_avif, &output, 60, 85).await);

    let preview = image::open(output).expect("generated AVIF preview");
    assert_eq!((preview.width(), preview.height()), (60, 40));
    assert_eq!(
        std::fs::read(source_avif).expect("unchanged AVIF original"),
        original_bytes
    );
}
