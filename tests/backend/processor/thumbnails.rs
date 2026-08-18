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
