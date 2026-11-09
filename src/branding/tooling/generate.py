# /// script
# requires-python = ">=3.11"
# dependencies = ["Pillow==12.0.0"]
# ///
"""Export every Momento brand asset from the same opaque artwork.

Run with `uv run src/branding/tooling/generate.py` from any directory.
"""
from io import BytesIO
import json
from pathlib import Path

from PIL import Image, ImageChops

ROOT = Path(__file__).resolve().parents[3]


def png(image: Image.Image, size: int) -> bytes:
    output = BytesIO()
    image.resize((size, size), Image.Resampling.LANCZOS).save(output, format="PNG", optimize=True)
    return output.getvalue()


def assets(root: Path) -> dict[Path, bytes]:
    with Image.open(root / "src/branding/resources/momento.png") as source:
        if source.width != source.height:
            raise ValueError("The brand master must be square")
        image = source.convert("RGB")
    result = {}
    def add(path: str, content: bytes) -> None:
        result[root / path] = content
    for size in [32, 180, 192, 512]:
        add(f"src/frontend/public/brand/momento-{size}.png", png(image, size))
    ico = BytesIO()
    image.save(ico, format="ICO", sizes=[(16, 16), (32, 32), (48, 48), (64, 64)])
    add("src/frontend/public/favicon.ico", ico.getvalue())
    add("src/ios/Resources/Assets.xcassets/AppIcon.appiconset/AppIcon.png", png(image, 1024))
    add("src/ios/Resources/Assets.xcassets/BrandMark.imageset/BrandMark.png", png(image, 256))
    add("src/ios/Resources/Assets.xcassets/BrandMark.imageset/Contents.json", json.dumps({
        "images": [{"filename": "BrandMark.png", "idiom": "universal"}],
        "info": {"author": "xcode", "version": 1}
    }, indent=2).encode() + b"\n")
    android = "src/android/app/src/main/res"
    add(f"{android}/drawable-nodpi/momento_brand.png", png(image, 512))
    add(f"{android}/drawable-nodpi/ic_launcher_foreground.png", png(image, 432))
    # Android's themed-icon channel uses the identical silhouette without the white tile.
    difference = ImageChops.difference(image, Image.new("RGB", image.size, "white"))
    channels = difference.split()
    mask = ImageChops.lighter(ImageChops.lighter(channels[0], channels[1]), channels[2])
    mask = mask.point(lambda value: min(255, max(0, (value - 10) * 255 // 25)))
    monochrome = Image.new("RGBA", image.size, (0, 0, 0, 0))
    monochrome.putalpha(mask)
    add(f"{android}/drawable-nodpi/ic_launcher_monochrome.png", png(monochrome, 432))
    for density, size in [("mdpi", 48), ("hdpi", 72), ("xhdpi", 96), ("xxhdpi", 144), ("xxxhdpi", 192)]:
        for name in ["ic_launcher", "ic_launcher_round"]:
            add(f"{android}/mipmap-{density}/{name}.png", png(image, size))
    return result


def main() -> None:
    for path, content in assets(ROOT).items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)


if __name__ == "__main__":
    main()
