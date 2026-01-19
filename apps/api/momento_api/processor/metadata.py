import subprocess
from collections.abc import Mapping
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Optional

from PIL import Image
from PIL.ExifTags import GPSTAGS, TAGS

ExifData = dict[str, object]


def _string_or_none(value: object | None) -> Optional[str]:
    if isinstance(value, str):
        return value
    return None


def _int_or_none(value: object | None) -> Optional[int]:
    if isinstance(value, int):
        return value
    if isinstance(value, str) and value.isdigit():
        return int(value)
    return None


def _float_or_none(value: object | None) -> Optional[float]:
    if isinstance(value, (int, float)):
        return float(value)
    if isinstance(value, str):
        try:
            return float(value)
        except ValueError:
            return None
    if isinstance(value, tuple) and len(value) == 2:
        numerator_value, denominator_value = value
        try:
            return float(numerator_value) / float(denominator_value)
        except (ZeroDivisionError, TypeError, ValueError):
            return None
    return None


def _format_exposure_time(value: object | None) -> Optional[str]:
    if value is None:
        return None
    if isinstance(value, str):
        return value
    return None


def _extract_keywords(exif: dict[str, object]) -> Optional[str]:
    keyword_value = exif.get("XPKeywords") or exif.get("Keywords")
    if keyword_value is None:
        return None
    if isinstance(keyword_value, bytes):
        try:
            return keyword_value.decode("utf-16le").strip("\x00")
        except UnicodeDecodeError:
            return None
    if isinstance(keyword_value, str):
        return keyword_value
    return None


def _coerce_exif_mapping(exif_data: Mapping[int, object]) -> ExifData:
    return {str(TAGS.get(k, k)): v for k, v in exif_data.items()}


@dataclass
class MediaMetadata:
    width: Optional[int] = None
    height: Optional[int] = None
    date_taken: Optional[datetime] = None
    gps_latitude: Optional[float] = None
    gps_longitude: Optional[float] = None
    gps_altitude: Optional[float] = None
    camera_make: Optional[str] = None
    camera_model: Optional[str] = None
    iso: Optional[int] = None
    exposure_time: Optional[str] = None
    f_number: Optional[float] = None
    focal_length: Optional[float] = None
    keywords: Optional[str] = None
    duration_seconds: Optional[float] = None
    mime_type: Optional[str] = None
    location_state: Optional[str] = None
    location_country: Optional[str] = None


def _convert_to_degrees(value) -> float:
    d, m, s = value
    return float(d) + float(m) / 60.0 + float(s) / 3600.0


def _parse_exif_datetime(dt_str: str) -> Optional[datetime]:
    formats = ["%Y:%m:%d %H:%M:%S", "%Y-%m-%d %H:%M:%S", "%Y:%m:%d", "%Y-%m-%d"]
    for fmt in formats:
        try:
            return datetime.strptime(dt_str.strip(), fmt)
        except ValueError:
            continue
    return None


def _fallback_to_mtime(file_path: Path) -> Optional[datetime]:
    try:
        return datetime.fromtimestamp(file_path.stat().st_mtime)
    except OSError:
        return None


def _extract_exif_date(exif: dict[str, object]) -> Optional[datetime]:
    date_fields = ["DateTimeOriginal", "DateTimeDigitized", "DateTime"]
    for field in date_fields:
        if field in exif:
            parsed = _parse_exif_datetime(str(exif[field]))
            if parsed:
                return parsed
    return None


def extract_image_metadata(file_path: Path) -> MediaMetadata:
    metadata = MediaMetadata()

    try:
        image = Image.open(file_path)
    except OSError:
        metadata.date_taken = _fallback_to_mtime(file_path)
        return metadata

    with image:
        metadata.width = image.width
        metadata.height = image.height
        img_format = image.format or "JPEG"
        metadata.mime_type = Image.MIME.get(img_format, "image/jpeg")

        exif_data = image.getexif()
        if not exif_data:
            metadata.date_taken = _fallback_to_mtime(file_path)
            return metadata

        exif = _coerce_exif_mapping(exif_data)

        metadata.date_taken = _extract_exif_date(exif) or _fallback_to_mtime(file_path)

        metadata.camera_make = _string_or_none(exif.get("Make"))
        metadata.camera_model = _string_or_none(exif.get("Model"))
        metadata.iso = _int_or_none(exif.get("ISOSpeedRatings"))
        metadata.exposure_time = _format_exposure_time(exif.get("ExposureTime"))
        metadata.f_number = _float_or_none(exif.get("FNumber"))
        metadata.focal_length = _float_or_none(exif.get("FocalLength"))
        metadata.keywords = _extract_keywords(exif)

        gps_data = exif.get("GPSInfo")
        if not isinstance(gps_data, dict):
            return metadata

        gps_info = {str(GPSTAGS.get(k, k)): v for k, v in gps_data.items()}

        if "GPSLatitude" in gps_info and "GPSLatitudeRef" in gps_info:
            lat = _convert_to_degrees(gps_info["GPSLatitude"])
            if gps_info["GPSLatitudeRef"] == "S":
                lat = -lat
            metadata.gps_latitude = lat

        if "GPSLongitude" in gps_info and "GPSLongitudeRef" in gps_info:
            lon = _convert_to_degrees(gps_info["GPSLongitude"])
            if gps_info["GPSLongitudeRef"] == "W":
                lon = -lon
            metadata.gps_longitude = lon

        if "GPSAltitude" in gps_info:
            metadata.gps_altitude = _float_or_none(gps_info["GPSAltitude"])

    return metadata


