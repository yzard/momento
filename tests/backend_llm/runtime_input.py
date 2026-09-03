import hashlib
import importlib.util
import io
import os
import tempfile
import unittest
from pathlib import Path

SOURCE_PATH = Path(__file__).resolve().parents[2] / "src" / "backend_llm" / "runtime_input.py"
SPECIFICATION = importlib.util.spec_from_file_location("runtime_input_source", SOURCE_PATH)
RUNTIME_INPUT = importlib.util.module_from_spec(SPECIFICATION)
SPECIFICATION.loader.exec_module(RUNTIME_INPUT)
read_runtime_input = RUNTIME_INPUT.read_runtime_input


class FakeHandler:
    def __init__(self, body):
        self.rfile = io.BytesIO(body)
        self.headers = {"Content-Type": "application/json", "Content-Length": str(len(body))}


def descriptor(job_id, content, mime_type):
    return (
        '{"jobId":"%s","sequence":0,"byteSize":%d,'
        '"contentHash":"%s","mimeType":"%s","inputFilename":"input-0"}'
        % (job_id, len(content), hashlib.sha256(content).hexdigest(), mime_type)
    ).encode()


class RuntimeInputTests(unittest.TestCase):
    def test_reads_the_derived_queue_input(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            job_id = "abcdef12"
            content = b"image"
            (root / job_id).mkdir()
            (root / job_id / "input-0").write_bytes(content)

            with read_runtime_input(FakeHandler(descriptor(job_id, content, "image/qoi")), root) as result:
                self.assertEqual(result.read(), content)

    def test_rejects_a_symlinked_input(self):
        if not hasattr(os, "O_NOFOLLOW"):
            self.skipTest("O_NOFOLLOW is unavailable")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            job_id = "abcdef12"
            content = b"image"
            target = root / "target"
            target.write_bytes(content)
            (root / job_id).mkdir()
            (root / job_id / "input-0").symlink_to(target)

            with self.assertRaises(OSError):
                read_runtime_input(FakeHandler(descriptor(job_id, content, "image/jpeg")), root)

    def test_rejects_unknown_descriptor_fields(self):
        body = (
            b'{"jobId":"aa","sequence":0,"byteSize":1,"contentHash":"'
            + (b"0" * 64)
            + b'","mimeType":"image/jpeg","inputFilename":"input-0","path":"/etc/passwd"}'
        )

        with self.assertRaisesRegex(ValueError, "invalid fields"):
            read_runtime_input(FakeHandler(body), Path("/unused"))


if __name__ == "__main__":
    unittest.main()
