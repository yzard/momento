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
    def test_runtime_prepares_ocr_input_and_calls_the_supplied_detector(self):
        decoded_image = Image.new("RGB", (20, 10), color=(10, 20, 30))
        text_regions = [{"text": "visible"}]
        expected_response = {"detected": True, "confidence": 0.9}
        detector = mock.Mock(return_value=expected_response)
        text_region_extractor = mock.Mock()
        text_region_extractor.infer.return_value = text_regions
        with mock.patch.object(COMMON, "decode_image", return_value=decoded_image):
            runtime = COMMON.DetectionRuntime(detector, text_region_extractor)
            prepared_input = runtime.prepare_input(b"image")
            response = runtime.infer(prepared_input)

        self.assertEqual(response, expected_response)
        paddle_image = text_region_extractor.infer.call_args.args[0]
        self.assertEqual(paddle_image.shape, (10, 20, 3))
        text_region_extractor.infer.assert_called_once_with(paddle_image, 20, 10)
        detector.assert_called_once_with(decoded_image, text_regions)

    def test_cpu_processing_slot_is_released_before_model_inference(self):
        events = []

        class Slot:
            def __init__(self, name):
                self.name = name

            def __enter__(self):
                events.append(f"{self.name} acquired")

            def __exit__(self, exception_type, exception, traceback):
                events.append(f"{self.name} released")

        class ImageSource:
            def __enter__(self):
                events.append("opened")
                return b"image"

            def __exit__(self, exception_type, exception, traceback):
                events.append("closed")

        class Runtime:
            def prepare_input(self, image_source):
                events.append("resized")
                return image_source

            def infer(self, prepared_input):
                events.append("model inference")
                return prepared_input

        handler = types.SimpleNamespace(
            cpu_processing_slots=Slot("cpu"),
            model_slots=Slot("model"),
            input_root=Path("/inputs"),
            runtime=Runtime(),
        )
        with mock.patch.object(
            COMMON, "read_runtime_input", return_value=ImageSource()
        ):
            response = COMMON.run_bounded_model_inference(handler)

        self.assertEqual(response, b"image")
        self.assertEqual(
            events,
            [
                "model acquired",
                "cpu acquired",
                "opened",
                "resized",
                "closed",
                "cpu released",
                "model inference",
                "model released",
            ],
        )

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
        self.assertNotIn("batch_size", configuration["SubModules"]["TextDetection"])
        self.assertEqual(
            configuration["SubModules"]["TextRecognition"]["batch_size"], 8
        )
        self.assertEqual(
            configuration["SubModules"]["TextDetection"]["max_side_limit"],
            COMMON.PADDLEOCR_MAX_SIDE_LENGTH,
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

    def test_resize_for_paddleocr_limits_landscape_and_portrait_images(self):
        landscape = Image.new("RGB", (4032, 3024))
        portrait = Image.new("RGB", (3024, 4032))

        resized_landscape = COMMON.resize_for_paddleocr(landscape, 4000)
        resized_portrait = COMMON.resize_for_paddleocr(portrait, 4000)

        self.assertEqual(resized_landscape.size, (4000, 3000))
        self.assertEqual(resized_portrait.size, (3000, 4000))

    def test_resize_for_paddleocr_does_not_upscale_smaller_images(self):
        image = Image.new("RGB", (2000, 1500))

        resized_image = COMMON.resize_for_paddleocr(image, 4000)

        self.assertIs(resized_image, image)

    def test_resize_for_paddleocr_rejects_non_positive_limit(self):
        with self.assertRaisesRegex(ValueError, "max_side_length must be positive"):
            COMMON.resize_for_paddleocr(Image.new("RGB", (20, 10)), 0)

    def test_concurrent_requests_use_one_pipeline_call_and_one_worker_thread(self):
        request_count = 4
        predictions = [
            paddleocr_prediction(f"text-{index}", 0.9, [0, 0, 20, 10])
            for index in range(request_count)
        ]
        pipeline = FakePaddleOCRPipeline(predictions)
        processor = COMMON.PaddleOCRBatchProcessor(
            pipeline,
            request_count,
            0.05,
        )
        start_barrier = threading.Barrier(request_count)
        responses = [None] * request_count

        def submit_request(request_index):
            start_barrier.wait()
            image = Image.new("RGB", (20, 10), color=(10, 20, 30))
            responses[request_index] = processor.infer(
                COMMON.image_to_paddle_array(image),
                image.width,
                image.height,
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
        processor = COMMON.PaddleOCRBatchProcessor(
            pipeline,
            1,
            0.0,
        )

        with self.assertRaisesRegex(RuntimeError, "different number of results"):
            image = Image.new("RGB", (20, 10))
            processor.infer(
                COMMON.image_to_paddle_array(image),
                image.width,
                image.height,
            )
        processor.close()

        self.assertTrue(pipeline.closed)
        with self.assertRaisesRegex(RuntimeError, "processor is closed"):
            processor.infer(
                COMMON.image_to_paddle_array(image),
                image.width,
                image.height,
            )

    def test_batch_processor_normalizes_boxes_against_resized_ocr_image(self):
        pipeline = FakePaddleOCRPipeline(
            [paddleocr_prediction("text", 0.9, [0, 0, 40, 30])]
        )
        processor = COMMON.PaddleOCRBatchProcessor(pipeline, 1, 0.0)
        resized_image = COMMON.resize_for_paddleocr(
            Image.new("RGB", (4032, 3024)),
            40,
        )

        regions = processor.infer(
            COMMON.image_to_paddle_array(resized_image),
            resized_image.width,
            resized_image.height,
        )
        processor.close()

        self.assertEqual(pipeline.batches[0][0].shape, (30, 40, 3))
        self.assertEqual(regions[0]["width"], 1.0)
        self.assertEqual(regions[0]["height"], 1.0)


if __name__ == "__main__":
    unittest.main()
