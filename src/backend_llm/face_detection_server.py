#!/usr/bin/env python3
"""On-demand InsightFace buffalo_l runtime for face detection and embeddings."""

import argparse
import base64
import json
import math
import sys
from array import array
from http.server import BaseHTTPRequestHandler
from pathlib import Path

from dynamic_batching import DynamicBatcher
from image_runtime import (
    InvalidImageError,
    ModelHTTPServer,
    create_inference_slots,
    decode_image,
    register_image_decoders,
    serve_until_stopped,
)
from runtime_input import read_runtime_input

EMBEDDING_DIMENSIONS = 512
EMBEDDING_ENCODING = "float32_le"
MODEL_NAME = "buffalo_l"
RECOGNITION_INPUT_SIZE = 112
REQUIRED_MODULES = ["detection", "recognition"]
SUPPORTED_FACE_DETECTION_SIZES = {640, 960, 1280}


def require_model_directory(cache_directory, model_name):
    model_directory = Path(cache_directory) / "models" / model_name
    if not model_directory.is_dir():
        raise RuntimeError(f"InsightFace model is missing: {model_directory}")
    return model_directory


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
    return {
        "x": left / image_width,
        "y": top / image_height,
        "width": width,
        "height": height,
    }


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


def normalize_embedding(values):
    import numpy

    embedding = numpy.asarray(values, dtype=numpy.float32).reshape(-1)
    if len(embedding) != EMBEDDING_DIMENSIONS:
        raise RuntimeError(f"model returned {len(embedding)} embedding dimensions")
    if not numpy.isfinite(embedding).all():
        raise RuntimeError("model returned a non-finite embedding")
    embedding_norm = float(numpy.linalg.norm(embedding))
    if not math.isfinite(embedding_norm) or embedding_norm <= 0.0:
        raise RuntimeError("model returned an embedding with zero norm")
    return [float(value) for value in embedding / embedding_norm]


def prepare_detected_faces(
    detected_bounding_boxes,
    detected_keypoints,
    image_array,
    image_width,
    image_height,
    minimum_face_likelihood,
    minimum_face_resolution_pixels,
    align_face,
):
    detected_faces = []
    for face_index, detector_output in enumerate(detected_bounding_boxes):
        confidence = float(detector_output[4])
        if not math.isfinite(confidence) or not 0.0 <= confidence <= 1.0:
            raise RuntimeError("detector returned an invalid confidence")
        bounding_box = normalized_bounding_box(
            detector_output, image_width, image_height
        )
        bounding_box["width"] = min(bounding_box["width"], 1.0 - bounding_box["x"])
        bounding_box["height"] = min(bounding_box["height"], 1.0 - bounding_box["y"])
        if not face_meets_thresholds(
            confidence,
            bounding_box,
            image_width,
            image_height,
            minimum_face_likelihood,
            minimum_face_resolution_pixels,
        ):
            continue
        if detected_keypoints is None or face_index >= len(detected_keypoints):
            raise RuntimeError("detector did not return face landmarks")
        keypoints = detected_keypoints[face_index]
        detected_faces.append(
            {
                "boundingBox": bounding_box,
                "eyeCenter": normalized_eye_center(
                    keypoints, image_width, image_height
                ),
                "confidence": confidence,
                "qualityScore": quality_score(confidence, bounding_box),
                "frontalityScore": face_frontality_score(keypoints),
                "alignedFace": align_face(image_array, keypoints),
            }
        )
    return sorted(
        detected_faces,
        key=lambda face: (
            -face["confidence"],
            face["boundingBox"]["x"],
            face["boundingBox"]["y"],
            face["boundingBox"]["width"],
            face["boundingBox"]["height"],
        ),
    )


