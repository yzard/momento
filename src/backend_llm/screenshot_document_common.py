"""Shared image analysis and HTTP runtime for screenshot and document detection."""

import argparse
import json
import math
import queue
import threading
import time
from http.server import BaseHTTPRequestHandler
from pathlib import Path

from image_runtime import (
    InvalidImageError,
    ModelHTTPServer,
    create_inference_slots,
    decode_image,
    register_image_decoders,
    serve_until_stopped,
)
from runtime_input import read_runtime_input

MINIMUM_TEXT_CONFIDENCE = 0.25
MAX_TEXT_BYTES = 4096
MAX_TEXT_REGIONS = 10000
PADDLEOCR_MAX_SIDE_LENGTH = 4000


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
            for row_offset, column_offset in (
                (-1, 0),
                (1, 0),
                (0, -1),
                (0, 1),
            ):
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
        if (
            3 <= area <= 600
            and component_width <= maximum_width
            and component_height <= maximum_height
        ):
            compact_components += 1
    return bounded_score(compact_components / 3.0, "compact component score")


def visual_metrics(image):
    import numpy

    pixels = analysis_pixels(image)
    maximum = pixels.max(axis=2)
    minimum = pixels.min(axis=2)
    grayscale = (
        0.299 * pixels[:, :, 0] + 0.587 * pixels[:, :, 1] + 0.114 * pixels[:, :, 2]
    )
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
        0.60 * min(float(saturation.mean()) / 0.30, 1.0) + 0.40 * min(entropy, 1.0),
        "photo likelihood",
    )
    return {
        "paper": float(neutral_bright.mean()),
        "geometry": geometry_score,
        "flat_color": flat_color_score,
        "compact_components": compact_status_component_score(grayscale),
        "photo_likelihood": photo_likelihood,
    }


def paddleocr_pipeline_configuration(
    text_detection_model_path,
    text_recognition_model_path,
    batch_size,
):
    if batch_size <= 0:
        raise ValueError("PaddleOCR batch_size must be positive")
    return {
        "pipeline_name": "OCR",
        "batch_size": batch_size,
        "text_type": "general",
        "use_doc_preprocessor": False,
        "use_textline_orientation": False,
        "SubModules": {
            "TextDetection": {
                "module_name": "text_detection",
                "model_name": "PP-OCRv6_small_det",
                "model_dir": str(text_detection_model_path),
                "limit_side_len": 64,
                "limit_type": "min",
                "max_side_limit": PADDLEOCR_MAX_SIDE_LENGTH,
                "thresh": 0.2,
                "box_thresh": 0.45,
                "unclip_ratio": 1.4,
            },
            "TextRecognition": {
                "module_name": "text_recognition",
                "model_name": "PP-OCRv6_small_rec",
                "model_dir": str(text_recognition_model_path),
                "batch_size": batch_size,
                "score_thresh": MINIMUM_TEXT_CONFIDENCE,
            },
        },
    }


def load_paddleocr_pipeline(
    text_detection_model_path,
    text_recognition_model_path,
    device,
    batch_size,
):
    import paddle
    from paddleocr import PaddleOCR

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

    for model_path in (text_detection_model_path, text_recognition_model_path):
        if not model_path.is_dir():
            raise RuntimeError(f"PaddleOCR model directory is missing: {model_path}")

    configuration = paddleocr_pipeline_configuration(
        text_detection_model_path,
        text_recognition_model_path,
        batch_size,
    )
    return PaddleOCR(
        paddlex_config=configuration,
        text_detection_model_name="PP-OCRv6_small_det",
        text_detection_model_dir=str(text_detection_model_path),
        text_recognition_model_name="PP-OCRv6_small_rec",
        text_recognition_model_dir=str(text_recognition_model_path),
        text_recognition_batch_size=batch_size,
        use_doc_orientation_classify=False,
        use_doc_unwarping=False,
        use_textline_orientation=False,
        text_rec_score_thresh=MINIMUM_TEXT_CONFIDENCE,
        device=device,
    )


def image_to_paddle_array(image):
    import numpy

    rgb_pixels = numpy.asarray(image, dtype=numpy.uint8)
    if rgb_pixels.ndim != 3 or rgb_pixels.shape[2] != 3:
        raise RuntimeError("PaddleOCR image must contain three color channels")
    return numpy.ascontiguousarray(rgb_pixels[:, :, ::-1])


