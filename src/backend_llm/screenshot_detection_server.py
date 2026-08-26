#!/usr/bin/env python3
"""GPU PaddleOCR-assisted runtime entrypoint for screenshot detection."""

import re

from screenshot_document_common import (
    bounded_score,
    detection_response,
    serve_detection,
    visual_metrics,
)

SCREENSHOT_THRESHOLD = 0.58
TIME_PATTERN = re.compile(r"\b(?:[01]?\d|2[0-3]):[0-5]\d\b")


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
    return detection_response(score, SCREENSHOT_THRESHOLD)


if __name__ == "__main__":
    serve_detection(classify_screenshot)
