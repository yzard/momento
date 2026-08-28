#!/usr/bin/env python3
"""On-demand InsightFace and BiSeNet runtime for face detection and quality metrics."""

import argparse
import base64
import concurrent.futures
import math
import sys
from array import array
from pathlib import Path

from dynamic_batching import DynamicBatcher
from image_runtime import (
    ModelHTTPServer,
    create_inference_slots,
    decode_image,
    register_image_decoders,
    serve_until_stopped,
)
from runtime_http import ImageRuntimeRequestHandler

EMBEDDING_DIMENSIONS = 512
EMBEDDING_ENCODING = "float32_le"
MODEL_NAME = "buffalo_l"
RECOGNITION_INPUT_SIZE = 112
FACE_PARSING_INPUT_SIZE = 512
FACE_PARSING_CLASS_COUNT = 19
FACE_PARSING_PRIMARY_OUTPUT_NAME = "output"
FACE_PARSING_BATCH_SIZE = 8
FACE_PARSING_BATCH_WAIT_MILLISECONDS = 5
REQUIRED_MODULES = ["detection", "recognition"]
SUPPORTED_FACE_DETECTION_SIZES = {640, 960, 1280}
VISIBLE_FACE_CLASSES = frozenset({1, 2, 3, 4, 5, 10, 11, 12, 13})
FACIAL_FEATURE_REGIONS = (
    (0.342, 0.462, 0.13, 0.09),
    (0.657, 0.462, 0.13, 0.09),
    (0.500, 0.640, 0.15, 0.18),
    (0.500, 0.823, 0.22, 0.12),
)
EXPECTED_VISIBLE_REGION_FRACTION = 0.72
CLARITY_NORMALIZATION = 12.0


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
    return {"x": min(max(center_x / image_width, 0.0), 1.0), "y": min(max(center_y / image_height, 0.0), 1.0)}


def face_frontality_score(keypoints):
    if keypoints is None or len(keypoints) < 5:
        raise RuntimeError("detector did not return five face landmarks")
    landmarks = [(float(keypoint[0]), float(keypoint[1])) for keypoint in keypoints[:5] if len(keypoint) >= 2]
    if len(landmarks) != 5 or any(not math.isfinite(coordinate) for landmark in landmarks for coordinate in landmark):
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
    confidence, bounding_box, image_width, image_height, minimum_face_likelihood, minimum_face_resolution_pixels
):
    if confidence < minimum_face_likelihood:
        return False
    face_width_pixels = bounding_box["width"] * image_width
    face_height_pixels = bounding_box["height"] * image_height
    return face_width_pixels >= minimum_face_resolution_pixels and face_height_pixels >= minimum_face_resolution_pixels


def face_size_score(bounding_box):
    area = bounding_box["width"] * bounding_box["height"]
    return round(max(0.0, min(1.0, math.sqrt(area) * 4.0)), 6)


def facial_feature_region_masks(image_height, image_width):
    import numpy

    coordinate_y, coordinate_x = numpy.ogrid[:image_height, :image_width]
    masks = []
    for center_x, center_y, radius_x, radius_y in FACIAL_FEATURE_REGIONS:
        normalized_x = (coordinate_x / max(image_width - 1, 1) - center_x) / radius_x
        normalized_y = (coordinate_y / max(image_height - 1, 1) - center_y) / radius_y
        masks.append((normalized_x * normalized_x + normalized_y * normalized_y) <= 1.0)
    return masks


def face_visibility_score(parsing_mask):
    import numpy

    if parsing_mask.ndim != 2:
        raise RuntimeError("BiSeNet returned an invalid parsing mask")
    visible_pixels = numpy.isin(parsing_mask, tuple(VISIBLE_FACE_CLASSES))
    region_scores = []
    for region_mask in facial_feature_region_masks(*parsing_mask.shape):
        region_pixel_count = int(numpy.count_nonzero(region_mask))
        if region_pixel_count == 0:
            raise RuntimeError("facial feature region is empty")
        visible_fraction = float(numpy.count_nonzero(visible_pixels & region_mask)) / region_pixel_count
        region_scores.append(min(visible_fraction / EXPECTED_VISIBLE_REGION_FRACTION, 1.0))
    return round(sum(region_scores) / len(region_scores), 6)


