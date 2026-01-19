import json
import threading
import time
from dataclasses import dataclass, field
from datetime import datetime
from enum import Enum
from pathlib import Path
from typing import Optional
from urllib.request import Request, urlopen
from urllib.parse import urlencode

from momento_api.config import Config
from momento_api.constants import ORIGINALS_DIR, THUMBNAILS_DIR
from momento_api.database import execute_query, fetch_all, fetch_one, get_connection
from momento_api.processor.media_processor import generate_complete_metadata


__all__ = [
    "RegenerationStatus",
    "RegenerationJob",
    "get_regeneration_status",
    "is_regeneration_running",
    "run_regeneration",
    "clear_all_metadata_and_thumbnails",
    "cancel_regeneration",
]
from momento_api.processor.thumbnails import generate_image_thumbnail, generate_video_thumbnail


class RegenerationStatus(str, Enum):
    IDLE = "idle"
    RUNNING = "running"
    COMPLETED = "completed"
    FAILED = "failed"
    CANCELLED = "cancelled"


@dataclass
class RegenerationJob:
    status: RegenerationStatus = RegenerationStatus.IDLE
    total_media: int = 0
    processed_media: int = 0
    updated_metadata: int = 0
    generated_thumbnails: int = 0
    updated_tags: int = 0
    started_at: Optional[datetime] = None
    completed_at: Optional[datetime] = None
    errors: list[str] = field(default_factory=list)


_current_job: Optional[RegenerationJob] = None
_job_lock = threading.Lock()
_cancel_requested = False


def get_regeneration_status() -> RegenerationJob:
    with _job_lock:
        if _current_job is None:
            return RegenerationJob()
        return RegenerationJob(
            status=_current_job.status,
            total_media=_current_job.total_media,
            processed_media=_current_job.processed_media,
            updated_metadata=_current_job.updated_metadata,
            generated_thumbnails=_current_job.generated_thumbnails,
            updated_tags=_current_job.updated_tags,
            started_at=_current_job.started_at,
            completed_at=_current_job.completed_at,
            errors=list(_current_job.errors),
        )


def is_regeneration_running() -> bool:
    with _job_lock:
        return _current_job is not None and _current_job.status == RegenerationStatus.RUNNING


def cancel_regeneration() -> bool:
    """Request cancellation of the current regeneration job."""
    global _cancel_requested
    with _job_lock:
        if _current_job is None or _current_job.status != RegenerationStatus.RUNNING:
            return False
        _cancel_requested = True
        return True


def _is_cancel_requested() -> bool:
    with _job_lock:
        return _cancel_requested


def _clear_cancel_request() -> None:
    global _cancel_requested
    with _job_lock:
        _cancel_requested = False


def _start_job() -> RegenerationJob:
    global _current_job
    with _job_lock:
        if _current_job is not None and _current_job.status == RegenerationStatus.RUNNING:
            return _current_job
        _current_job = RegenerationJob(status=RegenerationStatus.RUNNING, started_at=datetime.now())
        return _current_job


def _finalize_job_success() -> None:
    with _job_lock:
        if _current_job is None:
            return
        _current_job.status = RegenerationStatus.COMPLETED
        _current_job.completed_at = datetime.now()


def _finalize_job_failure(message: str) -> None:
    with _job_lock:
        if _current_job is None:
            return
        _current_job.status = RegenerationStatus.FAILED
        _current_job.completed_at = datetime.now()
        _current_job.errors.append(message)


def _finalize_job_cancelled() -> None:
    with _job_lock:
        if _current_job is None:
            return
        _current_job.status = RegenerationStatus.CANCELLED
        _current_job.completed_at = datetime.now()


def _update_job_totals(total_media: int) -> None:
    with _job_lock:
        if _current_job is None:
            return
        _current_job.total_media = total_media


def _update_job_progress(
    metadata_updated: bool, thumbnail_generated: bool, tags_updated: int, error: Optional[str]
) -> None:
    with _job_lock:
        if _current_job is None:
            return
        _current_job.processed_media += 1
        if metadata_updated:
            _current_job.updated_metadata += 1
        if thumbnail_generated:
            _current_job.generated_thumbnails += 1
        if tags_updated:
            _current_job.updated_tags += tags_updated
        if error:
            _current_job.errors.append(error)


