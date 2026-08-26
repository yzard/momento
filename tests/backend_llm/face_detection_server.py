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

import numpy

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

    def test_frontality_score_prefers_centered_level_landmarks(self):
        frontal = FACE_DETECTION_SERVER.face_frontality_score(
            [[30, 30], [70, 30], [50, 50], [35, 70], [65, 70]]
        )
        turned = FACE_DETECTION_SERVER.face_frontality_score(
            [[30, 30], [70, 36], [62, 50], [45, 70], [70, 70]]
        )

        self.assertEqual(frontal, 1.0)
        self.assertGreater(frontal, turned)

    def test_select_providers_requires_cuda_execution_provider(self):
        class FakeOnnxRuntime:
            @staticmethod
            def get_available_providers():
                return ["CPUExecutionProvider"]

        with self.assertRaisesRegex(RuntimeError, "NVIDIA CUDA GPU"):
            FACE_DETECTION_SERVER.select_providers(FakeOnnxRuntime())

    def test_runtime_loads_only_detection_and_recognition_models(self):
        self.assertEqual(
            FACE_DETECTION_SERVER.REQUIRED_MODULES, ["detection", "recognition"]
        )
        self.assertEqual(FACE_DETECTION_SERVER.MODEL_NAME, "buffalo_l")
        self.assertEqual(FACE_DETECTION_SERVER.RECOGNITION_INPUT_SIZE, 112)
        self.assertEqual(FACE_DETECTION_SERVER.EMBEDDING_DIMENSIONS, 512)

    def test_detection_size_accepts_only_supported_square_sizes(self):
        self.assertEqual(
            FACE_DETECTION_SERVER.SUPPORTED_FACE_DETECTION_SIZES, {640, 960, 1280}
        )

    def test_filters_small_faces_before_alignment(self):
        aligned_keypoints = []

        def align_face(_image_array, keypoints):
            aligned_keypoints.append(keypoints)
            return keypoints

        keypoints = numpy.asarray(
            [
                [[10, 10], [20, 10], [15, 15], [11, 20], [19, 20]],
                [[100, 100], [200, 100], [150, 150], [110, 200], [190, 200]],
            ],
            dtype=numpy.float32,
        )
        detected_faces = FACE_DETECTION_SERVER.prepare_detected_faces(
            numpy.asarray(
                [[0, 0, 50, 50, 0.99], [50, 50, 250, 250, 0.90]], dtype=numpy.float32
            ),
            keypoints,
            numpy.zeros((1000, 1000, 3), dtype=numpy.uint8),
            1000,
            1000,
            0.60,
            100,
            align_face,
        )

        self.assertEqual(len(detected_faces), 1)
        self.assertEqual(len(aligned_keypoints), 1)
        numpy.testing.assert_array_equal(aligned_keypoints[0], keypoints[1])

    def test_normalizes_recognition_embedding(self):
        embedding = FACE_DETECTION_SERVER.normalize_embedding([3.0, 4.0] + [0.0] * 510)

        self.assertAlmostEqual(embedding[0], 0.6)
        self.assertAlmostEqual(embedding[1], 0.8)
        self.assertAlmostEqual(sum(value * value for value in embedding), 1.0)

    def test_rejects_zero_norm_recognition_embedding(self):
        with self.assertRaisesRegex(RuntimeError, "zero norm"):
            FACE_DETECTION_SERVER.normalize_embedding([0.0] * 512)

    def test_model_directory_must_be_baked_into_the_image(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(RuntimeError, "model is missing"):
                FACE_DETECTION_SERVER.require_model_directory(directory, "buffalo_l")
            model_directory = Path(directory) / "models" / "buffalo_l"
            model_directory.mkdir(parents=True)
            self.assertEqual(
                FACE_DETECTION_SERVER.require_model_directory(directory, "buffalo_l"),
                model_directory,
            )

    def test_inference_endpoint_reads_the_queued_image_descriptor(self):
        class RecordingRuntime:
            received = None

            def infer(self, image_source):
                self.received = image_source.read()
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
            FACE_DETECTION_SERVER.Handler.input_root = input_root
            server = FACE_DETECTION_SERVER.ModelHTTPServer(
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