def facial_feature_clarity_score(aligned_face, parsing_mask):
    import numpy

    if aligned_face.ndim != 3 or aligned_face.shape[2] != 3:
        raise RuntimeError("aligned face must be a BGR image")
    if parsing_mask.shape != aligned_face.shape[:2]:
        raise RuntimeError("BiSeNet parsing mask does not match aligned face")
    grayscale = (0.114 * aligned_face[:, :, 0] + 0.587 * aligned_face[:, :, 1] + 0.299 * aligned_face[:, :, 2]).astype(
        numpy.float32
    )
    padded = numpy.pad(grayscale, 1, mode="edge")
    laplacian = numpy.abs(padded[:-2, 1:-1] + padded[2:, 1:-1] + padded[1:-1, :-2] + padded[1:-1, 2:] - 4.0 * grayscale)
    visible_pixels = numpy.isin(parsing_mask, tuple(VISIBLE_FACE_CLASSES))
    region_scores = []
    for region_mask in facial_feature_region_masks(*parsing_mask.shape):
        clarity_mask = region_mask & visible_pixels
        if numpy.count_nonzero(clarity_mask) < 4:
            region_scores.append(0.0)
            continue
        edge_strength = float(numpy.mean(laplacian[clarity_mask]))
        region_scores.append(edge_strength / (edge_strength + CLARITY_NORMALIZATION))
    return round(sum(region_scores) / len(region_scores), 6)


def preprocess_face_parsing_batch(aligned_faces):
    import cv2
    import numpy

    input_mean = numpy.asarray([0.485, 0.456, 0.406], dtype=numpy.float32)
    input_standard_deviation = numpy.asarray([0.229, 0.224, 0.225], dtype=numpy.float32)
    tensors = []
    for aligned_face in aligned_faces:
        resized_face = cv2.resize(
            aligned_face, (FACE_PARSING_INPUT_SIZE, FACE_PARSING_INPUT_SIZE), interpolation=cv2.INTER_LINEAR
        )
        rgb_face = cv2.cvtColor(resized_face, cv2.COLOR_BGR2RGB).astype(numpy.float32) / 255.0
        normalized_face = (rgb_face - input_mean) / input_standard_deviation
        tensors.append(numpy.transpose(normalized_face, (2, 0, 1)))
    return numpy.ascontiguousarray(numpy.stack(tensors), dtype=numpy.float32)


def select_face_parsing_output_name(model_outputs):
    if not model_outputs:
        raise RuntimeError("BiSeNet does not expose an output")
    for model_output in model_outputs:
        if model_output.name == FACE_PARSING_PRIMARY_OUTPUT_NAME:
            return model_output.name
    return model_outputs[0].name


