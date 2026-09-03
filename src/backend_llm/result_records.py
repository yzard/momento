import enum
import math
import struct
from dataclasses import dataclass

RESULT_RECORD_HEADER_BYTES = 24
MAX_LLM_RESULT_RECORD_BYTES = 1024 * 1024
MAX_LLM_RESULT_RECORD_PAYLOAD_BYTES = MAX_LLM_RESULT_RECORD_BYTES - RESULT_RECORD_HEADER_BYTES
RESULT_RECORD_VERSION = 1
MAX_NORMALIZED_RESULT_RECORD_BYTES = 2 * 1024 * 1024
MAX_LLM_RESULT_RECORDS = 1_000_000
MAX_LLM_RESULT_CONTINUATIONS_PER_VALUE = 4
MAX_LLM_RESULT_BYTES = 1024 * 1024 * 1024
MAX_LLM_RESULT_INPUTS = 1024


class ResultRecordKind(enum.IntEnum):
    FAILURE = 1
    INPUT_STARTED = 2
    OCR_TEXT = 3
    IMAGE_TAGS = 4
    IMAGE_CLUSTERING = 5
    IMAGE_AESTHETICS = 6
    FACE = 7
    SCREENSHOT_DETECTION = 8
    DOCUMENT_DETECTION = 9
    INPUT_FINISHED = 10
    OCR_TEXT_CONTINUATION = 11
    IMAGE_TAGS_CONTINUATION = 12


RESULT_RECORD_KIND_BOUNDS = {
    ResultRecordKind.FAILURE: (4 * 1024, 4 * 1024 - 4),
    ResultRecordKind.INPUT_STARTED: (16, 0),
    ResultRecordKind.OCR_TEXT: (MAX_LLM_RESULT_RECORD_PAYLOAD_BYTES, MAX_NORMALIZED_RESULT_RECORD_BYTES),
    ResultRecordKind.IMAGE_TAGS: (MAX_LLM_RESULT_RECORD_PAYLOAD_BYTES, MAX_NORMALIZED_RESULT_RECORD_BYTES),
    ResultRecordKind.IMAGE_CLUSTERING: (4 + 768 * 4 + 8 + 4, 768 * 4),
    ResultRecordKind.IMAGE_AESTHETICS: (20, 0),
    ResultRecordKind.FACE: (8 + 11 * 4 + 512 * 4, 512 * 4),
    ResultRecordKind.SCREENSHOT_DETECTION: (8, 0),
    ResultRecordKind.DOCUMENT_DETECTION: (8, 0),
    ResultRecordKind.INPUT_FINISHED: (0, 0),
    ResultRecordKind.OCR_TEXT_CONTINUATION: (MAX_LLM_RESULT_RECORD_PAYLOAD_BYTES, MAX_NORMALIZED_RESULT_RECORD_BYTES),
    ResultRecordKind.IMAGE_TAGS_CONTINUATION: (MAX_LLM_RESULT_RECORD_PAYLOAD_BYTES, MAX_NORMALIZED_RESULT_RECORD_BYTES),
}


@dataclass(frozen=True)
class ResultRecord:
    kind: ResultRecordKind
    flags: int
    record_sequence: int
    input_sequence: int
    payload: bytes


@dataclass(frozen=True)
class ResultRecordHeader:
    total_length: int
    kind: ResultRecordKind
    flags: int
    record_sequence: int
    input_sequence: int
    payload_length: int
    checksum: int