def extract_video_metadata(file_path: Path) -> MediaMetadata:
    metadata = MediaMetadata()

    try:
        result = subprocess.run(
            ["ffprobe", "-v", "quiet", "-print_format", "json", "-show_format", "-show_streams", str(file_path)],
            capture_output=True,
            text=True,
            timeout=30,
        )
    except (OSError, ValueError):
        metadata.date_taken = _fallback_to_mtime(file_path)
        metadata.duration_seconds = 0.0
        return metadata

    if result.returncode != 0:
        metadata.date_taken = _fallback_to_mtime(file_path)
        metadata.duration_seconds = 0.0
        return metadata

    ffprobe_data = _parse_ffprobe_output(result.stdout)
    if ffprobe_data is None:
        metadata.date_taken = _fallback_to_mtime(file_path)
        return metadata

    _apply_video_stream_metadata(metadata, ffprobe_data)
    _apply_video_format_metadata(metadata, ffprobe_data, file_path)
    return metadata


def _parse_ffprobe_output(output: str) -> Optional[dict[str, object]]:
    import json

    try:
        parsed = json.loads(output)
    except json.JSONDecodeError:
        return None

    if not isinstance(parsed, dict):
        return None
    return parsed


def _apply_video_stream_metadata(metadata: MediaMetadata, data: dict[str, object]) -> None:
    stream_entries = data.get("streams")
    if not isinstance(stream_entries, list):
        return

    for entry in stream_entries:
        if not isinstance(entry, dict):
            continue
        if entry.get("codec_type") != "video":
            continue
        metadata.width = entry.get("width")
        metadata.height = entry.get("height")
        return


def _apply_video_format_metadata(metadata: MediaMetadata, data: dict[str, object], file_path: Path) -> None:
    format_data = data.get("format")
    if not isinstance(format_data, dict):
        metadata.date_taken = _fallback_to_mtime(file_path)
        return

    duration_value = format_data.get("duration")
    if duration_value is None:
        # Try to get duration from video stream if format duration is missing
        stream_entries = data.get("streams")
        if isinstance(stream_entries, list):
            for entry in stream_entries:
                if isinstance(entry, dict) and entry.get("codec_type") == "video" and "duration" in entry:
                    duration_value = entry.get("duration")
                    break

    if duration_value is not None:
        try:
            metadata.duration_seconds = float(duration_value)
        except (TypeError, ValueError):
            metadata.duration_seconds = 0.0
    else:
        metadata.duration_seconds = 0.0

    tag_data = format_data.get("tags")
    if not isinstance(tag_data, dict):
        tag_data = {}

    creation_time_value = tag_data.get("creation_time") or tag_data.get("com.apple.quicktime.creationdate")
    if isinstance(creation_time_value, str):
        try:
            metadata.date_taken = datetime.fromisoformat(creation_time_value.replace("Z", "+00:00"))
        except ValueError:
            metadata.date_taken = None

    if metadata.date_taken is None:
        metadata.date_taken = _fallback_to_mtime(file_path)

    location_value = tag_data.get("location") or tag_data.get("com.apple.quicktime.location.ISO6709")
    if isinstance(location_value, str):
        metadata.gps_latitude, metadata.gps_longitude = _parse_iso6709_location(location_value)

    ext = file_path.suffix.lower()
    mime_map = {
        ".mp4": "video/mp4",
        ".mov": "video/quicktime",
        ".avi": "video/x-msvideo",
        ".mkv": "video/x-matroska",
        ".webm": "video/webm",
        ".m4v": "video/x-m4v",
    }
    metadata.mime_type = mime_map.get(ext, "video/mp4")


def _parse_iso6709_location(location: str) -> tuple[Optional[float], Optional[float]]:
    location_value = location.rstrip("/")
    if "+" not in location_value[1:] and "-" not in location_value[1:]:
        return None, None

    lat_str = ""
    lon_str = ""
    for index, character in enumerate(location_value[1:], 1):
        if character in "+-":
            lat_str = location_value[:index]
            lon_str = location_value[index:]
            break

    if not lat_str or not lon_str:
        return None, None

    try:
        lat = float(lat_str)
        lon_end = lon_str.find("+", 1) if "+" in lon_str[1:] else lon_str.find("-", 1)
        if lon_end == -1:
            lon = float(lon_str)
        else:
            lon = float(lon_str[:lon_end])
        return lat, lon
    except ValueError:
        return None, None
