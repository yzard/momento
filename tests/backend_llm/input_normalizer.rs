use std::path::Path;

use llm_service::input_normalizer::{requires_raw_normalization, runtime_input_path};

#[tokio::test]
async fn content_probe_distinguishes_dng_from_tiff_without_declared_type() {
    use llm_service::input_normalizer::detect_input_mime;
    let directory = crate::temporary::tempdir().unwrap();
    let path = directory.path().join("input-0");
    // Little-endian TIFF with one DNGVersion tag, stored inline in the IFD.
    let dng = b"II\x2a\x00\x08\x00\x00\x00\x01\x00\x12\xc6\x01\x00\x04\x00\x00\x00\x01\x07\x00\x00\x00\x00\x00\x00";
    std::fs::write(&path, dng).unwrap();
    assert_eq!(
        detect_input_mime(&path, &dng[..12]).await.unwrap(),
        "image/x-adobe-dng"
    );
    let tiff = b"II\x2a\x00\x08\x00\x00\x00\x00\x00\x00\x00\x00\x00";
    std::fs::write(&path, tiff).unwrap();
    assert_eq!(
        detect_input_mime(&path, &tiff[..12]).await.unwrap(),
        "image/tiff"
    );
    std::fs::write(&path, b"not an image").unwrap();
    assert!(detect_input_mime(&path, b"not an image").await.is_err());
    std::fs::write(&path, b"II\x2a").unwrap();
    assert!(detect_input_mime(&path, b"II\x2a").await.is_err());
}

#[tokio::test]
async fn jpeg_content_does_not_require_raw_decoder() {
    let mime = llm_service::input_normalizer::detect_input_mime(
        Path::new("/unused/input-0"),
        b"\xff\xd8\xff\xe1",
    )
    .await
    .unwrap();
    assert_eq!(mime, "image/jpeg");
    assert!(!requires_raw_normalization(&mime));
}

#[test]
fn llm_image_installs_content_probe() {
    assert!(include_str!("../../docker/Dockerfile.llm").contains("libimage-exiftool-perl"));
}

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