_CRC32C_TABLE = (
    0x00000000,
    0xF26B8303,
    0xE13B70F7,
    0x1350F3F4,
    0xC79A971F,
    0x35F1141C,
    0x26A1E7E8,
    0xD4CA64EB,
    0x8AD958CF,
    0x78B2DBCC,
    0x6BE22838,
    0x9989AB3B,
    0x4D43CFD0,
    0xBF284CD3,
    0xAC78BF27,
    0x5E133C24,
    0x105EC76F,
    0xE235446C,
    0xF165B798,
    0x030E349B,
    0xD7C45070,
    0x25AFD373,
    0x36FF2087,
    0xC494A384,
    0x9A879FA0,
    0x68EC1CA3,
    0x7BBCEF57,
    0x89D76C54,
    0x5D1D08BF,
    0xAF768BBC,
    0xBC267848,
    0x4E4DFB4B,
    0x20BD8EDE,
    0xD2D60DDD,
    0xC186FE29,
    0x33ED7D2A,
    0xE72719C1,
    0x154C9AC2,
    0x061C6936,
    0xF477EA35,
    0xAA64D611,
    0x580F5512,
    0x4B5FA6E6,
    0xB93425E5,
    0x6DFE410E,
    0x9F95C20D,
    0x8CC531F9,
    0x7EAEB2FA,
    0x30E349B1,
    0xC288CAB2,
    0xD1D83946,
    0x23B3BA45,
    0xF779DEAE,
    0x05125DAD,
    0x1642AE59,
    0xE4292D5A,
    0xBA3A117E,
    0x4851927D,
    0x5B016189,
    0xA96AE28A,
    0x7DA08661,
    0x8FCB0562,
    0x9C9BF696,
    0x6EF07595,
    0x417B1DBC,
    0xB3109EBF,
    0xA0406D4B,
    0x522BEE48,
    0x86E18AA3,
    0x748A09A0,
    0x67DAFA54,
    0x95B17957,
    0xCBA24573,
    0x39C9C670,
    0x2A993584,
    0xD8F2B687,
    0x0C38D26C,
    0xFE53516F,
    0xED03A29B,
    0x1F682198,
    0x5125DAD3,
    0xA34E59D0,
    0xB01EAA24,
    0x42752927,
    0x96BF4DCC,
    0x64D4CECF,
    0x77843D3B,
    0x85EFBE38,
    0xDBFC821C,
    0x2997011F,
    0x3AC7F2EB,
    0xC8AC71E8,
    0x1C661503,
    0xEE0D9600,
    0xFD5D65F4,
    0x0F36E6F7,
    0x61C69362,
    0x93AD1061,
    0x80FDE395,
    0x72966096,
    0xA65C047D,
    0x5437877E,
    0x4767748A,
    0xB50CF789,
    0xEB1FCBAD,
    0x197448AE,
    0x0A24BB5A,
    0xF84F3859,
    0x2C855CB2,
    0xDEEEDFB1,
    0xCDBE2C45,
    0x3FD5AF46,
    0x7198540D,
    0x83F3D70E,
    0x90A324FA,
    0x62C8A7F9,
    0xB602C312,
    0x44694011,
    0x5739B3E5,
    0xA55230E6,
    0xFB410CC2,
    0x092A8FC1,
    0x1A7A7C35,
    0xE811FF36,
    0x3CDB9BDD,
    0xCEB018DE,
    0xDDE0EB2A,
    0x2F8B6829,
    0x82F63B78,
    0x709DB87B,
    0x63CD4B8F,
    0x91A6C88C,
    0x456CAC67,
    0xB7072F64,
    0xA457DC90,
    0x563C5F93,
    0x082F63B7,
    0xFA44E0B4,
    0xE9141340,
    0x1B7F9043,
    0xCFB5F4A8,
    0x3DDE77AB,
    0x2E8E845F,
    0xDCE5075C,
    0x92A8FC17,
    0x60C37F14,
    0x73938CE0,
    0x81F80FE3,
    0x55326B08,
    0xA759E80B,
    0xB4091BFF,
    0x466298FC,
    0x1871A4D8,
    0xEA1A27DB,
    0xF94AD42F,
    0x0B21572C,
    0xDFEB33C7,
    0x2D80B0C4,
    0x3ED04330,
    0xCCBBC033,
    0xA24BB5A6,
    0x502036A5,
    0x4370C551,
    0xB11B4652,
    0x65D122B9,
    0x97BAA1BA,
    0x84EA524E,
    0x7681D14D,
    0x2892ED69,
    0xDAF96E6A,
    0xC9A99D9E,
    0x3BC21E9D,
    0xEF087A76,
    0x1D63F975,
    0x0E330A81,
    0xFC588982,
    0xB21572C9,
    0x407EF1CA,
    0x532E023E,
    0xA145813D,
    0x758FE5D6,
    0x87E466D5,
    0x94B49521,
    0x66DF1622,
    0x38CC2A06,
    0xCAA7A905,
    0xD9F75AF1,
    0x2B9CD9F2,
    0xFF56BD19,
    0x0D3D3E1A,
    0x1E6DCDEE,
    0xEC064EED,
    0xC38D26C4,
    0x31E6A5C7,
    0x22B65633,
    0xD0DDD530,
    0x0417B1DB,
    0xF67C32D8,
    0xE52CC12C,
    0x1747422F,
    0x49547E0B,
    0xBB3FFD08,
    0xA86F0EFC,
    0x5A048DFF,
    0x8ECEE914,
    0x7CA56A17,
    0x6FF599E3,
    0x9D9E1AE0,
    0xD3D3E1AB,
    0x21B862A8,
    0x32E8915C,
    0xC083125F,
    0x144976B4,
    0xE622F5B7,
    0xF5720643,
    0x07198540,
    0x590AB964,
    0xAB613A67,
    0xB831C993,
    0x4A5A4A90,
    0x9E902E7B,
    0x6CFBAD78,
    0x7FAB5E8C,
    0x8DC0DD8F,
    0xE330A81A,
    0x115B2B19,
    0x020BD8ED,
    0xF0605BEE,
    0x24AA3F05,
    0xD6C1BC06,
    0xC5914FF2,
    0x37FACCF1,
    0x69E9F0D5,
    0x9B8273D6,
    0x88D28022,
    0x7AB90321,
    0xAE7367CA,
    0x5C18E4C9,
    0x4F48173D,
    0xBD23943E,
    0xF36E6F75,
    0x0105EC76,
    0x12551F82,
    0xE03E9C81,
    0x34F4F86A,
    0xC69F7B69,
    0xD5CF889D,
    0x27A40B9E,
    0x79B737BA,
    0x8BDCB4B9,
    0x988C474D,
    0x6AE7C44E,
    0xBE2DA0A5,
    0x4C4623A6,
    0x5F16D052,
    0xAD7D5351,
)


