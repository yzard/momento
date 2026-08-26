import importlib.util
import sys
import tempfile
import threading
import types
import unittest
from pathlib import Path
from unittest import mock

from PIL import Image


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


def paddleocr_prediction(text, confidence, bounding_box):
    return {
        "rec_texts": [text],
        "rec_scores": [confidence],
        "rec_boxes": [bounding_box],
    }


class FakePaddleOCRPipeline:
    def __init__(self, predictions):
        self.predictions = predictions
        self.batches = []
        self.worker_thread_ids = []
        self.closed = False

    def predict(self, images):
        self.batches.append(images)
        self.worker_thread_ids.append(threading.get_ident())
        return self.predictions[: len(images)]

    def close(self):
        self.closed = True


class ScreenshotDocumentCommonTests(unittest.TestCase):
    def test_runtime_decodes_once_and_calls_the_supplied_detector(self):
        decoded_image = object()
        text_regions = [{"text": "visible"}]
        expected_response = {"detected": True, "confidence": 0.9}
        detector = mock.Mock(return_value=expected_response)
        text_region_extractor = mock.Mock()
        text_region_extractor.infer.return_value = text_regions
        with mock.patch.object(COMMON, "decode_image", return_value=decoded_image):
            response = COMMON.DetectionRuntime(
                detector, text_region_extractor
            ).infer(b"image")

        self.assertEqual(response, expected_response)
        text_region_extractor.infer.assert_called_once_with(decoded_image)
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

    def test_pipeline_configuration_groups_requests_and_batches_recognition(self):
        configuration = COMMON.paddleocr_pipeline_configuration(
            Path("/models/detection"),
            Path("/models/recognition"),
            8,
        )

        self.assertEqual(configuration["pipeline_name"], "OCR")
        self.assertEqual(configuration["batch_size"], 8)
        self.assertFalse(configuration["use_doc_preprocessor"])
        self.assertFalse(configuration["use_textline_orientation"])
        self.assertEqual(
            configuration["SubModules"]["TextDetection"]["model_dir"],
            "/models/detection",
        )
        self.assertNotIn(
            "batch_size", configuration["SubModules"]["TextDetection"]
        )
        self.assertEqual(
            configuration["SubModules"]["TextRecognition"]["batch_size"], 8
        )

    def test_pipeline_configuration_rejects_non_positive_batch_size(self):
        with self.assertRaisesRegex(ValueError, "batch_size must be positive"):
            COMMON.paddleocr_pipeline_configuration(
                Path("/models/detection"),
                Path("/models/recognition"),
                0,
            )

    def test_load_pipeline_creates_exactly_one_cuda_paddleocr_instance(self):
        created_arguments = []

        class FakePaddleOCR:
            def __init__(self, **arguments):
                created_arguments.append(arguments)

        paddle_module = types.SimpleNamespace(
            is_compiled_with_cuda=lambda: True,
            device=types.SimpleNamespace(
                cuda=types.SimpleNamespace(device_count=lambda: 1)
            ),
        )
        paddleocr_module = types.SimpleNamespace(PaddleOCR=FakePaddleOCR)
        with tempfile.TemporaryDirectory() as model_root:
            detection_model_path = Path(model_root) / "detection"
            recognition_model_path = Path(model_root) / "recognition"
            detection_model_path.mkdir()
            recognition_model_path.mkdir()
            with mock.patch.dict(
                sys.modules,
                {"paddle": paddle_module, "paddleocr": paddleocr_module},
            ):
                pipeline = COMMON.load_paddleocr_pipeline(
                    detection_model_path,
                    recognition_model_path,
                    "gpu:0",
                    4,
                )

        self.assertIsInstance(pipeline, FakePaddleOCR)
        self.assertEqual(len(created_arguments), 1)
        self.assertEqual(created_arguments[0]["device"], "gpu:0")
        self.assertEqual(created_arguments[0]["paddlex_config"]["batch_size"], 4)
        self.assertEqual(
            created_arguments[0]["text_detection_model_name"],
            "PP-OCRv6_small_det",
        )
        self.assertEqual(
            created_arguments[0]["text_recognition_model_name"],
            "PP-OCRv6_small_rec",
        )
        self.assertFalse(created_arguments[0]["use_doc_orientation_classify"])
        self.assertFalse(created_arguments[0]["use_doc_unwarping"])
        self.assertFalse(created_arguments[0]["use_textline_orientation"])

    def test_load_pipeline_rejects_unavailable_cuda_device(self):
        paddle_module = types.SimpleNamespace(
            is_compiled_with_cuda=lambda: True,
            device=types.SimpleNamespace(
                cuda=types.SimpleNamespace(device_count=lambda: 1)
            ),
        )
        paddleocr_module = types.SimpleNamespace(PaddleOCR=mock.Mock())
        with mock.patch.dict(
            sys.modules,
            {"paddle": paddle_module, "paddleocr": paddleocr_module},
        ):
            with self.assertRaisesRegex(RuntimeError, "device 1 is unavailable"):
                COMMON.load_paddleocr_pipeline(
                    Path("/models/detection"),
                    Path("/models/recognition"),
                    "gpu:1",
                    4,
                )

    def test_paddleocr_regions_filter_text_and_normalize_clipped_boxes(self):
        prediction = {
            "rec_texts": [" kept ", "low confidence", "x" * 5000],
            "rec_scores": [0.9, 0.2, 0.95],
            "rec_boxes": [[-10, 20, 110, 80], [0, 0, 50, 50], [0, 0, 10, 10]],
        }

        regions = COMMON.paddleocr_text_regions(prediction, 100, 100)

        self.assertEqual(
            regions,
            [
                {
                    "text": "kept",
                    "confidence": 0.9,
                    "x": 0.0,
                    "y": 0.2,
                    "width": 1.0,
                    "height": 0.6,
                }
            ],
        )

    def test_paddleocr_regions_reject_mismatched_result_fields(self):
        prediction = {
            "rec_texts": ["text"],
            "rec_scores": [],
            "rec_boxes": [[0, 0, 10, 10]],
        }

        with self.assertRaisesRegex(RuntimeError, "different lengths"):
            COMMON.paddleocr_text_regions(prediction, 100, 100)

    def test_concurrent_requests_use_one_pipeline_call_and_one_worker_thread(self):
        request_count = 4
        predictions = [
            paddleocr_prediction(f"text-{index}", 0.9, [0, 0, 20, 10])
            for index in range(request_count)
        ]
        pipeline = FakePaddleOCRPipeline(predictions)
        processor = COMMON.PaddleOCRBatchProcessor(pipeline, request_count, 0.05)
        start_barrier = threading.Barrier(request_count)
        responses = [None] * request_count

        def submit_request(request_index):
            start_barrier.wait()
            responses[request_index] = processor.infer(
                Image.new("RGB", (20, 10), color=(10, 20, 30))
            )

        request_threads = [
            threading.Thread(target=submit_request, args=(request_index,))
            for request_index in range(request_count)
        ]
        for request_thread in request_threads:
            request_thread.start()
        for request_thread in request_threads:
            request_thread.join()
        processor.close()

        self.assertEqual(len(pipeline.batches), 1)
        self.assertEqual(len(pipeline.batches[0]), request_count)
        self.assertEqual(len(set(pipeline.worker_thread_ids)), 1)
        self.assertNotIn(
            pipeline.worker_thread_ids[0],
            [request_thread.ident for request_thread in request_threads],
        )
        self.assertTrue(pipeline.closed)
        self.assertEqual(
            sorted(response[0]["text"] for response in responses),
            [f"text-{index}" for index in range(request_count)],
        )

    def test_pipeline_failure_is_delivered_to_every_request_without_hanging(self):
        pipeline = FakePaddleOCRPipeline([])
        processor = COMMON.PaddleOCRBatchProcessor(pipeline, 1, 0.0)

        with self.assertRaisesRegex(RuntimeError, "different number of results"):
            processor.infer(Image.new("RGB", (20, 10)))
        processor.close()

        self.assertTrue(pipeline.closed)
        with self.assertRaisesRegex(RuntimeError, "processor is closed"):
            processor.infer(Image.new("RGB", (20, 10)))


if __name__ == "__main__":
    unittest.main()
