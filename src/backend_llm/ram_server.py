#!/usr/bin/env python3
"""Small HTTP adapter for Recognize Anything Model++ image tagging."""

import argparse
import json
import shutil
import threading
import warnings
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from io import BytesIO
from pathlib import Path
from urllib.request import urlopen

import torch
from PIL import Image, ImageFile

warnings.filterwarnings("ignore", category=FutureWarning, module=r"fairscale\..*")

from ram import get_transform, inference_ram
from ram.models import ram_plus
from runtime_input import read_runtime_input

RAM_PLUS_CHECKPOINT_URL = (
    "https://huggingface.co/xinyu1205/recognize-anything-plus-model/"
    "resolve/main/ram_plus_swin_large_14m.pth"
)

ImageFile.LOAD_TRUNCATED_IMAGES = True


class InvalidImageError(ValueError):
    """The request body is not a readable image."""


def create_inference_slots(max_concurrent_jobs):
    if max_concurrent_jobs <= 0:
        raise ValueError("max_concurrent_jobs must be positive")
    return threading.BoundedSemaphore(max_concurrent_jobs)


def parse_tags(raw_tags):
    return [tag.strip() for tag in raw_tags.split(" | ") if tag.strip()]


def ensure_checkpoint(checkpoint):
    path = Path(checkpoint)
    if path.exists():
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary_path = path.with_suffix(f"{path.suffix}.download")
    with urlopen(RAM_PLUS_CHECKPOINT_URL) as response, temporary_path.open("wb") as output:
        shutil.copyfileobj(response, output)
    temporary_path.replace(path)


class TaggingRuntime:
    def __init__(self, checkpoint, image_size, device):
        self.device = select_device(device)
        self.transform = get_transform(image_size=image_size)
        self.model = ram_plus(pretrained=checkpoint, image_size=image_size, vit="swin_l")
        self.model.eval().to(self.device)

    def infer(self, image_bytes):
        try:
            with Image.open(BytesIO(image_bytes)) as source:
                source.load()
                image = source.convert("RGB")
        except (OSError, ValueError) as error:
            raise InvalidImageError(f"could not decode image: {error}") from error
        tensor = self.transform(image).unsqueeze(0).to(self.device)
        tags, _ = inference_ram(tensor, self.model)
        return parse_tags(tags)


def select_device(requested):
    if not requested.startswith("cuda"):
        raise RuntimeError("image tagging requires a CUDA device")
    if not torch.cuda.is_available():
        raise RuntimeError("image tagging requires an available NVIDIA CUDA GPU")
    return torch.device(requested)


class Handler(BaseHTTPRequestHandler):
    runtime = None
    inference_slots = None
    input_root = None

    def do_GET(self):
        if self.path != "/ready":
            self.send_error(404)
            return
        self.send_json(200, {"status": "ready"})

    def do_POST(self):
        if self.path != "/infer":
            self.send_error(404)
            return
        with self.inference_slots:
            self.handle_inference()

    def handle_inference(self):
        try:
            image = read_runtime_input(self, self.input_root)
        except (OSError, ValueError, json.JSONDecodeError) as error:
            self.send_json(400, {"detail": f"invalid runtime input: {error}"})
            return
        try:
            tags = self.runtime.infer(image)
        except InvalidImageError as error:
            self.send_json(400, {"detail": str(error)})
            return
        self.send_json(200, {"tags": tags})

    def log_message(self, format, *args):
        return

    def send_json(self, status, payload):
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


class ModelHTTPServer(ThreadingHTTPServer):
    daemon_threads = True
    request_queue_size = 1024


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--checkpoint", required=True)
    parser.add_argument("--image-size", type=int, default=384)
    parser.add_argument("--device", default="auto")
    parser.add_argument("--host", default="0.0.0.0")
    parser.add_argument("--port", type=int, default=8200)
    parser.add_argument("--max-concurrent-jobs", type=int, required=True)
    parser.add_argument("--input-root", required=True)
    args = parser.parse_args()
    if args.max_concurrent_jobs <= 0:
        parser.error("--max-concurrent-jobs must be positive")

    ensure_checkpoint(args.checkpoint)
    Handler.runtime = TaggingRuntime(args.checkpoint, args.image_size, args.device)
    Handler.inference_slots = create_inference_slots(args.max_concurrent_jobs)
    Handler.input_root = Path(args.input_root)
    server = ModelHTTPServer((args.host, args.port), Handler)
    server.serve_forever()


if __name__ == "__main__":
    main()
