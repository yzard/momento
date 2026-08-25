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
from pathlib import Path

from runtime_input import read_runtime_input


EMBEDDING_DIMENSIONS = 512
EMBEDDING_ENCODING = "float32_le"
REQUIRED_MODULES = ["detection", "recognition"]


def require_model_directory(cache_directory, model_name):
    model_directory = Path(cache_directory) / "models" / model_name
    if not model_directory.is_dir():
        raise RuntimeError(f"InsightFace model is missing: {model_directory}")
    return model_directory


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


def decode_image(image_source):
    from PIL import Image, ImageFile, ImageOps, UnidentifiedImageError

    ImageFile.LOAD_TRUNCATED_IMAGES = True
    try:
        with Image.open(image_source) as source:
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


def normalized_eye_center(keypoints, image_width, image_height):
    if keypoints is None or len(keypoints) < 2:
        raise RuntimeError("detector did not return both eye landmarks")
    left_eye = keypoints[0]
    right_eye = keypoints[1]
    if len(left_eye) < 2 or len(right_eye) < 2:
        raise RuntimeError("detector returned invalid eye landmarks")
    center_x = (float(left_eye[0]) + float(right_eye[0])) / 2.0
    center_y = (float(left_eye[1]) + float(right_eye[1])) / 2.0
    if not math.isfinite(center_x) or not math.isfinite(center_y):
        raise RuntimeError("detector returned non-finite eye landmarks")
    return {
        "x": min(max(center_x / image_width, 0.0), 1.0),
        "y": min(max(center_y / image_height, 0.0), 1.0),
    }


def face_frontality_score(keypoints):
    if keypoints is None or len(keypoints) < 5:
        raise RuntimeError("detector did not return five face landmarks")
    landmarks = [
        (float(keypoint[0]), float(keypoint[1]))
        for keypoint in keypoints[:5]
        if len(keypoint) >= 2
    ]
    if len(landmarks) != 5 or any(
        not math.isfinite(coordinate)
        for landmark in landmarks
        for coordinate in landmark
    ):
        raise RuntimeError("detector returned invalid face landmarks")
    left_eye, right_eye, nose, left_mouth, right_mouth = landmarks
    eye_span = abs(right_eye[0] - left_eye[0])
    if eye_span <= 1e-6:
        raise RuntimeError("detector returned overlapping eye landmarks")
    eye_center_x = (left_eye[0] + right_eye[0]) / 2.0
    mouth_center_x = (left_mouth[0] + right_mouth[0]) / 2.0
    half_eye_span = eye_span / 2.0
    roll_error = abs(right_eye[1] - left_eye[1]) / eye_span
    nose_offset = abs(nose[0] - eye_center_x) / half_eye_span
    mouth_offset = abs(mouth_center_x - eye_center_x) / half_eye_span
    frontality_error = (roll_error * 0.25) + (nose_offset * 0.45) + (mouth_offset * 0.3)
    return round(1.0 - min(frontality_error, 1.0), 6)


def face_meets_thresholds(
    confidence,
    bounding_box,
    image_width,
    image_height,
    minimum_face_likelihood,
    minimum_face_resolution_pixels,
):
    if confidence < minimum_face_likelihood:
        return False
    face_width_pixels = bounding_box["width"] * image_width
    face_height_pixels = bounding_box["height"] * image_height
    return (
        face_width_pixels >= minimum_face_resolution_pixels
        and face_height_pixels >= minimum_face_resolution_pixels
    )


def quality_score(confidence, bounding_box):
    area = bounding_box["width"] * bounding_box["height"]
    return round(max(0.0, min(1.0, confidence * min(1.0, math.sqrt(area) * 4.0))), 6)


