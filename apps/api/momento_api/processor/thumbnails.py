import subprocess
from pathlib import Path


def _run_command(cmd: list[str], timeout: int = 60, capture_output: bool = True) -> bool:
    try:
        subprocess.run(
            cmd, check=True, capture_output=capture_output, text=True if capture_output else False, timeout=timeout
        )
        return True
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired, OSError):
        return False


def _get_video_duration(source_path: Path) -> float:
    try:
        result = subprocess.run(
            [
                "ffprobe",
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "default=noprint_wrappers=1:nokey=1",
                str(source_path),
            ],
            capture_output=True,
            text=True,
            timeout=30,
            check=True,
        )
        stdout = result.stdout
        return float(stdout.strip()) if stdout.strip() else 0
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired, OSError, ValueError):
        return 0


def generate_image_thumbnail(source_path: Path, output_path: Path, max_size: int, quality: int) -> bool:
    if not _ensure_output_dir(output_path):
        return False

    return _generate_montage_thumbnail(source_path, output_path, max_size, quality)


def generate_video_thumbnail(
    source_path: Path, output_path: Path, max_size: int, quality: int, video_frame_quality: int
) -> bool:
    if not _ensure_output_dir(output_path):
        return False

    temp_frame = output_path.with_suffix(".temp.jpg")
    if not _extract_video_frame(source_path, temp_frame, video_frame_quality):
        return False

    success = _generate_montage_thumbnail(temp_frame, output_path, max_size, quality)
    try:
        temp_frame.unlink(missing_ok=True)
    except OSError:
        pass

    return success


def generate_image_preview(source_path: Path, output_path: Path, max_size: int, quality: int) -> bool:
    if not _ensure_output_dir(output_path):
        return False

    cmd = [
        "convert",
        str(source_path),
        "-auto-orient",
        "-resize",
        f"{max_size}x{max_size}>",
        "-quality",
        str(quality),
        str(output_path),
    ]

    return _run_command(cmd) and output_path.exists()


def _ensure_output_dir(output_path: Path) -> bool:
    try:
        output_path.parent.mkdir(parents=True, exist_ok=True)
        return True
    except OSError:
        return False


def _generate_montage_thumbnail(source_path: Path, output_path: Path, max_size: int, quality: int) -> bool:
    """Generate a thumbnail scaled to max_size and center-cropped to a square."""
    resized = f"{max_size}x{max_size}"
    cmd = [
        "convert",
        str(source_path),
        "-auto-orient",
        "-thumbnail",
        f"{resized}^",
        "-gravity",
        "center",
        "-extent",
        resized,
        "-quality",
        str(quality),
        str(output_path),
    ]

    return _run_command(cmd) and output_path.exists()


def _extract_video_frame(source_path: Path, output_path: Path, video_frame_quality: int) -> bool:
    duration = _get_video_duration(source_path)
    seek_time = min(duration * 0.1, 5.0) if duration > 0 else 0

    cmd = [
        "ffmpeg",
        "-y",
        "-ss",
        str(seek_time),
        "-i",
        str(source_path),
        "-vframes",
        "1",
        "-q:v",
        str(video_frame_quality),
        str(output_path),
    ]

    return _run_command(cmd) and output_path.exists()
