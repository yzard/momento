"""Shared primitives for managed image model runtimes."""

import threading
from http.server import ThreadingHTTPServer


class InvalidImageError(ValueError):
    """The runtime input is not a readable image."""


def create_inference_slots(max_concurrent_jobs):
    if max_concurrent_jobs <= 0:
        raise ValueError("max_concurrent_jobs must be positive")
    return threading.BoundedSemaphore(max_concurrent_jobs)


def select_cuda_device(requested_device, torch_module, task_name):
    if not requested_device.startswith("cuda"):
        raise RuntimeError(f"{task_name} requires a CUDA device")
    if not torch_module.cuda.is_available():
        raise RuntimeError(f"{task_name} requires an available NVIDIA CUDA GPU")
    return torch_module.device(requested_device)


def register_image_decoders():
    from pillow_heif import register_heif_opener

    register_heif_opener(thumbnails=False)


def decode_image(image_source):
    from PIL import Image, ImageFile, ImageOps, UnidentifiedImageError

    ImageFile.LOAD_TRUNCATED_IMAGES = True
    try:
        with Image.open(image_source) as source:
            source.load()
            return ImageOps.exif_transpose(source).convert("RGB")
    except (OSError, UnidentifiedImageError, ValueError) as error:
        raise InvalidImageError(f"could not decode image: {error}") from error


class ModelHTTPServer(ThreadingHTTPServer):
    daemon_threads = True
    request_queue_size = 1024


def serve_until_stopped(server):
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