def _reverse_geocode(config: Config, latitude: float, longitude: float) -> tuple[Optional[str], Optional[str]]:
    reverse_config = config.reverse_geocoding
    if not reverse_config.enabled:
        return None, None

    params = {"format": "json", "lat": f"{latitude}", "lon": f"{longitude}", "zoom": "10", "addressdetails": "1"}
    url = f"{reverse_config.base_url}?{urlencode(params)}"
    headers = {"User-Agent": reverse_config.user_agent}

    request = Request(url, headers=headers)
    try:
        with urlopen(request, timeout=reverse_config.timeout_seconds) as response:
            payload = json.loads(response.read().decode("utf-8"))
    except (OSError, ValueError, json.JSONDecodeError):
        return None, None

    address = payload.get("address") if isinstance(payload, dict) else None
    if not isinstance(address, dict):
        return None, None

    state = address.get("state") or address.get("region") or address.get("province")
    country = address.get("country")
    return state, country


def _merge_keyword_tags(media_id: int, keywords: Optional[str]) -> int:
    if not keywords:
        return 0

    raw_tags = [tag.strip() for tag in keywords.split(",")]
    tags = [tag for tag in raw_tags if tag]
    if not tags:
        return 0

    conn = get_connection()
    inserted_count = 0
    for tag in tags:
        existing = fetch_one("SELECT id FROM tags WHERE name = ?", (tag,))
        if existing:
            tag_id = existing["id"]
        else:
            conn.execute("INSERT INTO tags (name) VALUES (?)", (tag,))
            tag_id = conn.execute("SELECT last_insert_rowid()").fetchone()[0]
        conn.execute("INSERT OR IGNORE INTO media_tags (media_id, tag_id) VALUES (?, ?)", (media_id, tag_id))
        inserted_count += 1

    conn.commit()
    return inserted_count


def clear_all_metadata_and_thumbnails() -> int:
    """Clear all metadata fields and delete all thumbnail files."""
    import shutil

    rows = fetch_all("SELECT id, thumbnail_path FROM media", ())
    cleared_count = 0

    for row in rows:
        thumbnail_path = row["thumbnail_path"]
        if thumbnail_path:
            thumbnail_file = THUMBNAILS_DIR / thumbnail_path
            try:
                thumbnail_file.unlink(missing_ok=True)
            except OSError:
                pass

        execute_query(
            """
            UPDATE media SET
                thumbnail_path = NULL,
                width = NULL,
                height = NULL,
                duration_seconds = NULL,
                date_taken = NULL,
                gps_latitude = NULL,
                gps_longitude = NULL,
                gps_altitude = NULL,
                camera_make = NULL,
                camera_model = NULL,
                iso = NULL,
                exposure_time = NULL,
                f_number = NULL,
                focal_length = NULL,
                location_state = NULL,
                location_country = NULL,
                keywords = NULL
            WHERE id = ?
            """,
            (row["id"],),
        )
        cleared_count += 1

    get_connection().commit()
    return cleared_count


