import importlib.util
import io
import sys
import types
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
register_image_decoders = IMAGE_RUNTIME.register_image_decoders
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

        encoded.seek(0)
        decoded = IMAGE_RUNTIME.decode_image(encoded)

        self.assertEqual(decoded.mode, "RGB")
        self.assertEqual(decoded.size, (120, 40))

    def test_registers_heif_without_embedded_thumbnails(self):
        calls = []
        pillow_heif = types.ModuleType("pillow_heif")
        pillow_heif.register_heif_opener = lambda **options: calls.append(options)

        previous_module = sys.modules.get("pillow_heif")
        sys.modules["pillow_heif"] = pillow_heif
        try:
            IMAGE_RUNTIME.register_image_decoders()
        finally:
            if previous_module is None:
                del sys.modules["pillow_heif"]
            else:
                sys.modules["pillow_heif"] = previous_module

        self.assertEqual(calls, [{"thumbnails": False}])

    def test_decode_image_accepts_real_heif_bytes(self):
        try:
            from PIL import Image
            import pillow_heif
        except ImportError:
            self.skipTest("Pillow and pillow-heif are required")

        encoded = io.BytesIO()
        pillow_heif.from_pillow(
            Image.new("RGB", (48, 32), color=(10, 20, 30))
        ).save(encoded)
        self.assertEqual(encoded.getvalue()[4:12], b"ftypheic")

        IMAGE_RUNTIME.register_image_decoders()
        encoded.seek(0)
        decoded = IMAGE_RUNTIME.decode_image(encoded)

        self.assertEqual(decoded.mode, "RGB")
        self.assertEqual(decoded.size, (48, 32))


if __name__ == "__main__":
    unittest.main()