def _crc32c(parts):
    checksum = 0xFFFFFFFF
    for part in parts:
        for byte in part:
            checksum = _CRC32C_TABLE[(checksum ^ byte) & 0xFF] ^ (checksum >> 8)
    return checksum ^ 0xFFFFFFFF


def encode_result_record(record):
    if not isinstance(record.kind, ResultRecordKind):
        raise ValueError("result record kind is unknown")
    if not 0 <= record.flags <= 0xFFFF:
        raise ValueError("result record flags exceed u16")
    if not 0 <= record.record_sequence <= 0xFFFFFFFF:
        raise ValueError("result record sequence exceeds u32")
    if not 0 <= record.input_sequence <= 0xFFFFFFFF:
        raise ValueError("result record input sequence exceeds u32")
    if len(record.payload) > RESULT_RECORD_KIND_BOUNDS[record.kind][0]:
        raise ValueError("result record payload exceeds its kind-specific bound")

    total_length = RESULT_RECORD_HEADER_BYTES + len(record.payload)
    header_without_checksum = struct.pack(
        "<IBBHIII",
        total_length,
        RESULT_RECORD_VERSION,
        int(record.kind),
        record.flags,
        record.record_sequence,
        record.input_sequence,
        len(record.payload),
    )
    checksum = _crc32c((header_without_checksum[4:], record.payload))
    return header_without_checksum + struct.pack("<I", checksum) + record.payload


