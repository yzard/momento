use momento_api::constants::{image_mime_type, video_mime_type, IMAGE_EXTENSIONS};

#[test]
fn release_version_matches_the_backend_package() {
    let release_version = include_str!("../../src/backend/version.txt").trim();

    assert_eq!(momento_api::VERSION, release_version);
    assert_eq!(env!("CARGO_PKG_VERSION"), release_version);
}

#[test]
fn supported_media_extensions_have_canonical_mime_types() {
    for (filename, expected_mime_type) in [
        ("animation.GIF", "image/gif"),
        ("scan.TIFF", "image/tiff"),
        ("photo.WEBP", "image/webp"),
        ("lossless.QOI", "image/qoi"),
    ] {
        assert_eq!(
            image_mime_type(std::path::Path::new(filename)),
            Some(expected_mime_type)
        );
    }
    assert!(IMAGE_EXTENSIONS.contains(".qoi"));
    assert_eq!(
        video_mime_type(std::path::Path::new("clip.MOV")),
        Some("video/quicktime")
    );
}
