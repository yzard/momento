import importlib.util
from io import BytesIO
from pathlib import Path
import tempfile
import unittest

from PIL import Image

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("brand_generator", ROOT / "src/branding/tooling/generate.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class BrandAssetsTests(unittest.TestCase):
    def test_platform_assets_match_one_master(self):
        for path, content in MODULE.assets(ROOT).items():
            with self.subTest(asset=path.relative_to(ROOT)):
                self.assertEqual(path.read_bytes(), content)

    def test_launchers_have_opaque_white_tiles_and_monochrome_has_no_tile(self):
        paths = ["src/ios/Resources/Assets.xcassets/AppIcon.appiconset/AppIcon.png", "src/frontend/public/brand/momento-192.png", "src/android/app/src/main/res/drawable-nodpi/ic_launcher_foreground.png"]
        for path in paths:
            with Image.open(ROOT / path) as icon:
                self.assertEqual(icon.mode, "RGB")
                self.assertEqual(icon.width, icon.height)
                self.assertTrue(all(channel >= 248 for channel in icon.getpixel((0, 0))))
        with Image.open(ROOT / "src/android/app/src/main/res/drawable-nodpi/ic_launcher_monochrome.png") as icon:
            self.assertEqual(icon.getpixel((0, 0))[3], 0)
            self.assertEqual(icon.getchannel("A").getextrema(), (0, 255))

    def test_rejects_non_square_master(self):
        with tempfile.TemporaryDirectory(dir=ROOT / "build/tmp") as directory:
            root = Path(directory)
            master = root / "src/branding/resources/momento.png"
            master.parent.mkdir(parents=True)
            Image.new("RGB", (32, 16), "white").save(master)
            with self.assertRaisesRegex(ValueError, "square"):
                MODULE.assets(root)
