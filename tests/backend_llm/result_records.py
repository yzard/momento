import importlib.util
import math
import unittest
from pathlib import Path

SOURCE_PATH = Path(__file__).resolve().parents[2] / "src" / "backend_llm" / "result_records.py"
SPECIFICATION = importlib.util.spec_from_file_location("result_records", SOURCE_PATH)
RESULT_RECORDS = importlib.util.module_from_spec(SPECIFICATION)
SPECIFICATION.loader.exec_module(RESULT_RECORDS)


class ResultRecordTests(unittest.TestCase):
    def test_matches_the_rust_golden_vector(self):
        record = RESULT_RECORDS.ResultRecord(RESULT_RECORDS.ResultRecordKind.OCR_TEXT, 0x0201, 7, 9, b"text")

        encoded = RESULT_RECORDS.encode_result_record(record)

        self.assertEqual(
            encoded,
            bytes([28, 0, 0, 0, 1, 3, 1, 2, 7, 0, 0, 0, 9, 0, 0, 0, 4, 0, 0, 0, 220, 167, 65, 217, 116, 101, 120, 116]),
        )
        self.assertEqual(RESULT_RECORDS.decode_result_record(encoded), record)

    def test_accepts_empty_and_maximum_payloads(self):
        for payload in (b"", bytes(RESULT_RECORDS.MAX_LLM_RESULT_RECORD_PAYLOAD_BYTES)):
            record = RESULT_RECORDS.ResultRecord(
                RESULT_RECORDS.ResultRecordKind.OCR_TEXT, 0, 0xFFFFFFFF, 0xFFFFFFFF, payload
            )
            self.assertEqual(RESULT_RECORDS.decode_result_record(RESULT_RECORDS.encode_result_record(record)), record)

    def test_header_is_validated_before_payload_allocation(self):
        record = RESULT_RECORDS.ResultRecord(RESULT_RECORDS.ResultRecordKind.OCR_TEXT, 0, 4, 7, b"bounded text")
        encoded = RESULT_RECORDS.encode_result_record(record)
        header_bytes = encoded[: RESULT_RECORDS.RESULT_RECORD_HEADER_BYTES]
        header = RESULT_RECORDS.decode_result_record_header(header_bytes)
        self.assertEqual(header.total_length, len(encoded))
        self.assertEqual(header.payload_length, len(record.payload))
        self.assertEqual(
            RESULT_RECORDS.decode_result_record_parts(
                header_bytes, encoded[RESULT_RECORDS.RESULT_RECORD_HEADER_BYTES :]
            ),
            record,
        )
        with self.assertRaises(ValueError):
            RESULT_RECORDS.decode_result_record_header(header_bytes[:-1])
        with self.assertRaises(ValueError):
            RESULT_RECORDS.decode_result_record_parts(
                header_bytes, encoded[RESULT_RECORDS.RESULT_RECORD_HEADER_BYTES : -1]
            )

    def test_rejects_invalid_bounds_and_corruption(self):
        oversized = RESULT_RECORDS.ResultRecord(
            RESULT_RECORDS.ResultRecordKind.FACE, 0, 0, 0, bytes(RESULT_RECORDS.MAX_LLM_RESULT_RECORD_PAYLOAD_BYTES + 1)
        )
        with self.assertRaisesRegex(ValueError, "payload exceeds"):
            RESULT_RECORDS.encode_result_record(oversized)

        valid = bytearray(
            RESULT_RECORDS.encode_result_record(
                RESULT_RECORDS.ResultRecord(RESULT_RECORDS.ResultRecordKind.FAILURE, 0, 0, 0, b"failed")
            )
        )
        for cut in range(RESULT_RECORDS.RESULT_RECORD_HEADER_BYTES):
            with self.assertRaises(ValueError):
                RESULT_RECORDS.decode_result_record(valid[:cut])
        invalid_kind = valid.copy()
        invalid_kind[5] = 0xFF
        with self.assertRaisesRegex(ValueError, "kind is unknown"):
            RESULT_RECORDS.decode_result_record(invalid_kind)
        invalid_payload = valid.copy()
        invalid_payload[-1] ^= 1
        with self.assertRaisesRegex(ValueError, "CRC32C"):
            RESULT_RECORDS.decode_result_record(invalid_payload)

    def test_payload_layout_matches_the_rust_golden_vectors(self):
        self.assertEqual(
            RESULT_RECORDS.encode_input_started_payload(0x0102030405060708),
            bytes([1, 0, 0, 0, 0, 0, 0, 0, 8, 7, 6, 5, 4, 3, 2, 1]),
        )
        self.assertEqual(
            RESULT_RECORDS.encode_classification_payload(True, 0.5),
            bytes([1, 0, 0, 0, 0, 0, 0, 63]),
        )

    def test_every_payload_family_round_trips(self):
        payloads = (
            (
                RESULT_RECORDS.ResultRecordKind.FAILURE,
                RESULT_RECORDS.encode_failure_payload("model failed"),
            ),
            (
                RESULT_RECORDS.ResultRecordKind.INPUT_STARTED,
                RESULT_RECORDS.encode_input_started_payload(42),
            ),
            (
                RESULT_RECORDS.ResultRecordKind.OCR_TEXT,
                RESULT_RECORDS.encode_text_payload("hello 世界"),
            ),
            (
                RESULT_RECORDS.ResultRecordKind.IMAGE_TAGS,
                RESULT_RECORDS.encode_tags_payload(["cat", "night sky"]),
            ),
            (
                RESULT_RECORDS.ResultRecordKind.IMAGE_CLUSTERING,
                RESULT_RECORDS.encode_image_clustering_payload(
                    [0.25] * RESULT_RECORDS.IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS,
                    0x0102030405060708,
                    0.75,
                ),
            ),
            (
                RESULT_RECORDS.ResultRecordKind.IMAGE_AESTHETICS,
                RESULT_RECORDS.encode_image_aesthetics_payload(0.1, 0.2, 0.3, 0.4, 0.5),
            ),
            (
                RESULT_RECORDS.ResultRecordKind.FACE,
                RESULT_RECORDS.encode_face_payload(
                    3,
                    0.1,
                    0.2,
                    0.3,
                    0.4,
                    0.2,
                    0.3,
                    0.9,
                    0.8,
                    0.7,
                    0.6,
                    0.5,
                    [1.0 / math.sqrt(RESULT_RECORDS.FACE_EMBEDDING_DIMENSIONS)]
                    * RESULT_RECORDS.FACE_EMBEDDING_DIMENSIONS,
                ),
            ),
            (
                RESULT_RECORDS.ResultRecordKind.SCREENSHOT_DETECTION,
                RESULT_RECORDS.encode_classification_payload(True, 0.875),
            ),
            (RESULT_RECORDS.ResultRecordKind.INPUT_FINISHED, b""),
        )
        for kind, payload in payloads:
            RESULT_RECORDS.decode_result_payload(kind, payload)

    def test_payload_decoders_reject_malformed_or_non_finite_values(self):
        with self.assertRaises(ValueError):
            RESULT_RECORDS.decode_result_payload(RESULT_RECORDS.ResultRecordKind.INPUT_STARTED, bytes(15))
        with self.assertRaises(ValueError):
            RESULT_RECORDS.decode_result_payload(RESULT_RECORDS.ResultRecordKind.INPUT_FINISHED, b"x")
        with self.assertRaisesRegex(ValueError, "65536-tag"):
            RESULT_RECORDS.decode_result_payload(
                RESULT_RECORDS.ResultRecordKind.IMAGE_TAGS,
                (0xFFFFFFFF).to_bytes(4, "little"),
            )
        with self.assertRaisesRegex(ValueError, "finite"):
            RESULT_RECORDS.encode_classification_payload(False, float("nan"))

    def test_stream_validator_accepts_ordered_ocr_and_failed_results(self):
        validator = RESULT_RECORDS.ResultRecordStreamValidator("ocr", "completed", [(3, None)], 4, 256)
        records = (
            self.record(
                RESULT_RECORDS.ResultRecordKind.INPUT_STARTED, 0, 3, RESULT_RECORDS.encode_input_started_payload(None)
            ),
            self.record(RESULT_RECORDS.ResultRecordKind.OCR_TEXT, 1, 3, RESULT_RECORDS.encode_text_payload("first")),
            self.record(
                RESULT_RECORDS.ResultRecordKind.OCR_TEXT_CONTINUATION,
                2,
                3,
                RESULT_RECORDS.encode_text_payload(" page"),
            ),
            self.record(RESULT_RECORDS.ResultRecordKind.INPUT_FINISHED, 3, 3, b""),
        )
        for record in records:
            validator.push(record)
        validator.finish()

        failed = RESULT_RECORDS.ResultRecordStreamValidator("ocr", "failed", [(3, None)], 1, 64)
        failed.push(
            self.record(
                RESULT_RECORDS.ResultRecordKind.FAILURE,
                0,
                0xFFFFFFFF,
                RESULT_RECORDS.encode_failure_payload("inference failed"),
            )
        )
        failed.finish()

    def test_stream_validator_rejects_task_mismatch_and_fifth_continuation(self):
        validator = RESULT_RECORDS.ResultRecordStreamValidator("ocr", "completed", [(0, None)], 8, 4096)
        validator.push(
            self.record(
                RESULT_RECORDS.ResultRecordKind.INPUT_STARTED,
                0,
                0,
                RESULT_RECORDS.encode_input_started_payload(None),
            )
        )
        validator.push(
            self.record(
                RESULT_RECORDS.ResultRecordKind.OCR_TEXT,
                1,
                0,
                RESULT_RECORDS.encode_text_payload("base"),
            )
        )
        for sequence in range(2, 6):
            validator.push(
                self.record(
                    RESULT_RECORDS.ResultRecordKind.OCR_TEXT_CONTINUATION,
                    sequence,
                    0,
                    RESULT_RECORDS.encode_text_payload("part"),
                )
            )
        with self.assertRaisesRegex(ValueError, "too many"):
            validator.push(
                self.record(
                    RESULT_RECORDS.ResultRecordKind.OCR_TEXT_CONTINUATION,
                    6,
                    0,
                    RESULT_RECORDS.encode_text_payload("part"),
                )
            )

    @staticmethod
    def record(kind, record_sequence, input_sequence, payload):
        return RESULT_RECORDS.ResultRecord(kind, 0, record_sequence, input_sequence, payload)


if __name__ == "__main__":
    unittest.main()
