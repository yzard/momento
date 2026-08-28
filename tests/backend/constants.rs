use momento_api::constants::{image_mime_type, paths, video_mime_type, IMAGE_EXTENSIONS};

use crate::test_utils::init_test_paths;

#[test]
fn release_version_matches_the_backend_package() {
    let release_version = include_str!("../../src/backend/version.txt").trim();

    assert_eq!(momento_api::VERSION, release_version);
    assert_eq!(env!("CARGO_PKG_VERSION"), release_version);
}

#[test]
fn test_paths_derive_from_data_dir() {
    init_test_paths();
    let paths = paths();

    assert_eq!(paths.database, paths.data.join("database.sqlite"));
    assert_eq!(paths.originals, paths.data.join("originals"));
    assert_eq!(paths.thumbnails, paths.data.join("thumbnails"));
    assert_eq!(paths.thumbnails_tiny, paths.data.join("thumbnails_tiny"));
    assert_eq!(paths.previews, paths.data.join("previews"));
    assert_eq!(paths.imports, paths.data.join("imports"));
    assert_eq!(paths.albums, paths.data.join("albums"));
    assert_eq!(paths.trash, paths.data.join("trash"));
    assert_eq!(paths.webdav, paths.data.join("webdav"));
}

#[test]
fn test_paths_are_stable_across_calls() {
    init_test_paths();

    assert_eq!(paths().data, paths().data);
    assert!(paths().data.is_absolute());
}

#[test]
fn test_media_directories_are_distinct() {
    init_test_paths();
    let paths = paths();

    let dirs = [
        &paths.originals,
        &paths.thumbnails,
        &paths.thumbnails_tiny,
        &paths.previews,
        &paths.imports,
        &paths.albums,
        &paths.trash,
        &paths.webdav,
    ];

    for (i, first) in dirs.iter().enumerate() {
        for second in &dirs[i + 1..] {
            assert_ne!(first, second);
        }
    }
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
