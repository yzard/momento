#!/usr/bin/env python3
"""Shared CPU runtime for screenshot and document classification."""

import argparse
import csv
import io
import json
import math
import re
import subprocess
from http.server import BaseHTTPRequestHandler
from pathlib import Path

from image_runtime import (
    InvalidImageError,
    ModelHTTPServer,
    create_inference_slots,
    decode_image,
    serve_until_stopped,
)
from runtime_input import read_runtime_input


CLASSIFIERS = ("screenshot_detection", "document_detection")
SCREENSHOT_THRESHOLD = 0.58
DOCUMENT_THRESHOLD = 0.58
TIME_PATTERN = re.compile(r"\b(?:[01]?\d|2[0-3]):[0-5]\d\b")


def bounded_score(score, score_name):
    score = float(score)
    if not math.isfinite(score):
        raise RuntimeError(f"{score_name} is not finite")
    return max(0.0, min(1.0, score))


def classifier_response(score, threshold):
    confidence = round(bounded_score(score, "classifier confidence"), 6)
    return {"detected": confidence >= threshold, "confidence": confidence}


def analysis_pixels(image):
    import numpy
    from PIL import Image

    resized = image.copy()
    resized.thumbnail((512, 512), Image.Resampling.LANCZOS)
    pixels = numpy.asarray(resized, dtype=numpy.float32) / 255.0
    if pixels.ndim != 3 or pixels.shape[2] != 3:
        raise RuntimeError("classifier image must contain three color channels")
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
        0.299 * pixels[:, :, 0]
        + 0.587 * pixels[:, :, 1]
        + 0.114 * pixels[:, :, 2]
    )
    saturation = maximum - minimum
    neutral_bright = (saturation < 0.08) & (grayscale > 0.72)

    horizontal_difference = numpy.abs(numpy.diff(grayscale, axis=1))
    vertical_difference = numpy.abs(numpy.diff(grayscale, axis=0))
    horizontal_edges = horizontal_difference > 0.12
    vertical_edges = vertical_difference > 0.12
    edge_density = (
        float(horizontal_edges.mean()) + float(vertical_edges.mean())
    ) / 2.0
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
        0.60 * min(float(saturation.mean()) / 0.30, 1.0)
        + 0.40 * min(entropy, 1.0),
        "photo likelihood",
    )
    return {
        "paper": float(neutral_bright.mean()),
        "geometry": geometry_score,
        "flat_color": flat_color_score,
        "compact_components": compact_status_component_score(grayscale),
        "photo_likelihood": photo_likelihood,
    }


