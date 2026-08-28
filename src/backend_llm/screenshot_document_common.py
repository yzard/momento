"""Shared image analysis and HTTP runtime for screenshot and document detection."""

import argparse
import math
from collections.abc import Callable, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from dynamic_batching import DynamicBatcher
from image_runtime import (
    ModelHTTPServer,
    create_inference_slots,
    decode_image,
    register_image_decoders,
    serve_until_stopped,
)
from runtime_http import ImageRuntimeRequestHandler
from runtime_input import read_runtime_input

MINIMUM_TEXT_CONFIDENCE = 0.25
MAX_TEXT_BYTES = 4096
MAX_TEXT_REGIONS = 10000
PADDLEOCR_MODEL_IMAGE_SIZE = 1280
PIPELINE_BUFFER_BATCHES = 2


def bounded_score(score, score_name):
    score = float(score)
    if not math.isfinite(score):
        raise RuntimeError(f"{score_name} is not finite")
    return max(0.0, min(1.0, score))


def detection_response(score, threshold):
    confidence = round(bounded_score(score, "classifier confidence"), 6)
    return {"detected": confidence >= threshold, "confidence": confidence}


def analysis_pixels(image):
    import numpy
    from PIL import Image

    resized = image.copy()
    resized.thumbnail((512, 512), Image.Resampling.LANCZOS)
    pixels = numpy.asarray(resized, dtype=numpy.float32) / 255.0
    if pixels.ndim != 3 or pixels.shape[2] != 3:
        raise RuntimeError("detection image must contain three color channels")
    return pixels


