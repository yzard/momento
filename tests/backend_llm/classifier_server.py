import importlib.util
import io
import math
import unittest
from pathlib import Path


SOURCE_PATH = (
    Path(__file__).resolve().parents[2]
    / "src"
    / "backend_llm"
    / "classifier_server.py"
)
SPECIFICATION = importlib.util.spec_from_file_location("classifier_server", SOURCE_PATH)
CLASSIFIER_SERVER = importlib.util.module_from_spec(SPECIFICATION)
SPECIFICATION.loader.exec_module(CLASSIFIER_SERVER)


def text_region(text, x, y, width, height):
    return {
        "text": text,
        "confidence": 0.95,
        "x": x,
        "y": y,
        "width": width,
        "height": height,
    }


class ClassifierServerTests(unittest.TestCase):
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

        response = CLASSIFIER_SERVER.classify_screenshot(image, regions)

        self.assertEqual(set(response), {"detected", "confidence"})
        self.assertTrue(response["detected"], response)
        self.assertTrue(math.isfinite(response["confidence"]))
        self.assertGreaterEqual(response["confidence"], 0.0)
        self.assertLessEqual(response["confidence"], 1.0)

    def test_synthetic_photo_is_not_a_screenshot(self):
        random = self.numpy.random.default_rng(7)
        pixels = random.integers(0, 256, size=(780, 360, 3), dtype=self.numpy.uint8)
        image = self.Image.fromarray(pixels, mode="RGB")

        response = CLASSIFIER_SERVER.classify_screenshot(image, [])

        self.assertFalse(response["detected"], response)

    def test_synthetic_page_is_a_document(self):
        image = self.Image.new("RGB", (600, 800), color=(248, 246, 239))
        drawing = self.ImageDraw.Draw(image)
        regions = []
        for line in range(11):
            top = 95 + line * 52
            width = 430 if line % 4 else 330
            drawing.rectangle((75, top, 75 + width, top + 10), fill=(35, 35, 35))
            regions.append(
                text_region(
                    f"document line {line}",
                    75 / 600,
                    top / 800,
                    width / 600,
                    18 / 800,
                )
            )

        response = CLASSIFIER_SERVER.classify_document(image, regions)

        self.assertEqual(set(response), {"detected", "confidence"})
        self.assertTrue(response["detected"], response)

    def test_colorful_photo_without_text_is_not_a_document(self):
        random = self.numpy.random.default_rng(11)
        pixels = random.integers(0, 256, size=(600, 800, 3), dtype=self.numpy.uint8)
        image = self.Image.fromarray(pixels, mode="RGB")

        response = CLASSIFIER_SERVER.classify_document(image, [])

        self.assertFalse(response["detected"], response)

    def test_runtime_concurrency_slot_is_acquired_before_input_handling(self):
        events = []

        class Slot:
            def __enter__(self):
                events.append("acquired")

            def __exit__(self, exception_type, exception, traceback):
                events.append("released")

        class FakeHandler:
            inference_slots = Slot()

            def handle_inference(self):
                events.append("read")

        CLASSIFIER_SERVER.run_bounded_inference(FakeHandler())

        self.assertEqual(events, ["acquired", "read", "released"])

    def test_shared_runtime_selects_both_registered_classifiers(self):
        self.assertEqual(
            CLASSIFIER_SERVER.ClassifierRuntime("screenshot_detection").classifier,
            "screenshot_detection",
        )
        self.assertEqual(
            CLASSIFIER_SERVER.ClassifierRuntime("document_detection").classifier,
            "document_detection",
        )
        with self.assertRaisesRegex(ValueError, "unsupported classifier"):
            CLASSIFIER_SERVER.ClassifierRuntime("unknown")

    def test_tesseract_tsv_skips_oversized_text_fields_and_keeps_later_rows(self):
        header = "\t".join(CLASSIFIER_SERVER.TESSERACT_TSV_COLUMNS).encode("utf-8")
        oversized_text = b"x" * 140_000
        payload = b"\n".join(
            [
                header,
                b"5\t1\t1\t1\t1\t1\t10\t20\t30\t40\t95\t" + oversized_text,
                b"5\t1\t1\t1\t1\t2\t50\t60\t70\t80\t90\tkept",
                b"",
            ]
        )

        regions = CLASSIFIER_SERVER.parse_text_regions(
            io.BytesIO(payload), 1000, 1000
        )

        self.assertEqual(
            regions,
            [
                {
                    "text": "kept",
                    "confidence": 0.9,
                    "x": 0.05,
                    "y": 0.06,
                    "width": 0.07,
                    "height": 0.08,
                }
            ],
        )

    def test_tesseract_tsv_drains_an_oversized_row_before_continuing(self):
        header = "\t".join(CLASSIFIER_SERVER.TESSERACT_TSV_COLUMNS).encode("utf-8")
        oversized_row = b"x" * (CLASSIFIER_SERVER.MAX_TESSERACT_ROW_BYTES + 1)
        payload = b"\n".join(
            [
                header,
                oversized_row,
                b"5\t1\t1\t1\t1\t2\t100\t200\t300\t400\t80\tafter",
                b"",
            ]
        )

        regions = CLASSIFIER_SERVER.parse_text_regions(
            io.BytesIO(payload), 1000, 1000
        )

        self.assertEqual(len(regions), 1)
        self.assertEqual(regions[0]["text"], "after")


if __name__ == "__main__":
    unittest.main()