def decode_result_record(encoded):
    if len(encoded) < RESULT_RECORD_HEADER_BYTES:
        raise ValueError("result record header is truncated")
    if len(encoded) > MAX_LLM_RESULT_RECORD_BYTES:
        raise ValueError("result record exceeds 1048576 bytes")

    header_bytes = encoded[:RESULT_RECORD_HEADER_BYTES]
    header = decode_result_record_header(header_bytes)
    if header.total_length != len(encoded):
        raise ValueError("result record total length does not match its bytes")
    return decode_result_record_parts(header_bytes, encoded[RESULT_RECORD_HEADER_BYTES:])


def decode_result_record_header(header_bytes):
    if len(header_bytes) != RESULT_RECORD_HEADER_BYTES:
        raise ValueError("result record header must contain exactly 24 bytes")
    total_length, version, kind_value, flags, record_sequence, input_sequence, payload_length, checksum = struct.unpack(
        "<IBBHIIII", header_bytes
    )
    if not RESULT_RECORD_HEADER_BYTES <= total_length <= MAX_LLM_RESULT_RECORD_BYTES:
        raise ValueError("result record total length is outside its bound")
    if version != RESULT_RECORD_VERSION:
        raise ValueError("result record version is unsupported")
    try:
        kind = ResultRecordKind(kind_value)
    except ValueError as error:
        raise ValueError("result record kind is unknown") from error
    if RESULT_RECORD_HEADER_BYTES + payload_length != total_length:
        raise ValueError("result record payload length does not match its total length")
    if payload_length > RESULT_RECORD_KIND_BOUNDS[kind][0]:
        raise ValueError("result record payload exceeds its kind-specific bound")
    return ResultRecordHeader(total_length, kind, flags, record_sequence, input_sequence, payload_length, checksum)


def decode_result_record_parts(header_bytes, payload):
    header = decode_result_record_header(header_bytes)
    if len(payload) != header.payload_length:
        raise ValueError("result record payload length does not match its bytes")
    calculated_checksum = _crc32c((header_bytes[4:20], payload))
    if calculated_checksum != header.checksum:
        raise ValueError("result record CRC32C does not match")
    return ResultRecord(header.kind, header.flags, header.record_sequence, header.input_sequence, payload)


IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS = 768
FACE_EMBEDDING_DIMENSIONS = 512
MAX_FAILURE_TEXT_BYTES = 4 * 1024 - 4
MAX_TEXT_BYTES = MAX_LLM_RESULT_RECORD_PAYLOAD_BYTES - 4
MAX_TAGS = 65_536
MAX_TAG_BYTES = 4 * 1024
TAG_NORMALIZED_ENTRY_BYTES = 24


def encode_failure_payload(error):
    if not error:
        raise ValueError("failure error must not be empty")
    return _encode_string(error, MAX_FAILURE_TEXT_BYTES, "failure error")


def encode_input_started_payload(frame_timestamp_ms):
    if frame_timestamp_ms is None:
        return struct.pack("<B7xq", 0, 0)
    if not -(2**63) <= frame_timestamp_ms < 2**63:
        raise ValueError("frame timestamp exceeds i64")
    return struct.pack("<B7xq", 1, frame_timestamp_ms)


def encode_text_payload(text):
    return _encode_string(text, MAX_TEXT_BYTES, "result text")


def encode_tags_payload(tags):
    if len(tags) > MAX_TAGS:
        raise ValueError("image tags exceed the 65536-tag bound")
    encoded = bytearray(struct.pack("<I", len(tags)))
    normalized_bytes = len(tags) * TAG_NORMALIZED_ENTRY_BYTES
    for tag in tags:
        encoded_tag = _encode_string(tag, MAX_TAG_BYTES, "image tag")
        normalized_bytes += len(encoded_tag) - 4
        if normalized_bytes > MAX_NORMALIZED_RESULT_RECORD_BYTES:
            raise ValueError("image tags exceed the normalized result bound")
        encoded.extend(encoded_tag)
        if len(encoded) > MAX_LLM_RESULT_RECORD_PAYLOAD_BYTES:
            raise ValueError("image tags exceed the encoded result-record bound")
    return bytes(encoded)


