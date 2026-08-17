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
    pub previews: PathBuf,
    pub imports: PathBuf,
    pub albums: PathBuf,
    pub trash: PathBuf,
    pub webdav: PathBuf,
}

impl Paths {
    fn new(data_dir: &Path) -> Self {
        Self {
            data: data_dir.to_path_buf(),
            database: data_dir.join("database.sqlite"),
            originals: data_dir.join("originals"),
            thumbnails: data_dir.join("thumbnails"),
            thumbnails_tiny: data_dir.join("thumbnails_tiny"),
            previews: data_dir.join("previews"),
            imports: data_dir.join("imports"),
            albums: data_dir.join("albums"),
            trash: data_dir.join("trash"),
            webdav: data_dir.join("webdav"),
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
pub const FACE_DETECTION_MODEL_TYPE: &str = "face_detection";

pub fn media_text_model_name(model_type: &str) -> Option<&'static str> {
    match model_type {
        OCR_MODEL_TYPE => Some("OCR"),
        IMAGE_TAGGING_MODEL_TYPE => Some("Image Tags"),
        _ => None,
    }
}

pub static IMAGE_EXTENSIONS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        ".jpg", ".jpeg", ".png", ".gif", ".bmp", ".tiff", ".webp", ".heic", ".heif",
    ]
    .into_iter()
    .collect()
});

pub static VIDEO_EXTENSIONS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [".mp4", ".mov", ".avi", ".mkv", ".webm", ".m4v"]
        .into_iter()
        .collect()
});

pub static SUPPORTED_EXTENSIONS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    IMAGE_EXTENSIONS
        .iter()
        .chain(VIDEO_EXTENSIONS.iter())
        .copied()
        .collect()
});

pub const DEFAULT_THUMBNAIL_SIZE: u32 = 400;
pub const DEFAULT_TINY_THUMBNAIL_SIZE: u32 = 48;
pub const DEFAULT_THUMBNAIL_QUALITY: u8 = 85;
pub const DEFAULT_VIDEO_FRAME_QUALITY: u8 = 2;