def extract_text_regions(image):
    encoded = io.BytesIO()
    image.save(encoded, format="PNG")
    process = subprocess.run(
        ["tesseract", "stdin", "stdout", "--psm", "11", "tsv"],
        input=encoded.getvalue(),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if process.returncode != 0:
        detail = process.stderr.decode("utf-8", errors="replace").strip()
        raise RuntimeError(f"Tesseract failed with exit code {process.returncode}: {detail}")

    regions = []
    rows = csv.DictReader(io.StringIO(process.stdout.decode("utf-8")), delimiter="\t")
    for row in rows:
        text = (row.get("text") or "").strip()
        confidence_text = row.get("conf") or "-1"
        try:
            confidence = float(confidence_text)
            left = int(row.get("left") or 0)
            top = int(row.get("top") or 0)
            width = int(row.get("width") or 0)
            height = int(row.get("height") or 0)
        except ValueError:
            continue
        if not text or confidence < 25.0 or width <= 0 or height <= 0:
            continue
        regions.append(
            {
                "text": text,
                "confidence": confidence / 100.0,
                "x": left / image.width,
                "y": top / image.height,
                "width": width / image.width,
                "height": height / image.height,
            }
        )
    return regions


def mobile_aspect_score(image):
    if image.width <= 0 or image.height <= 0:
        raise RuntimeError("decoded image has invalid dimensions")
    if image.height < image.width:
        return 0.0
    aspect_ratio = image.height / image.width
    rise = bounded_score((aspect_ratio - 1.25) / 0.35, "mobile aspect rise")
    fall = bounded_score((2.65 - aspect_ratio) / 0.25, "mobile aspect fall")
    return min(rise, fall)


def status_region_score(text_regions):
    top_regions = [
        region
        for region in text_regions
        if region["y"] + region["height"] / 2.0 <= 0.13
    ]
    bottom_regions = [
        region
        for region in text_regions
        if region["y"] + region["height"] / 2.0 >= 0.87
    ]
    contains_time = any(TIME_PATTERN.search(region["text"]) for region in top_regions)
    return bounded_score(
        0.55 * float(contains_time)
        + 0.30 * min(len(top_regions) / 3.0, 1.0)
        + 0.15 * min(len(bottom_regions) / 2.0, 1.0),
        "status region score",
    )


def classify_screenshot(image, text_regions):
    metrics = visual_metrics(image)
    score = (
        0.27 * status_region_score(text_regions)
        + 0.13 * metrics["compact_components"]
        + 0.20 * mobile_aspect_score(image)
        + 0.22 * metrics["geometry"]
        + 0.18 * metrics["flat_color"]
    )
    return classifier_response(score, SCREENSHOT_THRESHOLD)


def text_layout_metrics(text_regions):
    if not text_regions:
        return 0.0, 0.0
    occupied_area = sum(
        region["width"] * region["height"] * region["confidence"]
        for region in text_regions
    )
    occupancy_score = bounded_score(occupied_area / 0.16, "text occupancy score")

    centers = sorted(
        region["y"] + region["height"] / 2.0 for region in text_regions
    )
    line_centers = []
    for center in centers:
        if not line_centers or center - line_centers[-1][-1] > 0.025:
            line_centers.append([center])
        else:
            line_centers[-1].append(center)
    averaged_lines = [sum(line) / len(line) for line in line_centers]
    line_count_score = min(len(averaged_lines) / 6.0, 1.0)
    if len(averaged_lines) < 3:
        return occupancy_score, 0.35 * line_count_score
    gaps = [
        current - previous
        for previous, current in zip(averaged_lines, averaged_lines[1:])
    ]
    mean_gap = sum(gaps) / len(gaps)
    gap_variance = sum((gap - mean_gap) ** 2 for gap in gaps) / len(gaps)
    regular_spacing = bounded_score(
        1.0 - math.sqrt(gap_variance) / max(mean_gap, 0.001),
        "line spacing score",
    )
    left_positions = [region["x"] for region in text_regions]
    mean_left = sum(left_positions) / len(left_positions)
    left_variance = sum(
        (position - mean_left) ** 2 for position in left_positions
    ) / len(left_positions)
    left_alignment = bounded_score(
        1.0 - math.sqrt(left_variance) / 0.20, "left alignment score"
    )
    regularity_score = line_count_score * (
        0.60 * regular_spacing + 0.40 * left_alignment
    )
    return occupancy_score, bounded_score(regularity_score, "line regularity score")


def classify_document(image, text_regions):
    metrics = visual_metrics(image)
    occupancy_score, regularity_score = text_layout_metrics(text_regions)
    score = (
        0.38 * occupancy_score
        + 0.25 * regularity_score
        + 0.25 * metrics["paper"]
        + 0.12 * (1.0 - metrics["photo_likelihood"])
    )
    return classifier_response(score, DOCUMENT_THRESHOLD)


class ClassifierRuntime:
    def __init__(self, classifier):
        if classifier not in CLASSIFIERS:
            raise ValueError(f"unsupported classifier: {classifier}")
        self.classifier = classifier

    def infer(self, image_bytes):
        image = decode_image(image_bytes)
        text_regions = extract_text_regions(image)
        if self.classifier == "screenshot_detection":
            return classify_screenshot(image, text_regions)
        return classify_document(image, text_regions)


def run_bounded_inference(handler):
    with handler.inference_slots:
        handler.handle_inference()


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
        # The slot is held before descriptor validation opens or reads the queued input.
        run_bounded_inference(self)

    def handle_inference(self):
        try:
            image_bytes = read_runtime_input(self, self.input_root)
        except (OSError, ValueError, json.JSONDecodeError) as error:
            self.send_json(400, {"detail": f"invalid request: {error}"})
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


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--classifier", choices=CLASSIFIERS, required=True)
    parser.add_argument("--host", required=True)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--max-concurrent-jobs", type=int, required=True)
    parser.add_argument("--input-root", required=True)
    arguments = parser.parse_args()
    if arguments.max_concurrent_jobs <= 0:
        parser.error("--max-concurrent-jobs must be positive")

    Handler.runtime = ClassifierRuntime(arguments.classifier)
    Handler.inference_slots = create_inference_slots(arguments.max_concurrent_jobs)
    Handler.input_root = Path(arguments.input_root)
    serve_until_stopped(ModelHTTPServer((arguments.host, arguments.port), Handler))


if __name__ == "__main__":
    main()
