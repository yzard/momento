import importlib.util
import io
import sys
import threading
import unittest
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from unittest.mock import Mock, patch

SOURCE = Path(__file__).resolve().parents[3] / "src/backend_llm/ocr/server.py"
sys.path.insert(0, str(SOURCE.parent.parent))
specification = importlib.util.spec_from_file_location("ocr_runtime_server", SOURCE)
server = importlib.util.module_from_spec(specification)
specification.loader.exec_module(server)


def response(text="", reason="stop"):
    return {"choices": [{"finish_reason": reason, "message": {"content": text}}]}


def image_fixture():
    image = Mock()
    image.save.side_effect = lambda output, format: output.write(b"lossless-image")
    return image


class OcrRuntimeTests(unittest.TestCase):
    def runtime(self, result, concurrency=4, max_tokens=65536):
        engine = Mock()
        engine.complete.return_value = result
        return server.OcrRuntime(engine, "pinned-processor", max_tokens, concurrency)

    def test_batch_request_preserves_full_output_budget_and_lossless_input(self):
        runtime = self.runtime(response())
        self.assertEqual(runtime.infer(image_fixture()), {"text": ""})
        payload = runtime.engine.complete.call_args.args[0]
        self.assertEqual(payload["max_tokens"], 65536)
        self.assertEqual(payload["custom_params"]["max_page_tokens"], 0)
        self.assertEqual(payload["custom_params"]["window_size"], 128)
        self.assertEqual(payload["custom_logit_processor"], "pinned-processor")
        self.assertEqual(
            payload["messages"][0]["content"][0]["image_url"]["url"],
            "data:image/png;base64,bG9zc2xlc3MtaW1hZ2U=",
        )
        self.assertFalse(payload["stream"])
        self.assertFalse(payload["skip_special_tokens"])

    def test_output_budget_uses_the_configured_value(self):
        for max_tokens in (8192, 32768, 65536, 122880):
            with self.subTest(max_tokens=max_tokens):
                runtime = self.runtime(response(), max_tokens=max_tokens)
                runtime.infer(image_fixture())
                self.assertEqual(
                    runtime.engine.complete.call_args.args[0]["max_tokens"], max_tokens
                )

    def test_rejects_truncated_or_invalid_results(self):
        for result in [
            response("partial", "length"),
            response(None),
            {},
            None,
            {"choices": []},
        ]:
            with self.subTest(result=result), self.assertRaises(RuntimeError):
                self.runtime(result).infer(image_fixture())

    def test_capacity_precedes_input_open_and_image_is_closed_on_model_failure(self):
        events = []

        class Slots:
            def __enter__(self):
                events.append("acquire")

            def __exit__(self, *args):
                events.append("release")

        runtime = self.runtime(response("partial", "length"))
        runtime.slots = Slots()
        handler = object.__new__(server.OcrRequestHandler)
        handler.runtime, handler.input_root = runtime, "/inputs"
        image = image_fixture()

        def opened(*args):
            events.append("open")
            return io.BytesIO(b"image")

        with patch.object(
            server, "read_runtime_input", side_effect=opened
        ), patch.object(server, "decode_image", return_value=image):
            with self.assertRaises(RuntimeError):
                handler.run_inference()
        self.assertEqual(events, ["acquire", "open", "release"])
        image.close.assert_called_once()

    def test_two_requests_reach_engine_concurrently(self):
        runtime = self.runtime(response("text"), concurrency=2)
        barrier = threading.Barrier(2, timeout=3)

        def completed(payload):
            barrier.wait()
            return response("text")

        runtime.engine.complete.side_effect = completed
        handlers = [object.__new__(server.OcrRequestHandler) for _ in range(2)]
        for handler in handlers:
            handler.runtime, handler.input_root = runtime, "/inputs"
        with patch.object(
            server, "read_runtime_input", side_effect=lambda *args: io.BytesIO(b"image")
        ), patch.object(
            server, "decode_image", side_effect=lambda *args: image_fixture()
        ), ThreadPoolExecutor(
            max_workers=2
        ) as executor:
            results = list(
                executor.map(lambda handler: handler.run_inference(), handlers)
            )
        self.assertEqual(results, [{"text": "text"}, {"text": "text"}])


if __name__ == "__main__":
    unittest.main()
