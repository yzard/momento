import os
from pathlib import Path

DATA_DIR = Path(os.environ.get("MOMENTO_DATA_DIR", "/data"))
CONFIG_PATH = DATA_DIR / "config.yaml"
DATABASE_PATH = DATA_DIR / "database.sqlite"
ORIGINALS_DIR = DATA_DIR / "originals"
THUMBNAILS_DIR = DATA_DIR / "thumbnails"
PREVIEWS_DIR = DATA_DIR / "previews"
IMPORTS_DIR = DATA_DIR / "imports"
ALBUMS_DIR = DATA_DIR / "albums"
TRASH_DIR = DATA_DIR / "trash"

TRASH_RETENTION_DAYS = 30

IMAGE_EXTENSIONS = {
    ".jpg",
    ".jpeg",
    ".png",
    ".gif",
    ".bmp",
    ".tiff",
    ".webp",
    ".heic",
    ".heif",
}
VIDEO_EXTENSIONS = {".mp4", ".mov", ".avi", ".mkv", ".webm", ".m4v"}
SUPPORTED_EXTENSIONS = IMAGE_EXTENSIONS | VIDEO_EXTENSIONS

DEFAULT_THUMBNAIL_SIZE = 400
DEFAULT_THUMBNAIL_QUALITY = 85
DEFAULT_VIDEO_FRAME_QUALITY = 2
