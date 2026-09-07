use momento_api::constants::{image_mime_type, video_mime_type, IMAGE_EXTENSIONS};

#[test]
fn camera_raw_preview_selection_excludes_regular_images() {
    use momento_api::constants::is_camera_raw_image;
    use std::path::Path;
    for extension in [
        "DNG", "CR2", "CR3", "NEF", "NRW", "ARW", "RW2", "ORF", "RAF", "PEF", "SRW", "RAW",
    ] {
        let filename = format!("photo.{extension}");
        let path = Path::new(&filename);
        let mime = image_mime_type(path).expect("supported RAW");
        assert!(is_camera_raw_image(path, None));
        assert!(is_camera_raw_image(Path::new("no-extension"), Some(mime)));
        assert!(IMAGE_EXTENSIONS.contains(format!(".{}", extension.to_lowercase()).as_str()));
    }
    for extension in [
        "heic", "heif", "qoi", "jpg", "png", "tiff", "gif", "webp", "avif", "bmp",
    ] {
        let filename = format!("photo.{extension}");
        let path = Path::new(&filename);
        assert!(!is_camera_raw_image(path, image_mime_type(path)));
    }
    assert!(!is_camera_raw_image(Path::new("unknown"), None));
}

#[test]
fn jpeg_preview_is_required_only_for_browser_unsupported_formats() {
    use momento_api::constants::requires_jpeg_preview;
    use std::path::Path;
    for extension in ["NEF", "CR3", "DNG", "HEIC", "heif", "tif", "qoi", "raf"] {
        let filename = format!("image.{extension}");
        assert!(requires_jpeg_preview(
            Path::new(&filename),
            image_mime_type(Path::new(&filename))
        ));
    }
    for extension in ["jpg", "png", "gif", "webp", "avif", "bmp"] {
        let filename = format!("image.{extension}");
        assert!(!requires_jpeg_preview(Path::new(&filename), None));
    }
    assert!(requires_jpeg_preview(Path::new("unknown"), None));
}

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