def encode_image_clustering_payload(embedding, perceptual_hash, quality_score):
    if len(embedding) != IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS:
        raise ValueError("image clustering embedding must contain 768 values")
    _require_finite(embedding, "image clustering embedding")
    _require_unit((quality_score,), "image clustering quality score")
    if not 0 <= perceptual_hash <= 0xFFFFFFFFFFFFFFFF:
        raise ValueError("perceptual hash exceeds u64")
    return (
        struct.pack("<I", len(embedding))
        + struct.pack(f"<{len(embedding)}f", *embedding)
        + struct.pack("<Qf", perceptual_hash, quality_score)
    )


def encode_image_aesthetics_payload(aesthetic, scenic, simplicity, landscape, technical_quality):
    values = (aesthetic, scenic, simplicity, landscape, technical_quality)
    _require_unit(values, "image aesthetics score")
    return struct.pack("<5f", *values)


def encode_face_payload(
    index,
    x,
    y,
    width,
    height,
    eye_center_x,
    eye_center_y,
    confidence,
    face_size_score,
    frontality_score,
    visibility_score,
    feature_clarity_score,
    embedding,
):
    if not 0 <= index <= 0xFFFFFFFF:
        raise ValueError("face index exceeds u32")
    if len(embedding) != FACE_EMBEDDING_DIMENSIONS:
        raise ValueError("face embedding must contain 512 values")
    scores = (
        x,
        y,
        width,
        height,
        eye_center_x,
        eye_center_y,
        confidence,
        face_size_score,
        frontality_score,
        visibility_score,
        feature_clarity_score,
    )
    _validate_face(scores, embedding)
    return (
        struct.pack("<II", index, len(embedding))
        + struct.pack("<11f", *scores)
        + struct.pack(f"<{len(embedding)}f", *embedding)
    )


def encode_classification_payload(detected, confidence):
    if not isinstance(detected, bool):
        raise ValueError("classification detected flag must be boolean")
    _require_unit((confidence,), "classification confidence")
    return struct.pack("<B3xf", int(detected), confidence)


def decode_result_payload(kind, payload):
    if kind == ResultRecordKind.FAILURE:
        error = _decode_only_string(payload, MAX_FAILURE_TEXT_BYTES, "failure error")
        if not error:
            raise ValueError("failure error must not be empty")
        return {"error": error}
    if kind == ResultRecordKind.INPUT_STARTED:
        if len(payload) != 16:
            raise ValueError("input-started payload must contain exactly 16 bytes")
        present, reserved, timestamp = struct.unpack("<B7sq", payload)
        if reserved != bytes(7):
            raise ValueError("input-started reserved bytes must be zero")
        if present == 0 and timestamp == 0:
            return {"frameTimestampMs": None}
        if present == 1:
            return {"frameTimestampMs": timestamp}
        if present == 0:
            raise ValueError("absent frame timestamp must encode zero")
        raise ValueError("input-started timestamp presence flag is invalid")
    if kind in (ResultRecordKind.OCR_TEXT, ResultRecordKind.OCR_TEXT_CONTINUATION):
        return {"text": _decode_only_string(payload, MAX_TEXT_BYTES, "result text")}
    if kind in (ResultRecordKind.IMAGE_TAGS, ResultRecordKind.IMAGE_TAGS_CONTINUATION):
        return {"tags": _decode_tags(payload)}
    if kind == ResultRecordKind.IMAGE_CLUSTERING:
        return _decode_image_clustering(payload)
    if kind == ResultRecordKind.IMAGE_AESTHETICS:
        if len(payload) != 20:
            raise ValueError("image aesthetics payload must contain exactly five float32 scores")
        values = struct.unpack("<5f", payload)
        _require_unit(values, "image aesthetics score")
        return dict(zip(("aesthetic", "scenic", "simplicity", "landscape", "technicalQuality"), values))
    if kind == ResultRecordKind.FACE:
        return _decode_face(payload)
    if kind in (ResultRecordKind.SCREENSHOT_DETECTION, ResultRecordKind.DOCUMENT_DETECTION):
        if len(payload) != 8:
            raise ValueError("classification payload must contain exactly 8 bytes")
        detected, reserved, confidence = struct.unpack("<B3sf", payload)
        if reserved != bytes(3):
            raise ValueError("classification reserved bytes must be zero")
        if detected not in (0, 1):
            raise ValueError("classification detected flag is invalid")
        _require_unit((confidence,), "classification confidence")
        return {"detected": bool(detected), "confidence": confidence}
    if kind == ResultRecordKind.INPUT_FINISHED:
        if payload:
            raise ValueError("input-finished payload must be empty")
        return None
    raise ValueError("result record kind is unknown")


