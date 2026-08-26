#!/usr/bin/env python3
"""Small HTTP adapter for Recognize Anything Model++ image tagging."""

import argparse
import json
import warnings
from http.server import BaseHTTPRequestHandler
from pathlib import Path

import torch

warnings.filterwarnings("ignore", category=FutureWarning, module=r"fairscale\..*")

from ram import get_transform, inference_ram
from ram.models import ram_plus
from image_runtime import (
    InvalidImageError,
    ModelHTTPServer,
    create_inference_slots,
    decode_image,
    register_image_decoders,
    serve_until_stopped,
)
from runtime_input import read_runtime_input


def parse_tags(raw_tags):
    return [tag.strip() for tag in raw_tags.split(" | ") if tag.strip()]


def require_checkpoint(checkpoint):
    path = Path(checkpoint)
    if not path.is_file():
        raise RuntimeError(f"RAM++ checkpoint is missing: {path}")
    return path


class TaggingRuntime:
    def __init__(self, checkpoint, image_size, device):
        self.device = select_device(device)
        self.transform = get_transform(image_size=image_size)
        self.model = ram_plus(pretrained=checkpoint, image_size=image_size, vit="swin_l")
        self.model.eval().to(self.device)

    def infer(self, image_source):
        image = decode_image(image_source)
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
            with read_runtime_input(self, self.input_root) as image_source:
                tags = self.runtime.infer(image_source)
        except InvalidImageError as error:
            self.send_json(400, {"detail": str(error)})
            return
        except (OSError, ValueError, json.JSONDecodeError) as error:
            self.send_json(400, {"detail": f"invalid runtime input: {error}"})
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

    register_image_decoders()
    require_checkpoint(args.checkpoint)
    Handler.runtime = TaggingRuntime(args.checkpoint, args.image_size, args.device)
    Handler.inference_slots = create_inference_slots(args.max_concurrent_jobs)
    Handler.input_root = Path(args.input_root)
    server = ModelHTTPServer((args.host, args.port), Handler)
    serve_until_stopped(server)


if __name__ == "__main__":
    main()
