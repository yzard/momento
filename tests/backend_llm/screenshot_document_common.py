import importlib.util
import sys
import tempfile
import threading
import types
import unittest
from pathlib import Path
from unittest import mock

import numpy
from PIL import Image

SOURCE_DIRECTORY = Path(__file__).resolve().parents[2] / "src" / "backend_llm"
sys.path.insert(0, str(SOURCE_DIRECTORY))
SOURCE_PATH = SOURCE_DIRECTORY / "screenshot_document_common.py"
MODULE_NAME = "screenshot_document_common_test_source"
SPECIFICATION = importlib.util.spec_from_file_location(MODULE_NAME, SOURCE_PATH)
COMMON = importlib.util.module_from_spec(SPECIFICATION)
sys.modules[MODULE_NAME] = COMMON
SPECIFICATION.loader.exec_module(COMMON)


def quadrilateral(left, top, right, bottom):
    return [[left, top], [right, top], [right, bottom], [left, bottom]]


class FakePaddleOCRModelComponents:
    def __init__(self, load_recognition_model):
        self.recognition_model = object() if load_recognition_model else None
        self.detection_batches = []
        self.recognition_batches = []
        self.crop_calls = []
        self.closed = False
        self.detect_callback = None

    def detect(self, model_images):
        self.detection_batches.append(model_images)
        if self.detect_callback is not None:
            return self.detect_callback(model_images)
        return [{"dt_polys": [], "dt_scores": []} for _ in model_images]

    def recognize(self, text_crops):
        self.recognition_batches.append(text_crops)
        return [{"rec_text": f"text-{index}", "rec_score": 0.9} for index, _text_crop in enumerate(text_crops)]

    def crop(self, model_image, polygons):
        self.crop_calls.append((model_image, polygons))
        return [numpy.asarray(polygon) for polygon in polygons]

    def close(self):
        self.closed = True


