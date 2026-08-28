import importlib.util
import io
import sys
import types
import unittest
from pathlib import Path

SOURCE_PATH = Path(__file__).resolve().parents[2] / "src" / "backend_llm" / "image_runtime.py"
SPECIFICATION = importlib.util.spec_from_file_location("image_runtime", SOURCE_PATH)
IMAGE_RUNTIME = importlib.util.module_from_spec(SPECIFICATION)
SPECIFICATION.loader.exec_module(IMAGE_RUNTIME)
InvalidImageError = IMAGE_RUNTIME.InvalidImageError
ModelHTTPServer = IMAGE_RUNTIME.ModelHTTPServer
create_inference_slots = IMAGE_RUNTIME.create_inference_slots
decode_image = IMAGE_RUNTIME.decode_image
register_image_decoders = IMAGE_RUNTIME.register_image_decoders
resize_for_analysis = IMAGE_RUNTIME.resize_for_analysis
select_cuda_device = IMAGE_RUNTIME.select_cuda_device
serve_until_stopped = IMAGE_RUNTIME.serve_until_stopped


class ImageRuntimeTests(unittest.TestCase):
    QOI_FIXTURE = bytes.fromhex("716f696600000003000000020301fe0a141ec40000000000000001")

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

    def test_decode_image_accepts_gif_tiff_webp_and_qoi(self):
        try:
            from PIL import Image
        except ImportError:
            self.skipTest("Pillow is not installed")

        encoded_images = {"QOI": io.BytesIO(self.QOI_FIXTURE)}
        for image_format in ("GIF", "TIFF", "WEBP"):
            encoded = io.BytesIO()
            Image.new("RGB", (3, 2), color=(10, 20, 30)).save(encoded, format=image_format)
            encoded.seek(0)
            encoded_images[image_format] = encoded

        for image_format, encoded in encoded_images.items():
            with self.subTest(image_format=image_format):
                decoded = IMAGE_RUNTIME.decode_image(encoded)

                self.assertEqual(decoded.mode, "RGB")
                self.assertEqual(decoded.size, (3, 2))

    def test_decode_image_uses_the_first_animated_frame_or_tiff_page(self):
        try:
            from PIL import Image
        except ImportError:
            self.skipTest("Pillow is not installed")

        first = Image.new("RGB", (3, 2), color=(255, 0, 0))
        second = Image.new("RGB", (3, 2), color=(0, 0, 255))
        for image_format in ("GIF", "TIFF", "WEBP"):
            encoded = io.BytesIO()
            save_options = {"format": image_format, "save_all": True, "append_images": [second]}
            if image_format == "WEBP":
                save_options["lossless"] = True
            first.save(encoded, **save_options)
            encoded.seek(0)

            with self.subTest(image_format=image_format):
                decoded = IMAGE_RUNTIME.decode_image(encoded)
                self.assertEqual(decoded.getpixel((0, 0)), (255, 0, 0))

    def test_decode_image_rejects_qoi_decompression_bombs(self):
        try:
            import PIL  # noqa: F401
        except ImportError:
            self.skipTest("Pillow is not installed")

        oversized_qoi_header = bytes.fromhex("716f6966ffffffffffffffff0301fe0a141ec40000000000000001")

        with self.assertRaisesRegex(InvalidImageError, "decompression bomb"):
            IMAGE_RUNTIME.decode_image(io.BytesIO(oversized_qoi_header))

    def test_decode_image_rejects_images_above_the_pixel_limit(self):
        try:
            from PIL import Image
        except ImportError:
            self.skipTest("Pillow is not installed")

        original_limit = IMAGE_RUNTIME.MAXIMUM_DECODED_IMAGE_PIXELS
        IMAGE_RUNTIME.MAXIMUM_DECODED_IMAGE_PIXELS = 100
        try:
            encoded = io.BytesIO()
            Image.new("RGB", (11, 10)).save(encoded, format="PNG")
            encoded.seek(0)

            with self.assertRaisesRegex(InvalidImageError, "decompression bomb"):
                IMAGE_RUNTIME.decode_image(encoded)
        finally:
            IMAGE_RUNTIME.MAXIMUM_DECODED_IMAGE_PIXELS = original_limit

    def test_resize_for_analysis_preserves_aspect_ratio_and_bounds_the_maximum_side(self):
        try:
            from PIL import Image
        except ImportError:
            self.skipTest("Pillow is not installed")

        resized = IMAGE_RUNTIME.resize_for_analysis(Image.new("RGB", (1200, 400)), 512)

        self.assertEqual(resized.size, (512, 171))

    def test_resize_for_analysis_rejects_non_positive_maximum_side(self):
        with self.assertRaisesRegex(ValueError, "maximum_side_length"):
            IMAGE_RUNTIME.resize_for_analysis(object(), 0)

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
            import pillow_heif
            from PIL import Image
        except ImportError:
            self.skipTest("Pillow and pillow-heif are required")

        encoded = io.BytesIO()
        pillow_heif.from_pillow(Image.new("RGB", (48, 32), color=(10, 20, 30))).save(encoded)
        self.assertEqual(encoded.getvalue()[4:12], b"ftypheic")

        IMAGE_RUNTIME.register_image_decoders()
        encoded.seek(0)
        decoded = IMAGE_RUNTIME.decode_image(encoded)

        self.assertEqual(decoded.mode, "RGB")
        self.assertEqual(decoded.size, (48, 32))


if __name__ == "__main__":
    unittest.main()