def _decode_tags(payload):
    if len(payload) < 4:
        raise ValueError("result payload is truncated")
    tag_count = struct.unpack_from("<I", payload)[0]
    if tag_count > MAX_TAGS:
        raise ValueError("image tags exceed the 65536-tag bound")
    tags = []
    offset = 4
    normalized_bytes = tag_count * TAG_NORMALIZED_ENTRY_BYTES
    for _ in range(tag_count):
        tag, offset = _decode_string(payload, offset, MAX_TAG_BYTES, "image tag")
        normalized_bytes += len(tag.encode("utf-8"))
        if normalized_bytes > MAX_NORMALIZED_RESULT_RECORD_BYTES:
            raise ValueError("image tags exceed the normalized result bound")
        tags.append(tag)
    if offset != len(payload):
        raise ValueError("result payload contains trailing bytes")
    return tags


def _decode_image_clustering(payload):
    expected_bytes = 4 + IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS * 4 + 8 + 4
    if len(payload) != expected_bytes:
        raise ValueError("image clustering payload length is invalid")
    dimensions = struct.unpack_from("<I", payload)[0]
    if dimensions != IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS:
        raise ValueError("image clustering embedding must contain 768 values")
    embedding_offset = 4
    embedding_end = embedding_offset + dimensions * 4
    embedding = list(struct.unpack(f"<{dimensions}f", payload[embedding_offset:embedding_end]))
    perceptual_hash, quality_score = struct.unpack("<Qf", payload[embedding_end:])
    _require_finite(embedding, "image clustering embedding")
    _require_unit((quality_score,), "image clustering quality score")
    return {
        "embedding": embedding,
        "perceptualHash": perceptual_hash,
        "qualityScore": quality_score,
    }


def _decode_face(payload):
    expected_bytes = 8 + 11 * 4 + FACE_EMBEDDING_DIMENSIONS * 4
    if len(payload) != expected_bytes:
        raise ValueError("face payload length is invalid")
    index, dimensions = struct.unpack_from("<II", payload)
    if dimensions != FACE_EMBEDDING_DIMENSIONS:
        raise ValueError("face embedding must contain 512 values")
    scores = struct.unpack_from("<11f", payload, 8)
    embedding = list(struct.unpack_from(f"<{dimensions}f", payload, 8 + 11 * 4))
    _validate_face(scores, embedding)
    names = (
        "x",
        "y",
        "width",
        "height",
        "eyeCenterX",
        "eyeCenterY",
        "confidence",
        "faceSizeScore",
        "frontalityScore",
        "visibilityScore",
        "featureClarityScore",
    )
    return {"index": index, **dict(zip(names, scores)), "embedding": embedding}


def _encode_string(value, maximum_bytes, field):
    if not isinstance(value, str):
        raise ValueError(f"{field} must be a string")
    encoded = value.encode("utf-8")
    if len(encoded) > maximum_bytes:
        raise ValueError(f"{field} exceeds its byte bound")
    return struct.pack("<I", len(encoded)) + encoded