class ScreenshotDocumentCommonTests(unittest.TestCase):
    def test_prepare_detection_input_letterboxes_landscape_and_portrait_images(self):
        landscape = Image.new("RGB", (40, 30), color=(10, 20, 30))
        portrait = Image.new("RGB", (30, 40), color=(10, 20, 30))

        prepared_landscape = COMMON.prepare_detection_input(landscape, 128)
        prepared_portrait = COMMON.prepare_detection_input(portrait, 128)

        self.assertIs(prepared_landscape.image, landscape)
        self.assertEqual(prepared_landscape.model_image.shape, (128, 128, 3))
        self.assertEqual(
            (
                prepared_landscape.content_left,
                prepared_landscape.content_top,
                prepared_landscape.content_width,
                prepared_landscape.content_height,
            ),
            (0, 16, 128, 96),
        )
        self.assertEqual(
            (
                prepared_portrait.content_left,
                prepared_portrait.content_top,
                prepared_portrait.content_width,
                prepared_portrait.content_height,
            ),
            (16, 0, 96, 128),
        )
        numpy.testing.assert_array_equal(prepared_landscape.model_image[0, 0], [255, 255, 255])
        numpy.testing.assert_array_equal(prepared_landscape.model_image[16, 0], [30, 20, 10])

    def test_prepare_detection_input_rejects_non_positive_model_size(self):
        with self.assertRaisesRegex(ValueError, "model_image_size must be positive"):
            COMMON.prepare_detection_input(Image.new("RGB", (20, 10)), 0)

    def test_normalized_text_region_removes_letterbox_coordinates(self):
        prepared_input = COMMON.prepare_detection_input(Image.new("RGB", (40, 30)), 128)

        text_region = COMMON.normalized_text_region(prepared_input, quadrilateral(32, 16, 96, 64), 0.8)

        self.assertEqual(text_region, {"text": "", "confidence": 0.8, "x": 0.25, "y": 0.0, "width": 0.5, "height": 0.5})

    def test_normalized_text_region_rejects_invalid_polygon_and_confidence(self):
        prepared_input = COMMON.prepare_detection_input(Image.new("RGB", (20, 10)), 128)

        with self.assertRaisesRegex(RuntimeError, "four points"):
            COMMON.normalized_text_region(prepared_input, [[0, 0]], 0.8)
        with self.assertRaisesRegex(RuntimeError, "must be finite"):
            COMMON.normalized_text_region(prepared_input, quadrilateral(0, 0, float("nan"), 10), 0.8)
        with self.assertRaisesRegex(RuntimeError, "between zero and one"):
            COMMON.normalized_text_region(prepared_input, quadrilateral(0, 0, 10, 10), 1.1)

    def test_model_components_submit_real_detection_and_recognition_batches(self):
        detection_calls = []
        recognition_calls = []

        class FakePredictor:
            def __init__(self, calls, output_factory):
                self.calls = calls
                self.output_factory = output_factory
                self.closed = False

            def __call__(self, inputs, **arguments):
                self.calls.append((inputs, arguments))
                return [self.output_factory(index) for index, _input_value in enumerate(inputs)]

            def close(self):
                self.closed = True

        class FakeCropper:
            def __call__(self, _image, polygons):
                return [f"crop-{index}" for index, _polygon in enumerate(polygons)]

        detection_model = FakePredictor(detection_calls, lambda _index: {"dt_polys": [], "dt_scores": []})
        recognition_model = FakePredictor(
            recognition_calls, lambda index: {"rec_text": f"text-{index}", "rec_score": 0.9}
        )
        components = COMMON.PaddleOCRModelComponents(detection_model, recognition_model, FakeCropper())

        detection_results = components.detect(["image-1", "image-2"])
        recognition_results = components.recognize(["crop-1", "crop-2"])
        crops = components.crop("image", ["polygon-1", "polygon-2"])
        components.close()

        self.assertEqual(len(detection_results), 2)
        self.assertEqual(detection_calls[0][1]["batch_size"], 2)
        self.assertEqual(detection_calls[0][1]["max_side_limit"], COMMON.PADDLEOCR_MODEL_IMAGE_SIZE)
        self.assertEqual(len(recognition_results), 2)
        self.assertEqual(recognition_calls[0][1]["batch_size"], 2)
        self.assertEqual(crops, ["crop-0", "crop-1"])
        self.assertTrue(detection_model.closed)
        self.assertTrue(recognition_model.closed)

    def test_model_components_reject_mismatched_model_result_counts(self):
        model = mock.Mock(return_value=[])
        components = COMMON.PaddleOCRModelComponents(model, model, mock.Mock(return_value=[]))

        with self.assertRaisesRegex(RuntimeError, "different number of results"):
            components.detect(["image"])
        with self.assertRaisesRegex(RuntimeError, "different number of results"):
            components.recognize(["crop"])

    def test_load_models_creates_one_detector_and_optional_recognizer_on_cuda(self):
        created_predictors = []

        class FakePredictor:
            def close(self):
                return None

        def create_predictor(**arguments):
            created_predictors.append(arguments)
            return FakePredictor()

        class FakeCropByPolys:
            def __init__(self, **arguments):
                self.arguments = arguments

        modules = self.paddlex_modules(create_predictor, FakeCropByPolys, cuda_device_count=1)
        with tempfile.TemporaryDirectory() as model_root:
            detection_model_path = Path(model_root) / "detection"
            recognition_model_path = Path(model_root) / "recognition"
            detection_model_path.mkdir()
            recognition_model_path.mkdir()
            with mock.patch.dict(sys.modules, modules):
                models = COMMON.load_paddleocr_models(detection_model_path, recognition_model_path, "gpu:0", 8, True)

        self.assertEqual(
            [arguments["model_name"] for arguments in created_predictors], ["PP-OCRv6_small_det", "PP-OCRv6_small_rec"]
        )
        self.assertEqual([arguments["batch_size"] for arguments in created_predictors], [8, 8])
        self.assertIsNotNone(models.recognition_model)

    def test_document_model_load_does_not_require_or_create_recognizer(self):
        created_predictors = []

        def create_predictor(**arguments):
            created_predictors.append(arguments)
            return mock.Mock()

        modules = self.paddlex_modules(create_predictor, mock.Mock, cuda_device_count=1)
        with tempfile.TemporaryDirectory() as model_root:
            detection_model_path = Path(model_root) / "detection"
            detection_model_path.mkdir()
            missing_recognition_model_path = Path(model_root) / "missing-recognition"
            with mock.patch.dict(sys.modules, modules):
                models = COMMON.load_paddleocr_models(
                    detection_model_path, missing_recognition_model_path, "gpu:0", 8, False
                )

        self.assertEqual(len(created_predictors), 1)
        self.assertEqual(created_predictors[0]["model_name"], "PP-OCRv6_small_det")
        self.assertIsNone(models.recognition_model)

    def test_load_models_rejects_unavailable_cuda_device(self):
        modules = self.paddlex_modules(mock.Mock(), mock.Mock, cuda_device_count=1)
        with mock.patch.dict(sys.modules, modules):
            with self.assertRaisesRegex(RuntimeError, "device 1 is unavailable"):
                COMMON.load_paddleocr_models(Path("/models/detection"), Path("/models/recognition"), "gpu:1", 8, True)

    def test_screenshot_runtime_recognizes_only_selected_regions(self):
        components = FakePaddleOCRModelComponents(load_recognition_model=True)
        detector = mock.Mock(return_value={"detected": True, "confidence": 0.9})
        runtime = COMMON.DetectionRuntime(
            detector, lambda region: region["y"] + region["height"] / 2.0 <= 0.13, components, 2, 2, 0
        )
        prepared_input = COMMON.prepare_detection_input(Image.new("RGB", (100, 200)), 1280)
        detection_prediction = {
            "dt_polys": [quadrilateral(320, 20, 600, 100), quadrilateral(320, 800, 600, 900)],
            "dt_scores": [0.8, 0.85],
        }

        try:
            prepared_layout = runtime.prepare_text_layout(prepared_input, detection_prediction)
            recognition_predictions = [{"rec_text": " 09:41 ", "rec_score": 0.95}]
            response = runtime.classify(prepared_input, prepared_layout, recognition_predictions)
        finally:
            runtime.close()

        self.assertEqual(response, {"detected": True, "confidence": 0.9})
        self.assertEqual(len(components.crop_calls), 1)
        self.assertEqual(len(components.crop_calls[0][1]), 1)
        regions = detector.call_args.args[1]
        self.assertEqual(regions[0]["text"], "09:41")
        self.assertEqual(regions[0]["confidence"], 0.95)
        self.assertEqual(regions[1]["text"], "")

    def test_document_runtime_uses_detection_geometry_without_recognition(self):
        components = FakePaddleOCRModelComponents(load_recognition_model=False)
        detector = mock.Mock(return_value={"detected": True, "confidence": 0.8})
        runtime = COMMON.DetectionRuntime(detector, None, components, 2, 2, 0)
        prepared_input = COMMON.prepare_detection_input(Image.new("RGB", (100, 200)), 1280)
        detection_prediction = {"dt_polys": [quadrilateral(320, 100, 600, 200)], "dt_scores": [0.85]}

        try:
            prepared_layout = runtime.prepare_text_layout(prepared_input, detection_prediction)
            response = runtime.classify(prepared_input, prepared_layout, [])
        finally:
            runtime.close()

        self.assertEqual(response, {"detected": True, "confidence": 0.8})
        self.assertEqual(components.crop_calls, [])
        self.assertEqual(components.recognition_batches, [])
        self.assertEqual(detector.call_args.args[1][0]["text"], "")

    def test_detection_pipeline_runs_cpu_and_gpu_stages_in_order(self):
        events = []

        class Slot:
            def __init__(self, name):
                self.name = name

            def __enter__(self):
                events.append(f"{self.name} acquired")

            def __exit__(self, _exception_type, _exception, _traceback):
                events.append(f"{self.name} released")

        class ImageSource:
            def __enter__(self):
                events.append("opened")
                return b"image"

            def __exit__(self, _exception_type, _exception, _traceback):
                events.append("closed")

        class Runtime:
            pipeline_slots = Slot("pipeline")
            processing_slots = Slot("cpu")

            def prepare_input(self, image_source):
                events.append("prepared")
                return image_source

            def detect(self, prepared_input):
                events.append("detected")
                return prepared_input

            def prepare_text_layout(self, prepared_input, detection_prediction):
                events.append("layout prepared")
                return types.SimpleNamespace(
                    source=prepared_input, prediction=detection_prediction, recognition_crops=[b"crop"]
                )

            def recognize(self, recognition_crops):
                events.append("recognized")
                return recognition_crops

            def classify(self, prepared_input, prepared_layout, recognition_predictions):
                events.append("classified")
                return prepared_input, prepared_layout.prediction, recognition_predictions

        handler = types.SimpleNamespace(input_root=Path("/inputs"), runtime=Runtime())
        with mock.patch.object(COMMON, "read_runtime_input", return_value=ImageSource()):
            response = COMMON.run_detection_pipeline(handler)

        self.assertEqual(response, (b"image", b"image", [b"crop"]))
        self.assertEqual(
            events,
            [
                "pipeline acquired",
                "cpu acquired",
                "opened",
                "prepared",
                "closed",
                "cpu released",
                "detected",
                "cpu acquired",
                "layout prepared",
                "cpu released",
                "recognized",
                "cpu acquired",
                "classified",
                "cpu released",
                "pipeline released",
            ],
        )

    def test_concurrent_requests_are_combined_into_one_detection_model_batch(self):
        request_count = 4
        components = FakePaddleOCRModelComponents(load_recognition_model=False)
        runtime = COMMON.DetectionRuntime(mock.Mock(), None, components, 2, request_count, 100)
        prepared_input = COMMON.prepare_detection_input(Image.new("RGB", (20, 10)), 1280)
        start_barrier = threading.Barrier(request_count + 1)
        responses = []

        def detect():
            start_barrier.wait()
            responses.append(runtime.detect(prepared_input))

        request_threads = [threading.Thread(target=detect) for _request_index in range(request_count)]
        try:
            for request_thread in request_threads:
                request_thread.start()
            start_barrier.wait()
            for request_thread in request_threads:
                request_thread.join(2.0)
        finally:
            runtime.close()

        self.assertTrue(all(not request_thread.is_alive() for request_thread in request_threads))
        self.assertEqual(len(responses), request_count)
        self.assertEqual(len(components.detection_batches), 1)
        self.assertEqual(len(components.detection_batches[0]), request_count)

    def test_recognition_crops_from_concurrent_requests_share_one_model_batch(self):
        request_count = 4
        components = FakePaddleOCRModelComponents(load_recognition_model=True)
        runtime = COMMON.DetectionRuntime(mock.Mock(), lambda _region: True, components, 2, request_count, 100)
        start_barrier = threading.Barrier(request_count + 1)
        responses = []

        def recognize(crop_index):
            start_barrier.wait()
            responses.append(runtime.recognize([f"crop-{crop_index}"])[0])

        request_threads = [threading.Thread(target=recognize, args=(index,)) for index in range(request_count)]
        try:
            for request_thread in request_threads:
                request_thread.start()
            start_barrier.wait()
            for request_thread in request_threads:
                request_thread.join(2.0)
        finally:
            runtime.close()

        self.assertTrue(all(not request_thread.is_alive() for request_thread in request_threads))
        self.assertEqual(len(responses), request_count)
        self.assertEqual(len(components.recognition_batches), 1)
        self.assertEqual(len(components.recognition_batches[0]), request_count)

    def test_double_buffer_prepares_one_following_request_while_gpu_is_busy(self):
        first_detection_started = threading.Event()
        release_first_detection = threading.Event()
        sources = (b"first", b"second", b"third")
        prepared_events = {source: threading.Event() for source in sources}
        components = FakePaddleOCRModelComponents(load_recognition_model=False)

        def blocking_detection(model_images):
            if not first_detection_started.is_set():
                first_detection_started.set()
                if not release_first_detection.wait(2.0):
                    raise RuntimeError("test timed out waiting to release detection")
            return [{"dt_polys": [], "dt_scores": []} for _model_image in model_images]

        components.detect_callback = blocking_detection
        runtime = COMMON.DetectionRuntime(
            mock.Mock(return_value={"detected": False, "confidence": 0.0}), None, components, 1, 1, 0
        )

        class ImageSource:
            def __init__(self, source):
                self.source = source

            def __enter__(self):
                return self.source

            def __exit__(self, _exception_type, _exception, _traceback):
                return None

        def decode_source(source):
            prepared_events[source].set()
            return Image.new("RGB", (20, 10), color=(10, 20, 30))

        handlers = [
            types.SimpleNamespace(runtime=runtime, input_root=Path("/inputs"), source=source) for source in sources
        ]
        responses = []
        errors = []

        def run_request(handler):
            try:
                responses.append(COMMON.run_detection_pipeline(handler))
            except RuntimeError as error:
                errors.append(error)

        request_threads = []
        try:
            with mock.patch.object(COMMON, "decode_image", side_effect=decode_source), mock.patch.object(
                COMMON, "read_runtime_input", side_effect=lambda handler, _input_root: ImageSource(handler.source)
            ):
                first_thread = threading.Thread(target=run_request, args=(handlers[0],))
                request_threads.append(first_thread)
                first_thread.start()
                self.assertTrue(first_detection_started.wait(1.0))

                second_thread = threading.Thread(target=run_request, args=(handlers[1],))
                request_threads.append(second_thread)
                second_thread.start()
                self.assertTrue(prepared_events[b"second"].wait(1.0))

                third_thread = threading.Thread(target=run_request, args=(handlers[2],))
                request_threads.append(third_thread)
                third_thread.start()
                self.assertFalse(prepared_events[b"third"].wait(0.1))

                release_first_detection.set()
                for request_thread in request_threads:
                    request_thread.join(3.0)
        finally:
            release_first_detection.set()
            runtime.close()

        self.assertEqual(errors, [])
        self.assertEqual(len(responses), 3)
        self.assertTrue(all(not request_thread.is_alive() for request_thread in request_threads))
        self.assertTrue(prepared_events[b"third"].is_set())

    @staticmethod
    def paddlex_modules(create_predictor, crop_by_polys, cuda_device_count):
        paddle_module = types.ModuleType("paddle")
        paddle_module.is_compiled_with_cuda = lambda: True
        paddle_module.device = types.SimpleNamespace(cuda=types.SimpleNamespace(device_count=lambda: cuda_device_count))
        paddlex_module = types.ModuleType("paddlex")
        paddlex_module.__path__ = []
        inference_module = types.ModuleType("paddlex.inference")
        inference_module.__path__ = []
        models_module = types.ModuleType("paddlex.inference.models")
        models_module.create_predictor = create_predictor
        pipelines_module = types.ModuleType("paddlex.inference.pipelines")
        pipelines_module.__path__ = []
        components_module = types.ModuleType("paddlex.inference.pipelines.components")
        components_module.CropByPolys = crop_by_polys
        return {
            "paddle": paddle_module,
            "paddlex": paddlex_module,
            "paddlex.inference": inference_module,
            "paddlex.inference.models": models_module,
            "paddlex.inference.pipelines": pipelines_module,
            "paddlex.inference.pipelines.components": components_module,
        }


if __name__ == "__main__":
    unittest.main()
