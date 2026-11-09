# Durable LLM queue and local runtimes

Read when changing queue capacity/recovery, model activation, inference concurrency, or result delivery.

### Durable llm-service queue

Admission links already-cached content and streams only missing files into `.tmp/<job-id>/`, validates
all descriptors and received bytes, syncs the staged data, publishes missing bytes unchanged into
`content/<sha256>/source`, and atomically renames the directory into `queuing/<job-id>/`. Job input
files are hard links to that content store. Each manifest records the authenticated client ID. Duplicate job
IDs owned by that client are acknowledged idempotently and are not enqueued twice; cross-client
collisions are rejected.

Admission computes byte counts and SHA-256 incrementally while writing and rejects a field before
writing beyond its declared size. It never rereads a complete input into memory. Abandoned staging
directories are removed when admission fails. Admission atomically reserves the uncached unique
source bytes against `[scheduler].max_queue_bytes` and the filesystem space remaining after
`working_space_reserve_bytes`. A full queue returns `submissionDeferred` before requesting input
bytes; Momento requeues the existing job after the returned delay without consuming a submission
attempt. A single job whose uncached inputs exceed `max_queue_bytes` is rejected permanently.
Cached content is counted once regardless of how many job directories hard-link it. Active
reservations prevent concurrent submissions of the same missing content from double-uploading it.

Cancellation markers are stored as client-scoped zero-byte files in `cancelled/`. They contain no
media or model result data and remain durable so delayed or retried submissions from that client
cannot recreate cancelled work.

```text
.tmp -> queuing -> processing -> deleted after Momento durably receives the result
                            \-> callback_pending -> deleted after a successful retry
                                                 \-> failed after permanent receipt rejection
                  processing -> failed for terminal local queue/processing failure
```

Each job directory contains `manifest.json` and `input-N` files. Inference atomically publishes
`result-records.bin` and `result-manifest.json` through their synced `.tmp` files; callback retry state
adds `callback.json`; terminal queue failures add `failure.json`. There is
no `completed/` directory. A matching WebSocket durable result-receipt acknowledgement permits
deletion of all llm-service job data and immediately releases any unique source bytes no longer
referenced by another job. It does not report whether Momento's later result persistence and
downstream processing succeeded. Startup rebuilds capacity usage from the durable content store;
temporary admission reservations exist only in memory and incomplete `.tmp` directories are
discarded during recovery.

Startup recovery removes incomplete `.tmp` admissions, promotes interrupted `processing` jobs that
have a complete validated result manifest/record pair to `callback_pending`, moves other interrupted
processing jobs back to `queuing`, keeps callback jobs with a complete pair, and requeues callback jobs
that do not have a durable result. Therefore interrupted inference may run again, while a
completed inference awaiting acknowledgement is delivered again without rerunning the model.

### Multiple-request scheduling

Only one model runtime may be active in llm-service. Every model provider is a managed process in
the llm-service container and listens only on its application-owned loopback address. Packages and
model weights are installed in the image; activation performs no package installation or model
download. llm-service itself may run on a different machine from Momento, but it never delegates
inference to a remote model provider.
For each scheduler cycle:

1. A separate result-delivery loop sends durable `callback_pending` results through a rolling
   `result_delivery_max_concurrent_deliveries` window. Never-attempted results take priority over
   retries, each completed delivery immediately refills its slot, and result delivery never gates
   model inference or reruns completed inference.
2. Read valid queued manifests and sort them by job ID.
3. If the currently active task still has queued work, select that task to keep its runtime warm.
4. Otherwise select the task belonging to the first sorted queued job.
5. Keep at most the scheduler's global `max_in_flight_jobs`, moving only same-task jobs from
   `queuing` to `processing` and replenishing each slot as soon as its job completes.
6. Keep only lightweight disk descriptors in the active window. Providers send validated job/input
   descriptors to the active local runtime. The runtime opens the derived file below
   `queue/processing`; providers never retransmit image payloads to same-container subservices.
7. Activate or reuse the selected local runtime and dispatch the homogeneous rolling window. The scheduler
   and provider do not apply a model concurrency limit.
8. The model subservice alone enforces its configured `max_concurrent_jobs` through each runtime
   inference semaphore. Unlimited-OCR-2 uses Baidu's pinned SGLang build with BF16 weights.
   The default `max_concurrent_jobs = 8` bounds decoded images and engine requests.
   The adapter sends lossless image data to the engine through a loopback interface.
   SGLang continuously batches those requests and owns all model and KV-cache state.
   OCR `max_tokens` controls generated output and defaults to 65,536.
   The engine uses the model's 131,072-token context to leave room for image input.
   An output that reaches its limit remains a failure and preserves existing stored text.
   Python runtimes acquire that semaphore before opening and reading each queued input, bounding
   decoded image memory and applying HTTP backpressure.
9. Finish every claimed job independently and refill its window slot immediately while matching work remains.

This warm-task preference drains one task through a rolling window before switching when that task
continues to have work. Switching task type shuts down the old runtime before starting and
readiness-checking the new one. A runtime is also shut down when no inference job is claimed for
`idle_shutdown_seconds`; result delivery does not require or keep a model runtime active.
Each runtime is started in its own process group. Switching or idle shutdown sends the whole group
`SIGTERM`, waits for the bounded shutdown timeout, and escalates to `SIGKILL` so runtime workers do
not survive their parent process.

One failed job does not prevent other in-flight jobs from finishing. Provider or runtime inference
errors become durable failed result payloads. Loss of the local runtime transport is retried from
the durable queued bytes up to `runtime_max_attempts`; model-result errors are not retried. Result
delivery uses its acknowledgement timeout and fixed retry delay without an attempt limit. A durable
receipt acknowledgement deletes the queue directory. A deferred receipt updates its next delivery time
without consuming an attempt. A permanent receipt rejection writes durable failure evidence and moves
the job to `failed`; timeout or disconnect keeps it in `callback_pending` until receipt or cancellation.
Startup restores complete results parked by older versions solely due to delivery retry exhaustion.
Momento reconciles submitted jobs on connection and every 60 seconds in pages of at most 256 IDs.
Only jobs missing from llm-service and without a usable local result receipt are requeued, preserving
the original wire attempt. Completed and cancelled jobs are never resurrected.
