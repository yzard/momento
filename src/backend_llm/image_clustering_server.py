#!/usr/bin/env python3
"""On-demand DINOv2 image embedding runtime for image clustering."""

import argparse
import base64
import math
import sys
from array import array
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from dynamic_batching import DynamicBatcher
from image_runtime import (
    ModelHTTPServer,
    create_inference_slots,
    decode_image,
    register_image_decoders,
    resize_for_analysis,
    select_cuda_device,
    serve_until_stopped,
)
from runtime_http import (
    ImageRuntimeRequestHandler,
    add_batched_image_runtime_arguments,
    validate_batched_image_runtime_arguments,
)

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
        processing_concurrency: int,
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
        self.processing_slots = create_inference_slots(processing_concurrency)
        self.model_batcher = DynamicBatcher(
            self._infer_model_batch, model_concurrency, model_batch_wait_milliseconds, "image-clustering-model-batcher"
        )

    def infer(self, image_source: Any) -> dict[str, Any]:
        with self.processing_slots:
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


class Handler(ImageRuntimeRequestHandler):
    pass


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True)
    parser.add_argument("--cache-dir", required=True)
    add_batched_image_runtime_arguments(parser)
    arguments = parser.parse_args()
    validate_batched_image_runtime_arguments(parser, arguments)

    register_image_decoders()
    Handler.runtime = ImageClusteringRuntime(
        arguments.model,
        arguments.cache_dir,
        arguments.device,
        arguments.processing_concurrency,
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
