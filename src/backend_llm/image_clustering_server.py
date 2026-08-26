#!/usr/bin/env python3
"""On-demand DINOv2 image embedding runtime for image clustering."""

import argparse
import base64
import json
import math
import sys
from array import array
from http.server import BaseHTTPRequestHandler
from pathlib import Path

from image_runtime import (
    InvalidImageError,
    ModelHTTPServer,
    create_inference_slots,
    decode_image,
    register_image_decoders,
    select_cuda_device,
    serve_until_stopped,
)
from runtime_input import read_runtime_input


EMBEDDING_DIMENSIONS = 384
EMBEDDING_ENCODING = "float32_le"


def encode_float32_le(values):
    embedding = array("f", values)
    if embedding.itemsize != 4:
        raise RuntimeError("runtime float size is not 32 bits")
    if sys.byteorder != "little":
        embedding.byteswap()
    return base64.b64encode(embedding.tobytes()).decode("ascii")


def calculate_perceptual_hash(image):
    from PIL import Image

    grayscale = image.convert("L").resize((9, 8), Image.Resampling.LANCZOS)
    pixels = grayscale.tobytes()
    hash_value = 0
    for row in range(8):
        row_offset = row * 9
        for column in range(8):
            hash_value = (hash_value << 1) | int(
                pixels[row_offset + column] > pixels[row_offset + column + 1]
            )
    return f"{hash_value:016x}"


def calculate_quality_score(image):
    from PIL import ImageFilter, ImageStat

    grayscale = image.convert("L")
    brightness = ImageStat.Stat(grayscale).mean[0]
    exposure_score = 1.0 - abs(brightness - 127.5) / 127.5
    edge_variance = ImageStat.Stat(grayscale.filter(ImageFilter.FIND_EDGES)).var[0]
    sharpness_score = min(edge_variance / 1000.0, 1.0)
    return round(max(0.0, min(1.0, 0.7 * sharpness_score + 0.3 * exposure_score)), 6)


def select_device(requested_device, torch_module):
    return select_cuda_device(requested_device, torch_module, "image clustering")


class ImageClusteringRuntime:
    def __init__(self, model_name, cache_directory, requested_device):
        import torch
        from transformers import AutoImageProcessor, AutoModel

        self.torch = torch
        self.device = select_device(requested_device, torch)
        self.processor = AutoImageProcessor.from_pretrained(
            model_name, cache_dir=cache_directory, local_files_only=True
        )
        self.model = AutoModel.from_pretrained(
            model_name, cache_dir=cache_directory, local_files_only=True
        )
        hidden_size = int(self.model.config.hidden_size)
        if hidden_size != EMBEDDING_DIMENSIONS:
            raise RuntimeError(
                f"model hidden size {hidden_size} does not match {EMBEDDING_DIMENSIONS}"
            )
        self.model.eval().to(self.device)

    def infer(self, image_bytes):
        image = decode_image(image_bytes)

        model_inputs = self.processor(images=image, return_tensors="pt")
        model_inputs = {
            name: tensor.to(self.device) for name, tensor in model_inputs.items()
        }
        with self.torch.inference_mode():
            output = self.model(**model_inputs)
            embedding = output.last_hidden_state[:, 0, :]
            embedding = self.torch.nn.functional.normalize(embedding, p=2, dim=1)
            embedding = embedding[0].to(dtype=self.torch.float32, device="cpu")

        values = embedding.tolist()
        if len(values) != EMBEDDING_DIMENSIONS:
            raise RuntimeError(f"model returned {len(values)} embedding dimensions")
        if not all(math.isfinite(value) for value in values):
            raise RuntimeError("model returned a non-finite embedding")

        return {
            "embedding": encode_float32_le(values),
            "embeddingEncoding": EMBEDDING_ENCODING,
            "embeddingDimensions": EMBEDDING_DIMENSIONS,
            "perceptualHash": calculate_perceptual_hash(image),
            "qualityScore": calculate_quality_score(image),
        }


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
                response = self.runtime.infer(image_source)
        except InvalidImageError as error:
            self.send_json(400, {"detail": str(error)})
            return
        except (OSError, ValueError, json.JSONDecodeError) as error:
            self.send_json(400, {"detail": f"invalid request: {error}"})
            return
        except RuntimeError as error:
            self.send_json(500, {"detail": str(error)})
            return
        self.send_json(200, response)

    def log_message(self, message_format, *args):
        return

    def send_json(self, status, payload):
        body = json.dumps(payload, allow_nan=False, separators=(",", ":")).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True)
    parser.add_argument("--cache-dir", required=True)
    parser.add_argument("--device", required=True)
    parser.add_argument("--host", required=True)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--max-concurrent-jobs", type=int, required=True)
    parser.add_argument("--input-root", required=True)
    arguments = parser.parse_args()
    if arguments.max_concurrent_jobs <= 0:
        parser.error("--max-concurrent-jobs must be positive")

    register_image_decoders()
    Handler.runtime = ImageClusteringRuntime(
        arguments.model, arguments.cache_dir, arguments.device
    )
    Handler.inference_slots = create_inference_slots(arguments.max_concurrent_jobs)
    Handler.input_root = Path(arguments.input_root)
    server = ModelHTTPServer((arguments.host, arguments.port), Handler)
    serve_until_stopped(server)

if __name__ == "__main__":
    main()
