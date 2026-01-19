import posixpath
import threading
from dataclasses import dataclass, field
from datetime import datetime
from enum import Enum
from pathlib import Path, PurePosixPath
from typing import Optional

from webdav3.client import Client

from momento_api.config import ReverseGeocodingConfig
from momento_api.constants import IMPORTS_DIR, SUPPORTED_EXTENSIONS
from momento_api.processor.media_processor import process_media_file


class ImportStatus(str, Enum):
    IDLE = "idle"
    RUNNING = "running"
    COMPLETED = "completed"
    FAILED = "failed"


@dataclass
class ImportJob:
    status: ImportStatus = ImportStatus.IDLE
    total_files: int = 0
    processed_files: int = 0
    successful_imports: int = 0
    failed_imports: int = 0
    started_at: Optional[datetime] = None
    completed_at: Optional[datetime] = None
    errors: list[str] = field(default_factory=list)


_current_job: Optional[ImportJob] = None
_job_lock = threading.Lock()


def get_import_status() -> ImportJob:
    with _job_lock:
        if _current_job is None:
            return ImportJob()
        return ImportJob(
            status=_current_job.status,
            total_files=_current_job.total_files,
            processed_files=_current_job.processed_files,
            successful_imports=_current_job.successful_imports,
            failed_imports=_current_job.failed_imports,
            started_at=_current_job.started_at,
            completed_at=_current_job.completed_at,
            errors=list(_current_job.errors),
        )


def is_import_running() -> bool:
    with _job_lock:
        return _current_job is not None and _current_job.status == ImportStatus.RUNNING


def _start_import_job() -> ImportJob:
    global _current_job

    with _job_lock:
        if _current_job is not None and _current_job.status == ImportStatus.RUNNING:
            return _current_job
        _current_job = ImportJob(status=ImportStatus.RUNNING, started_at=datetime.now())
        return _current_job


def _finalize_job_success() -> None:
    with _job_lock:
        if _current_job is None:
            return
        _current_job.status = ImportStatus.COMPLETED
        _current_job.completed_at = datetime.now()


def _finalize_job_failure(message: str) -> None:
    with _job_lock:
        if _current_job is None:
            return
        _current_job.status = ImportStatus.FAILED
        _current_job.completed_at = datetime.now()
        _current_job.errors.append(message)


def _update_job_totals(total_files: int) -> None:
    with _job_lock:
        if _current_job is None:
            return
        _current_job.total_files = total_files


def _update_job_progress(success: bool, error_message: Optional[str]) -> None:
    with _job_lock:
        if _current_job is None:
            return
        _current_job.processed_files += 1
        if success:
            _current_job.successful_imports += 1
            return
        _current_job.failed_imports += 1
        if error_message:
            _current_job.errors.append(error_message)


def _collect_import_files(root: Path) -> list[Path]:
    files_to_import: list[Path] = []
    for ext in SUPPORTED_EXTENSIONS:
        files_to_import.extend(root.glob(f"*{ext}"))
        files_to_import.extend(root.glob(f"*{ext.upper()}"))
        files_to_import.extend(root.glob(f"**/*{ext}"))
        files_to_import.extend(root.glob(f"**/*{ext.upper()}"))

    return list(set(files_to_import))


def run_local_import(
    user_id: int,
    thumbnail_max_size: int,
    thumbnail_quality: int,
    video_frame_quality: int,
    delete_after_import: bool,
    reverse_geo_config: Optional[ReverseGeocodingConfig] = None,
) -> None:
    _start_import_job()

    try:
        files_to_import = _collect_import_files(IMPORTS_DIR)
        _update_job_totals(len(files_to_import))

        for file_path in files_to_import:
            if not file_path.exists():
                _update_job_progress(success=False, error_message=f"Missing file: {file_path.name}")
                continue

            media_id = process_media_file(
                file_path, user_id, thumbnail_max_size, thumbnail_quality, video_frame_quality, reverse_geo_config
            )

            if media_id is None:
                _update_job_progress(success=False, error_message=f"Failed to process: {file_path.name}")
                continue

            if delete_after_import:
                try:
                    file_path.unlink(missing_ok=True)
                except OSError as error:
                    _update_job_progress(success=False, error_message=f"Failed to delete {file_path.name}: {error}")
                    continue

            _update_job_progress(success=True, error_message=None)

        _finalize_job_success()

    except (OSError, ValueError) as error:
        _finalize_job_failure(f"Import failed: {error}")


def _ensure_webdav_path(path: str) -> str:
    if not path:
        return "/"
    normalized = posixpath.normpath(path)
    if normalized == ".":
        normalized = "/"
    if not normalized.startswith("/"):
        normalized = f"/{normalized}"
    return normalized


def _list_webdav_files(client: Client, remote_path: str) -> list[str]:
    try:
        return client.list(remote_path)
    except (OSError, ValueError):
        return []


def _collect_webdav_files(client: Client, remote_path: str) -> list[str]:
    stack = [remote_path]
    files: list[str] = []

    while stack:
        current = stack.pop()
        entries = _list_webdav_files(client, current)
        for entry in entries:
            if entry in (".", ".."):
                continue
            full_path = posixpath.join(current, entry)
            if full_path.endswith("/"):
                stack.append(full_path.rstrip("/"))
            else:
                files.append(full_path)

    return files


def _filter_supported_remote_files(files: list[str]) -> list[str]:
    supported: list[str] = []
    for file_path in files:
        ext = PurePosixPath(file_path).suffix.lower()
        if ext in SUPPORTED_EXTENSIONS:
            supported.append(file_path)
    return supported


def run_webdav_import(
    user_id: int,
    hostname: str,
    username: str,
    password: str,
    remote_path: str,
    thumbnail_max_size: int,
    thumbnail_quality: int,
    video_frame_quality: int,
    reverse_geo_config: Optional[ReverseGeocodingConfig] = None,
) -> None:
    _start_import_job()

    try:
        options = {"webdav_hostname": hostname, "webdav_login": username, "webdav_password": password}
        client = Client(options)
        base_path = _ensure_webdav_path(remote_path)
        remote_files = _collect_webdav_files(client, base_path)
        supported_files = _filter_supported_remote_files(remote_files)
        _update_job_totals(len(supported_files))

        for remote_file in supported_files:
            local_target = IMPORTS_DIR / PurePosixPath(remote_file).name
            try:
                client.download_sync(remote_path=remote_file, local_path=str(local_target))
            except OSError as error:
                _update_job_progress(success=False, error_message=f"Failed to download {remote_file}: {error}")
                continue

            media_id = process_media_file(
                local_target, user_id, thumbnail_max_size, thumbnail_quality, video_frame_quality, reverse_geo_config
            )

            if media_id is None:
                _update_job_progress(success=False, error_message=f"Failed to process: {remote_file}")
                continue

            try:
                local_target.unlink(missing_ok=True)
            except OSError as error:
                _update_job_progress(success=False, error_message=f"Failed to delete {remote_file}: {error}")
                continue

            _update_job_progress(success=True, error_message=None)

        _finalize_job_success()

    except (OSError, ValueError) as error:
        _finalize_job_failure(f"WebDAV import failed: {error}")
