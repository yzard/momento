import base64
import io
import importlib.util
import struct
import unittest
from pathlib import Path


SOURCE_PATH = (
    Path(__file__).resolve().parents[2]
    / "src"
    / "backend_llm"
    / "image_clustering_server.py"
)
SPECIFICATION = importlib.util.spec_from_file_location(
    "image_clustering_server", SOURCE_PATH
)
IMAGE_CLUSTERING_SERVER = importlib.util.module_from_spec(SPECIFICATION)
SPECIFICATION.loader.exec_module(IMAGE_CLUSTERING_SERVER)


class ImageClusteringServerTests(unittest.TestCase):
    def test_encodes_float32_values_in_little_endian_order(self):
        encoded = IMAGE_CLUSTERING_SERVER.encode_float32_le([1.0, -0.5])

        self.assertEqual(base64.b64decode(encoded), struct.pack("<ff", 1.0, -0.5))

    def test_perceptual_hash_and_quality_score_are_bounded(self):
        try:
            from PIL import Image
        except ImportError:
            self.skipTest("Pillow is not installed")

        image = Image.new("RGB", (16, 16), color=(128, 128, 128))

        perceptual_hash = IMAGE_CLUSTERING_SERVER.calculate_perceptual_hash(image)
        quality_score = IMAGE_CLUSTERING_SERVER.calculate_quality_score(image)

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

        self.assertEqual(
            IMAGE_CLUSTERING_SERVER.calculate_perceptual_hash(image),
            "0000000000000000",
        )

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

        image = IMAGE_CLUSTERING_SERVER.decode_image(truncated)

        self.assertEqual(image.mode, "RGB")
        self.assertEqual(image.size, (32, 32))


if __name__ == "__main__":
    unittest.main()
