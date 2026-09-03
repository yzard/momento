import argparse
import hashlib
import http.client
import importlib.util
import json
import tempfile
import threading
import unittest
from pathlib import Path

from image_runtime import ModelHTTPServer

RUNTIME_HTTP_SOURCE_PATH = Path(__file__).resolve().parents[2] / "src" / "backend_llm" / "runtime_http.py"
SPECIFICATION = importlib.util.spec_from_file_location("runtime_http_source", RUNTIME_HTTP_SOURCE_PATH)
RUNTIME_HTTP = importlib.util.module_from_spec(SPECIFICATION)
SPECIFICATION.loader.exec_module(RUNTIME_HTTP)


class RecordingRuntime:
    def __init__(self):
        self.received = None

    def infer(self, image_source):
        self.received = image_source.read()
        return {"processed": True}


class RuntimeHttpTests(unittest.TestCase):
    def test_batched_runtime_arguments_require_positive_concurrency(self):
        parser = argparse.ArgumentParser()
        RUNTIME_HTTP.add_batched_image_runtime_arguments(parser)
        arguments = parser.parse_args(
            [
                "--device",
                "cuda:0",
                "--host",
                "127.0.0.1",
                "--port",
                "8400",
                "--processing-concurrency",
                "2",
                "--model-concurrency",
                "1",
                "--model-batch-wait-milliseconds",
                "5",
                "--input-root",
                "/queue",
            ]
        )

        RUNTIME_HTTP.validate_batched_image_runtime_arguments(parser, arguments)

        self.assertEqual(arguments.processing_concurrency, 2)
        self.assertEqual(arguments.model_concurrency, 1)

    def test_handler_reads_owned_input_and_returns_compact_json(self):
        runtime = RecordingRuntime()
        with tempfile.TemporaryDirectory() as directory:
            input_root = Path(directory)
            job_id = "abcdef12"
            image_bytes = b"queued-image"
            (input_root / job_id).mkdir()
            (input_root / job_id / "input-0").write_bytes(image_bytes)
            descriptor = json.dumps(
                {
                    "jobId": job_id,
                    "sequence": 0,
                    "byteSize": len(image_bytes),
                    "contentHash": hashlib.sha256(image_bytes).hexdigest(),
                    "mimeType": "image/jpeg",
                    "inputFilename": "input-0",
                }
            ).encode()

            RUNTIME_HTTP.ImageRuntimeRequestHandler.runtime = runtime
            RUNTIME_HTTP.ImageRuntimeRequestHandler.input_root = input_root
            server = ModelHTTPServer(("127.0.0.1", 0), RUNTIME_HTTP.ImageRuntimeRequestHandler)
            server_thread = threading.Thread(target=server.serve_forever)
            server_thread.start()
            try:
                connection = http.client.HTTPConnection("127.0.0.1", server.server_address[1])
                connection.request("POST", "/infer", body=descriptor, headers={"Content-Type": "application/json"})
                response = connection.getresponse()
                response_body = response.read()
                connection.close()
            finally:
                server.shutdown()
                server.server_close()
                server_thread.join()

        self.assertEqual(response.status, 200)
        self.assertEqual(response_body, b'{"processed":true}')
        self.assertEqual(runtime.received, image_bytes)


if __name__ == "__main__":
    unittest.main()
