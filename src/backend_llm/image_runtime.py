"""Shared primitives for managed image model runtimes."""

import threading
import warnings
from http.server import ThreadingHTTPServer


class InvalidImageError(ValueError):
    """The runtime input is not a readable image."""


NATIVE_INFERENCE_IMAGE_FORMATS = frozenset({"GIF", "QOI", "TIFF", "WEBP"})
MAXIMUM_DECODED_IMAGE_PIXELS = 200_000_000


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
    from PIL import Image
    from pillow_heif import register_heif_opener

    Image.init()
    missing_formats = NATIVE_INFERENCE_IMAGE_FORMATS.difference(Image.OPEN)
    if missing_formats:
        missing = ", ".join(sorted(missing_formats))
        raise RuntimeError(f"Pillow is missing required image decoders: {missing}")
    register_heif_opener(thumbnails=False)


def decode_image(image_source):
    from PIL import Image, ImageFile, ImageOps, UnidentifiedImageError

    ImageFile.LOAD_TRUNCATED_IMAGES = True
    Image.MAX_IMAGE_PIXELS = MAXIMUM_DECODED_IMAGE_PIXELS
    try:
        with warnings.catch_warnings():
            warnings.simplefilter("error", Image.DecompressionBombWarning)
            with Image.open(image_source) as source:
                # Animated images and multi-page TIFF files have one deterministic
                # inference input: their first fully rendered frame/page.
                source.seek(0)
                source.load()
                return ImageOps.exif_transpose(source).convert("RGB")
    except (
        Image.DecompressionBombError,
        Image.DecompressionBombWarning,
        OSError,
        UnidentifiedImageError,
        ValueError,
    ) as error:
        raise InvalidImageError(f"could not decode image: {error}") from error


def resize_for_analysis(image, maximum_side_length):
    from PIL import Image

    if maximum_side_length <= 0:
        raise ValueError("analysis maximum_side_length must be positive")
    resized_image = image.copy()
    resized_image.thumbnail((maximum_side_length, maximum_side_length), Image.Resampling.LANCZOS)
    return resized_image


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
