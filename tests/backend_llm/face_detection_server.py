import base64
import hashlib
import http.client
import importlib.util
import json
import struct
import tempfile
import threading
import unittest
from pathlib import Path


SOURCE_PATH = (
    Path(__file__).resolve().parents[2]
    / "src"
    / "backend_llm"
    / "face_detection_server.py"
)
SPECIFICATION = importlib.util.spec_from_file_location(
    "face_detection_server", SOURCE_PATH
)
FACE_DETECTION_SERVER = importlib.util.module_from_spec(SPECIFICATION)
SPECIFICATION.loader.exec_module(FACE_DETECTION_SERVER)


class FaceDetectionServerTests(unittest.TestCase):
    def test_encodes_float32_values_in_little_endian_order(self):
        encoded = FACE_DETECTION_SERVER.encode_float32_le([1.0, -0.5])

        self.assertEqual(base64.b64decode(encoded), struct.pack("<ff", 1.0, -0.5))

    def test_normalizes_and_clamps_face_bounding_box(self):
        bounding_box = FACE_DETECTION_SERVER.normalized_bounding_box(
            [-10.0, 10.0, 210.0, 110.0], 200, 100
        )

        self.assertEqual(
            bounding_box, {"x": 0.0, "y": 0.1, "width": 1.0, "height": 0.9}
        )

    def test_quality_score_is_bounded(self):
        score = FACE_DETECTION_SERVER.quality_score(
            0.9, {"x": 0.1, "y": 0.1, "width": 0.25, "height": 0.25}
        )

        self.assertGreaterEqual(score, 0.0)
        self.assertLessEqual(score, 1.0)

    def test_normalizes_eye_center_from_first_two_landmarks(self):
        eye_center = FACE_DETECTION_SERVER.normalized_eye_center(
            [[40.0, 20.0], [60.0, 24.0], [50.0, 35.0]], 100, 80
        )

        self.assertEqual(eye_center, {"x": 0.5, "y": 0.275})

    def test_face_thresholds_filter_low_likelihood_and_resolution(self):
        bounding_box = {"x": 0.1, "y": 0.1, "width": 0.2, "height": 0.25}

        self.assertTrue(
            FACE_DETECTION_SERVER.face_meets_thresholds(
                0.9, bounding_box, 1000, 800, 0.8, 112
            )
        )
        self.assertFalse(
            FACE_DETECTION_SERVER.face_meets_thresholds(
                0.79, bounding_box, 1000, 800, 0.8, 112
            )
        )
        self.assertFalse(
            FACE_DETECTION_SERVER.face_meets_thresholds(
                0.9, bounding_box, 500, 400, 0.8, 112
            )
        )

    def test_select_providers_requires_cuda_execution_provider(self):
        class FakeOnnxRuntime:
            @staticmethod
            def get_available_providers():
                return ["CPUExecutionProvider"]

        with self.assertRaisesRegex(RuntimeError, "NVIDIA CUDA GPU"):
            FACE_DETECTION_SERVER.select_providers(FakeOnnxRuntime())

    def test_runtime_loads_only_detection_and_recognition_models(self):
        self.assertEqual(
            FACE_DETECTION_SERVER.REQUIRED_MODULES,
            ["detection", "recognition"],
        )

    def test_inference_endpoint_reads_the_queued_image_descriptor(self):
        class RecordingRuntime:
            received = None

            def infer(self, image_bytes):
                self.received = image_bytes
                return {"faces": []}

        runtime = RecordingRuntime()
        with tempfile.TemporaryDirectory() as directory:
            input_root = Path(directory)
            job_id = "abcdef12"
            image_bytes = b"queued-image"
            (input_root / job_id).mkdir()
            (input_root / job_id / "input-0").write_bytes(image_bytes)
            descriptor = json.dumps(
                {
                    "jobId": job_id,
                    "sequence": 0,
                    "byteSize": len(image_bytes),
                    "contentHash": hashlib.sha256(image_bytes).hexdigest(),
                    "mimeType": "image/jpeg",
                }
            ).encode()
            FACE_DETECTION_SERVER.Handler.runtime = runtime
            FACE_DETECTION_SERVER.Handler.inference_slots = (
                FACE_DETECTION_SERVER.create_inference_slots(1)
            )
            FACE_DETECTION_SERVER.Handler.input_root = input_root
            server = FACE_DETECTION_SERVER.ThreadingHTTPServer(
                ("127.0.0.1", 0), FACE_DETECTION_SERVER.Handler
            )
            server_thread = threading.Thread(target=server.serve_forever)
            server_thread.start()
            try:
                connection = http.client.HTTPConnection(
                    "127.0.0.1", server.server_address[1]
                )
                connection.request(
                    "POST",
                    "/infer",
                    body=descriptor,
                    headers={"Content-Type": "application/json"},
                )
                response = connection.getresponse()
                response.read()
                connection.close()
            finally:
                server.shutdown()
                server.server_close()
                server_thread.join()

        self.assertEqual(response.status, 200)
        self.assertEqual(runtime.received, b"queued-image")

    def test_model_concurrency_is_bounded_inside_runtime(self):
        slots = FACE_DETECTION_SERVER.create_inference_slots(2)

        self.assertTrue(slots.acquire(blocking=False))
        self.assertTrue(slots.acquire(blocking=False))
        self.assertFalse(slots.acquire(blocking=False))
        slots.release()
        slots.release()

    def test_keyboard_interrupt_closes_server_without_escaping(self):
        class InterruptedServer:
            def __init__(self):
                self.closed = False

            def serve_forever(self):
                raise KeyboardInterrupt

            def server_close(self):
                self.closed = True

        server = InterruptedServer()
        FACE_DETECTION_SERVER.serve_until_stopped(server)
        self.assertTrue(server.closed)


if __name__ == "__main__":
    unittest.main()
