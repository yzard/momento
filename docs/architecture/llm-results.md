# LLM results and adding inference types

Read when changing result receipt/persistence or adding an inference type. For a new type, also read the linked pipeline contracts in AGENTS.md.

### Result contract

The SQLite writer coalesces consecutive ready AI staging/persistence commands into one
bounded transaction (at most 16 commands and 16 MiB of declared input bounds). It never
waits to fill a batch, skips a FIFO barrier, or combines two operations for the same result.
Every command owns a savepoint; success is returned only after the outer commit. A failed
commit leaves durable work retryable. Other writes retain their independent transaction
semantics. Small non-face results (at most 8 KiB encoded and 32 records) release application
admission before waiting on the bounded SQLite writer while retaining their durable claim;
the runtime accounts 64 KiB per detached lane. Larger results and prepared face artifacts keep
their admission so detached payloads cannot exceed the memory budget. Receipt publication, result processing,
and result cleanup have separate source counters, not separate OS thread pools.

Result processing claims only enough candidates to fill vacant bounded lanes. Each lane
processes one result; completion or a result-work event refills vacancies without waiting
for other lanes to drain. Start lanes only for actual candidates; do not start one empty
writer transaction per global worker on every wake. After a claim or lane failure, finish
already claimed lanes before returning to the retry loop. Each persisted result wakes
downstream finalization independently of whether other result lanes remain busy. Cleanup selection
starts from active SQLite reservations, not historical terminal receipts. Journal completion
wakes only its owning metadata/result pipeline. Staging cleanup drains independently of
result preparation, using the same event signal; it never waits for every result lane to finish.
Result start, chunk writes, expiry rollback, and final publication run as bounded asynchronous
operations outside WebSocket framing. Ready controls and submission frames alternate, and
inbound frames have no strict priority over outbound work. Completed incoming result publication waits
asynchronously for scheduler admission independently of WebSocket framing and heartbeats;
disconnect must not cancel an in-flight publication/commit. Bound receiving plus publishing
sessions, and defer new receipts before accepting bytes when that bound is reached.

llm-service sends a validated manifest first on the active WebSocket matching the manifest's client ID.
After `resultReady`, it reads its durable record file in at most 64 KiB frames and waits for the exact
cumulative-offset `resultChunkReady` credit after every frame before sending `resultFinished`.
Momento incrementally verifies size, SHA-256, CRC32C, task-specific record order, and input correlation.
Receipt preparation snapshots the job's monotonic `state_version`; every later permanent disposition
must compare that version, so cancellation or another terminal transition always wins a result race.
The current Momento implementation then converts that validated typed stream into its bounded `JobResult`
DTO for the existing SQLite JSON inbox; the approved `plan.md` still requires replacing that final
conversion with the Journal-owned record inbox and direct typed persistence.

Momento first stores an incoming result in its durable `llm_job_results` inbox and then sends
`resultReceived`. That receipt is the llm-service success boundary and permits llm-service to delete
all local task data. `resultReceiptRejected` is reserved for permanent manifest, framing, hash, or
record-contract failures and is sent only after Momento atomically marks the matching active job
failed. A stale or terminal delivery is acknowledged; failure to commit a terminal rejection returns
`resultReceiptDeferred`. A temporary scheduler, SQLite, I/O, or reservation failure returns
`resultReceiptDeferred` so llm-service retains the durable result without consuming an attempt.
Momento uses available workers from its global application pool to prepare durable inbox results,
including expensive face-image normalization and cropping. Completed preparations enter the
shared SQLite writer immediately and may share a bounded transaction using isolated savepoints;
there is no dedicated
AI-result OS thread or separate CPU pool. A slow image never forms a batch barrier. A permanently
invalid payload fails only its Momento job. A transient database, pool, I/O, or internal failure
leaves that inbox row available for unbounded retry; transient failures never exhaust an attempt
limit, delete the inbox payload, or mark the Momento job failed. Matching duplicate deliveries are
received idempotently, and late results for terminal or removed jobs are acknowledged without
recreating work.

Persistence is deliberately type-specific. OCR and tagging store present text results in
input-level `media_text_inputs` rows and derive ordered media-level text in `media_text`; missing
text currently becomes an empty string. Aesthetics validates five finite scores in `[0, 1]`, stores
ordered input-level scores in `media_aesthetic_inputs`, and stores the first-input aggregate in
`media_aesthetics`. Clustering validates its embedding/hash result and updates
similarity tables. Face detection
validates bounding boxes, eye centers, confidence, face size, frontality, BiSeNet visibility,
facial-feature clarity, and 512-dimensional embeddings before writing crops and face rows. A generic transport response does not remove the
requirement for explicit validation, storage, clean/reset behavior, and optional downstream
scheduling for each inference type.
Screenshot and document results require a boolean `detected` and finite `confidence` in `[0, 1]`.
Momento stores ordered input-level results plus a first-input aggregate in separate screenshot and
document tables. The categories are independent, so a photo may be positive for both.

