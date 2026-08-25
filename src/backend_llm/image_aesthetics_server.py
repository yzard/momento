#!/usr/bin/env python3
"""On-demand CLIP and LAION image aesthetics scoring runtime."""

import argparse
import json
import math
from http.server import BaseHTTPRequestHandler
from pathlib import Path

from image_runtime import (
    InvalidImageError,
    ModelHTTPServer,
    create_inference_slots,
    decode_image,
    select_cuda_device,
    serve_until_stopped,
)
from runtime_input import read_runtime_input


CLIP_EMBEDDING_DIMENSIONS = 512
CLIP_IMAGE_MEAN = (0.48145466, 0.4578275, 0.40821073)
CLIP_IMAGE_STANDARD_DEVIATION = (0.26862954, 0.26130258, 0.27577711)
SCENIC_PROMPTS = (
    "a scenic landscape photograph",
    "a beautiful natural vista",
    "a breathtaking outdoor scene",
    "a photograph of impressive scenery",
)
NON_SCENIC_PROMPTS = (
    "an ordinary indoor snapshot",
    "a close-up photograph of an object",
    "a document or screenshot",
    "an unremarkable everyday photograph",
)
AESTHETIC_MINIMUM_RATING = 1.0
AESTHETIC_MAXIMUM_RATING = 10.0
SCENIC_LOGIT_SCALE = 10.0


def require_model_file(model_path, model_name):
    path = Path(model_path)
    if not path.is_file():
        raise RuntimeError(f"{model_name} is missing: {path}")
    return path


def bounded_score(score, score_name):
    score = float(score)
    if not math.isfinite(score):
        raise RuntimeError(f"{score_name} is not finite")
    return round(max(0.0, min(1.0, score)), 6)


def aesthetic_score(raw_rating):
    if not math.isfinite(float(raw_rating)):
        raise RuntimeError("aesthetic model returned a non-finite rating")
    rating_range = AESTHETIC_MAXIMUM_RATING - AESTHETIC_MINIMUM_RATING
    return bounded_score(
        (float(raw_rating) - AESTHETIC_MINIMUM_RATING) / rating_range,
        "aesthetic score",
    )


def landscape_score(image):
    if image.width <= 0 or image.height <= 0:
        raise RuntimeError("decoded image has invalid dimensions")
    aspect_ratio = image.width / image.height
    # Square and portrait inputs score zero; 3:2 and wider inputs score one.
    return bounded_score((aspect_ratio - 1.0) / 0.5, "landscape score")


def simplicity_score(image):
    from PIL import ImageFilter, ImageStat

    grayscale = analysis_image(image).convert("L")
    entropy = float(grayscale.entropy()) / 8.0
    edge_mean = float(ImageStat.Stat(grayscale.filter(ImageFilter.FIND_EDGES)).mean[0]) / 255.0
    if not math.isfinite(entropy) or not math.isfinite(edge_mean):
        raise RuntimeError("simplicity metrics are not finite")
    complexity = (0.45 * min(entropy, 1.0)) + (0.55 * min(edge_mean, 1.0))
    return bounded_score(1.0 - complexity, "simplicity score")


def technical_quality_score(image):
    from PIL import ImageFilter, ImageStat

    grayscale = analysis_image(image).convert("L")
    statistics = ImageStat.Stat(grayscale)
    brightness = float(statistics.mean[0])
    edge_variance = float(ImageStat.Stat(grayscale.filter(ImageFilter.FIND_EDGES)).var[0])
    histogram = grayscale.histogram()
    pixel_count = sum(histogram)
    if pixel_count <= 0:
        raise RuntimeError("technical quality image is empty")
    clipped_pixels = sum(histogram[:6]) + sum(histogram[250:])
    exposure = 1.0 - abs(brightness - 127.5) / 127.5
    sharpness = min(edge_variance / 1000.0, 1.0)
    unclipped = 1.0 - clipped_pixels / pixel_count
    metrics = (brightness, edge_variance, exposure, sharpness, unclipped)
    if not all(math.isfinite(metric) for metric in metrics):
        raise RuntimeError("technical quality metrics are not finite")
    return bounded_score(
        (0.45 * sharpness) + (0.35 * exposure) + (0.20 * unclipped),
        "technical quality score",
    )


def analysis_image(image):
    from PIL import Image

    resized = image.copy()
    resized.thumbnail((512, 512), Image.Resampling.LANCZOS)
    return resized


