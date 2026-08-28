use once_cell::sync::Lazy;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Every filesystem location the backend writes to, derived from the configured
/// `server.data_dir`.
#[derive(Debug)]
pub struct Paths {
    pub data: PathBuf,
    pub database: PathBuf,
    pub originals: PathBuf,
    pub thumbnails: PathBuf,
    pub thumbnails_tiny: PathBuf,
    pub thumbnails_places: PathBuf,
    pub previews: PathBuf,
    pub imports: PathBuf,
    pub albums: PathBuf,
    pub trash: PathBuf,
    pub webdav: PathBuf,
    pub backups: PathBuf,
}

impl Paths {
    fn new(data_dir: &Path) -> Self {
        Self {
            data: data_dir.to_path_buf(),
            database: data_dir.join("database.sqlite"),
            originals: data_dir.join("originals"),
            thumbnails: data_dir.join("thumbnails"),
            thumbnails_tiny: data_dir.join("thumbnails_tiny"),
            thumbnails_places: data_dir.join("thumbnails_places"),
            previews: data_dir.join("previews"),
            imports: data_dir.join("imports"),
            albums: data_dir.join("albums"),
            trash: data_dir.join("trash"),
            webdav: data_dir.join("webdav"),
            backups: data_dir.join("backups"),
        }
    }
}

static PATHS: OnceLock<Paths> = OnceLock::new();

/// Derives the process-wide paths from the configured data directory.
///
/// Call exactly once, from the executable entry point, after the config is parsed and
/// before anything touches the filesystem.
pub fn init_paths(data_dir: &Path) {
    PATHS
        .set(Paths::new(data_dir))
        .expect("init_paths called more than once");
}

/// Returns the process-wide paths. Panics if [`init_paths`] has not run.
pub fn paths() -> &'static Paths {
    PATHS
        .get()
        .expect("init_paths must run before any filesystem access")
}

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