def run_regeneration(missing_only: bool, config: Config) -> None:
    _clear_cancel_request()
    _start_job()
    try:
        rows = fetch_all(
            """
            SELECT id, user_id, file_path, thumbnail_path, media_type, width, height, file_size,
                   duration_seconds, date_taken, gps_latitude, gps_longitude, gps_altitude,
                   camera_make, camera_model, iso, exposure_time, f_number, focal_length,
                   location_state, location_country, keywords
            FROM media
            ORDER BY id
            """,
            (),
        )
        _update_job_totals(len(rows))

        cache: dict[tuple[float, float], tuple[Optional[str], Optional[str]]] = {}

        for row in rows:
            if _is_cancel_requested():
                _finalize_job_cancelled()
                _clear_cancel_request()
                return

            original_path = ORIGINALS_DIR / row["file_path"]
            if not original_path.exists():
                _update_job_progress(False, False, 0, f"Missing file: {row['file_path']}")
                continue

            thumbnail_path = row["thumbnail_path"]
            thumbnail_file = THUMBNAILS_DIR / thumbnail_path if thumbnail_path else None
            thumbnail_missing = thumbnail_path is None or thumbnail_file is None or not thumbnail_file.exists()

            metadata_missing = any(
                row[field] is None
                for field in (
                    "width",
                    "height",
                    "date_taken",
                    "gps_latitude",
                    "gps_longitude",
                    "camera_make",
                    "camera_model",
                    "iso",
                    "exposure_time",
                    "f_number",
                    "focal_length",
                    "gps_altitude",
                    "keywords",
                )
            )

            if missing_only and not metadata_missing and not thumbnail_missing:
                _update_job_progress(False, False, 0, None)
                continue

            if row["media_type"] == "image":
                media_type = "image"
            else:
                media_type = "video"

            metadata = generate_complete_metadata(
                original_path, media_type, config.reverse_geocoding if not missing_only else None
            )

            # If missing_only, we might still want to reverse geocode if coordinates exist but location is missing
            # However, generate_complete_metadata handles reverse geocoding if config is passed.
            # Logic adjustment: if missing_only is True, we only pass config if location is missing.
            # But the helper already checks for missing location before geocoding.
            # So we can safely pass config if we want to enable geocoding.
            # To strictly follow "missing_only" affecting existing fields:
            # The helper fills in location only if it's missing. So it aligns with "missing_only" spirit for location.
            # For other fields, we merge.

            def choose(existing, new_value):
                if missing_only and existing is not None:
                    return existing
                return new_value if new_value is not None else existing

            gps_latitude = choose(row["gps_latitude"], metadata.gps_latitude)
            gps_longitude = choose(row["gps_longitude"], metadata.gps_longitude)

            # Helper already populated location_state/country if possible
            location_state = choose(row["location_state"], metadata.location_state)
            location_country = choose(row["location_country"], metadata.location_country)

            metadata_values = {
                "width": choose(row["width"], metadata.width),
                "height": choose(row["height"], metadata.height),
                "date_taken": metadata.date_taken.isoformat() if metadata.date_taken else row["date_taken"],
                "gps_latitude": gps_latitude,
                "gps_longitude": gps_longitude,
                "gps_altitude": choose(row["gps_altitude"], metadata.gps_altitude),
                "camera_make": choose(row["camera_make"], metadata.camera_make),
                "camera_model": choose(row["camera_model"], metadata.camera_model),
                "iso": choose(row["iso"], metadata.iso),
                "exposure_time": choose(row["exposure_time"], metadata.exposure_time),
                "f_number": choose(row["f_number"], metadata.f_number),
                "focal_length": choose(row["focal_length"], metadata.focal_length),
                "location_state": location_state,
                "location_country": location_country,
                "keywords": choose(row["keywords"], metadata.keywords),
                "duration_seconds": choose(row["duration_seconds"], metadata.duration_seconds),
            }

            update_fields = ", ".join(f"{key} = ?" for key in metadata_values)
            update_params = list(metadata_values.values()) + [row["id"]]
            execute_query(f"UPDATE media SET {update_fields} WHERE id = ?", tuple(update_params))
            get_connection().commit()

            metadata_updated = True
            thumbnail_generated = False

            if not missing_only or thumbnail_missing:
                thumbnail_relative = (
                    Path(thumbnail_path)
                    if thumbnail_path
                    else Path(str(row["user_id"])) / f"{Path(row['file_path']).stem}.jpg"
                )
                thumbnail_output = THUMBNAILS_DIR / thumbnail_relative
                if row["media_type"] == "image":
                    thumbnail_generated = generate_image_thumbnail(
                        original_path, thumbnail_output, config.thumbnails.max_size, config.thumbnails.quality
                    )
                else:
                    thumbnail_generated = generate_video_thumbnail(
                        original_path,
                        thumbnail_output,
                        config.thumbnails.max_size,
                        config.thumbnails.quality,
                        config.thumbnails.video_frame_quality,
                    )

                if thumbnail_generated:
                    execute_query(
                        "UPDATE media SET thumbnail_path = ? WHERE id = ?", (str(thumbnail_relative), row["id"])
                    )
                    get_connection().commit()

            tags_updated = _merge_keyword_tags(row["id"], metadata_values["keywords"])

            _update_job_progress(metadata_updated, thumbnail_generated, tags_updated, None)

        _finalize_job_success()

    except (OSError, ValueError) as error:
        _finalize_job_failure(f"Regeneration failed: {error}")