class FaceDetectionRuntime:
    def __init__(
        self,
        model_name,
        cache_directory,
        minimum_face_likelihood,
        minimum_face_resolution_pixels,
        face_detection_size,
        cpu_processing_concurrency,
        model_concurrency,
        recognition_batch_size,
        recognition_batch_wait_milliseconds,
    ):
        import cv2
        import onnxruntime
        from insightface.app import FaceAnalysis
        from insightface.utils import face_align

        if model_name != MODEL_NAME:
            raise RuntimeError(f"unsupported InsightFace model: {model_name}")
        if face_detection_size not in SUPPORTED_FACE_DETECTION_SIZES:
            raise RuntimeError("face detection size must be one of 640, 960, or 1280")
        require_model_directory(cache_directory, model_name)
        self.application = FaceAnalysis(
            name=model_name,
            root=cache_directory,
            providers=select_providers(onnxruntime),
            allowed_modules=REQUIRED_MODULES,
        )
        self.minimum_face_likelihood = minimum_face_likelihood
        self.minimum_face_resolution_pixels = minimum_face_resolution_pixels
        self.cpu_processing_slots = create_inference_slots(cpu_processing_concurrency)
        self.detection_slots = create_inference_slots(model_concurrency)
        self.application.prepare(
            ctx_id=0,
            det_thresh=minimum_face_likelihood,
            det_size=(face_detection_size, face_detection_size),
        )
        self.detection_model = self.application.models["detection"]
        self.recognition_model = self.application.models["recognition"]
        if tuple(self.recognition_model.input_size) != (
            RECOGNITION_INPUT_SIZE,
            RECOGNITION_INPUT_SIZE,
        ):
            raise RuntimeError(
                f"{MODEL_NAME} recognition input must be "
                f"{RECOGNITION_INPUT_SIZE}x{RECOGNITION_INPUT_SIZE}"
            )
        self.align_face = lambda image_array, keypoints: face_align.norm_crop(
            image_array, landmark=keypoints, image_size=RECOGNITION_INPUT_SIZE
        )

        def recognize_faces(aligned_faces):
            try:
                model_embeddings = self.recognition_model.get_feat(aligned_faces)
            except cv2.error as error:
                raise RuntimeError(
                    f"failed to prepare recognition batch: {error}"
                ) from error
            return [
                normalize_embedding(model_embedding)
                for model_embedding in model_embeddings
            ]

        self.recognition_batcher = DynamicBatcher(
            recognize_faces,
            recognition_batch_size,
            recognition_batch_wait_milliseconds,
            "face-recognition-batcher",
        )

    def infer(self, image_source):
        import numpy

        with self.cpu_processing_slots:
            image = decode_image(image_source)
            image_width = image.width
            image_height = image.height
            image_array = numpy.asarray(image)[:, :, ::-1]
        with self.cpu_processing_slots:
            with self.detection_slots:
                detected_bounding_boxes, detected_keypoints = (
                    self.detection_model.detect(
                        image_array, max_num=0, metric="default"
                    )
                )
        with self.cpu_processing_slots:
            detected_faces = prepare_detected_faces(
                detected_bounding_boxes,
                detected_keypoints,
                image_array,
                image_width,
                image_height,
                self.minimum_face_likelihood,
                self.minimum_face_resolution_pixels,
                self.align_face,
            )
        del image_array
        del image
        embeddings = self.recognition_batcher.infer(
            [face.pop("alignedFace") for face in detected_faces]
        )
        faces = []
        for detected_face, embedding in zip(detected_faces, embeddings):
            detected_face.update(
                {
                    "index": len(faces),
                    "embedding": encode_float32_le(embedding),
                    "embeddingEncoding": EMBEDDING_ENCODING,
                    "embeddingDimensions": EMBEDDING_DIMENSIONS,
                }
            )
            faces.append(detected_face)
        return {"faces": faces}

    def close(self):
        self.recognition_batcher.close()


class Handler(BaseHTTPRequestHandler):
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
        body = json.dumps(payload, allow_nan=False, separators=(",", ":")).encode(
            "utf-8"
        )
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True)
    parser.add_argument("--cache-dir", required=True)
    parser.add_argument("--host", required=True)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--cpu-processing-concurrency", type=int, required=True)
    parser.add_argument("--model-concurrency", type=int, required=True)
    parser.add_argument("--input-root", required=True)
    parser.add_argument("--face-detection-size", type=int, required=True)
    parser.add_argument("--recognition-batch-size", type=int, required=True)
    parser.add_argument(
        "--recognition-batch-wait-milliseconds", type=int, required=True
    )
    parser.add_argument("--minimum-face-likelihood", type=float, required=True)
    parser.add_argument("--minimum-face-resolution-pixels", type=int, required=True)
    arguments = parser.parse_args()
    if arguments.cpu_processing_concurrency <= 0:
        parser.error("--cpu-processing-concurrency must be positive")
    if arguments.model_concurrency <= 0:
        parser.error("--model-concurrency must be positive")
    if arguments.face_detection_size not in SUPPORTED_FACE_DETECTION_SIZES:
        parser.error("--face-detection-size must be one of 640, 960, or 1280")
    if arguments.recognition_batch_size <= 0:
        parser.error("--recognition-batch-size must be positive")
    if arguments.recognition_batch_wait_milliseconds < 0:
        parser.error("--recognition-batch-wait-milliseconds must not be negative")
    if not 0.0 < arguments.minimum_face_likelihood <= 1.0:
        parser.error("--minimum-face-likelihood must be within (0, 1]")
    if arguments.minimum_face_resolution_pixels <= 0:
        parser.error("--minimum-face-resolution-pixels must be positive")
    register_image_decoders()
    Handler.runtime = FaceDetectionRuntime(
        arguments.model,
        arguments.cache_dir,
        arguments.minimum_face_likelihood,
        arguments.minimum_face_resolution_pixels,
        arguments.face_detection_size,
        arguments.cpu_processing_concurrency,
        arguments.model_concurrency,
        arguments.recognition_batch_size,
        arguments.recognition_batch_wait_milliseconds,
    )
    Handler.input_root = Path(arguments.input_root)
    try:
        serve_until_stopped(ModelHTTPServer((arguments.host, arguments.port), Handler))
    finally:
        Handler.runtime.close()


if __name__ == "__main__":
    main()
