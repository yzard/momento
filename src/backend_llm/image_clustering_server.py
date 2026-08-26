#!/usr/bin/env python3
"""On-demand DINOv2 image embedding runtime for image clustering."""

import argparse
import base64
import json
import math
import sys
from array import array
from dataclasses import dataclass
from http.server import BaseHTTPRequestHandler
from pathlib import Path
from typing import Any

from dynamic_batching import DynamicBatcher
from image_runtime import (
    InvalidImageError,
    ModelHTTPServer,
    create_inference_slots,
    decode_image,
    register_image_decoders,
    resize_for_analysis,
    select_cuda_device,
    serve_until_stopped,
)
from runtime_input import read_runtime_input

EMBEDDING_DIMENSIONS = 768
EMBEDDING_ENCODING = "float32_le"
QUALITY_ANALYSIS_MAXIMUM_SIDE = 512


def encode_float32_le(values):
    embedding = array("f", values)
    if embedding.itemsize != 4:
        raise RuntimeError("runtime float size is not 32 bits")
    if sys.byteorder != "little":
        embedding.byteswap()
    return base64.b64encode(embedding.tobytes()).decode("ascii")


def calculate_perceptual_hash(grayscale_image):
    from PIL import Image

    hash_image = grayscale_image.resize((9, 8), Image.Resampling.LANCZOS)
    pixels = hash_image.tobytes()
    hash_value = 0
    for row in range(8):
        row_offset = row * 9
        for column in range(8):
            hash_value = (hash_value << 1) | int(pixels[row_offset + column] > pixels[row_offset + column + 1])
    return f"{hash_value:016x}"


def calculate_image_metrics(image):
    from PIL import ImageFilter, ImageStat

    grayscale = image.convert("L")
    perceptual_hash = calculate_perceptual_hash(grayscale)
    analysis_grayscale = resize_for_analysis(grayscale, QUALITY_ANALYSIS_MAXIMUM_SIDE)
    brightness = ImageStat.Stat(analysis_grayscale).mean[0]
    exposure_score = 1.0 - abs(brightness - 127.5) / 127.5
    edge_variance = ImageStat.Stat(analysis_grayscale.filter(ImageFilter.FIND_EDGES)).var[0]
    sharpness_score = min(edge_variance / 1000.0, 1.0)
    quality_score = round(max(0.0, min(1.0, 0.7 * sharpness_score + 0.3 * exposure_score)), 6)
    return perceptual_hash, quality_score


def select_device(requested_device, torch_module):
    return select_cuda_device(requested_device, torch_module, "image clustering")


@dataclass(frozen=True)
class PreparedClusteringInput:
    pixel_values: Any
    perceptual_hash: str
    quality_score: float


def create_clustering_responses(
    prepared_inputs: list[PreparedClusteringInput], embedding_values: list[list[float]]
) -> list[dict[str, Any]]:
    if len(embedding_values) != len(prepared_inputs):
        raise RuntimeError("DINOv2 returned a different number of embeddings than prepared inputs")

    responses = []
    for prepared_input, values in zip(prepared_inputs, embedding_values):
        if len(values) != EMBEDDING_DIMENSIONS:
            raise RuntimeError(f"model returned {len(values)} embedding dimensions")
        if not all(math.isfinite(value) for value in values):
            raise RuntimeError("model returned a non-finite embedding")
        responses.append(
            {
                "embedding": encode_float32_le(values),
                "embeddingEncoding": EMBEDDING_ENCODING,
                "embeddingDimensions": EMBEDDING_DIMENSIONS,
                "perceptualHash": prepared_input.perceptual_hash,
                "qualityScore": prepared_input.quality_score,
            }
        )
    return responses


def extract_dinov2_pixel_values(model_inputs):
    if set(model_inputs) != {"pixel_values"}:
        raise RuntimeError("DINOv2 processor returned unsupported model inputs")
    return model_inputs["pixel_values"]


class ImageClusteringRuntime:
    def __init__(
        self,
        model_name: str,
        cache_directory: str,
        requested_device: str,
        cpu_processing_concurrency: int,
        model_concurrency: int,
        model_batch_wait_milliseconds: int,
    ) -> None:
        import torch
        from transformers import AutoImageProcessor, AutoModel

        self.torch = torch
        self.device = select_device(requested_device, torch)
        self.processor = AutoImageProcessor.from_pretrained(
            model_name, cache_dir=cache_directory, local_files_only=True
        )
        self.model = AutoModel.from_pretrained(model_name, cache_dir=cache_directory, local_files_only=True)
        hidden_size = int(self.model.config.hidden_size)
        if hidden_size != EMBEDDING_DIMENSIONS:
            raise RuntimeError(f"model hidden size {hidden_size} does not match {EMBEDDING_DIMENSIONS}")
        self.model.eval().to(self.device)
        self.cpu_processing_slots = create_inference_slots(cpu_processing_concurrency)
        self.model_batcher = DynamicBatcher(
            self._infer_model_batch, model_concurrency, model_batch_wait_milliseconds, "image-clustering-model-batcher"
        )

    def infer(self, image_source: Any) -> dict[str, Any]:
        with self.cpu_processing_slots:
            prepared_input = self._prepare_input(image_source)
        return self.model_batcher.infer([prepared_input])[0]

    def close(self) -> None:
        self.model_batcher.close()

    def _prepare_input(self, image_source: Any) -> PreparedClusteringInput:
        image = decode_image(image_source)
        model_inputs = self.processor(images=image, return_tensors="pt")
        perceptual_hash, quality_score = calculate_image_metrics(image)
        return PreparedClusteringInput(
            pixel_values=extract_dinov2_pixel_values(model_inputs),
            perceptual_hash=perceptual_hash,
            quality_score=quality_score,
        )

    def _infer_model_batch(self, prepared_inputs: list[PreparedClusteringInput]) -> list[dict[str, Any]]:
        pixel_values = self.torch.cat([prepared_input.pixel_values for prepared_input in prepared_inputs], dim=0).to(
            self.device
        )
        with self.torch.inference_mode():
            output = self.model(pixel_values=pixel_values)
            embedding = output.last_hidden_state[:, 0, :]
            embedding = self.torch.nn.functional.normalize(embedding, p=2, dim=1)
            embedding_values = embedding.to(dtype=self.torch.float32, device="cpu").tolist()

        return create_clustering_responses(prepared_inputs, embedding_values)


class Handler(BaseHTTPRequestHandler):
    runtime = None
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
    parser.add_argument("--cpu-processing-concurrency", type=int, required=True)
    parser.add_argument("--model-concurrency", type=int, required=True)
    parser.add_argument("--model-batch-wait-milliseconds", type=int, required=True)
    parser.add_argument("--input-root", required=True)
    arguments = parser.parse_args()
    if arguments.cpu_processing_concurrency <= 0:
        parser.error("--cpu-processing-concurrency must be positive")
    if arguments.model_concurrency <= 0:
        parser.error("--model-concurrency must be positive")
    if arguments.model_batch_wait_milliseconds < 0:
        parser.error("--model-batch-wait-milliseconds must not be negative")

    register_image_decoders()
    Handler.runtime = ImageClusteringRuntime(
        arguments.model,
        arguments.cache_dir,
        arguments.device,
        arguments.cpu_processing_concurrency,
        arguments.model_concurrency,
        arguments.model_batch_wait_milliseconds,
    )
    Handler.input_root = Path(arguments.input_root)
    try:
        serve_until_stopped(ModelHTTPServer((arguments.host, arguments.port), Handler))
    finally:
        Handler.runtime.close()


if __name__ == "__main__":
    main()
