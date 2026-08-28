"""Shared HTTP request handling for local image inference runtimes."""

import json
from http.server import BaseHTTPRequestHandler

from image_runtime import InvalidImageError
from runtime_input import read_runtime_input


def add_batched_image_runtime_arguments(parser):
    parser.add_argument("--device", required=True)
    parser.add_argument("--host", required=True)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--processing-concurrency", type=int, required=True)
    parser.add_argument("--model-concurrency", type=int, required=True)
    parser.add_argument("--model-batch-wait-milliseconds", type=int, required=True)
    parser.add_argument("--input-root", required=True)


def validate_batched_image_runtime_arguments(parser, arguments):
    if arguments.processing_concurrency <= 0:
        parser.error("--processing-concurrency must be positive")
    if arguments.model_concurrency <= 0:
        parser.error("--model-concurrency must be positive")
    if arguments.model_batch_wait_milliseconds < 0:
        parser.error("--model-batch-wait-milliseconds must not be negative")


class ImageRuntimeRequestHandler(BaseHTTPRequestHandler):
    runtime = None
    input_root = None

    def do_GET(self):
        if self.path != "/ready":
            self.send_error(404)
            return
        self.send_json(200, {"status": "ready"})

    def do_POST(self):
        if self.path != "/infer":
            self.send_error(404)
            return
        self.handle_inference()

    def handle_inference(self):
        try:
            response = self.run_inference()
        except InvalidImageError as error:
            self.send_json(400, {"detail": str(error)})
            return
        except (OSError, ValueError, json.JSONDecodeError) as error:
            self.send_json(400, {"detail": f"invalid request: {error}"})
            return
        except RuntimeError as error:
            self.send_json(500, {"detail": str(error)})
            return
        self.send_json(200, response)

    def run_inference(self):
        with read_runtime_input(self, self.input_root) as image_source:
            return self.runtime.infer(image_source)

    def log_message(self, message_format, *arguments):
        return

    def send_json(self, status, payload):
        body = json.dumps(payload, allow_nan=False, separators=(",", ":")).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
