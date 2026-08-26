import importlib.util
import sys
import types
import unittest
from pathlib import Path
from unittest.mock import patch


SOURCE_PATH = (
    Path(__file__).resolve().parents[2] / "src" / "backend_llm" / "ocr_server.py"
)


class OcrServerTests(unittest.TestCase):
    def test_registers_image_decoders_before_starting_vllm(self):
        events = []
        image_runtime = types.ModuleType("image_runtime")
        image_runtime.register_image_decoders = lambda: events.append("registered")
        specification = importlib.util.spec_from_file_location("ocr_server", SOURCE_PATH)
        ocr_server = importlib.util.module_from_spec(specification)

        with patch.dict(sys.modules, {"image_runtime": image_runtime}):
            specification.loader.exec_module(ocr_server)
        with patch.object(
            ocr_server.runpy,
            "run_module",
            side_effect=lambda *args, **kwargs: events.append((args, kwargs)),
        ):
            ocr_server.main()

        self.assertEqual(events[0], "registered")
        self.assertEqual(
            events[1],
            (("vllm.entrypoints.openai.api_server",), {"run_name": "__main__"}),
        )


if __name__ == "__main__":
    unittest.main()
