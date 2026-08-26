import importlib.util
import sys
import unittest
from pathlib import Path
from unittest import mock

SOURCE_DIRECTORY = Path(__file__).resolve().parents[2] / "src" / "backend_llm"
COMMON_SPECIFICATION = importlib.util.spec_from_file_location(
    "screenshot_document_common_source", SOURCE_DIRECTORY / "screenshot_document_common.py"
)
SCREENSHOT_DOCUMENT_COMMON = importlib.util.module_from_spec(COMMON_SPECIFICATION)
COMMON_SPECIFICATION.loader.exec_module(SCREENSHOT_DOCUMENT_COMMON)
SPECIFICATION = importlib.util.spec_from_file_location(
    "document_detection_server_source", SOURCE_DIRECTORY / "document_detection_server.py"
)
DOCUMENT_DETECTION_SERVER = importlib.util.module_from_spec(SPECIFICATION)
with mock.patch.dict(sys.modules, {"screenshot_document_common": SCREENSHOT_DOCUMENT_COMMON}):
    SPECIFICATION.loader.exec_module(DOCUMENT_DETECTION_SERVER)


def text_region(text, x, y, width, height):
    return {"text": text, "confidence": 0.95, "x": x, "y": y, "width": width, "height": height}


class DocumentDetectionServerTests(unittest.TestCase):
    def setUp(self):
        try:
            import numpy
            from PIL import Image, ImageDraw
        except ImportError:
            self.skipTest("NumPy and Pillow are required")
        self.numpy = numpy
        self.Image = Image
        self.ImageDraw = ImageDraw

    def test_synthetic_page_is_a_document(self):
        image = self.Image.new("RGB", (600, 800), color=(248, 246, 239))
        drawing = self.ImageDraw.Draw(image)
        regions = []
        for line in range(11):
            top = 95 + line * 52
            width = 430 if line % 4 else 330
            drawing.rectangle((75, top, 75 + width, top + 10), fill=(35, 35, 35))
            regions.append(text_region(f"document line {line}", 75 / 600, top / 800, width / 600, 18 / 800))

        response = DOCUMENT_DETECTION_SERVER.classify_document(image, regions)

        self.assertEqual(set(response), {"detected", "confidence"})
        self.assertTrue(response["detected"], response)

    def test_colorful_photo_without_text_is_not_a_document(self):
        random_generator = self.numpy.random.default_rng(11)
        pixels = random_generator.integers(0, 256, size=(600, 800, 3), dtype=self.numpy.uint8)
        image = self.Image.fromarray(pixels, mode="RGB")

        response = DOCUMENT_DETECTION_SERVER.classify_document(image, [])

        self.assertFalse(response["detected"], response)


if __name__ == "__main__":
    unittest.main()