def _decode_only_string(payload, maximum_bytes, field):
    value, offset = _decode_string(payload, 0, maximum_bytes, field)
    if offset != len(payload):
        raise ValueError("result payload contains trailing bytes")
    return value


def _decode_string(payload, offset, maximum_bytes, field):
    if offset + 4 > len(payload):
        raise ValueError("result payload is truncated")
    length = struct.unpack_from("<I", payload, offset)[0]
    if length > maximum_bytes:
        raise ValueError(f"{field} exceeds its byte bound")
    start = offset + 4
    end = start + length
    if end > len(payload):
        raise ValueError("result payload is truncated")
    try:
        value = payload[start:end].decode("utf-8")
    except UnicodeDecodeError as error:
        raise ValueError(f"{field} is not valid UTF-8") from error
    return value, end


def _require_finite(values, field):
    if not all(math.isfinite(value) for value in values):
        raise ValueError(f"{field} must be finite")


def _require_unit(values, field):
    _require_finite(values, field)
    if not all(0.0 <= value <= 1.0 for value in values):
        raise ValueError(f"{field} must be within [0, 1]")


def _validate_face(scores, embedding):
    _require_unit(scores, "face score")
    x, y, width, height = scores[:4]
    if x >= 1.0 or y >= 1.0 or width <= 0.0 or height <= 0.0 or x + width > 1.0 + 1e-6 or y + height > 1.0 + 1e-6:
        raise ValueError("face bounding box must be normalized within the input")
    _require_finite(embedding, "face embedding")
    norm = math.sqrt(sum(value * value for value in embedding))
    if abs(norm - 1.0) > 0.01:
        raise ValueError("face embedding must be normalized")


