import hashlib
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("validate_oci", ROOT / "docker/lib/validate_oci.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class OciValidationTest(unittest.TestCase):
    def test_reusable_image_requires_all_referenced_blobs(self):
        (ROOT / "build/tmp").mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=ROOT / "build/tmp") as temporary:
            root = Path(temporary)
            blobs = root / "blobs/sha256"
            blobs.mkdir(parents=True)

            def store(payload, media_type):
                digest = hashlib.sha256(payload).hexdigest()
                (blobs / digest).write_bytes(payload)
                return {"digest": "sha256:" + digest, "size": len(payload), "mediaType": media_type}

            layer = store(b"model weights", "application/vnd.oci.image.layer.v1.tar")
            config = store(b"{}", "application/vnd.oci.image.config.v1+json")
            manifest = store(
                json.dumps({"schemaVersion": 2, "config": config, "layers": [layer]}).encode(),
                "application/vnd.oci.image.manifest.v1+json",
            )
            (root / "index.json").write_text(json.dumps({"schemaVersion": 2, "manifests": [manifest]}))
            MODULE.validate(root)
            weight_file = blobs / layer["digest"].split(":")[1]
            weight_file.write_bytes(b"truncated")
            with self.assertRaises(ValueError):
                MODULE.validate(root)
            weight_file.unlink()
            with self.assertRaises(FileNotFoundError):
                MODULE.validate(root)


if __name__ == "__main__":
    unittest.main()