def resize_for_paddleocr(image, max_side_length):
    from PIL import Image

    if max_side_length <= 0:
        raise ValueError("PaddleOCR max_side_length must be positive")
    if image.width <= 0 or image.height <= 0:
        raise RuntimeError("PaddleOCR image dimensions must be positive")

    longest_side = max(image.width, image.height)
    if longest_side <= max_side_length:
        return image
    if image.width >= image.height:
        resized_size = (
            max_side_length,
            max(1, image.height * max_side_length // image.width),
        )
    else:
        resized_size = (
            max(1, image.width * max_side_length // image.height),
            max_side_length,
        )
    return image.resize(resized_size, Image.Resampling.LANCZOS)


def paddleocr_text_regions(prediction, image_width, image_height):
    if image_width <= 0 or image_height <= 0:
        raise RuntimeError("PaddleOCR image dimensions must be positive")
    try:
        recognized_texts = prediction["rec_texts"]
        recognition_scores = prediction["rec_scores"]
        recognition_boxes = prediction["rec_boxes"]
    except (KeyError, TypeError) as error:
        raise RuntimeError("PaddleOCR result is missing text-region fields") from error
    if not (len(recognized_texts) == len(recognition_scores) == len(recognition_boxes)):
        raise RuntimeError("PaddleOCR text-region fields have different lengths")

    regions = []
    for region_index, text in enumerate(recognized_texts):
        if len(regions) >= MAX_TEXT_REGIONS:
            break
        if not isinstance(text, str):
            raise RuntimeError("PaddleOCR recognized text must be a string")
        text = text.strip()
        if not text or len(text.encode("utf-8")) > MAX_TEXT_BYTES:
            continue
        try:
            confidence = float(recognition_scores[region_index])
            left, top, right, bottom = (
                float(coordinate) for coordinate in recognition_boxes[region_index]
            )
        except (TypeError, ValueError):
            raise RuntimeError("PaddleOCR text-region values are invalid") from None
        if not all(
            math.isfinite(number) for number in (confidence, left, top, right, bottom)
        ):
            raise RuntimeError("PaddleOCR text-region values must be finite")
        if confidence < 0.0 or confidence > 1.0:
            raise RuntimeError("PaddleOCR text confidence must be between zero and one")
        if confidence < MINIMUM_TEXT_CONFIDENCE:
            continue

        clipped_left = max(0.0, min(float(image_width), left))
        clipped_top = max(0.0, min(float(image_height), top))
        clipped_right = max(0.0, min(float(image_width), right))
        clipped_bottom = max(0.0, min(float(image_height), bottom))
        width = clipped_right - clipped_left
        height = clipped_bottom - clipped_top
        if width <= 0.0 or height <= 0.0:
            continue
        regions.append(
            {
                "text": text,
                "confidence": confidence,
                "x": clipped_left / image_width,
                "y": clipped_top / image_height,
                "width": width / image_width,
                "height": height / image_height,
            }
        )
    return regions


class PaddleOCRBatchRequest:
    def __init__(self, paddle_image, image_width, image_height):
        self.paddle_image = paddle_image
        self.image_width = image_width
        self.image_height = image_height
        self.completed = threading.Event()
        self.text_regions = None
        self.error = None


class PaddleOCRBatchProcessor:
    def __init__(
        self,
        pipeline,
        model_concurrency,
        batch_wait_seconds,
    ):
        if model_concurrency <= 0:
            raise ValueError("model_concurrency must be positive")
        if batch_wait_seconds < 0.0:
            raise ValueError("batch_wait_seconds must not be negative")
        self.pipeline = pipeline
        self.model_concurrency = model_concurrency
        self.batch_wait_seconds = batch_wait_seconds
        self.pending_requests = queue.Queue(maxsize=model_concurrency)
        self.stop_marker = object()
        self.submission_lock = threading.Lock()
        self.closed = False
        self.worker = threading.Thread(
            target=self.process_batches,
            name="paddleocr-batch-worker",
        )
        self.worker.start()

    def infer(self, paddle_image, image_width, image_height):
        pending_request = PaddleOCRBatchRequest(
            paddle_image,
            image_width,
            image_height,
        )
        with self.submission_lock:
            if self.closed:
                raise RuntimeError("PaddleOCR batch processor is closed")
            self.pending_requests.put(pending_request)
        pending_request.completed.wait()
        if pending_request.error is not None:
            raise pending_request.error
        return pending_request.text_regions

    def collect_batch(self, first_request):
        pending_batch = [first_request]
        stop_after_batch = False
        deadline = time.monotonic() + self.batch_wait_seconds
        while len(pending_batch) < self.model_concurrency:
            remaining_seconds = deadline - time.monotonic()
            if remaining_seconds <= 0.0:
                break
            try:
                pending_request = self.pending_requests.get(timeout=remaining_seconds)
            except queue.Empty:
                break
            if pending_request is self.stop_marker:
                stop_after_batch = True
                break
            pending_batch.append(pending_request)
        return pending_batch, stop_after_batch

    def process_batch(self, pending_batch):
        try:
            paddle_images = [
                pending_request.paddle_image for pending_request in pending_batch
            ]
            predictions = self.pipeline.predict(paddle_images)
            if len(predictions) != len(pending_batch):
                raise RuntimeError(
                    "PaddleOCR returned a different number of results than inputs"
                )
            for pending_request, prediction in zip(
                pending_batch,
                predictions,
            ):
                pending_request.text_regions = paddleocr_text_regions(
                    prediction,
                    pending_request.image_width,
                    pending_request.image_height,
                )
        except (
            IndexError,
            KeyError,
            OSError,
            RuntimeError,
            TypeError,
            ValueError,
        ) as error:
            for pending_request in pending_batch:
                pending_request.error = RuntimeError(
                    f"PaddleOCR inference failed: {error}"
                )
        finally:
            for pending_request in pending_batch:
                pending_request.completed.set()

    def process_batches(self):
        while True:
            first_request = self.pending_requests.get()
            if first_request is self.stop_marker:
                return
            pending_batch, stop_after_batch = self.collect_batch(first_request)
            self.process_batch(pending_batch)
            if stop_after_batch:
                return

    def close(self):
        with self.submission_lock:
            if self.closed:
                return
            self.closed = True
            self.pending_requests.put(self.stop_marker)
        self.worker.join()
        self.pipeline.close()


class DetectionRuntime:
    def __init__(self, detector, text_region_extractor):
        self.detector = detector
        self.text_region_extractor = text_region_extractor

    def prepare_input(self, image_source):
        image = decode_image(image_source)
        resized_image = resize_for_paddleocr(image, PADDLEOCR_MAX_SIDE_LENGTH)
        return image, image_to_paddle_array(resized_image), resized_image.size

    def infer(self, prepared_input):
        image, paddle_image, resized_size = prepared_input
        resized_width, resized_height = resized_size
        text_regions = self.text_region_extractor.infer(
            paddle_image,
            resized_width,
            resized_height,
        )
        return self.detector(image, text_regions)


def prepare_runtime_input(handler):
    with handler.cpu_processing_slots:
        with read_runtime_input(handler, handler.input_root) as image_source:
            return handler.runtime.prepare_input(image_source)


def run_bounded_model_inference(handler):
    with handler.model_slots:
        prepared_input = prepare_runtime_input(handler)
        return handler.runtime.infer(prepared_input)


class DetectionHandler(BaseHTTPRequestHandler):
    runtime = None
    cpu_processing_slots = None
    model_slots = None
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
            # Bound decoded image memory before opening or reading the queued input.
            response = run_bounded_model_inference(self)
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

    def log_message(self, message_format, *arguments):
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


def serve_detection(detector):
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", required=True)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--cpu-processing-concurrency", type=int, required=True)
    parser.add_argument("--model-concurrency", type=int, required=True)
    parser.add_argument("--input-root", required=True)
    parser.add_argument("--text-detection-model", required=True)
    parser.add_argument("--text-recognition-model", required=True)
    parser.add_argument("--device", required=True)
    parser.add_argument("--batch-wait-milliseconds", type=float, required=True)
    arguments = parser.parse_args()
    if arguments.cpu_processing_concurrency <= 0:
        parser.error("--cpu-processing-concurrency must be positive")
    if arguments.model_concurrency <= 0:
        parser.error("--model-concurrency must be positive")
    if arguments.batch_wait_milliseconds < 0.0:
        parser.error("--batch-wait-milliseconds must not be negative")

    register_image_decoders()
    pipeline = load_paddleocr_pipeline(
        Path(arguments.text_detection_model),
        Path(arguments.text_recognition_model),
        arguments.device,
        arguments.model_concurrency,
    )
    batch_processor = PaddleOCRBatchProcessor(
        pipeline,
        arguments.model_concurrency,
        arguments.batch_wait_milliseconds / 1000.0,
    )
    DetectionHandler.runtime = DetectionRuntime(detector, batch_processor)
    DetectionHandler.cpu_processing_slots = create_inference_slots(
        arguments.cpu_processing_concurrency
    )
    DetectionHandler.model_slots = create_inference_slots(arguments.model_concurrency)
    DetectionHandler.input_root = Path(arguments.input_root)
    try:
        serve_until_stopped(
            ModelHTTPServer((arguments.host, arguments.port), DetectionHandler)
        )
    finally:
        batch_processor.close()
