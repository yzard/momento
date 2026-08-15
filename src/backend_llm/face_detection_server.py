#!/usr/bin/env python3
"""On-demand InsightFace buffalo_l runtime for face detection and embeddings."""

import argparse
import base64
import json
import math
import sys
import threading
from array import array
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from io import BytesIO


EMBEDDING_DIMENSIONS = 512
EMBEDDING_ENCODING = "float32_le"
MAX_REQUEST_BYTES = 50 * 1024 * 1024
REQUIRED_MODULES = ["detection", "recognition"]


class InvalidImageError(ValueError):
    """The request body is not a readable image."""


def create_inference_slots(max_concurrent_jobs):
    if max_concurrent_jobs <= 0:
        raise ValueError("max_concurrent_jobs must be positive")
    return threading.BoundedSemaphore(max_concurrent_jobs)


def encode_float32_le(values):
    embedding = array("f", values)
    if embedding.itemsize != 4:
        raise RuntimeError("runtime float size is not 32 bits")
    if sys.byteorder != "little":
        embedding.byteswap()
    return base64.b64encode(embedding.tobytes()).decode("ascii")


def select_providers(onnxruntime_module):
    if "CUDAExecutionProvider" not in onnxruntime_module.get_available_providers():
        raise RuntimeError("face detection requires an available NVIDIA CUDA GPU")
    return ["CUDAExecutionProvider"]


def decode_image(image_bytes):
    from PIL import Image, ImageFile, ImageOps, UnidentifiedImageError

    ImageFile.LOAD_TRUNCATED_IMAGES = True
    try:
        with Image.open(BytesIO(image_bytes)) as source:
            source.load()
            return ImageOps.exif_transpose(source).convert("RGB")
    except (OSError, UnidentifiedImageError, ValueError) as error:
        raise InvalidImageError(f"could not decode image: {error}") from error


def normalized_bounding_box(bounding_box, image_width, image_height):
    left, top, right, bottom = bounding_box[:4]
    left = min(max(float(left), 0.0), float(image_width))
    top = min(max(float(top), 0.0), float(image_height))
    right = min(max(float(right), left), float(image_width))
    bottom = min(max(float(bottom), top), float(image_height))
    width = (right - left) / image_width
    height = (bottom - top) / image_height
    if width <= 0.0 or height <= 0.0:
        raise RuntimeError("detector returned an empty face bounding box")
    return {"x": left / image_width, "y": top / image_height, "width": width, "height": height}


def quality_score(confidence, bounding_box):
    area = bounding_box["width"] * bounding_box["height"]
    return round(max(0.0, min(1.0, confidence * min(1.0, math.sqrt(area) * 4.0))), 6)


class FaceDetectionRuntime:
    def __init__(self, model_name, cache_directory):
        import onnxruntime
        from insightface.app import FaceAnalysis

        if model_name != "buffalo_l":
            raise RuntimeError(f"unsupported InsightFace model: {model_name}")
        self.application = FaceAnalysis(
            name=model_name,
            root=cache_directory,
            providers=select_providers(onnxruntime),
            allowed_modules=REQUIRED_MODULES,
        )
        self.application.prepare(ctx_id=0, det_size=(640, 640))

    def infer(self, image_bytes):
        import numpy

        image = decode_image(image_bytes)
        image_array = numpy.asarray(image)[:, :, ::-1]
        detected_faces = self.application.get(image_array)
        ordered_faces = sorted(
            detected_faces,
            key=lambda face: (-float(face.det_score), *[float(value) for value in face.bbox[:4]]),
        )
        faces = []
        for index, face in enumerate(ordered_faces):
            confidence = float(face.det_score)
            if not math.isfinite(confidence) or not 0.0 <= confidence <= 1.0:
                raise RuntimeError("detector returned an invalid confidence")
            bounding_box = normalized_bounding_box(face.bbox, image.width, image.height)
            embedding = [float(value) for value in face.normed_embedding]
            if len(embedding) != EMBEDDING_DIMENSIONS:
                raise RuntimeError(f"model returned {len(embedding)} embedding dimensions")
            if not all(math.isfinite(value) for value in embedding):
                raise RuntimeError("model returned a non-finite embedding")
            faces.append(
                {
                    "index": index,
                    "boundingBox": bounding_box,
                    "confidence": confidence,
                    "qualityScore": quality_score(confidence, bounding_box),
                    "embedding": encode_float32_le(embedding),
                    "embeddingEncoding": EMBEDDING_ENCODING,
                    "embeddingDimensions": EMBEDDING_DIMENSIONS,
                }
            )
        return {"faces": faces}


class Handler(BaseHTTPRequestHandler):
    runtime = None
    inference_slots = None

    def do_GET(self):
        if self.path != "/ready":
            self.send_error(404)
            return
        self.send_json(200, {"status": "ready"})

    def do_POST(self):
        if self.path != "/infer":
            self.send_error(404)
            return
        try:
            content_length = int(self.headers.get("Content-Length", "0"))
            if content_length <= 0 or content_length > MAX_REQUEST_BYTES:
                raise ValueError(f"Content-Length must be between 1 and {MAX_REQUEST_BYTES}")
            if self.headers.get("Content-Type") != "application/octet-stream":
                raise ValueError("Content-Type must be application/octet-stream")
            image_bytes = self.rfile.read(content_length)
        except ValueError as error:
            self.send_json(400, {"detail": f"invalid request: {error}"})
            return
        if not image_bytes:
            self.send_json(400, {"detail": "image must not be empty"})
            return
        try:
            with self.inference_slots:
                response = self.runtime.infer(image_bytes)
        except InvalidImageError as error:
            self.send_json(400, {"detail": str(error)})
            return
        except RuntimeError as error:
            self.send_json(500, {"detail": str(error)})
            return
        self.send_json(200, response)

    def log_message(self, message_format, *args):
        return

    def send_json(self, status, payload):
        body = json.dumps(payload, allow_nan=False, separators=(",", ":")).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def serve_until_stopped(server):
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True)
    parser.add_argument("--cache-dir", required=True)
    parser.add_argument("--host", required=True)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--max-concurrent-jobs", type=int, required=True)
    arguments = parser.parse_args()
    if arguments.max_concurrent_jobs <= 0:
        parser.error("--max-concurrent-jobs must be positive")
    Handler.runtime = FaceDetectionRuntime(arguments.model, arguments.cache_dir)
    Handler.inference_slots = create_inference_slots(arguments.max_concurrent_jobs)
    serve_until_stopped(ThreadingHTTPServer((arguments.host, arguments.port), Handler))


if __name__ == "__main__":
    main()