def compact_status_component_score(grayscale):
    import numpy

    status_height = max(1, grayscale.shape[0] // 9)
    status_left = grayscale.shape[1] * 3 // 5
    status_region = grayscale[:status_height, status_left:]
    background = float(numpy.median(status_region))
    foreground = numpy.abs(status_region - background) > 0.16
    visited = numpy.zeros(foreground.shape, dtype=bool)
    compact_components = 0
    maximum_width = max(2, status_region.shape[1] // 4)
    maximum_height = max(2, status_region.shape[0] * 4 // 5)
    for row, column in zip(*numpy.nonzero(foreground)):
        if visited[row, column]:
            continue
        stack = [(int(row), int(column))]
        visited[row, column] = True
        minimum_row = maximum_row = int(row)
        minimum_column = maximum_column = int(column)
        area = 0
        while stack:
            current_row, current_column = stack.pop()
            area += 1
            minimum_row = min(minimum_row, current_row)
            maximum_row = max(maximum_row, current_row)
            minimum_column = min(minimum_column, current_column)
            maximum_column = max(maximum_column, current_column)
            for row_offset, column_offset in ((-1, 0), (1, 0), (0, -1), (0, 1)):
                neighbor_row = current_row + row_offset
                neighbor_column = current_column + column_offset
                if (
                    neighbor_row < 0
                    or neighbor_row >= foreground.shape[0]
                    or neighbor_column < 0
                    or neighbor_column >= foreground.shape[1]
                    or visited[neighbor_row, neighbor_column]
                    or not foreground[neighbor_row, neighbor_column]
                ):
                    continue
                visited[neighbor_row, neighbor_column] = True
                stack.append((neighbor_row, neighbor_column))
        component_width = maximum_column - minimum_column + 1
        component_height = maximum_row - minimum_row + 1
        if 3 <= area <= 600 and component_width <= maximum_width and component_height <= maximum_height:
            compact_components += 1
    return bounded_score(compact_components / 3.0, "compact component score")


def visual_metrics(image):
    import numpy

    pixels = analysis_pixels(image)
    maximum = pixels.max(axis=2)
    minimum = pixels.min(axis=2)
    grayscale = 0.299 * pixels[:, :, 0] + 0.587 * pixels[:, :, 1] + 0.114 * pixels[:, :, 2]
    saturation = maximum - minimum
    neutral_bright = (saturation < 0.08) & (grayscale > 0.72)

    horizontal_difference = numpy.abs(numpy.diff(grayscale, axis=1))
    vertical_difference = numpy.abs(numpy.diff(grayscale, axis=0))
    horizontal_edges = horizontal_difference > 0.12
    vertical_edges = vertical_difference > 0.12
    edge_density = (float(horizontal_edges.mean()) + float(vertical_edges.mean())) / 2.0
    strong_columns = float((horizontal_edges.mean(axis=0) > 0.08).mean())
    strong_rows = float((vertical_edges.mean(axis=1) > 0.08).mean())
    geometry_score = bounded_score(
        min(edge_density / 0.08, 1.0)
        * min((0.32 - min(edge_density, 0.32)) / 0.20, 1.0)
        * min((strong_columns + strong_rows) / 0.16, 1.0),
        "geometry score",
    )

    quantized = (pixels * 15.0).astype(numpy.uint8).reshape((-1, 3))
    unique_colors = len(numpy.unique(quantized, axis=0))
    flat_color_score = bounded_score(1.0 - unique_colors / 512.0, "flat color score")

    histogram, _ = numpy.histogram(grayscale, bins=64, range=(0.0, 1.0))
    probabilities = histogram[histogram > 0] / histogram.sum()
    entropy = float(-(probabilities * numpy.log2(probabilities)).sum()) / 6.0
    photo_likelihood = bounded_score(
        0.60 * min(float(saturation.mean()) / 0.30, 1.0) + 0.40 * min(entropy, 1.0), "photo likelihood"
    )
    return {
        "paper": float(neutral_bright.mean()),
        "geometry": geometry_score,
        "flat_color": flat_color_score,
        "compact_components": compact_status_component_score(grayscale),
        "photo_likelihood": photo_likelihood,
    }


class PaddleOCRModelComponents:
    def __init__(self, detection_model: Any, recognition_model: Any | None, polygon_cropper: Any) -> None:
        self.detection_model = detection_model
        self.recognition_model = recognition_model
        self.polygon_cropper = polygon_cropper

    def detect(self, model_images: list[Any]) -> list[Any]:
        if not model_images:
            raise ValueError("PaddleOCR detection batch must not be empty")
        try:
            predictions = list(
                self.detection_model(
                    model_images,
                    batch_size=len(model_images),
                    limit_side_len=64,
                    limit_type="min",
                    max_side_limit=PADDLEOCR_MODEL_IMAGE_SIZE,
                    thresh=0.2,
                    box_thresh=0.45,
                    unclip_ratio=1.4,
                )
            )
        except (IndexError, KeyError, OSError, RuntimeError, TypeError, ValueError) as error:
            raise RuntimeError(f"PaddleOCR text detection failed: {error}") from error
        if len(predictions) != len(model_images):
            raise RuntimeError("PaddleOCR text detection returned a different number of results than inputs")
        return predictions

    def recognize(self, text_crops: list[Any]) -> list[Any]:
        if not text_crops:
            raise ValueError("PaddleOCR recognition batch must not be empty")
        if self.recognition_model is None:
            raise RuntimeError("PaddleOCR recognition model is not loaded")
        try:
            predictions = list(self.recognition_model(text_crops, batch_size=len(text_crops), return_word_box=False))
        except (IndexError, KeyError, OSError, RuntimeError, TypeError, ValueError) as error:
            raise RuntimeError(f"PaddleOCR text recognition failed: {error}") from error
        if len(predictions) != len(text_crops):
            raise RuntimeError("PaddleOCR text recognition returned a different number of results than inputs")
        return predictions

    def crop(self, model_image: Any, polygons: list[Any]) -> list[Any]:
        text_crops = list(self.polygon_cropper(model_image, polygons))
        if len(text_crops) != len(polygons):
            raise RuntimeError("PaddleOCR cropper returned a different number of crops than polygons")
        return text_crops

    def close(self) -> None:
        if self.recognition_model is not None:
            self.recognition_model.close()
        self.detection_model.close()


def load_paddleocr_models(
    text_detection_model_path: Path,
    text_recognition_model_path: Path,
    device: str,
    model_batch_size: int,
    load_recognition_model: bool,
) -> PaddleOCRModelComponents:
    import paddle
    from paddlex.inference.models import create_predictor
    from paddlex.inference.pipelines.components import CropByPolys

    if not device.startswith("gpu:"):
        raise RuntimeError("PaddleOCR detection requires a gpu:<index> device")
    try:
        device_index = int(device.removeprefix("gpu:"))
    except ValueError as error:
        raise RuntimeError("PaddleOCR GPU device index is invalid") from error
    if device_index < 0:
        raise RuntimeError("PaddleOCR GPU device index must not be negative")
    if not paddle.is_compiled_with_cuda():
        raise RuntimeError("PaddleOCR requires a CUDA-enabled PaddlePaddle build")
    if device_index >= paddle.device.cuda.device_count():
        raise RuntimeError(f"PaddleOCR CUDA device {device_index} is unavailable")

    if not text_detection_model_path.is_dir():
        raise RuntimeError(f"PaddleOCR model directory is missing: {text_detection_model_path}")
    if load_recognition_model and not text_recognition_model_path.is_dir():
        raise RuntimeError(f"PaddleOCR model directory is missing: {text_recognition_model_path}")

    detection_model = create_predictor(
        model_name="PP-OCRv6_small_det",
        model_dir=str(text_detection_model_path),
        device=device,
        batch_size=model_batch_size,
        limit_side_len=64,
        limit_type="min",
        max_side_limit=PADDLEOCR_MODEL_IMAGE_SIZE,
        thresh=0.2,
        box_thresh=0.45,
        unclip_ratio=1.4,
    )
    recognition_model = None
    if load_recognition_model:
        recognition_model = create_predictor(
            model_name="PP-OCRv6_small_rec",
            model_dir=str(text_recognition_model_path),
            device=device,
            batch_size=model_batch_size,
            return_word_box=False,
        )
    return PaddleOCRModelComponents(detection_model, recognition_model, CropByPolys(det_box_type="quad"))


def image_to_paddle_array(image):
    import numpy

    rgb_pixels = numpy.asarray(image, dtype=numpy.uint8)
    if rgb_pixels.ndim != 3 or rgb_pixels.shape[2] != 3:
        raise RuntimeError("PaddleOCR image must contain three color channels")
    return numpy.ascontiguousarray(rgb_pixels[:, :, ::-1])


@dataclass(frozen=True)
class PreparedDetectionInput:
    image: Any
    model_image: Any
    content_left: int
    content_top: int
    content_width: int
    content_height: int


def prepare_detection_input(image: Any, model_image_size: int) -> PreparedDetectionInput:
    from PIL import Image

    if model_image_size <= 0:
        raise ValueError("PaddleOCR model_image_size must be positive")
    if image.width <= 0 or image.height <= 0:
        raise RuntimeError("PaddleOCR image dimensions must be positive")

    scale = min(model_image_size / image.width, model_image_size / image.height)
    content_width = max(1, min(model_image_size, round(image.width * scale)))
    content_height = max(1, min(model_image_size, round(image.height * scale)))
    content_left = (model_image_size - content_width) // 2
    content_top = (model_image_size - content_height) // 2
    resized_image = image.resize((content_width, content_height), Image.Resampling.LANCZOS)
    model_canvas = Image.new("RGB", (model_image_size, model_image_size), color=(255, 255, 255))
    model_canvas.paste(resized_image, (content_left, content_top))
    return PreparedDetectionInput(
        image=image,
        model_image=image_to_paddle_array(model_canvas),
        content_left=content_left,
        content_top=content_top,
        content_width=content_width,
        content_height=content_height,
    )


def normalized_text_region(prepared_input: PreparedDetectionInput, polygon: Any, confidence: Any) -> dict[str, Any]:
    import numpy

    polygon_array = numpy.asarray(polygon, dtype=numpy.float64)
    if polygon_array.shape != (4, 2):
        raise RuntimeError("PaddleOCR text detection polygon must contain four points")
    if not numpy.isfinite(polygon_array).all():
        raise RuntimeError("PaddleOCR text detection polygon must be finite")
    try:
        numeric_confidence = float(confidence)
    except (TypeError, ValueError):
        raise RuntimeError("PaddleOCR text detection confidence is invalid") from None
    if not math.isfinite(numeric_confidence) or not 0.0 <= numeric_confidence <= 1.0:
        raise RuntimeError("PaddleOCR text detection confidence must be finite and between zero and one")

    content_right = prepared_input.content_left + prepared_input.content_width
    content_bottom = prepared_input.content_top + prepared_input.content_height
    left = max(float(prepared_input.content_left), min(float(content_right), float(polygon_array[:, 0].min())))
    top = max(float(prepared_input.content_top), min(float(content_bottom), float(polygon_array[:, 1].min())))
    right = max(float(prepared_input.content_left), min(float(content_right), float(polygon_array[:, 0].max())))
    bottom = max(float(prepared_input.content_top), min(float(content_bottom), float(polygon_array[:, 1].max())))
    return {
        "text": "",
        "confidence": numeric_confidence,
        "x": (left - prepared_input.content_left) / prepared_input.content_width,
        "y": (top - prepared_input.content_top) / prepared_input.content_height,
        "width": max(0.0, right - left) / prepared_input.content_width,
        "height": max(0.0, bottom - top) / prepared_input.content_height,
    }


@dataclass(frozen=True)
class PreparedTextLayout:
    text_regions: list[dict[str, Any]]
    recognition_region_indexes: list[int]
    recognition_crops: list[Any]


class DetectionRuntime:
    def __init__(
        self,
        detector: Callable[[Any, list[dict[str, Any]]], dict[str, Any]],
        recognition_region_filter: Callable[[dict[str, Any]], bool] | None,
        model_components: PaddleOCRModelComponents,
        processing_concurrency: int,
        model_concurrency: int,
        model_batch_wait_milliseconds: int,
    ) -> None:
        self.detector = detector
        self.recognition_region_filter = recognition_region_filter
        self.model_components = model_components
        self.processing_slots = create_inference_slots(processing_concurrency)
        self.pipeline_slots = create_inference_slots(model_concurrency * PIPELINE_BUFFER_BATCHES)
        self.detection_batcher = DynamicBatcher(
            self._detect_batch, model_concurrency, model_batch_wait_milliseconds, "paddleocr-detection-batcher"
        )
        self.recognition_batcher = None
        if recognition_region_filter is not None:
            if model_components.recognition_model is None:
                raise ValueError("PaddleOCR recognition filter requires a recognition model")
            self.recognition_batcher = DynamicBatcher(
                self._recognize_batch, model_concurrency, model_batch_wait_milliseconds, "paddleocr-recognition-batcher"
            )

    def prepare_input(self, image_source: Any) -> PreparedDetectionInput:
        image = decode_image(image_source)
        return prepare_detection_input(image, PADDLEOCR_MODEL_IMAGE_SIZE)

    def detect(self, prepared_input: PreparedDetectionInput) -> Any:
        return self.detection_batcher.infer([prepared_input])[0]

    def prepare_text_layout(
        self, prepared_input: PreparedDetectionInput, detection_prediction: Any
    ) -> PreparedTextLayout:
        try:
            polygons = detection_prediction["dt_polys"]
            confidences = detection_prediction["dt_scores"]
        except (KeyError, TypeError) as error:
            raise RuntimeError("PaddleOCR text detection result is missing required fields") from error
        if len(polygons) != len(confidences):
            raise RuntimeError("PaddleOCR text detection polygons and confidences have different lengths")

        text_regions = []
        recognition_region_indexes = []
        recognition_polygons = []
        for polygon, confidence in zip(polygons, confidences):
            if len(text_regions) >= MAX_TEXT_REGIONS:
                break
            text_region = normalized_text_region(prepared_input, polygon, confidence)
            if text_region["width"] <= 0.0 or text_region["height"] <= 0.0:
                continue
            text_regions.append(text_region)
            if self.recognition_region_filter is not None and self.recognition_region_filter(text_region):
                recognition_region_indexes.append(len(text_regions) - 1)
                recognition_polygons.append(polygon)

        recognition_crops = []
        if recognition_polygons:
            recognition_crops = self.model_components.crop(prepared_input.model_image, recognition_polygons)
        return PreparedTextLayout(text_regions, recognition_region_indexes, recognition_crops)

    def recognize(self, recognition_crops: list[Any]) -> list[Any]:
        if not recognition_crops:
            return []
        if self.recognition_batcher is None:
            raise RuntimeError("PaddleOCR recognition crops exist without a recognition batcher")
        return self.recognition_batcher.infer(recognition_crops)

    def classify(
        self,
        prepared_input: PreparedDetectionInput,
        prepared_layout: PreparedTextLayout,
        recognition_predictions: Sequence[Any],
    ) -> dict[str, Any]:
        if len(recognition_predictions) != len(prepared_layout.recognition_region_indexes):
            raise RuntimeError("PaddleOCR recognition results do not match requested text regions")
        for region_index, recognition_prediction in zip(
            prepared_layout.recognition_region_indexes, recognition_predictions
        ):
            try:
                recognized_text = recognition_prediction["rec_text"]
                recognition_confidence = float(recognition_prediction["rec_score"])
            except (KeyError, TypeError, ValueError) as error:
                raise RuntimeError("PaddleOCR text recognition result is invalid") from error
            if not isinstance(recognized_text, str):
                raise RuntimeError("PaddleOCR recognized text must be a string")
            recognized_text = recognized_text.strip()
            if not math.isfinite(recognition_confidence) or not 0.0 <= recognition_confidence <= 1.0:
                raise RuntimeError("PaddleOCR recognition confidence must be finite and between zero and one")
            if recognition_confidence < MINIMUM_TEXT_CONFIDENCE or not recognized_text:
                continue
            if len(recognized_text.encode("utf-8")) > MAX_TEXT_BYTES:
                continue
            prepared_layout.text_regions[region_index]["text"] = recognized_text
            prepared_layout.text_regions[region_index]["confidence"] = recognition_confidence
        return self.detector(prepared_input.image, prepared_layout.text_regions)

    def close(self) -> None:
        self.detection_batcher.close()
        if self.recognition_batcher is not None:
            self.recognition_batcher.close()
        self.model_components.close()

    def _detect_batch(self, prepared_inputs: list[PreparedDetectionInput]) -> list[Any]:
        return self.model_components.detect([prepared_input.model_image for prepared_input in prepared_inputs])

    def _recognize_batch(self, recognition_crops: list[Any]) -> list[Any]:
        return self.model_components.recognize(recognition_crops)


def run_detection_pipeline(handler: Any) -> dict[str, Any]:
    runtime = handler.runtime
    with runtime.pipeline_slots:
        with runtime.processing_slots:
            with read_runtime_input(handler, handler.input_root) as image_source:
                prepared_input = runtime.prepare_input(image_source)
        detection_prediction = runtime.detect(prepared_input)
        with runtime.processing_slots:
            prepared_layout = runtime.prepare_text_layout(prepared_input, detection_prediction)
        recognition_predictions = runtime.recognize(prepared_layout.recognition_crops)
        with runtime.processing_slots:
            return runtime.classify(prepared_input, prepared_layout, recognition_predictions)


class DetectionHandler(ImageRuntimeRequestHandler):
    def run_inference(self):
        # Bound decoded image memory before opening or reading the queued input.
        return run_detection_pipeline(self)


def serve_detection(
    detector: Callable[[Any, list[dict[str, Any]]], dict[str, Any]],
    recognition_region_filter: Callable[[dict[str, Any]], bool] | None,
) -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", required=True)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--processing-concurrency", type=int, required=True)
    parser.add_argument("--model-concurrency", type=int, required=True)
    parser.add_argument("--input-root", required=True)
    parser.add_argument("--text-detection-model", required=True)
    parser.add_argument("--text-recognition-model", required=True)
    parser.add_argument("--device", required=True)
    parser.add_argument("--batch-wait-milliseconds", type=int, required=True)
    arguments = parser.parse_args()
    if arguments.processing_concurrency <= 0:
        parser.error("--processing-concurrency must be positive")
    if arguments.model_concurrency <= 0:
        parser.error("--model-concurrency must be positive")
    if arguments.batch_wait_milliseconds < 0.0:
        parser.error("--batch-wait-milliseconds must not be negative")

    register_image_decoders()
    model_components = load_paddleocr_models(
        Path(arguments.text_detection_model),
        Path(arguments.text_recognition_model),
        arguments.device,
        arguments.model_concurrency,
        recognition_region_filter is not None,
    )
    DetectionHandler.runtime = DetectionRuntime(
        detector,
        recognition_region_filter,
        model_components,
        arguments.processing_concurrency,
        arguments.model_concurrency,
        arguments.batch_wait_milliseconds,
    )
    DetectionHandler.input_root = Path(arguments.input_root)
    try:
        serve_until_stopped(ModelHTTPServer((arguments.host, arguments.port), DetectionHandler))
    finally:
        DetectionHandler.runtime.close()
