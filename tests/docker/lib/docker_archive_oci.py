import gzip
import hashlib
import importlib.util
import io
import json
import tarfile
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("docker_archive_oci", ROOT / "docker/lib/docker_archive_oci.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
VALIDATION_SPEC = importlib.util.spec_from_file_location("validate_oci", ROOT / "docker/lib/validate_oci.py")
VALIDATOR = importlib.util.module_from_spec(VALIDATION_SPEC)
VALIDATION_SPEC.loader.exec_module(VALIDATOR)


class DockerArchiveTest(unittest.TestCase):
    def test_export_preserves_layer_identity_and_rejects_corruption(self):
        (ROOT / "build/tmp").mkdir(parents=True, exist_ok=True)
        for compressed in (False, True):
            for corrupt in (False, True):
                with self.subTest(compressed=compressed, corrupt=corrupt):
                    with tempfile.TemporaryDirectory(dir=ROOT / "build/tmp") as temporary:
                        root = Path(temporary)
                        layer = b"original model layer"
                        digest = hashlib.sha256(layer).hexdigest()
                        config = json.dumps({"rootfs": {"diff_ids": ["sha256:" + digest]}}).encode()
                        manifest = json.dumps([{"Config": "config.json", "Layers": ["layer.tar"]}]).encode()
                        content = b"corrupt layer" if corrupt else layer
                        if compressed:
                            content = gzip.compress(content)
                        with tarfile.open(root / "image.tar", "w") as archive:
                            for name, data in [("manifest.json", manifest), ("config.json", config), ("layer.tar", content)]:
                                member = tarfile.TarInfo(name)
                                member.size = len(data)
                                archive.addfile(member, io.BytesIO(data))
                        output = root / "oci"
                        if corrupt:
                            with self.assertRaises(ValueError):
                                MODULE.convert(root / "image.tar", output)
                        else:
                            MODULE.convert(root / "image.tar", output)
                            VALIDATOR.validate(output)
                            self.assertEqual(layer, (output / "blobs/sha256" / digest).read_bytes())


if __name__ == "__main__":
    unittest.main()