class FaceDetectionRuntime:
    def __init__(
        self,
        model_name,
        cache_directory,
        minimum_face_likelihood,
        minimum_face_resolution_pixels,
    ):
        import onnxruntime
        from insightface.app import FaceAnalysis

        if model_name != "buffalo_l":
            raise RuntimeError(f"unsupported InsightFace model: {model_name}")
        require_model_directory(cache_directory, model_name)
        self.application = FaceAnalysis(
            name=model_name,
            root=cache_directory,
            providers=select_providers(onnxruntime),
            allowed_modules=REQUIRED_MODULES,
        )
        self.minimum_face_likelihood = minimum_face_likelihood
        self.minimum_face_resolution_pixels = minimum_face_resolution_pixels
        self.application.prepare(
            ctx_id=0,
            det_thresh=minimum_face_likelihood,
            det_size=(640, 640),
        )

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
        for face in ordered_faces:
            confidence = float(face.det_score)
            if not math.isfinite(confidence) or not 0.0 <= confidence <= 1.0:
                raise RuntimeError("detector returned an invalid confidence")
            bounding_box = normalized_bounding_box(face.bbox, image.width, image.height)
            bounding_box["width"] = min(
                bounding_box["width"], 1.0 - bounding_box["x"]
            )
            bounding_box["height"] = min(
                bounding_box["height"], 1.0 - bounding_box["y"]
            )
            if not face_meets_thresholds(
                confidence,
                bounding_box,
                image.width,
                image.height,
                self.minimum_face_likelihood,
                self.minimum_face_resolution_pixels,
            ):
                continue
            embedding = [float(value) for value in face.normed_embedding]
            if len(embedding) != EMBEDDING_DIMENSIONS:
                raise RuntimeError(f"model returned {len(embedding)} embedding dimensions")
            if not all(math.isfinite(value) for value in embedding):
                raise RuntimeError("model returned a non-finite embedding")
            faces.append(
                {
                    "index": len(faces),
                    "boundingBox": bounding_box,
                    "eyeCenter": normalized_eye_center(
                        face.kps, image.width, image.height
                    ),
                    "confidence": confidence,
                    "qualityScore": quality_score(confidence, bounding_box),
                    "frontalityScore": face_frontality_score(face.kps),
                    "embedding": encode_float32_le(embedding),
                    "embeddingEncoding": EMBEDDING_ENCODING,
                    "embeddingDimensions": EMBEDDING_DIMENSIONS,
                }
            )
        return {"faces": faces}


class Handler(BaseHTTPRequestHandler):
    runtime = None
    inference_slots = None
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
        with self.inference_slots:
            self.handle_inference()

    def handle_inference(self):
        try:
            with read_runtime_input(self, self.input_root) as image_source:
                response = self.runtime.infer(image_source)
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

    def log_message(self, message_format, *args):
        return

    def send_json(self, status, payload):
        body = json.dumps(payload, allow_nan=False, separators=(",", ":")).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


class ModelHTTPServer(ThreadingHTTPServer):
    daemon_threads = True
    request_queue_size = 1024


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
    parser.add_argument("--input-root", required=True)
    parser.add_argument("--minimum-face-likelihood", type=float, required=True)
    parser.add_argument("--minimum-face-resolution-pixels", type=int, required=True)
    arguments = parser.parse_args()
    if arguments.max_concurrent_jobs <= 0:
        parser.error("--max-concurrent-jobs must be positive")
    if not 0.0 < arguments.minimum_face_likelihood <= 1.0:
        parser.error("--minimum-face-likelihood must be within (0, 1]")
    if arguments.minimum_face_resolution_pixels <= 0:
        parser.error("--minimum-face-resolution-pixels must be positive")
    Handler.runtime = FaceDetectionRuntime(
        arguments.model,
        arguments.cache_dir,
        arguments.minimum_face_likelihood,
        arguments.minimum_face_resolution_pixels,
    )
    Handler.inference_slots = create_inference_slots(arguments.max_concurrent_jobs)
    Handler.input_root = Path(arguments.input_root)
    serve_until_stopped(ModelHTTPServer((arguments.host, arguments.port), Handler))


if __name__ == "__main__":
    main()
