import importlib.util
import math
import tempfile
import unittest
from pathlib import Path


SOURCE_PATH = (
    Path(__file__).resolve().parents[2]
    / "src"
    / "backend_llm"
    / "image_aesthetics_server.py"
)
SPECIFICATION = importlib.util.spec_from_file_location(
    "image_aesthetics_server", SOURCE_PATH
)
IMAGE_AESTHETICS_SERVER = importlib.util.module_from_spec(SPECIFICATION)
SPECIFICATION.loader.exec_module(IMAGE_AESTHETICS_SERVER)


class ImageAestheticsServerTests(unittest.TestCase):
    def setUp(self):
        try:
            from PIL import Image
        except ImportError:
            self.skipTest("Pillow is not installed")
        self.Image = Image

    def test_model_concurrency_is_bounded_inside_runtime(self):
        slots = IMAGE_AESTHETICS_SERVER.create_inference_slots(2)

        self.assertTrue(slots.acquire(blocking=False))
        self.assertTrue(slots.acquire(blocking=False))
        self.assertFalse(slots.acquire(blocking=False))
        slots.release()
        slots.release()

    def test_aesthetic_rating_is_normalized_and_finite(self):
        self.assertEqual(IMAGE_AESTHETICS_SERVER.aesthetic_score(1.0), 0.0)
        self.assertEqual(IMAGE_AESTHETICS_SERVER.aesthetic_score(5.5), 0.5)
        self.assertEqual(IMAGE_AESTHETICS_SERVER.aesthetic_score(10.0), 1.0)
        self.assertEqual(IMAGE_AESTHETICS_SERVER.aesthetic_score(12.0), 1.0)

        for invalid_rating in [math.nan, math.inf, -math.inf]:
            with self.assertRaisesRegex(RuntimeError, "non-finite"):
                IMAGE_AESTHETICS_SERVER.aesthetic_score(invalid_rating)

    def test_orientation_score_uses_only_full_image_dimensions(self):
        portrait = self.Image.new("RGB", (60, 120))
        square = self.Image.new("RGB", (100, 100))
        moderate_landscape = self.Image.new("RGB", (400, 300))
        wide_landscape = self.Image.new("RGB", (300, 200))

        self.assertEqual(IMAGE_AESTHETICS_SERVER.landscape_score(portrait), 0.0)
        self.assertEqual(IMAGE_AESTHETICS_SERVER.landscape_score(square), 0.0)
        self.assertAlmostEqual(
            IMAGE_AESTHETICS_SERVER.landscape_score(moderate_landscape),
            0.666667,
        )
        self.assertEqual(
            IMAGE_AESTHETICS_SERVER.landscape_score(wide_landscape), 1.0
        )

    def test_clip_canvas_letterboxes_without_cropping(self):
        source = self.Image.new("RGB", (400, 200), color=(255, 0, 0))

        canvas = IMAGE_AESTHETICS_SERVER.aspect_preserving_square(source, 224)

        self.assertEqual(canvas.size, (224, 224))
        self.assertEqual(
            canvas.crop((0, 56, 224, 168)).getextrema(),
            ((255, 255), (0, 0), (0, 0)),
        )
        background = tuple(
            round(channel * 255.0)
            for channel in IMAGE_AESTHETICS_SERVER.CLIP_IMAGE_MEAN
        )
        self.assertEqual(canvas.getpixel((0, 0)), background)
        self.assertEqual(canvas.getpixel((223, 223)), background)

    def test_representative_images_produce_bounded_deterministic_metrics(self):
        flat = self.Image.new("RGB", (128, 128), color=(128, 128, 128))
        checkerboard = self.Image.new("RGB", (128, 128))
        checkerboard.putdata(
            [
                (255, 255, 255) if (x // 8 + y // 8) % 2 else (0, 0, 0)
                for y in range(128)
                for x in range(128)
            ]
        )

        flat_simplicity = IMAGE_AESTHETICS_SERVER.simplicity_score(flat)
        checkerboard_simplicity = IMAGE_AESTHETICS_SERVER.simplicity_score(
            checkerboard
        )
        flat_quality = IMAGE_AESTHETICS_SERVER.technical_quality_score(flat)
        checkerboard_quality = IMAGE_AESTHETICS_SERVER.technical_quality_score(
            checkerboard
        )

        self.assertGreater(flat_simplicity, checkerboard_simplicity)
        for score in [
            flat_simplicity,
            checkerboard_simplicity,
            flat_quality,
            checkerboard_quality,
        ]:
            self.assertTrue(math.isfinite(score))
            self.assertGreaterEqual(score, 0.0)
            self.assertLessEqual(score, 1.0)

    def test_required_models_must_be_local_files(self):
        with tempfile.TemporaryDirectory() as directory:
            model_path = Path(directory) / "model.pt"
            model_path.write_bytes(b"model")

            self.assertEqual(
                IMAGE_AESTHETICS_SERVER.require_model_file(model_path, "model"),
                model_path,
            )
            with self.assertRaisesRegex(RuntimeError, "is missing"):
                IMAGE_AESTHETICS_SERVER.require_model_file(
                    Path(directory) / "missing.pt", "model"
                )


if __name__ == "__main__":
    unittest.main()
