import base64
import importlib.util
import io
import struct
import unittest
from pathlib import Path
from unittest.mock import patch

SOURCE_PATH = Path(__file__).resolve().parents[2] / "src" / "backend_llm" / "image_clustering_server.py"
SPECIFICATION = importlib.util.spec_from_file_location("image_clustering_server", SOURCE_PATH)
IMAGE_CLUSTERING_SERVER = importlib.util.module_from_spec(SPECIFICATION)
SPECIFICATION.loader.exec_module(IMAGE_CLUSTERING_SERVER)


class ImageClusteringServerTests(unittest.TestCase):
    def test_dinov2_base_contract_uses_768_embedding_dimensions(self):
        self.assertEqual(IMAGE_CLUSTERING_SERVER.EMBEDDING_DIMENSIONS, 768)

    def test_select_device_requires_available_cuda_gpu(self):
        class FakeCuda:
            @staticmethod
            def is_available():
                return False

        class FakeTorch:
            cuda = FakeCuda()

            @staticmethod
            def device(value):
                return value

        with self.assertRaisesRegex(RuntimeError, "NVIDIA CUDA GPU"):
            IMAGE_CLUSTERING_SERVER.select_device("cuda", FakeTorch())

    def test_encodes_float32_values_in_little_endian_order(self):
        encoded = IMAGE_CLUSTERING_SERVER.encode_float32_le([1.0, -0.5])

        self.assertEqual(base64.b64decode(encoded), struct.pack("<ff", 1.0, -0.5))

    def test_perceptual_hash_and_quality_score_are_bounded(self):
        try:
            from PIL import Image
        except ImportError:
            self.skipTest("Pillow is not installed")

        image = Image.new("RGB", (16, 16), color=(128, 128, 128))

        perceptual_hash, quality_score = IMAGE_CLUSTERING_SERVER.calculate_image_metrics(image)

        self.assertEqual(len(perceptual_hash), 16)
        self.assertTrue(all(character in "0123456789abcdef" for character in perceptual_hash))
        self.assertGreaterEqual(quality_score, 0.0)
        self.assertLessEqual(quality_score, 1.0)

    def test_perceptual_hash_uses_stable_image_bytes_api(self):
        try:
            from PIL import Image
        except ImportError:
            self.skipTest("Pillow is not installed")

        image = Image.frombytes("L", (9, 8), bytes(range(72)))

        self.assertEqual(IMAGE_CLUSTERING_SERVER.calculate_perceptual_hash(image), "0000000000000000")

    def test_image_metrics_bound_quality_analysis_without_changing_hash_input(self):
        try:
            from PIL import Image
        except ImportError:
            self.skipTest("Pillow is not installed")
        image = Image.new("RGB", (1200, 400), color=(128, 128, 128))

        with patch.object(
            IMAGE_CLUSTERING_SERVER, "resize_for_analysis", wraps=IMAGE_CLUSTERING_SERVER.resize_for_analysis
        ) as resize:
            perceptual_hash, quality_score = IMAGE_CLUSTERING_SERVER.calculate_image_metrics(image)

        grayscale_image, maximum_side_length = resize.call_args.args
        self.assertEqual(grayscale_image.size, image.size)
        self.assertEqual(maximum_side_length, 512)
        self.assertEqual(perceptual_hash, "0000000000000000")
        self.assertGreaterEqual(quality_score, 0.0)
        self.assertLessEqual(quality_score, 1.0)

    def test_creates_ordered_responses_for_a_model_batch(self):
        prepared_inputs = [
            IMAGE_CLUSTERING_SERVER.PreparedClusteringInput(None, "0123456789abcdef", 0.25),
            IMAGE_CLUSTERING_SERVER.PreparedClusteringInput(None, "fedcba9876543210", 0.75),
        ]
        first_embedding = [1.0] + [0.0] * 767
        second_embedding = [0.0, 1.0] + [0.0] * 766

        responses = IMAGE_CLUSTERING_SERVER.create_clustering_responses(
            prepared_inputs, [first_embedding, second_embedding]
        )

        self.assertEqual(responses[0]["perceptualHash"], "0123456789abcdef")
        self.assertEqual(responses[0]["qualityScore"], 0.25)
        self.assertEqual(base64.b64decode(responses[0]["embedding"]), struct.pack("<768f", *first_embedding))
        self.assertEqual(responses[1]["perceptualHash"], "fedcba9876543210")
        self.assertEqual(responses[1]["qualityScore"], 0.75)

    def test_rejects_invalid_model_batch_output(self):
        prepared_input = IMAGE_CLUSTERING_SERVER.PreparedClusteringInput(None, "0123456789abcdef", 0.25)

        with self.assertRaisesRegex(RuntimeError, "different number"):
            IMAGE_CLUSTERING_SERVER.create_clustering_responses([prepared_input], [])
        with self.assertRaisesRegex(RuntimeError, "embedding dimensions"):
            IMAGE_CLUSTERING_SERVER.create_clustering_responses([prepared_input], [[1.0]])
        with self.assertRaisesRegex(RuntimeError, "non-finite"):
            IMAGE_CLUSTERING_SERVER.create_clustering_responses([prepared_input], [[float("nan")] + [0.0] * 767])

    def test_extracts_only_the_supported_dinov2_model_input(self):
        pixel_values = object()

        extracted = IMAGE_CLUSTERING_SERVER.extract_dinov2_pixel_values({"pixel_values": pixel_values})

        self.assertIs(extracted, pixel_values)
        with self.assertRaisesRegex(RuntimeError, "unsupported model inputs"):
            IMAGE_CLUSTERING_SERVER.extract_dinov2_pixel_values({"pixel_values": pixel_values, "pixel_mask": object()})

    def test_keyboard_interrupt_closes_server_without_escaping(self):
        class InterruptedServer:
            def __init__(self):
                self.closed = False

            def serve_forever(self):
                raise KeyboardInterrupt

            def server_close(self):
                self.closed = True

        server = InterruptedServer()

        IMAGE_CLUSTERING_SERVER.serve_until_stopped(server)

        self.assertTrue(server.closed)

    def test_decode_image_accepts_truncated_jpeg(self):
        try:
            from PIL import Image
        except ImportError:
            self.skipTest("Pillow is not installed")

        encoded = io.BytesIO()
        Image.new("RGB", (32, 32), color=(20, 40, 60)).save(encoded, format="JPEG")
        truncated = encoded.getvalue()[:-2]

        image = IMAGE_CLUSTERING_SERVER.decode_image(io.BytesIO(truncated))

        self.assertEqual(image.mode, "RGB")
        self.assertEqual(image.size, (32, 32))


if __name__ == "__main__":
    unittest.main()