def postprocess_face_parsing_batch(model_output, aligned_faces):
    import cv2
    import numpy

    if model_output.ndim != 4 or model_output.shape[1] != FACE_PARSING_CLASS_COUNT:
        raise RuntimeError("BiSeNet returned an invalid output tensor")
    parsing_masks = numpy.argmax(model_output, axis=1).astype(numpy.uint8)
    if len(parsing_masks) != len(aligned_faces):
        raise RuntimeError("BiSeNet returned a different number of masks than faces")
    return [
        cv2.resize(parsing_mask, (aligned_face.shape[1], aligned_face.shape[0]), interpolation=cv2.INTER_NEAREST)
        for parsing_mask, aligned_face in zip(parsing_masks, aligned_faces)
    ]


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
        bounding_box = normalized_bounding_box(detector_output, image_width, image_height)
        bounding_box["width"] = min(bounding_box["width"], 1.0 - bounding_box["x"])
        bounding_box["height"] = min(bounding_box["height"], 1.0 - bounding_box["y"])
        if not face_meets_thresholds(
            confidence, bounding_box, image_width, image_height, minimum_face_likelihood, minimum_face_resolution_pixels
        ):
            continue
        if detected_keypoints is None or face_index >= len(detected_keypoints):
            raise RuntimeError("detector did not return face landmarks")
        keypoints = detected_keypoints[face_index]
        detected_faces.append(
            {
                "boundingBox": bounding_box,
                "eyeCenter": normalized_eye_center(keypoints, image_width, image_height),
                "confidence": confidence,
                "faceSizeScore": face_size_score(bounding_box),
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
        processing_concurrency,
        model_concurrency,
        recognition_batch_size,
        recognition_batch_wait_milliseconds,
        face_parsing_model,
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
        face_parsing_model = Path(face_parsing_model)
        if not face_parsing_model.is_file():
            raise RuntimeError(f"BiSeNet model is missing: {face_parsing_model}")
        self.application = FaceAnalysis(
            name=model_name,
            root=cache_directory,
            providers=select_providers(onnxruntime),
            allowed_modules=REQUIRED_MODULES,
        )
        self.minimum_face_likelihood = minimum_face_likelihood
        self.minimum_face_resolution_pixels = minimum_face_resolution_pixels
        self.processing_slots = create_inference_slots(processing_concurrency)
        self.detection_slots = create_inference_slots(model_concurrency)
        self.application.prepare(
            ctx_id=0, det_thresh=minimum_face_likelihood, det_size=(face_detection_size, face_detection_size)
        )
        self.detection_model = self.application.models["detection"]
        self.recognition_model = self.application.models["recognition"]
        if tuple(self.recognition_model.input_size) != (RECOGNITION_INPUT_SIZE, RECOGNITION_INPUT_SIZE):
            raise RuntimeError(
                f"{MODEL_NAME} recognition input must be " f"{RECOGNITION_INPUT_SIZE}x{RECOGNITION_INPUT_SIZE}"
            )
        self.align_face = lambda image_array, keypoints: face_align.norm_crop(
            image_array, landmark=keypoints, image_size=RECOGNITION_INPUT_SIZE
        )

        def recognize_faces(aligned_faces):
            try:
                model_embeddings = self.recognition_model.get_feat(aligned_faces)
            except cv2.error as error:
                raise RuntimeError(f"failed to prepare recognition batch: {error}") from error
            return [normalize_embedding(model_embedding) for model_embedding in model_embeddings]

        self.recognition_batcher = DynamicBatcher(
            recognize_faces, recognition_batch_size, recognition_batch_wait_milliseconds, "face-recognition-batcher"
        )

        face_parsing_session = onnxruntime.InferenceSession(
            str(face_parsing_model), providers=select_providers(onnxruntime)
        )
        face_parsing_input = face_parsing_session.get_inputs()[0]
        face_parsing_outputs = face_parsing_session.get_outputs()
        self.face_parsing_session = face_parsing_session
        self.face_parsing_input_name = face_parsing_input.name
        self.face_parsing_output_name = select_face_parsing_output_name(face_parsing_outputs)

        def parse_faces(aligned_faces):
            model_input = preprocess_face_parsing_batch(aligned_faces)
            model_output = self.face_parsing_session.run(
                [self.face_parsing_output_name], {self.face_parsing_input_name: model_input}
            )[0]
            return postprocess_face_parsing_batch(model_output, aligned_faces)

        self.face_parsing_batcher = DynamicBatcher(
            parse_faces, FACE_PARSING_BATCH_SIZE, FACE_PARSING_BATCH_WAIT_MILLISECONDS, "face-parsing-batcher"
        )
        self.post_detection_executor = concurrent.futures.ThreadPoolExecutor(
            max_workers=2, thread_name_prefix="face-post-detection"
        )

    def infer(self, image_source):
        import numpy

        with self.processing_slots:
            image = decode_image(image_source)
            image_width = image.width
            image_height = image.height
            image_array = numpy.asarray(image)[:, :, ::-1]
        with self.processing_slots:
            with self.detection_slots:
                detected_bounding_boxes, detected_keypoints = self.detection_model.detect(
                    image_array, max_num=0, metric="default"
                )
        with self.processing_slots:
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
        aligned_faces = [face.pop("alignedFace") for face in detected_faces]
        parsing_future = self.post_detection_executor.submit(self.face_parsing_batcher.infer, aligned_faces)
        embeddings = self.recognition_batcher.infer(aligned_faces)
        parsing_masks = parsing_future.result()
        faces = []
        for detected_face, aligned_face, parsing_mask, embedding in zip(
            detected_faces, aligned_faces, parsing_masks, embeddings
        ):
            detected_face.update(
                {
                    "index": len(faces),
                    "visibilityScore": face_visibility_score(parsing_mask),
                    "featureClarityScore": facial_feature_clarity_score(aligned_face, parsing_mask),
                    "embedding": encode_float32_le(embedding),
                    "embeddingEncoding": EMBEDDING_ENCODING,
                    "embeddingDimensions": EMBEDDING_DIMENSIONS,
                }
            )
            faces.append(detected_face)
        return {"faces": faces}

    def close(self):
        self.post_detection_executor.shutdown(wait=True)
        self.face_parsing_batcher.close()
        self.recognition_batcher.close()


class Handler(ImageRuntimeRequestHandler):
    pass


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True)
    parser.add_argument("--cache-dir", required=True)
    parser.add_argument("--host", required=True)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--processing-concurrency", type=int, required=True)
    parser.add_argument("--model-concurrency", type=int, required=True)
    parser.add_argument("--input-root", required=True)
    parser.add_argument("--face-detection-size", type=int, required=True)
    parser.add_argument("--recognition-batch-size", type=int, required=True)
    parser.add_argument("--recognition-batch-wait-milliseconds", type=int, required=True)
    parser.add_argument("--minimum-face-likelihood", type=float, required=True)
    parser.add_argument("--minimum-face-resolution-pixels", type=int, required=True)
    parser.add_argument("--face-parsing-model", required=True)
    arguments = parser.parse_args()
    if arguments.processing_concurrency <= 0:
        parser.error("--processing-concurrency must be positive")
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
        arguments.processing_concurrency,
        arguments.model_concurrency,
        arguments.recognition_batch_size,
        arguments.recognition_batch_wait_milliseconds,
        arguments.face_parsing_model,
    )
    Handler.input_root = Path(arguments.input_root)
    try:
        serve_until_stopped(ModelHTTPServer((arguments.host, arguments.port), Handler))
    finally:
        Handler.runtime.close()


if __name__ == "__main__":
    main()