class ResultRecordStreamValidator:
    _TASK_KINDS = {
        "ocr": (ResultRecordKind.OCR_TEXT, ResultRecordKind.OCR_TEXT_CONTINUATION, False),
        "image_tagging": (ResultRecordKind.IMAGE_TAGS, ResultRecordKind.IMAGE_TAGS_CONTINUATION, False),
        "image_clustering": (ResultRecordKind.IMAGE_CLUSTERING, None, False),
        "image_aesthetics": (ResultRecordKind.IMAGE_AESTHETICS, None, False),
        "face_detection": (ResultRecordKind.FACE, None, True),
        "screenshot_detection": (ResultRecordKind.SCREENSHOT_DETECTION, None, False),
        "document_detection": (ResultRecordKind.DOCUMENT_DETECTION, None, False),
    }

    def __init__(self, task, status, inputs, declared_record_count, declared_byte_size):
        if task not in self._TASK_KINDS:
            raise ValueError("result task is unknown")
        if status not in ("completed", "failed"):
            raise ValueError("result status is unknown")
        if not 1 <= len(inputs) <= MAX_LLM_RESULT_INPUTS:
            raise ValueError("result manifest must contain between 1 and 1024 inputs")
        if not 1 <= declared_record_count <= MAX_LLM_RESULT_RECORDS:
            raise ValueError("result manifest record count is outside its bound")
        if not 1 <= declared_byte_size <= MAX_LLM_RESULT_BYTES:
            raise ValueError("result manifest byte size is outside its bound")
        normalized_inputs = []
        previous_sequence = None
        for sequence, frame_timestamp_ms in inputs:
            if not 0 <= sequence <= 0xFFFFFFFF:
                raise ValueError("result manifest input sequence exceeds u32")
            if previous_sequence is not None and previous_sequence >= sequence:
                raise ValueError("result manifest input sequences must be strictly ordered")
            previous_sequence = sequence
            normalized_inputs.append((sequence, frame_timestamp_ms))
        permits_empty = self._TASK_KINDS[task][2]
        minimum_count = 1 if status == "failed" else len(inputs) * (2 if permits_empty else 3)
        if declared_record_count < minimum_count:
            raise ValueError("result manifest contains too few records for its task")
        self.task = task
        self.status = status
        self.inputs = normalized_inputs
        self.declared_record_count = declared_record_count
        self.next_record_sequence = 0
        self.input_index = 0
        self.input_state = "awaiting_start"
        self.continuation_count = 0
        self.aggregate_normalized_bytes = 0
        self.failed_record_seen = False

    def push(self, record):
        if self.next_record_sequence >= self.declared_record_count:
            raise ValueError("result stream contains more records than declared")
        if record.record_sequence != self.next_record_sequence:
            raise ValueError("result record sequence is not contiguous")
        if record.flags != 0:
            raise ValueError("result record flags are unsupported")
        decoded = decode_result_payload(record.kind, record.payload)
        if self.status == "failed":
            self._push_failed(record, decoded)
        else:
            self._push_completed(record, decoded)
        self.next_record_sequence += 1
        return decoded

    def finish(self):
        if self.next_record_sequence != self.declared_record_count:
            raise ValueError("result stream record count does not match its manifest")
        if self.status == "failed":
            if not self.failed_record_seen:
                raise ValueError("failed result stream has no failure record")
            return
        if self.input_index != len(self.inputs) or self.input_state != "awaiting_start":
            raise ValueError("completed result stream ended inside an input")

    def _push_failed(self, record, decoded):
        if (
            self.failed_record_seen
            or record.kind != ResultRecordKind.FAILURE
            or record.input_sequence != 0xFFFFFFFF
            or set(decoded) != {"error"}
        ):
            raise ValueError("failed result must contain exactly one unscoped failure record")
        self.failed_record_seen = True

    def _push_completed(self, record, decoded):
        if self.input_index >= len(self.inputs):
            raise ValueError("result stream contains records after its final input")
        expected_sequence, expected_timestamp = self.inputs[self.input_index]
        if record.input_sequence != expected_sequence:
            raise ValueError("result record input sequence does not match its manifest")
        primary_kind, continuation_kind, permits_empty = self._TASK_KINDS[self.task]
        if self.input_state == "awaiting_start":
            if record.kind != ResultRecordKind.INPUT_STARTED:
                raise ValueError("result input must begin with input-started")
            if decoded["frameTimestampMs"] != expected_timestamp:
                raise ValueError("result input timestamp does not match its manifest")
            self.input_state = "accepting_values" if permits_empty else "awaiting_primary"
            self.continuation_count = 0
            return
        if self.input_state == "awaiting_primary":
            if record.kind != primary_kind:
                raise ValueError("result input primary record does not match its task")
            self._charge_normalized(record.kind, decoded)
            self.input_state = "accepting_values"
            return
        if record.kind == ResultRecordKind.INPUT_FINISHED:
            self.input_index += 1
            self.input_state = "awaiting_start"
            self.continuation_count = 0
            return
        if self.task == "face_detection" and record.kind == ResultRecordKind.FACE:
            return
        if continuation_kind is not None and record.kind == continuation_kind:
            self.continuation_count += 1
            if self.continuation_count > MAX_LLM_RESULT_CONTINUATIONS_PER_VALUE:
                raise ValueError("result value has too many continuation records")
            self._charge_normalized(record.kind, decoded)
            return
        raise ValueError("result record is not valid in the current input state")

    def _charge_normalized(self, kind, decoded):
        if kind in (ResultRecordKind.OCR_TEXT, ResultRecordKind.OCR_TEXT_CONTINUATION):
            added = len(decoded["text"].encode("utf-8"))
        elif kind in (ResultRecordKind.IMAGE_TAGS, ResultRecordKind.IMAGE_TAGS_CONTINUATION):
            added = sum(len(tag.encode("utf-8")) + 1 for tag in decoded["tags"])
        else:
            added = 0
        self.aggregate_normalized_bytes += added
        if self.aggregate_normalized_bytes > MAX_NORMALIZED_RESULT_RECORD_BYTES:
            raise ValueError("result text/tag aggregate exceeds 2 MiB")
