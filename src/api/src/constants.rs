use once_cell::sync::Lazy;
use std::collections::HashSet;
use std::path::PathBuf;

pub static DATA_DIR: Lazy<PathBuf> = Lazy::new(|| {
    std::env::var("MOMENTO_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/data"))
});

pub static CONFIG_PATH: Lazy<PathBuf> = Lazy::new(|| DATA_DIR.join("config.yaml"));
pub static DATABASE_PATH: Lazy<PathBuf> = Lazy::new(|| DATA_DIR.join("database.sqlite"));
pub static ORIGINALS_DIR: Lazy<PathBuf> = Lazy::new(|| DATA_DIR.join("originals"));
pub static THUMBNAILS_DIR: Lazy<PathBuf> = Lazy::new(|| DATA_DIR.join("thumbnails"));
pub static THUMBNAILS_TINY_DIR: Lazy<PathBuf> = Lazy::new(|| DATA_DIR.join("thumbnails_tiny"));
pub static PREVIEWS_DIR: Lazy<PathBuf> = Lazy::new(|| DATA_DIR.join("previews"));
pub static IMPORTS_DIR: Lazy<PathBuf> = Lazy::new(|| DATA_DIR.join("imports"));
pub static ALBUMS_DIR: Lazy<PathBuf> = Lazy::new(|| DATA_DIR.join("albums"));
pub static TRASH_DIR: Lazy<PathBuf> = Lazy::new(|| DATA_DIR.join("trash"));
pub static WEBDAV_DIR: Lazy<PathBuf> = Lazy::new(|| DATA_DIR.join("webdav"));

pub const TRASH_RETENTION_DAYS: i64 = 30;

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
