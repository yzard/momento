import sys
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

SOURCE = Path(__file__).resolve().parents[3] / "src/backend_llm"
sys.path.insert(0, str(SOURCE))
from ocr.engine import EngineUnavailable, SglangEngine, engine_command


class EngineTests(unittest.TestCase):
    def test_engine_command_enables_matching_decode_and_prefill_batch_limits(self):
        for concurrency in (1, 4, 8, 16):
            with self.subTest(concurrency=concurrency):
                command = engine_command("/models/ocr", "cuda:1", 8401, concurrency)
                self.assertEqual(
                    command[command.index("--max-running-requests") + 1],
                    str(concurrency),
                )
                self.assertEqual(
                    command[command.index("--cuda-graph-max-bs-decode") + 1],
                    str(concurrency),
                )
                self.assertEqual(command[command.index("--base-gpu-id") + 1], "1")
                self.assertEqual(
                    command[command.index("--cuda-graph-max-bs-prefill") + 1], "2048"
                )
                self.assertNotIn("--allow-auto-truncate", command)
        with self.assertRaises(ValueError):
            engine_command("/models/ocr", "cpu", 8401, 4)

    def test_context_leaves_input_room_for_120k_generated_tokens(self):
        command = engine_command("/models/ocr", "cuda", 8401, 16)
        context = int(command[command.index("--context-length") + 1])
        self.assertEqual(context, 131072)
        self.assertGreaterEqual(context - 122880, 8192)

    def test_engine_transport_loss_remains_distinct_from_model_failure(self):
        import urllib.error

        engine = object.__new__(SglangEngine)
        engine.base_url, engine.request_timeout = "http://127.0.0.1:8401", 1
        engine.opener = Mock()
        engine.opener.open.side_effect = urllib.error.URLError("connection lost")
        with self.assertRaises(EngineUnavailable):
            engine.complete({})

    def test_engine_exit_during_startup_is_a_failure(self):
        engine = object.__new__(SglangEngine)
        engine.process = Mock(returncode=7)
        engine.process.poll.return_value = 7
        with self.assertRaisesRegex(RuntimeError, "exited during startup"):
            engine.wait_until_ready(10)

    def test_engine_rejects_oversized_or_invalid_json(self):
        engine = object.__new__(SglangEngine)
        engine.base_url, engine.request_timeout = "http://127.0.0.1:8401", 1
        for body in (b"bad", b"123456"):
            result = Mock()
            result.__enter__ = Mock(return_value=result)
            result.__exit__ = Mock(return_value=False)
            result.read.return_value = body
            engine.opener = Mock()
            engine.opener.open.return_value = result
            with self.subTest(body=body), patch(
                "ocr.engine.MAX_RESPONSE_BYTES", 5
            ), self.assertRaises(RuntimeError):
                engine.complete({})


if __name__ == "__main__":
    unittest.main()
