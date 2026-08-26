#!/usr/bin/env python3
"""GPU PaddleOCR-assisted runtime entrypoint for document detection."""

import math

from screenshot_document_common import (
    bounded_score,
    detection_response,
    serve_detection,
    visual_metrics,
)

DOCUMENT_THRESHOLD = 0.58


def text_layout_metrics(text_regions):
    if not text_regions:
        return 0.0, 0.0
    occupied_area = sum(
        region["width"] * region["height"] * region["confidence"]
        for region in text_regions
    )
    occupancy_score = bounded_score(occupied_area / 0.16, "text occupancy score")

    centers = sorted(region["y"] + region["height"] / 2.0 for region in text_regions)
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
    return detection_response(score, DOCUMENT_THRESHOLD)


if __name__ == "__main__":
    serve_detection(classify_document)
