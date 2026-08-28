"""Thread-safe cross-request dynamic batching for local model runtimes."""

import queue
import threading
import time
from collections.abc import Callable, Sequence
from typing import Generic, TypeVar, cast

BatchInput = TypeVar("BatchInput")
BatchOutput = TypeVar("BatchOutput")
STOP_BATCH_WORKER = object()


class PendingBatchItem(Generic[BatchInput, BatchOutput]):
    input_value: BatchInput
    output_value: BatchOutput | None
    error: Exception | None

    def __init__(self, input_value: BatchInput) -> None:
        self.input_value = input_value
        self.output_value = None
        self.error = None
        self.completed = threading.Event()

    def succeed(self, output_value: BatchOutput) -> None:
        self.output_value = output_value
        self.completed.set()

    def fail(self, error: Exception) -> None:
        self.error = error
        self.completed.set()

    def wait(self, worker_name: str) -> BatchOutput:
        self.completed.wait()
        if self.error is not None:
            raise RuntimeError(f"{worker_name} failed: {self.error}") from self.error
        return cast(BatchOutput, self.output_value)


class DynamicBatcher(Generic[BatchInput, BatchOutput]):
    def __init__(
        self,
        process_batch: Callable[[list[BatchInput]], Sequence[BatchOutput]],
        batch_size: int,
        batch_wait_milliseconds: int,
        worker_name: str,
    ) -> None:
        if batch_size <= 0:
            raise ValueError("dynamic batch_size must be positive")
        if batch_wait_milliseconds < 0:
            raise ValueError("dynamic batch_wait_milliseconds must not be negative")
        if not worker_name.strip():
            raise ValueError("dynamic batch worker_name must not be empty")

        self.process_batch = process_batch
        self.batch_size = batch_size
        self.batch_wait_seconds = batch_wait_milliseconds / 1000.0
        self.worker_name = worker_name
        self.pending_items: queue.Queue[object] = queue.Queue(maxsize=batch_size)
        self.worker = threading.Thread(target=self._run, name=worker_name, daemon=True)
        self.worker.start()

    def infer(self, input_values: Sequence[BatchInput]) -> list[BatchOutput]:
        pending_items = [PendingBatchItem[BatchInput, BatchOutput](input_value) for input_value in input_values]
        for pending_item in pending_items:
            self.pending_items.put(pending_item)
        return [pending_item.wait(self.worker_name) for pending_item in pending_items]

    def close(self) -> None:
        self.pending_items.put(STOP_BATCH_WORKER)
        self.worker.join()

    def _run(self) -> None:
        while True:
            first_item = self.pending_items.get()
            if first_item is STOP_BATCH_WORKER:
                return
            pending_batch, should_stop = self._collect_batch(
                cast(PendingBatchItem[BatchInput, BatchOutput], first_item)
            )
            self._process_batch(pending_batch)
            if should_stop:
                return

    def _collect_batch(
        self, first_item: PendingBatchItem[BatchInput, BatchOutput]
    ) -> tuple[list[PendingBatchItem[BatchInput, BatchOutput]], bool]:
        pending_batch = [first_item]
        batch_deadline = time.monotonic() + self.batch_wait_seconds
        while len(pending_batch) < self.batch_size:
            remaining_seconds = batch_deadline - time.monotonic()
            if remaining_seconds <= 0.0:
                break
            try:
                pending_item = self.pending_items.get(timeout=remaining_seconds)
            except queue.Empty:
                break
            if pending_item is STOP_BATCH_WORKER:
                return pending_batch, True
            pending_batch.append(cast(PendingBatchItem[BatchInput, BatchOutput], pending_item))
        return pending_batch, False

    def _process_batch(self, pending_batch: list[PendingBatchItem[BatchInput, BatchOutput]]) -> None:
        try:
            output_values = self.process_batch([pending_item.input_value for pending_item in pending_batch])
            if len(output_values) != len(pending_batch):
                raise RuntimeError("dynamic batch processor returned a different number of outputs than inputs")
        except (RuntimeError, TypeError, ValueError) as error:
            for pending_item in pending_batch:
                pending_item.fail(error)
            return

        for pending_item, output_value in zip(pending_batch, output_values):
            pending_item.succeed(output_value)
