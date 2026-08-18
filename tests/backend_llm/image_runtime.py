import importlib.util
import io
import unittest
from pathlib import Path


SOURCE_PATH = (
    Path(__file__).resolve().parents[2] / "src" / "backend_llm" / "image_runtime.py"
)
SPECIFICATION = importlib.util.spec_from_file_location("image_runtime", SOURCE_PATH)
IMAGE_RUNTIME = importlib.util.module_from_spec(SPECIFICATION)
SPECIFICATION.loader.exec_module(IMAGE_RUNTIME)
InvalidImageError = IMAGE_RUNTIME.InvalidImageError
ModelHTTPServer = IMAGE_RUNTIME.ModelHTTPServer
create_inference_slots = IMAGE_RUNTIME.create_inference_slots
decode_image = IMAGE_RUNTIME.decode_image
select_cuda_device = IMAGE_RUNTIME.select_cuda_device
serve_until_stopped = IMAGE_RUNTIME.serve_until_stopped


class ImageRuntimeTests(unittest.TestCase):
    def test_model_concurrency_is_bounded(self):
        slots = IMAGE_RUNTIME.create_inference_slots(2)

        self.assertTrue(slots.acquire(blocking=False))
        self.assertTrue(slots.acquire(blocking=False))
        self.assertFalse(slots.acquire(blocking=False))
        slots.release()
        slots.release()

    def test_cuda_selection_rejects_an_unavailable_gpu(self):
        class FakeCuda:
            @staticmethod
            def is_available():
                return False

        class FakeTorch:
            cuda = FakeCuda()

        with self.assertRaisesRegex(RuntimeError, "image aesthetics.*NVIDIA CUDA GPU"):
            IMAGE_RUNTIME.select_cuda_device("cuda:0", FakeTorch(), "image aesthetics")

    def test_decode_image_preserves_the_complete_aspect_ratio(self):
        try:
            from PIL import Image
        except ImportError:
            self.skipTest("Pillow is not installed")

        encoded = io.BytesIO()
        Image.new("RGB", (120, 40), color=(10, 20, 30)).save(encoded, format="PNG")

        decoded = IMAGE_RUNTIME.decode_image(encoded.getvalue())

        self.assertEqual(decoded.mode, "RGB")
        self.assertEqual(decoded.size, (120, 40))


if __name__ == "__main__":
    unittest.main()
