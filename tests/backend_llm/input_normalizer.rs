use std::path::Path;

use llm_service::input_normalizer::{requires_raw_normalization, runtime_input_path};

#[test]
fn encoded_headers_override_mislabeled_raw_but_not_real_raw_containers() {
    use llm_service::input_normalizer::encoded_image_mime_type;
    for (bytes, mime) in [
        (&b"\xff\xd8\xff\xe1"[..], "image/jpeg"),
        (&b"\x89PNG\r\n\x1a\n"[..], "image/png"),
        (&b"GIF89a"[..], "image/gif"),
        (&b"RIFF1234WEBP"[..], "image/webp"),
    ] {
        let detected = encoded_image_mime_type(bytes).unwrap();
        assert_eq!(detected, mime);
        assert!(!requires_raw_normalization(detected));
    }
    for bytes in [
        &b"II\x2a\x00"[..],
        &b"MM\x00\x2a"[..],
        &b""[..],
        &b"\xff\xd8"[..],
        &b"RIFF1234WAVE"[..],
    ] {
        assert_eq!(encoded_image_mime_type(bytes), None);
    }
}

#[test]
fn raw_normalization_enables_dng_sdk_without_downsampling() {
    let arguments = llm_service::input_normalizer::RAW_NORMALIZATION_ARGUMENTS;
    assert_eq!(
        arguments,
        ["-dngsdk", "-w", "+M", "-o", "1", "-q", "3", "-T", "-Z"]
    );
    assert!(!arguments.contains(&"-h"));
}

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