def aspect_preserving_square(image, resolution):
    from PIL import Image

    if resolution <= 0 or image.width <= 0 or image.height <= 0:
        raise RuntimeError("CLIP input dimensions must be positive")
    scale = min(resolution / image.width, resolution / image.height)
    resized_width = max(1, round(image.width * scale))
    resized_height = max(1, round(image.height * scale))
    resized = image.resize((resized_width, resized_height), Image.Resampling.BICUBIC)
    background = tuple(round(channel * 255.0) for channel in CLIP_IMAGE_MEAN)
    canvas = Image.new("RGB", (resolution, resolution), background)
    canvas.paste(
        resized,
        ((resolution - resized_width) // 2, (resolution - resized_height) // 2),
    )
    return canvas


def prepare_clip_tensor(image, resolution, torch_module):
    from torchvision.transforms.functional import normalize, pil_to_tensor

    canvas = aspect_preserving_square(image, resolution)
    tensor = pil_to_tensor(canvas).to(dtype=torch_module.float32).div(255.0)
    tensor = normalize(tensor, CLIP_IMAGE_MEAN, CLIP_IMAGE_STANDARD_DEVIATION)
    return tensor.unsqueeze(0)


class ImageAestheticsRuntime:
    def __init__(self, clip_model_path, aesthetic_head_path, requested_device):
        import clip
        import torch

        self.torch = torch
        self.device = select_cuda_device(
            requested_device, torch, "image aesthetics"
        )
        clip_model_path = require_model_file(clip_model_path, "CLIP model")
        aesthetic_head_path = require_model_file(
            aesthetic_head_path, "LAION aesthetic head"
        )
        self.model, _ = clip.load(str(clip_model_path), device=self.device, jit=False)
        self.model.eval()
        self.input_resolution = int(self.model.visual.input_resolution)
        self.aesthetic_head = torch.nn.Linear(CLIP_EMBEDDING_DIMENSIONS, 1)
        state = torch.load(aesthetic_head_path, map_location="cpu", weights_only=True)
        self.aesthetic_head.load_state_dict(state, strict=True)
        self.aesthetic_head.eval().to(self.device)
        self.scenic_text_features = self._create_scenic_text_features(clip)

    def _create_scenic_text_features(self, clip_module):
        prompts = SCENIC_PROMPTS + NON_SCENIC_PROMPTS
        tokens = clip_module.tokenize(prompts).to(self.device)
        with self.torch.inference_mode():
            features = self.model.encode_text(tokens).to(dtype=self.torch.float32)
            features = self.torch.nn.functional.normalize(features, p=2, dim=1)
            scenic = features[: len(SCENIC_PROMPTS)].mean(dim=0)
            non_scenic = features[len(SCENIC_PROMPTS) :].mean(dim=0)
            prototypes = self.torch.stack((scenic, non_scenic))
            prototypes = self.torch.nn.functional.normalize(prototypes, p=2, dim=1)
        if not self.torch.isfinite(prototypes).all().item():
            raise RuntimeError("CLIP returned non-finite scenic prompt embeddings")
        return prototypes

    def infer(self, image_bytes):
        image = decode_image(image_bytes)
        model_input = prepare_clip_tensor(
            image, self.input_resolution, self.torch
        ).to(self.device)
        with self.torch.inference_mode():
            embedding = self.model.encode_image(model_input).to(dtype=self.torch.float32)
            embedding = self.torch.nn.functional.normalize(embedding, p=2, dim=1)
            if embedding.shape != (1, CLIP_EMBEDDING_DIMENSIONS):
                raise RuntimeError(
                    f"CLIP returned embedding shape {tuple(embedding.shape)}"
                )
            if not self.torch.isfinite(embedding).all().item():
                raise RuntimeError("CLIP returned a non-finite image embedding")
            raw_aesthetic_rating = float(self.aesthetic_head(embedding).item())
            scenic_probabilities = self.torch.softmax(
                SCENIC_LOGIT_SCALE * embedding @ self.scenic_text_features.T,
                dim=1,
            )
            raw_scenic_score = float(scenic_probabilities[0, 0].item())

        response = {
            "aestheticScore": aesthetic_score(raw_aesthetic_rating),
            "scenicScore": bounded_score(raw_scenic_score, "scenic score"),
            "simplicityScore": simplicity_score(image),
            "landscapeScore": landscape_score(image),
            "technicalQualityScore": technical_quality_score(image),
        }
        if not all(math.isfinite(score) for score in response.values()):
            raise RuntimeError("image aesthetics response contains a non-finite score")
        return response


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
        body = json.dumps(payload, allow_nan=False, separators=(",", ":")).encode(
            "utf-8"
        )
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--clip-model", required=True)
    parser.add_argument("--aesthetic-head", required=True)
    parser.add_argument("--device", required=True)
    parser.add_argument("--host", required=True)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--max-concurrent-jobs", type=int, required=True)
    parser.add_argument("--input-root", required=True)
    arguments = parser.parse_args()
    if arguments.max_concurrent_jobs <= 0:
        parser.error("--max-concurrent-jobs must be positive")

    Handler.runtime = ImageAestheticsRuntime(
        arguments.clip_model, arguments.aesthetic_head, arguments.device
    )
    Handler.inference_slots = create_inference_slots(arguments.max_concurrent_jobs)
    Handler.input_root = Path(arguments.input_root)
    serve_until_stopped(ModelHTTPServer((arguments.host, arguments.port), Handler))


if __name__ == "__main__":
    main()
