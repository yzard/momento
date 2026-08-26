import importlib.util
import io
import unittest
from pathlib import Path
from unittest import mock


SOURCE_PATH = (
    Path(__file__).resolve().parents[2]
    / "src"
    / "backend_llm"
    / "screenshot_document_common.py"
)
SPECIFICATION = importlib.util.spec_from_file_location(
    "screenshot_document_common_source", SOURCE_PATH
)
COMMON = importlib.util.module_from_spec(SPECIFICATION)
SPECIFICATION.loader.exec_module(COMMON)


class ScreenshotDocumentCommonTests(unittest.TestCase):
    def test_runtime_decodes_once_and_calls_the_supplied_detector(self):
        decoded_image = object()
        text_regions = [{"text": "visible"}]
        expected_response = {"detected": True, "confidence": 0.9}
        detector = mock.Mock(return_value=expected_response)
        with mock.patch.object(COMMON, "decode_image", return_value=decoded_image), mock.patch.object(
            COMMON, "extract_text_regions", return_value=text_regions
        ):
            response = COMMON.DetectionRuntime(detector).infer(b"image")

        self.assertEqual(response, expected_response)
        detector.assert_called_once_with(decoded_image, text_regions)

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

        COMMON.run_bounded_inference(FakeHandler())

        self.assertEqual(events, ["acquired", "read", "released"])

    def test_tesseract_tsv_skips_oversized_text_fields_and_keeps_later_rows(self):
        header = "\t".join(COMMON.TESSERACT_TSV_COLUMNS).encode("utf-8")
        oversized_text = b"x" * 140_000
        payload = b"\n".join(
            [
                header,
                b"5\t1\t1\t1\t1\t1\t10\t20\t30\t40\t95\t" + oversized_text,
                b"5\t1\t1\t1\t1\t2\t50\t60\t70\t80\t90\tkept",
                b"",
            ]
        )

        regions = COMMON.parse_text_regions(io.BytesIO(payload), 1000, 1000)

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
        header = "\t".join(COMMON.TESSERACT_TSV_COLUMNS).encode("utf-8")
        oversized_row = b"x" * (COMMON.MAX_TESSERACT_ROW_BYTES + 1)
        payload = b"\n".join(
            [
                header,
                oversized_row,
                b"5\t1\t1\t1\t1\t2\t100\t200\t300\t400\t80\tafter",
                b"",
            ]
        )

        regions = COMMON.parse_text_regions(io.BytesIO(payload), 1000, 1000)

        self.assertEqual(len(regions), 1)
        self.assertEqual(regions[0]["text"], "after")


if __name__ == "__main__":
    unittest.main()
