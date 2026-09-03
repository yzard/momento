use once_cell::sync::Lazy;
use std::collections::HashSet;
use std::path::Path;

pub const TRASH_RETENTION_DAYS: i64 = 30;

pub const OCR_MODEL_TYPE: &str = "ocr";
pub const IMAGE_TAGGING_MODEL_TYPE: &str = "image_tagging";
pub const IMAGE_AESTHETICS_MODEL_TYPE: &str = "image_aesthetics";
pub const SCREENSHOT_DETECTION_MODEL_TYPE: &str = "screenshot_detection";
pub const DOCUMENT_DETECTION_MODEL_TYPE: &str = "document_detection";
pub const FACE_DETECTION_MODEL_TYPE: &str = "face_detection";

const IMAGE_FORMATS: &[(&str, &str)] = &[
    (".jpg", "image/jpeg"),
    (".jpeg", "image/jpeg"),
    (".png", "image/png"),
    (".gif", "image/gif"),
    (".bmp", "image/bmp"),
    (".tif", "image/tiff"),
    (".tiff", "image/tiff"),
    (".webp", "image/webp"),
    (".qoi", "image/qoi"),
    (".heic", "image/heic"),
    (".heif", "image/heic"),
    (".avif", "image/avif"),
    (".dng", "image/x-adobe-dng"),
    (".cr2", "image/x-canon-cr2"),
    (".cr3", "image/x-canon-cr3"),
    (".nef", "image/x-nikon-nef"),
    (".nrw", "image/x-nikon-nef"),
    (".arw", "image/x-sony-arw"),
    (".rw2", "image/x-panasonic-rw2"),
    (".orf", "image/x-olympus-orf"),
    (".raf", "image/x-fuji-raf"),
    (".pef", "image/x-pentax-pef"),
    (".srw", "image/x-samsung-srw"),
    (".raw", "image/x-raw"),
];

const VIDEO_FORMATS: &[(&str, &str)] = &[
    (".mp4", "video/mp4"),
    (".mov", "video/quicktime"),
    (".avi", "video/x-msvideo"),
    (".mkv", "video/x-matroska"),
    (".webm", "video/webm"),
    (".m4v", "video/x-m4v"),
];

pub static IMAGE_EXTENSIONS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    IMAGE_FORMATS
        .iter()
        .map(|(extension, _)| *extension)
        .collect()
});

pub static VIDEO_EXTENSIONS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    VIDEO_FORMATS
        .iter()
        .map(|(extension, _)| *extension)
        .collect()
});

pub static SUPPORTED_EXTENSIONS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    IMAGE_EXTENSIONS
        .iter()
        .chain(VIDEO_EXTENSIONS.iter())
        .copied()
        .collect()
});

pub fn image_mime_type(file_path: &Path) -> Option<&'static str> {
    mime_type_for_path(file_path, IMAGE_FORMATS)
}

pub fn video_mime_type(file_path: &Path) -> Option<&'static str> {
    mime_type_for_path(file_path, VIDEO_FORMATS)
}

fn mime_type_for_path(
    file_path: &Path,
    formats: &'static [(&'static str, &'static str)],
) -> Option<&'static str> {
    let extension = file_path.extension()?.to_str()?;
    formats
        .iter()
        .find(|(candidate, _)| candidate[1..].eq_ignore_ascii_case(extension))
        .map(|(_, mime_type)| *mime_type)
}
