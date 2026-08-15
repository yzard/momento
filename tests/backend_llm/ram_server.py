import importlib.util
import sys
import types
import unittest
from pathlib import Path


SOURCE_PATH = (
    Path(__file__).resolve().parents[2] / "src" / "backend_llm" / "ram_server.py"
)


class RamServerTests(unittest.TestCase):
    def test_model_concurrency_is_bounded_inside_runtime(self):
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

        slots = ram_server.create_inference_slots(2)

        self.assertTrue(slots.acquire(blocking=False))
        self.assertTrue(slots.acquire(blocking=False))
        self.assertFalse(slots.acquire(blocking=False))
        slots.release()
        slots.release()


if __name__ == "__main__":
    unittest.main()