Deduplication start and scheduled execution create a durable run plus image-clustering jobs and
return without running inference inline. Clustering results only persist similarity data;
`finalize_ready_runs` separately creates duplicate groups after the run's jobs are terminal.
Cancellation prevents further creation and finalization. Startup marks interrupted runs failed and
schedules replacement work rather than resuming the same run record.

Momento has independent five-field cron schedules for OCR, image tagging, deduplication, image
aesthetics, face detection, screenshot detection, and document detection. Each schedule lives in
`[llm]`, uses the system timezone, and creates work through the same durable processor operations as
manual triggers. The field remains `deduplicate_cron`, not
`image_clustering_cron`, because it starts the complete deduplication pipeline; image clustering is
only its inference stage. `[llm].enabled` is the only Momento AI enablement switch: when it is true,
all seven features and their schedules are active; when it is false, all are disabled. Per-feature
`*_enabled` configuration fields do not exist.

### Adding an inference type

Every new inference type must use one exact snake-case task identifier and propagate through all
layers below in the same change. Do not add a second submission endpoint, bypass prepared inputs,
call llm-service inline from metadata/import, let llm-service access Momento storage, or add a
type-specific queue/scheduler.

1. Add metadata reference/preparation and completion verification for task-ready inputs, including
   an explicit Momento storage root, stable sequence/timestamp rules, and durable
   `media_ai_inputs` descriptors. Reuse the canonical original or shared full-resolution video
   frame; do not create a task-specific media copy.
2. Extend schema constraints and centralized queries for eligibility, idempotent job creation,
   active-job uniqueness, status, cancellation, reset, clean, retry, and any run relationship.
3. Add administrator trigger/status API behavior and matching frontend API/UI behavior where the
   task is user-controllable.
4. Reuse the shared Momento `llm_jobs` submission worker and manifest-first WebSocket protocol.
5. Register the task in llm-service `ServiceType`, configuration validation, `ActiveService`,
   provider dispatch, and local runtime activation/readiness/liveness/shutdown.
6. Keep scheduler dispatch batches homogeneous, enforce `max_concurrent_jobs` only inside the
   local model subservice, and preserve ordered per-input correlation in provider responses.
7. Extend the result DTO only for required fields, then add strict type-specific result
   validation and transactional persistence in Momento.
8. Define failure, cancellation, restart recovery, duplicate result, multi-input aggregation,
   clean/reset, and optional downstream-stage semantics explicitly.
9. Add mirrored tests for preparation and eligibility, WebSocket admission and raw-byte
   preservation, runtime reuse/switching, configured concurrency, result validation, result
   retries/idempotency, persistence, cancellation, and recovery.
10. Extend the shared failed-job lifecycle matrix in
   `tests/backend/routes/ai.rs::failed_jobs_can_be_cleaned_and_restarted_for_every_ai_feature`.
   The new case must create a terminal `failed` job together with representative persisted result
   data, call the feature's `clean` endpoint, prove that every task-owned job and result row was
   removed, call `start`, and prove that a fresh eligible job was queued. A feature that owns runs,
   generations, finalization state, groups, members, or generated files must include those artifacts
   in the fixture and prove they are removed in foreign-key-safe order. A new AI feature is not
   complete if a historical failed job can block clean or prevent the clean-to-start sequence.
11. Extend both failed-job retry matrices:
   `tests/backend/routes/ai.rs::manual_start_queues_a_new_attempt_after_failure_for_every_ai_feature`
   and
   `tests/backend/cronjob.rs::scheduled_occurrences_queue_a_new_attempt_after_failure_for_every_ai_feature`.
   With prepared inputs still present and no successfully persisted result, both the administrator
   `start` endpoint and the scheduled cron entry must preserve the terminal failed job as history
   and queue a new job without requiring clean first. Run-based features must create a new run and
   associate the replacement job with it. These matrices must remain exhaustive with `AiFeature::ALL`;
   adding an AI feature without adding both cases is a test failure and an incomplete implementation.

Metadata generation lives in `processor/metadata/`; `processor/regenerator.rs` does not exist.

### OCR model upgrades

OCR eligibility compares stored `media_text.model_version` with the shared current OCR model version.
Manual starts and scheduled starts queue missing or older OCR results through the existing durable job pipeline.
An active job prevents duplicate admission. Existing OCR text remains until successful transactional persistence replaces it.
A failed or truncated inference preserves the previous text and records a failed job.
Do not clean OCR results before an upgrade rescan.
