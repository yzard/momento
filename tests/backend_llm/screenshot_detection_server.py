import importlib.util
import math
import sys
import unittest
from pathlib import Path
from unittest import mock


SOURCE_DIRECTORY = Path(__file__).resolve().parents[2] / "src" / "backend_llm"
COMMON_SPECIFICATION = importlib.util.spec_from_file_location(
    "screenshot_document_common_source",
    SOURCE_DIRECTORY / "screenshot_document_common.py",
)
SCREENSHOT_DOCUMENT_COMMON = importlib.util.module_from_spec(COMMON_SPECIFICATION)
COMMON_SPECIFICATION.loader.exec_module(SCREENSHOT_DOCUMENT_COMMON)
SPECIFICATION = importlib.util.spec_from_file_location(
    "screenshot_detection_server_source",
    SOURCE_DIRECTORY / "screenshot_detection_server.py",
)
SCREENSHOT_DETECTION_SERVER = importlib.util.module_from_spec(SPECIFICATION)
with mock.patch.dict(
    sys.modules,
    {"screenshot_document_common": SCREENSHOT_DOCUMENT_COMMON},
):
    SPECIFICATION.loader.exec_module(SCREENSHOT_DETECTION_SERVER)


def text_region(text, x, y, width, height):
    return {
        "text": text,
        "confidence": 0.95,
        "x": x,
        "y": y,
        "width": width,
        "height": height,
    }


class ScreenshotDetectionServerTests(unittest.TestCase):
    def setUp(self):
        try:
            import numpy
            from PIL import Image, ImageDraw
        except ImportError:
            self.skipTest("NumPy and Pillow are required")
        self.numpy = numpy
        self.Image = Image
        self.ImageDraw = ImageDraw

    def test_synthetic_mobile_ui_is_a_screenshot(self):
        image = self.Image.new("RGB", (360, 780), color=(242, 244, 248))
        drawing = self.ImageDraw.Draw(image)
        drawing.rectangle((0, 0, 359, 44), fill=(250, 250, 250))
        drawing.rectangle((292, 14, 316, 25), outline=(20, 20, 20), width=2)
        drawing.rectangle((320, 13, 346, 27), outline=(20, 20, 20), width=2)
        drawing.rectangle((60, 86, 300, 142), fill=(56, 118, 255))
        for row in range(5):
            top = 188 + row * 92
            drawing.rounded_rectangle(
                (24, top, 336, top + 66), radius=10, fill=(255, 255, 255)
            )
            drawing.line((48, top + 24, 280, top + 24), fill=(55, 55, 60), width=3)
            drawing.line((48, top + 42, 220, top + 42), fill=(145, 145, 150), width=2)
        drawing.rectangle((0, 724, 359, 779), fill=(250, 250, 250))
        regions = [
            text_region("09:41", 0.04, 0.015, 0.14, 0.025),
            text_region("5G", 0.74, 0.015, 0.06, 0.025),
            text_region("Home", 0.08, 0.93, 0.12, 0.025),
        ]

        response = SCREENSHOT_DETECTION_SERVER.classify_screenshot(image, regions)

        self.assertEqual(set(response), {"detected", "confidence"})
        self.assertTrue(response["detected"], response)
        self.assertTrue(math.isfinite(response["confidence"]))
        self.assertGreaterEqual(response["confidence"], 0.0)
        self.assertLessEqual(response["confidence"], 1.0)

    def test_synthetic_photo_is_not_a_screenshot(self):
        random_generator = self.numpy.random.default_rng(7)
        pixels = random_generator.integers(
            0, 256, size=(780, 360, 3), dtype=self.numpy.uint8
        )
        image = self.Image.fromarray(pixels, mode="RGB")

        response = SCREENSHOT_DETECTION_SERVER.classify_screenshot(image, [])

        self.assertFalse(response["detected"], response)


if __name__ == "__main__":
    unittest.main()
