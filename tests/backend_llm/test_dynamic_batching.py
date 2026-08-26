import threading
import unittest

from dynamic_batching import DynamicBatcher


class DynamicBatcherTests(unittest.TestCase):
    def test_combines_concurrent_requests_and_preserves_outputs(self):
        batch_sizes = []

        def process_batch(input_values):
            batch_sizes.append(len(input_values))
            return [input_value * 2 for input_value in input_values]

        batcher = DynamicBatcher(process_batch, 4, 100, "test-batcher")
        start_barrier = threading.Barrier(5)
        outputs = []

        def infer_value(input_value):
            start_barrier.wait()
            outputs.append(batcher.infer([input_value])[0])

        workers = [
            threading.Thread(target=infer_value, args=(input_value,))
            for input_value in range(4)
        ]
        for worker in workers:
            worker.start()
        start_barrier.wait()
        for worker in workers:
            worker.join()
        batcher.close()

        self.assertEqual(batch_sizes, [4])
        self.assertEqual(sorted(outputs), [0, 2, 4, 6])

    def test_returns_batch_failure_to_every_input(self):
        def fail_batch(_input_values):
            raise RuntimeError("model unavailable")

        batcher = DynamicBatcher(fail_batch, 4, 0, "failing-batcher")

        with self.assertRaisesRegex(RuntimeError, "model unavailable"):
            batcher.infer([1, 2])
        batcher.close()

    def test_rejects_invalid_configuration(self):
        for batch_size, wait_milliseconds, worker_name, message in [
            (0, 0, "batcher", "batch_size"),
            (1, -1, "batcher", "batch_wait_milliseconds"),
            (1, 0, " ", "worker_name"),
        ]:
            with self.assertRaisesRegex(ValueError, message):
                DynamicBatcher(
                    lambda input_values: input_values,
                    batch_size,
                    wait_milliseconds,
                    worker_name,
                )


if __name__ == "__main__":
    unittest.main()
