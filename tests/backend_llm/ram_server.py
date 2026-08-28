import importlib.util
import sys
import tempfile
import types
import unittest
from pathlib import Path

SOURCE_PATH = Path(__file__).resolve().parents[2] / "src" / "backend_llm" / "ram_server.py"


class RamServerTests(unittest.TestCase):
    def test_checkpoint_must_be_baked_into_the_image(self):
        ram_server = self.load_module()
        with tempfile.TemporaryDirectory() as directory:
            checkpoint = Path(directory) / "ram.pth"
            with self.assertRaisesRegex(RuntimeError, "checkpoint is missing"):
                ram_server.require_checkpoint(checkpoint)
            checkpoint.write_bytes(b"model")
            self.assertEqual(ram_server.require_checkpoint(checkpoint), checkpoint)

    def test_model_concurrency_is_bounded_inside_runtime(self):
        ram_server = self.load_module()

        slots = ram_server.create_inference_slots(2)

        self.assertTrue(slots.acquire(blocking=False))
        self.assertTrue(slots.acquire(blocking=False))
        self.assertFalse(slots.acquire(blocking=False))
        slots.release()
        slots.release()

    def load_module(self):
        torch_module = types.ModuleType("torch")
        torch_module.cuda = types.SimpleNamespace(is_available=lambda: False)
        torch_module.device = lambda value: value
        sys.modules["torch"] = torch_module
        ram_module = types.ModuleType("ram")
        ram_module.get_transform = lambda **_arguments: None
        ram_module.inference_ram = lambda *_arguments: ("", None)
        sys.modules["ram"] = ram_module
        ram_models_module = types.ModuleType("ram.models")
        ram_models_module.ram_plus = lambda **_arguments: None
        sys.modules["ram.models"] = ram_models_module
        specification = importlib.util.spec_from_file_location("ram_server", SOURCE_PATH)
        ram_server = importlib.util.module_from_spec(specification)
        specification.loader.exec_module(ram_server)
        return ram_server


if __name__ == "__main__":
    unittest.main()
