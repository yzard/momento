import hashlib
import shutil
import uuid
from datetime import datetime
from pathlib import Path
from typing import Literal, Optional

from momento_api.constants import (
    IMAGE_EXTENSIONS,
    ORIGINALS_DIR,
    SUPPORTED_EXTENSIONS,
    THUMBNAILS_DIR,
    VIDEO_EXTENSIONS,
)
from momento_api.database import get_connection, insert_returning_id
from momento_api.processor.metadata import MediaMetadata, extract_image_metadata, extract_video_metadata
from momento_api.processor.thumbnails import generate_image_thumbnail, generate_video_thumbnail


def get_media_type(file_path: Path) -> Optional[Literal["image", "video"]]:
    ext = file_path.suffix.lower()
    if ext in IMAGE_EXTENSIONS:
        return "image"
    if ext in VIDEO_EXTENSIONS:
        return "video"
    return None


def compute_file_hash(file_path: Path) -> str:
    hasher = hashlib.sha256()
    with open(file_path, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def _validate_media_file(source_path: Path) -> Optional[Literal["image", "video"]]:
    if not source_path.exists():
        return None
    return get_media_type(source_path)


def _get_media_date(metadata: MediaMetadata, source_path: Path) -> datetime:
    if metadata.date_taken:
        return metadata.date_taken
    try:
        return datetime.fromtimestamp(source_path.stat().st_mtime)
    except OSError:
        return datetime.now()


def _save_original_file(source_path: Path, date_taken: datetime, user_id: int) -> tuple[Path, Path, str]:
    year_month = date_taken.strftime("%Y-%m")
    unique_id = uuid.uuid4().hex[:12]
    ext = source_path.suffix.lower()
    new_filename = f"{date_taken.strftime('%Y%m%d_%H%M%S')}_{unique_id}{ext}"

    relative_path = Path(str(user_id)) / year_month / new_filename
    dest_path = ORIGINALS_DIR / relative_path
    dest_path.parent.mkdir(parents=True, exist_ok=True)

    shutil.copy2(source_path, dest_path)
    return dest_path, relative_path, new_filename


def _generate_thumbnails(
    dest_path: Path,
    user_id: int,
    media_type: str,
    thumbnail_max_size: int,
    thumbnail_quality: int,
    video_frame_quality: int,
) -> tuple[Optional[str], bool]:
    thumbnail_filename = f"{dest_path.stem}.jpg"
    thumbnail_relative = Path(str(user_id)) / thumbnail_filename
    thumbnail_path = THUMBNAILS_DIR / thumbnail_relative

    if media_type == "image":
        thumb_ok = generate_image_thumbnail(dest_path, thumbnail_path, thumbnail_max_size, thumbnail_quality)
    else:
        thumb_ok = generate_video_thumbnail(
            dest_path, thumbnail_path, thumbnail_max_size, thumbnail_quality, video_frame_quality
        )

    return str(thumbnail_relative) if thumb_ok else None, thumb_ok


def _insert_media_record(
    user_id: int,
    source_path: Path,
    new_filename: str,
    relative_path: Path,
    thumbnail_relative: Optional[str],
    media_type: str,
    metadata: MediaMetadata,
    file_size: int,
    date_taken: Optional[datetime],
) -> int:
    return insert_returning_id(
        """
        INSERT INTO media (
            user_id, filename, original_filename, file_path, thumbnail_path,
            media_type, mime_type, width, height, file_size, duration_seconds,
            date_taken, gps_latitude, gps_longitude, camera_make, camera_model,
            iso, exposure_time, f_number, focal_length, gps_altitude,
            location_state, location_country, keywords
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        """,
        (
            user_id,
            new_filename,
            source_path.name,
            str(relative_path),
            thumbnail_relative,
            media_type,
            metadata.mime_type,
            metadata.width,
            metadata.height,
            file_size,
            metadata.duration_seconds,
            date_taken.isoformat() if date_taken else None,
            metadata.gps_latitude,
            metadata.gps_longitude,
            metadata.camera_make,
            metadata.camera_model,
            metadata.iso,
            metadata.exposure_time,
            metadata.f_number,
            metadata.focal_length,
            metadata.gps_altitude,
            None,
            None,
            metadata.keywords,
        ),
    )


import time
from momento_api.config import Config, ReverseGeocodingConfig
from urllib.request import Request, urlopen
from urllib.parse import urlencode
import json


def _reverse_geocode(
    config: ReverseGeocodingConfig, latitude: float, longitude: float
) -> tuple[Optional[str], Optional[str]]:
    if not config.enabled:
        return None, None

    params = {"format": "json", "lat": f"{latitude}", "lon": f"{longitude}", "zoom": "10", "addressdetails": "1"}
    url = f"{config.base_url}?{urlencode(params)}"
    headers = {"User-Agent": config.user_agent}

    request = Request(url, headers=headers)
    try:
        with urlopen(request, timeout=config.timeout_seconds) as response:
            payload = json.loads(response.read().decode("utf-8"))
    except (OSError, ValueError, json.JSONDecodeError):
        return None, None

    address = payload.get("address") if isinstance(payload, dict) else None
    if not isinstance(address, dict):
        return None, None

    state = address.get("state") or address.get("region") or address.get("province")
    country = address.get("country")
    return state, country


def generate_complete_metadata(
    source_path: Path, media_type: str, reverse_geo_config: Optional[ReverseGeocodingConfig] = None
) -> MediaMetadata:
    if media_type == "image":
        metadata = extract_image_metadata(source_path)
    else:
        metadata = extract_video_metadata(source_path)

    # Ensure date_taken is populated
    if metadata.date_taken is None:
        try:
            metadata.date_taken = datetime.fromtimestamp(source_path.stat().st_mtime)
        except OSError:
            metadata.date_taken = datetime.now()

    # Perform reverse geocoding if config matches and coordinates exist
    if (
        reverse_geo_config
        and reverse_geo_config.enabled
        and metadata.gps_latitude is not None
        and metadata.gps_longitude is not None
        and (metadata.location_state is None or metadata.location_country is None)
    ):
        state, country = _reverse_geocode(reverse_geo_config, metadata.gps_latitude, metadata.gps_longitude)
        if state:
            metadata.location_state = state
        if country:
            metadata.location_country = country

        # Respect rate limit
        time.sleep(reverse_geo_config.rate_limit_seconds)

    return metadata


def process_media_file(
    source_path: Path,
    user_id: int,
    thumbnail_max_size: int,
    thumbnail_quality: int,
    video_frame_quality: int,
    reverse_geo_config: Optional[ReverseGeocodingConfig] = None,
) -> Optional[int]:
    media_type = _validate_media_file(source_path)
    if not media_type:
        return None

    metadata = generate_complete_metadata(source_path, media_type, reverse_geo_config)

    date_taken = metadata.date_taken
    dest_path, relative_path, new_filename = _save_original_file(source_path, date_taken, user_id)

    thumbnail_relative, _ = _generate_thumbnails(
        dest_path, user_id, media_type, thumbnail_max_size, thumbnail_quality, video_frame_quality
    )

    file_size = dest_path.stat().st_size

    return _insert_media_record(
        user_id,
        source_path,
        new_filename,
        relative_path,
        thumbnail_relative,
        media_type,
        metadata,
        file_size,
        date_taken,
    )


def delete_media_files(file_path: str, thumbnail_path: Optional[str]) -> None:
    raw_file = ORIGINALS_DIR / file_path
    if raw_file.exists():
        raw_file.unlink()

    if thumbnail_path:
        thumb_file = THUMBNAILS_DIR / thumbnail_path
        if thumb_file.exists():
            thumb_file.unlink()
