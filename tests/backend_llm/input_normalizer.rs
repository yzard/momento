use std::path::Path;

use llm_service::input_normalizer::{requires_raw_normalization, runtime_input_path};

#[test]
fn raw_mime_types_require_full_resolution_normalization() {
    for mime_type in [
        "image/x-adobe-dng",
        "image/x-canon-cr2",
        "image/x-canon-cr3",
        "image/x-nikon-nef",
        "image/x-sony-arw",
        "image/x-panasonic-rw2",
        "image/x-olympus-orf",
        "image/x-fuji-raf",
        "image/x-pentax-pef",
        "image/x-samsung-srw",
        "image/x-raw",
    ] {
        assert!(requires_raw_normalization(mime_type), "{mime_type}");
    }
    assert!(!requires_raw_normalization("image/jpeg"));
    assert!(!requires_raw_normalization("image/heic"));
}

#[test]
fn runtime_paths_are_derived_without_accepting_user_paths() {
    let job = Path::new("/queue/processing/abcdef");
    assert_eq!(runtime_input_path(job, 7, false), job.join("input-7"));
    assert_eq!(
        runtime_input_path(job, 7, true),
        job.join("normalized-input-7.tiff")
    );
}
