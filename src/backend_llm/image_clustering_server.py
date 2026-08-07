#!/usr/bin/env python3
"""On-demand DINOv2 image embedding runtime for image clustering."""

import argparse
import base64
import binascii
import json
import math
import sys
from array import array
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from io import BytesIO


EMBEDDING_DIMENSIONS = 384
EMBEDDING_ENCODING = "float32_le"
MAX_REQUEST_BYTES = 50 * 1024 * 1024


class InvalidImageError(ValueError):
    """The request body is not a readable image."""


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
    if requested_device != "auto":
        return torch_module.device(requested_device)
    if torch_module.cuda.is_available():
        return torch_module.device("cuda")
    if hasattr(torch_module, "xpu") and torch_module.xpu.is_available():
        return torch_module.device("xpu")
    return torch_module.device("cpu")


def decode_image(image_bytes):
    from PIL import Image, ImageFile, ImageOps, UnidentifiedImageError

    ImageFile.LOAD_TRUNCATED_IMAGES = True
    try:
        with Image.open(BytesIO(image_bytes)) as source:
            source.load()
            return ImageOps.exif_transpose(source).convert("RGB")
    except (OSError, UnidentifiedImageError, ValueError) as error:
        raise InvalidImageError(f"could not decode image: {error}") from error


class ImageClusteringRuntime:
    def __init__(self, model_name, cache_directory, requested_device):
        import torch
        from transformers import AutoImageProcessor, AutoModel

        self.torch = torch
        self.device = select_device(requested_device, torch)
        self.processor = AutoImageProcessor.from_pretrained(
            model_name, cache_dir=cache_directory
        )
        self.model = AutoModel.from_pretrained(model_name, cache_dir=cache_directory)
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

    def do_GET(self):
        if self.path != "/ready":
            self.send_error(404)
            return
        self.send_json(200, {"status": "ready"})

    def do_POST(self):
        if self.path != "/infer":
            self.send_error(404)
            return

        try:
            content_length = int(self.headers.get("Content-Length", "0"))
            if content_length <= 0 or content_length > MAX_REQUEST_BYTES:
                raise ValueError(
                    f"Content-Length must be between 1 and {MAX_REQUEST_BYTES}"
                )
            request = json.loads(self.rfile.read(content_length))
            image_bytes = base64.b64decode(request["image"], validate=True)
        except (ValueError, json.JSONDecodeError, KeyError, binascii.Error) as error:
            self.send_json(400, {"detail": f"invalid request: {error}"})
            return
        if not image_bytes:
            self.send_json(400, {"detail": "image must not be empty"})
            return

        try:
            response = self.runtime.infer(image_bytes)
        except InvalidImageError as error:
            self.send_json(400, {"detail": str(error)})
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
    arguments = parser.parse_args()

    Handler.runtime = ImageClusteringRuntime(
        arguments.model, arguments.cache_dir, arguments.device
    )
    server = ThreadingHTTPServer((arguments.host, arguments.port), Handler)
    serve_until_stopped(server)


def serve_until_stopped(server):
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
