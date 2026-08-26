# AGENTS.md - Momento Codebase Guide

> Guidelines for AI coding agents working in this repository.

## Project Overview

Momento is a self-hosted photo management application with:
- **Backend**: Axum + SQLite (Rust) in `src/backend/`
- **LLM service**: Separate Axum durable-inference queue in `src/backend_llm/`
- **Common**: Shared Rust infrastructure used by both services in `src/common/`
- **Frontend**: React + TypeScript + Vite + Tailwind in `src/frontend/`

Monorepo managed with pnpm workspaces and Turborepo.

---

## Conventions

This repo follows the shared agent skills. Read them before changing code — they are the
source of truth, and this file only records what is specific to Momento.

| Skill | Governs |
|-------|---------|
| `project-structure` | Where every file goes: `src/`, tests mirroring new source paths, `playground/`, `build/`, `dist/`, `docker/` |
| `add-modify-codebase` | How changes land: breaking changes over shims, refactor over copy-paste, unit tests for touched code |
| `general-coding` | Guard clauses, no broad catch, no default arguments, no backward-compat layers, no environment variables in source |
| `naming-conventions` | Cross-layer name consistency (database → Rust → TypeScript) |
| `axum-server` | Backend module layout and Axum patterns |
| `restful-api-design` | Endpoint paths and HTTP methods |
| `sql-coding` | Centralized schema and query constants |
| `docker-build` | Dockerfile, entrypoint, and `build_docker.sh` contents |

Three rules are worth repeating because they are the ones most often violated:

1. **Break the signature, don't add a default.** New argument means every caller is found and updated to pass it explicitly. No default values, no compatibility shims, no deprecated wrappers.
2. **Extract before you copy.** Implementing something similar to existing code means refactoring the existing code into a shared function first, then calling it from both places.
3. **Tests ship with the change.** Every added or modified path gets a unit test at its mirrored location under `tests/`, in the same change.

---

## LLM Task Scheduling

Every AI inference type follows one staged, durable propagation pattern. Current task identifiers
are `ocr`, `image_tagging`, `image_clustering`, `image_aesthetics`, `face_detection`,
`screenshot_detection`, and `document_detection`; adding another identifier means extending this
same pattern end to end, not creating a direct or type-specific transport path.

### End-to-end ownership

```text
import
  -> metadata job
  -> task-ready inputs in Momento previews + media_ai_inputs descriptors
  -> durable Momento llm_jobs row
  -> authenticated client-aware WebSocket submission
  -> durable llm-service disk queue
  -> one task runtime performs inference
  -> result returned on the originating client's WebSocket
  -> durable Momento result inbox receipt
  -> result receipt acknowledgement + llm-service queue deletion
  -> independent transactional Momento result persistence + terminal job state
  -> optional separately scheduled downstream work
```

No stage runs its downstream stage inline. Import only creates metadata work; metadata prepares
inputs; an AI trigger creates inference jobs; the Momento submission worker sends them;
llm-service performs inference; the WebSocket handler durably receives results; the independent
Momento result worker persists them. Type-specific work after inference,
such as deduplication cluster generation, is another scheduled stage.

Local and WebDAV imports use the same `finalize_staged_original` implementation. A source is
claimed before finalization and hashed before a media ID is allocated. A partial unique index on
the content hash and a matching-hash import lock prevent concurrent imports from creating multiple
media rows. New content is stored as the canonical original, receives media ownership, is marked
imported, records the source modification time in `media.created_at`, and queues exactly one
metadata job. Exact duplicate content reuses the imported media ID, grants or restores access for
the importing user, keeps the canonical original bytes, and moves an incoming supplemental
metadata sidecar beside that original. An older duplicate source modification time lowers
`media.created_at`; a newer time never replaces it. A duplicate sidecar
requests another metadata run, including when one is already processing. Supplemental values are
authoritative for fields they contain, and sidecars remain beside originals so later regeneration
is deterministic. Import does not generate metadata or thumbnails, prepare AI inputs, or create
LLM jobs. Source cleanup failure after a committed import is logged as a warning rather than
changing the durable import result.

WebDAV PUT accepts either a valid declared length or an undeclared chunked body. Undeclared PUT
bodies are bounded incrementally by `webdav.max_upload_bytes`; they are never buffered in full, and
an oversized partial file is removed before returning 413. PATCH still requires a declared chunk
size because partial-update range validation depends on it. Every staging mutation invalidates its
durable `webdav_ready_files` entry before touching bytes. Only a successful complete PUT, a PATCH
whose `Content-Range` reaches the declared total, or a successful MOVE/COPY records the resulting
path as ready. The import worker selects only these durable ready paths, acquires the exclusive
mutation gate, revalidates and claims each file, and calculates its content hash only after the
transfer has completed and the file has been closed. GET, HEAD, OPTIONS, and PROPFIND do not hold
the mutation gate. The modification-age check is a secondary settling delay, not the completion
signal.

Momento owns every AI input reference. Photo tasks reference the immutable canonical original
without resizing or creating a task-specific copy. Video-capable tasks share one lossless,
full-resolution representative PNG frame below Momento previews; screenshot and document detection
remain photo-only. UI thumbnails and place thumbnails are separate presentation assets and are
never AI inputs. The transport supports multiple ordered inputs even though current metadata
generation creates one input per task. llm-service may decode the received bytes, apply orientation,
and perform model-required tensor transforms, but it never reads Momento paths, generates task
inputs, or assumes a shared filesystem.

The primary implementation points are:

- `src/backend/processor/metadata/generation.rs`: prepare task inputs.
- `src/backend/processor/metadata_worker.rs`: verify required inputs before metadata completes.
- `src/backend/processor/ai/mod.rs`: create/claim jobs and verify prepared bytes.
- `src/backend/processor/ai/transport.rs`: connect, stream jobs, cancel, and receive results.
- `src/backend/processor/ai/result.rs`: validate and transactionally persist results.
- `src/backend_llm/routes.rs`: authenticate clients and stream WebSocket admission.
- `src/backend_llm/transport.rs`: active-client registry and durable-receipt acknowledgements.
- `src/backend_llm/scheduler.rs`: durable queue, batching, result retries, and recovery.
- `src/backend_llm/provider.rs`: task registry, local runtime lifecycle, and inference dispatch.
- `src/backend_llm/screenshot_document_common.py`: shared image analysis and HTTP runtime used by
  screenshot and document detection.
- `src/backend_llm/screenshot_detection_server.py`: exact `screenshot_detection` runtime entrypoint.
- `src/backend_llm/document_detection_server.py`: exact `document_detection` runtime entrypoint.

New source and test paths must follow the repository's resource hierarchy and test-mirroring
convention; existing flat route, query, and frontend API modules are layout debt, not templates to
copy. All AI control and status endpoints require an administrator. User-facing duplicate-group
and face-group browsing uses normal authenticated access and filters through `media_access`.
The LLM WebSocket uses the configured client ID and shared API key rather than a user JWT.

Metadata jobs use `queued`, `processing`, `completed`, and `failed` states. Workers atomically
claim rows, reclaim expired processing leases, retry through `available_at`, and verify every
enabled task's prepared inputs before completion. Metadata reset currently deletes matching AI
job rows directly; do not claim that reset uses the cancellation outbox unless that implementation
is changed.

### Prepared input contract

`media_ai_inputs` records a Momento-owned `storage_root` (`originals` or `previews`) plus the task,
sequence, input kind, relative file path, filename, MIME type, byte size, SHA-256 content hash, and
optional frame timestamp. Every photo task points at the same canonical original descriptor.
Every video-capable task points at one file named by the canonical original hash below
`previews/ai/<media-id>/frames/`; metadata reruns reuse it, and no task directory or copy is created. Job eligibility requires
imported media, completed metadata, and at least one
matching descriptor. Screenshot and document detection are photo-only and receive the canonical
original. A missing or non-image original MIME type is an explicit metadata failure; Momento never
falls back to a thumbnail.

`llm_job_inputs` snapshots the storage root and descriptor when the job is queued. Before each
submission, Momento resolves the path only inside the selected Momento storage root, opens it,
streams its exact byte size and SHA-256 hash, rewinds that same open handle, and sends from the
verified handle. Missing or changed bytes fail the Momento job and are never submitted. The wire
manifest contains no storage root or path: llm-service receives only descriptors and raw bytes,
persists those bytes below its own data root, and can run on another server with unrelated storage.
Input and job admission allow up to 32 GiB so large streamed media is not rejected by the previous
50 MiB cap; deployments still must monitor durable queue space.

Multi-input jobs preserve every descriptor's `sequence` and optional `frameTimestampMs` through
the queue, provider response, result message, and input-level persistence. Concurrency is across jobs;
inputs within one job are currently inferred sequentially in descriptor order. A new type must
define explicit aggregation and persistence semantics for all inputs rather than silently using
only the first result.

### Submission wire contract

Momento opens:

```text
GET ws[s]://<llm-service>/api/v1/llm/connect
x-api-key: <configured API key>
x-momento-client-id: <configured client ID>
Sec-WebSocket-Protocol: momento-llm-v1
```

The API key is shared by every allowed Momento client. Client IDs contain only letters, numbers,
hyphens, and underscores. llm-service keeps active client IDs only in memory: different IDs may be
connected concurrently, but a second live connection using the same ID is rejected. The ID is
stored in each durable job manifest so a disconnected client can reconnect and receive its own
pending results.

Submission control messages are camel-case tagged JSON. Momento sends `submissionStart` with:

```text
jobId, mediaId, task, attempt, inputs[]
```

Each input descriptor contains:

```text
sequence, filename, mimeType, byteSize, contentHash, inputKind, frameTimestampMs
```

llm-service returns `submissionReady` before bytes are sent. Momento then streams bounded binary
frames containing the job ID, input sequence, and at most 64 KiB of raw prepared bytes. Each input
ends with `inputFinished`; the job ends with `submissionFinished`. A non-empty hexadecimal `jobId`,
a known task, and at least one input are required. Every declared input must be present, non-empty,
and exactly match its descriptor's byte size and SHA-256 hash. The current admission contract
accepts image MIME types; supporting another payload requires deliberately extending the shared
descriptor and admission abstractions.

`submissionAcknowledged` is sent only after llm-service has synced the staged files and atomically
renamed the directory into `queuing`. Duplicate IDs owned by the same client are acknowledged
idempotently; an ID already owned by another client is rejected. Rejections explicitly state
whether they are retryable. A lost connection before an acknowledgement requeues the Momento job
without changing its correlation attempt, allowing the same durable admission to be replayed.

Momento job states follow:

```text
queued -> submitting -> submitted -> completed | failed
                  \-> queued       transient network/5xx retry
queued | submitting | submitted -> cancelled
```

Momento retries transport and retryable admission errors but treats permanent rejections as
submission failures. An acknowledgement means only that llm-service has durably admitted the job,
not that inference has completed. The result must return the same `jobId`, `mediaId`, `task`, and
exact submitted `attempt`.

The Momento submission worker keeps a global `max_in_flight` rolling window. It claims and streams
a replacement job as soon as any submission completes, and sleeps only when no eligible queued job
remains. This transport window is independent from every model runtime's inference concurrency.

OCR eligibility is independent of image-tagging configuration. Tagging, clustering, aesthetics, and face
detection are queued only through their enabled trigger or run paths. The submission worker sends
existing queued jobs; it does not create task jobs. Submission reads the immutable
`llm_job_inputs` snapshot created with each job rather than re-querying live `media_ai_inputs`.

Cancellation commits the Momento terminal state, an all-task or task-specific
`llm_cancellation_scopes` outbox row, and matching exact `llm_job_cancellations` rows in one
transaction. Momento immediately attempts and durably retries authenticated
`cancelJobs` WebSocket messages containing that scope and the exact job IDs. llm-service scopes
every cancellation to the authenticated client, scans
`.tmp`, `queuing`, `processing`, `callback_pending`, and `failed` for every matching task and writes
job markers before deleting non-running data. A matching job already in `processing` finishes its
local inference, then llm-service discards its result and queue directory without delivering a
result. Exact-ID markers prevent an admission that was already in flight from recreating
cancelled work. Cancellation acknowledgements remove the matching Momento outbox rows; a
disconnect or rejection leaves them for retry.
Queued jobs matching a pending task or all-task cancellation scope are not submitted until that
scope is acknowledged. This keeps reset replacements from racing ahead of their cancellation.

### Durable llm-service queue

Admission streams files into `.tmp/<job-id>/`, validates all descriptors and bytes, syncs the
staged data, and atomically renames the directory into `queuing/<job-id>/`. llm-service stores
the submitted bytes unchanged. Each manifest records the authenticated client ID. Duplicate job
IDs owned by that client are acknowledged idempotently and are not enqueued twice; cross-client
collisions are rejected.

Admission computes byte counts and SHA-256 incrementally while writing and rejects a field before
writing beyond its declared size. It never rereads a complete input into memory. Abandoned staging
directories are removed when admission fails. Queue job count has no configured limit, but each
manifest and input is bounded; deployments must monitor free space because an unbounded disk queue
is not protection against an exhausted filesystem.

Cancellation markers are stored as client-scoped zero-byte files in `cancelled/`. They contain no
media or model result data and remain durable so delayed or retried submissions from that client
cannot recreate cancelled work.

```text
.tmp -> queuing -> processing -> deleted after Momento durably receives the result
                            \-> callback_pending -> deleted after a successful retry
                                                 \-> failed after retry exhaustion
                  processing -> failed for terminal local queue/processing failure
```

Each job directory contains `manifest.json` and `input-N` files. Inference adds `result.json`;
callback retry state adds `callback.json`; terminal queue failures add `failure.json`. There is
no `completed/` directory and no configurable queue-size limit. A matching WebSocket durable
result-receipt acknowledgement permits deletion of all llm-service job data. It does not report
whether Momento's later result persistence and downstream processing succeeded.

Startup recovery removes incomplete `.tmp` admissions, moves interrupted `processing` jobs back
to `queuing`, keeps `callback_pending` jobs that already have `result.json`, and requeues callback
jobs that do not have a durable result. Therefore interrupted inference may run again, while a
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
8. The model subservice alone enforces its configured `max_concurrent_jobs`; vLLM uses
   `--max-num-seqs`, and Python runtimes use their inference semaphore.
   Python runtimes acquire that semaphore before opening and reading each queued input, bounding
   decoded image memory and applying HTTP backpressure.
9. Finish every claimed job independently and refill its window slot immediately while matching work remains.

This warm-task preference drains one task through a rolling window before switching when that task
continues to have work. Switching task type shuts down the old runtime before starting and
readiness-checking the new one. A runtime is also shut down when no inference job is claimed for
`idle_shutdown_seconds`; result delivery does not require or keep a model runtime active.
Each runtime is started in its own process group. Switching or idle shutdown sends the whole group
`SIGTERM`, waits for the bounded shutdown timeout, and escalates to `SIGKILL` so vLLM workers do
not survive their parent process.

One failed job does not prevent other in-flight jobs from finishing. Provider or runtime inference
errors become durable failed result payloads. Loss of the local runtime transport is retried from
the durable queued bytes up to `runtime_max_attempts`; model-result errors are not retried. Result
delivery uses its acknowledgement timeout, fixed retry delay, and maximum-attempt policy. A durable
receipt acknowledgement deletes the queue directory; receipt rejection, timeout, or disconnect keeps it in
`callback_pending`, and retry exhaustion moves it to `failed`.

### Result contract

llm-service sends `result.json` on the active WebSocket matching the manifest's client ID. A
completed payload contains matching correlation fields,
`status = completed`, model type/version, a top-level result derived from the first input, and
ordered `inputResults` carrying each original sequence and frame timestamp. A failed payload
contains `status = failed` and an error.

Momento first stores an incoming result in its durable `llm_job_results` inbox and then sends
`resultReceived`. That receipt is the llm-service success boundary and permits llm-service to delete
all local task data. `resultReceiptRejected` is reserved for failure to durably receive the payload;
it is never used for later validation, crop generation, database persistence, or downstream work.
Momento's independent result worker validates correlation and task-specific payload fields, persists
the result, performs required local post-processing, and atomically transitions its job to
`completed`. Any permanent validation or post-processing failure transitions only the Momento job
to `failed`; transient local failures retry from the durable inbox. Matching duplicate deliveries
are received idempotently, and late results for terminal or removed jobs are acknowledged without
recreating work.

Persistence is deliberately type-specific. OCR and tagging store present text results in
input-level `media_text_inputs` rows and derive ordered media-level text in `media_text`; missing
text currently becomes an empty string. Aesthetics validates five finite scores in `[0, 1]`, stores
ordered input-level scores in `media_aesthetic_inputs`, and stores the first-input aggregate in
`media_aesthetics`. Clustering validates its embedding/hash result and updates
similarity tables. Face detection
validates bounding boxes, eye centers, confidence, quality, frontality, and 512-dimensional
embeddings before writing crops and face rows. A generic transport response does not remove the
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
aesthetics, face detection, screenshot detection, and document detection. They are evaluated in
the configured IANA timezone and create work through the same durable processor operations as
manual triggers. The field remains `deduplicate_cron`, not
`image_clustering_cron`, because it starts the complete deduplication pipeline; image clustering is
only its inference stage. Global LLM enablement and each task's feature flag gate scheduled work.

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

Metadata generation lives in `processor/metadata/`; `processor/regenerator.rs` does not exist.

---

## Configuration

Both binaries (`momento-api`, `llm-service`) require `-c|--config PATH`. A missing or malformed
config is a hard startup failure — it never falls back to defaults, because that would silently
start the server against the wrong data directory. Momento resolves the exact
`${LLM_SERVICE_ADDRESS}` placeholder when it appears in `llm.server_address`; the environment value is
required and must be non-empty in that case. Momento also accepts exact runtime overrides from
`RESET_ADMIN_PASSWORD`, `SECRET_KEY`, and `LLM_SERVICE_API_KEY`; llm-service accepts
`LLM_SERVICE_API_KEY`. No other config
interpolation or application environment variable is supported.

Both binaries also support `--init-config`, which writes their source-owned commented operational
template and exits. The Docker entrypoints invoke it only when the expected config file is absent:
`/data/config.toml` for Momento and `/data/config_llm.toml` for llm-service. Each service owns all
of its fallback and operational-template values in its `config/defaults.rs`; its commented TOML
source contains named placeholders rather than duplicated values. The rendered templates exactly
match `playground/config.toml` and `playground/config_llm.toml`, including their environment
placeholders. Normal startup never generates or replaces a configuration file except when atomically
consuming the one-shot administrator password-reset request described below.

Momento has no `[admin]` configuration section. An empty database creates `admin` / `admin` with a
required password change. Setting `[server].reset_admin_password = true` is a one-start recovery
request: startup atomically writes it back to `false`, keeps the stored password unchanged, and
accepts temporary `admin` / `admin` API authentication only in that process for the existing
administrator. A successful forced password change replaces the stored hash; a restart before the
change removes the temporary override and restores normal authentication with the unchanged stored
username and password. `RESET_ADMIN_PASSWORD=true` applies the same request from the environment;
operators must remove that override after startup so it is not requested again on every restart.

Momento filesystem locations derive from `server.data_dir`:

```toml
[server]
data_dir = "/data"          # database.sqlite, originals/, thumbnails/, previews/, imports/, webdav/, ...
static_dir = "/app/static"  # built frontend served as a fallback
```

`main` calls `constants::init_paths(&config.server.data_dir)` once after parsing the
config; everything else reads `constants::paths()`. Never hardcode a path under the data
directory and never add a new `std::env::var` call. The config loaders own all supported environment
access: `LLM_SERVICE_ADDRESS`, `RESET_ADMIN_PASSWORD`, `SECRET_KEY`, and `LLM_SERVICE_API_KEY` for
Momento, and `LLM_SERVICE_API_KEY` for llm-service. Add a config field rather than another
environment setting.

Log paths are not configurable. Each service writes plain daily rotated files below
`server.data_dir/logs/` (`momento-api.YYYY-MM-DD.log` or `llm-service.YYYY-MM-DD.log`). Log events
contain the timestamp, level, message, and structured fields; they do not repeat the service name
or process ID. File logs never contain ANSI escapes. Console logs keep the timestamp dim regardless
of severity, then color the level, message, and structured fields as one span: DEBUG/INFO white,
WARN yellow, and ERROR/fatal paths red.

llm-service configures only `server.data_dir`; its durable queue and runtime cache are fixed at
`server.data_dir/llm/queue/` and `server.data_dir/llm/cache/`, while its logs remain in
`server.data_dir/logs/`. The `llm/` subtree must be durable. Queue jobs are accepted only with a
non-empty hexadecimal Momento job ID and a manifest sent before its binary input frames.

Runtime executables, scripts, model paths, model versions, loopback URLs, CUDA device selection,
and embedding dimensions are owned by `RuntimeCatalog` and the llm-service image, not TOML.
Service configuration retains only enablement, startup/request timeouts, model concurrency, OCR
token limits, and task-specific thresholds. Local runtime requests contain job/input descriptors,
never media bytes or caller-supplied paths.

`llm.server_address` contains only the llm-service host and port. Momento owns the WebSocket scheme
and `/api/v1/llm/connect` path as implementation details; `0.0.0.0` is only a server bind address
and is never a client destination. llm-service has no Momento address. Its
top-level `[scheduler]` section owns inference settings and every result-delivery setting, with
result-delivery fields prefixed by `result_delivery_`. The Compose deployment mounts one shared
data root into both containers; llm-service only uses its config, `logs/`, and `llm/` subtree and
does not read Momento originals, previews, or thumbnails.

### Face detection and grouping

The `face_detection` service requires `minimum_face_likelihood` in `(0, 1]` and a positive
`minimum_face_resolution_pixels`. These values belong to the llm-service face service entry and
are passed explicitly to the local runtime. A detection is returned only when its confidence
reaches the likelihood threshold and both detected face-box dimensions reach the configured
source-pixel resolution. Tests that load playground configuration validate the values and
required ranges rather than pinning locally tuned thresholds.

Face results include a normalized `eyeCenter` derived from InsightFace's first two landmarks and a
normalized `frontalityScore` derived from all five landmarks. Frontality accounts for eye-line
roll plus nose and mouth-center horizontal offsets, is constrained to `[0, 1]`, and is persisted
with the face row. Momento keeps the 256x256 portrait output size and the existing crop dimensions;
only the crop origin changes so the portrait is centered on `eyeCenter`, subject to image-edge
clamping. Face crops reference the immutable submitted original snapshot. Momento uses ImageMagick
to select its first frame, apply stored orientation, and normalize it to PNG in memory before Rust
decodes and crops it; conversion failures are logged and fail only the corresponding Momento job.

Automatic grouping processes faces in face-ID order and compares each embedding against the fixed
seed embedding that first created each automatic group. A face joins the first automatic seed whose
cosine similarity reaches `llm.face_group_similarity_threshold`; the default is `0.41`, lower values
are more tolerant, and higher values are stricter. Grouping is deliberately greedy: it does not use
the thumbnail representative, compare every automatic member pair, apply transitive closure, or run
a second group-to-group merge pass. A manual merge makes every face selected by that merge a fixed
manual anchor. Later detections compare against every anchor and join the best matching manual group
before automatic grouping. Automatically attached members are reevaluated on each run and never
become anchors, preventing transitive similarity drift. Manual groups and their anchors are never
deleted by automatic regrouping. Changing these semantics requires explicit false-merge analysis
and grouping tests, not just changing the thumbnail representative.

`face_groups.representative_face_id` is a thumbnail choice, not the grouping seed. Select it only
after automatic membership is complete and select it again after a manual merge. Rank each face by
`0.2 * center_proximity + 0.8 * frontality`, where center proximity normalizes squared
face-box-center distance from the media center to `[0, 1]`. Higher scores win, followed by quality
descending, confidence descending, and face ID ascending as deterministic tie-breakers. If the
global representative is not visible to a requesting user, thumbnail lookup applies the same score
to that user's accessible members.

The face-group list is sorted in the backend before `LIMIT`/`OFFSET`: distinct visible media count
descending, then face-group ID ascending as the stable tie-breaker. Do not sort paginated face
groups in the frontend, and do not use raw face count in place of distinct accessible media count.

### Places and aesthetic covers

Places are identified by the exact `location_city`, nullable `location_state`, and
`location_country` tuple. Lists and galleries filter through active `media_access`; list ordering is
visible media count descending, then city, state, and country ascending. Place identifiers are
opaque encodings of the complete tuple. Manual GPS changes immediately recompute or clear the local
reverse-geocoded fields so grouping cannot retain stale location names.

Metadata generates a separate aspect-preserving place thumbnail for UI cover rendering only.
`image_aesthetics` uses the canonical photo original or the shared full-resolution video frame.
Cover ranking prefers completed
aesthetic inference and combines aesthetic 40%, scenic 25%, simplicity 20%, landscape 10%, and
technical quality 5%, then applies OCR-clutter and dominant-face penalties. Media without an
aesthetic result use a deterministic landscape, capture-date, and media-ID fallback. A user's place
cover is always selected only from media visible to that user. Place cover selection is never
stored or cached as a representative media ID. Every place-thumbnail request reruns the ranking
against current metadata, aesthetic results, and active `media_access`, so changed membership is
visible on the next request.

`schema.sql` and the Android Room database define only their current schemas. Keep only the current
Room schema export. Do not add schema migration or compatibility code; breaking schema changes
require a fresh database and data directory.

Reverse geocoding is always local and uses the pinned GeoNames `cities500` asset embedded in the
Momento API binary. It has no enablement, URL, user-agent, timeout, or rate-limit configuration.
Normal metadata generation fills missing location fields immediately; each metadata-worker cycle
does not backfill existing metadata rows. Existing non-empty location fields are preserved.
Updating the dataset requires recording its snapshot date, source checksums, output checksum, and
CC BY 4.0 attribution in the source manifest.

The Dockerfiles and entrypoints use `PUID`/`PGID`/`UMASK`/`TZ`; Compose additionally supplies
`LLM_SERVICE_ADDRESS`, `RESET_ADMIN_PASSWORD`, `SECRET_KEY`, and the shared
`LLM_SERVICE_API_KEY`. They prepare
filesystem ownership and invoke each binary with its generated config path. `RUST_LOG` and `RUST_BACKTRACE` are ecosystem-standard runtime knobs read
by `tracing-subscriber`, not application config.

---

## Build, Lint & Test Commands

### Root
```bash
pnpm install              # Install all dependencies
pnpm build                # Build all packages
pnpm dev                  # Dev servers (backend + frontend)
pnpm lint                 # Lint all packages
pnpm test                 # Run all tests

./run_playground.sh /path/to/keystore                         # Build and run the playground stack
./build_docker.sh /path/to/keystore                           # Build both images locally
./build_docker.sh publish github yzard /path/to/keystore      # Build and publish both images to GHCR
./build_android_client.sh verify                              # Android compile, JVM tests, and lint in Docker
./build_android_client.sh assemble-debug                      # Android debug APK in Docker
./build_android_client.sh instrumented-test                   # Containerized emulator tests; requires /dev/kvm
./build_android_client.sh shell                               # Containerized Java/Gradle/SDK/ADB shell
./build_android_client.sh release --keystore-dir /path/to/keystore
```

`run_playground.sh`, `build_docker.sh`, and the explicitly separate Android entrypoint
`build_android_client.sh` are the supported scripts at the git root. They resolve paths from their
own location, so they work from any working directory. Android compilation, debugging, lint, and
tests must use `build_android_client.sh`; direct host Gradle, Java, Android SDK, emulator, or ADB
commands are unsupported.

### Playground containers

`run_playground.sh` uses `build_docker.sh` and the root `docker-compose.yaml` to build and start exactly two containers:
`momento-api` and `momento-llm-service`. It does not compile or run host binaries. The Momento image embeds
the built frontend, while the llm-service image embeds four isolated Python environments and all
model weights. Docker layer caching avoids repeating package/model downloads when their inputs do
not change.

The script passes the invoking UID/GID and mounts `playground/` as `/data` in both containers. Each
service therefore has one volume mount. It tears down both containers on exit and never
mounts the Docker socket. Runtime model processes are spawned inside the llm-service container, so
inference never creates additional containers.

`playground/logs/` holds both services' logs. llm-service queue and runtime cache remain below
`playground/llm/`, while its config remains at `playground/config_llm.toml`. Build artifacts do not
belong in either directory.

### Android client

`build_android_client.sh` is the only Android build, debug, and test entrypoint. Its `verify`,
`assemble-debug`, `instrumented-test`, `shell`, and `release` commands all run in Docker. Only
`release` accepts `--keystore-dir`; only `instrumented-test` uses the emulator image and `/dev/kvm`.
Intermediate state belongs below `build/android/`, debug APKs below
`dist/android/debug/`, and the single release APK plus AAB directly below
`dist/android/`. Run `./build_android_client.sh --help` for the complete option contract.

`build_docker.sh` calls only `build_android_client.sh release`, then verifies and embeds the one
release APK into the momento-api image as `/app/static/momento-android.apk`. It separately builds
llm-service and never routes Android development or test commands.

### Backend (src/backend)
```bash
cd src/backend

# Build (release/no-debug is the default project workflow)
CARGO_TARGET_DIR=../../build/backend/target cargo build --release

# Run development server
CARGO_TARGET_DIR=../../build/backend/target cargo run --release -- -c ../../playground/config.toml

# Linting & formatting
cargo fmt                  # Format code
CARGO_TARGET_DIR=../../build/backend/target cargo clippy --release --all-targets

# Testing (integration tests live in tests/backend/, not #[cfg(test)] modules)
CARGO_TARGET_DIR=../../build/backend/target cargo test --release --all-targets
CARGO_TARGET_DIR=../../build/backend/target cargo test --release auth

# Troubleshooting only: explicitly enable debug symbols in an isolated directory.
CARGO_PROFILE_DEV_DEBUG=2 CARGO_TARGET_DIR=../../build/backend/debug-target cargo build
CARGO_PROFILE_TEST_DEBUG=2 CARGO_TARGET_DIR=../../build/backend/debug-target cargo test auth
```

### Frontend (src/frontend)
```bash
cd src/frontend

pnpm dev                  # Dev server (Vite)
pnpm build                # Production build (tsc + vite)
pnpm lint                 # ESLint
pnpm preview              # Preview production build
```

### Docker

Docker build files live in `docker/`; the canonical `docker-compose.yaml` and
`build_docker.sh` live at the git root. Builds use the git root as their context.

```bash
./build_docker.sh /path/to/keystore                                      # Build both images locally
./build_docker.sh publish github yzard /path/to/keystore                 # Build and publish to GHCR
./build_docker.sh publish docker zhuoyin /path/to/keystore               # Build and publish to Docker Hub
docker compose up                                      # Full stack
```

The ignore files are `docker/Dockerfile.dockerignore` and
`docker/Dockerfile.llm.dockerignore`, not `.dockerignore`. Docker resolves ignore files against the
build context, so each file is named for the Dockerfile that uses it.

---

## Code Style Guidelines

### Rust (Backend)

**Formatting**:
- Use `cargo fmt` for formatting
- Use `cargo clippy` for linting

**Imports** (order):
```rust
// 1. Standard library
use std::sync::Arc;
use std::path::PathBuf;

// 2. External crates
use axum::{extract::State, routing::post, Json, Router};
use serde::{Deserialize, Serialize};

// 3. Local crate
use crate::auth::{AppState, CurrentUser};
use crate::error::AppError;
```

**Naming Conventions**:
- Files: `snake_case.rs`
- Structs/Enums: `PascalCase` (e.g., `MediaResponse`, `UserCreateRequest`)
- Functions/variables: `snake_case`
- Constants: `UPPER_SNAKE_CASE`
- Modules: `snake_case`

**Error Handling**:
```rust
// Use AppError for API errors
return Err(AppError::NotFound("Media not found".to_string()));
return Err(AppError::BadRequest("Invalid input".to_string()));
return Err(AppError::Authentication("Invalid token".to_string()));
```

**Route Patterns**:
- Routers use `Router::new()` with `.route()` methods
- POST for all mutations and queries (RPC-style API)
- Request bodies use `Json<T>` extractor
- Response types implement `IntoResponse`

### TypeScript (Frontend)

**Formatting**:
- ESLint + typescript-eslint for linting
- Strict TypeScript (`strict: true`, `noUncheckedIndexedAccess: true`)

**Imports**:
```typescript
// React/external first, then local
import { useState, useEffect } from 'react'
import { useQuery } from '@tanstack/react-query'

import { apiClient } from './client'
import type { Media } from './types'
```

**Naming Conventions**:
- Files: `PascalCase.tsx` for components, `camelCase.ts` for utilities
- Components: `PascalCase`
- Hooks: `useCamelCase`
- Types/Interfaces: `PascalCase`
- Variables/functions: `camelCase`
- API clients: `<resource>Api` (e.g., `mediaApi`, `albumsApi`)

**Component Structure**:
```typescript
// Functional components with explicit return types optional
function MediaCard({ media }: { media: Media }) {
  return <div>...</div>
}

// Or with React.FC (less common in codebase)
const MediaCard: React.FC<{ media: Media }> = ({ media }) => { ... }
```

**API Calls**:
- Use `apiClient` from `src/frontend/api/client.ts`
- API methods return typed responses
- URLs relative to baseURL (`/api`)

---

## Project Structure

```
src/
├── backend/
│   ├── auth/               # JWT, password, extractors
│   ├── config/             # YAML config loading
│   ├── database/           # SQLite pool, schema, queries
│   ├── models/             # Request/response DTOs (serde)
│   ├── processor/          # Import, metadata, AI, deduplication, thumbnails
│   ├── routes/             # Public and internal Axum route handlers
│   │   ├── ai/             # AI control/status endpoints
│   │   ├── import/         # Local/WebDAV import endpoints
│   ├── utils/              # Helpers (datetime, geocoding)
│   ├── webdav/             # WebDAV server and upload processing
│   ├── app.rs              # App factory
│   ├── constants.rs        # Paths, defaults
│   ├── error.rs            # AppError type
│   ├── logging.rs          # Request logging
│   ├── lib.rs              # Library root
│   ├── main.rs             # Entry point
│   └── Cargo.toml          # Rust dependencies
│
├── backend_llm/            # llm-service binary: providers, manifest queue, scheduler
├── common/                 # shared Rust infrastructure for both service binaries
└── frontend/
    ├── api/                # API client modules
    ├── components/         # React components
    ├── context/            # React context providers
    ├── hooks/              # Custom hooks
    ├── lib/                # Shared frontend helpers
    ├── pages/              # Route pages
    ├── styles/             # Tailwind and global CSS
    ├── utils/              # Frontend utilities
    └── package.json

tests/                      # Mirrors src/ 1:1 — see below
├── backend/
│   ├── processor/
│   ├── routes/
│   └── test_utils/
├── backend_llm/
├── common/
└── frontend/

docker/
├── Dockerfile
├── Dockerfile.dockerignore # NOT .dockerignore — context is the git root
├── Dockerfile.android
├── Dockerfile.android.dockerignore
├── Dockerfile.llm
├── Dockerfile.llm.dockerignore
├── entrypoint_android.sh
├── entrypoint.sh
└── entrypoint_llm.sh

playground/                 # End-to-end config and data
├── config.toml             # The one config run_playground.sh starts the stack with
├── config_llm.toml         # Checked-in llm-service config
├── llm/
│   ├── logs/               # llm-service daily logs
│   ├── cache/              # Local model runtime cache
│   └── queue/              # Durable inference queue
├── database.sqlite         # SQLite database
├── originals/              # Original media files
├── previews/               # Generated previews, including one shared full-resolution AI frame per video
├── thumbnails/             # Generated thumbnails
├── imports/                # Import staging area
├── webdav/                 # WebDAV processing area
└── logs/                   # Momento API daily logs

build/                      # Optional local intermediate build artifacts
dist/                       # Optional local final build artifacts

build_android_client.sh     # All Android build, debug, lint, and test operations in Docker
build_docker.sh             # Builds locally or explicitly publishes both images
run_playground.sh           # Builds and starts the two-container playground stack
docker-compose.yaml         # Canonical two-container deployment
```

### The `tests/` mirror

New tests must structurally mirror `src/`. A new source file's test location is derived
mechanically by swapping the leading `src/` for `tests/` and keeping every intermediate
directory. Existing flat tests are layout debt and should not be copied:

```
src/backend/routes/map.rs              →  tests/backend/routes/map.rs
src/backend/processor/media.rs         →  tests/backend/processor/media.rs
src/frontend/components/MediaCard.tsx  →  tests/frontend/components/MediaCard.test.tsx
```

Adding a source file means adding its test file in the same change. Moving or deleting
one means moving or deleting the other. A directory present in `src/` but missing from
`tests/` is a coverage gap, not a convention.

---

## API Conventions

**Endpoint Pattern**: `/api/v1/<resource>/<operation>`
- Resources: `user`, `media`, `album`, `tag`, `share`, `import`, `metadata`, `ai`, `map`, `timeline`, `trash`
- Operations: `list`, `get`, `create`, `update`, `delete`

**Authentication**:
- Bearer token in `Authorization` header
- Token refresh via `/api/v1/user/refresh`
- Basic auth only for initial login (`/api/v1/user/authenticate`)
- The outbound llm-service WebSocket uses `x-momento-client-id` and `x-api-key`, never a user JWT.

**Request/Response**:
- All bodies are JSON
- Serde handles serialization (camelCase for responses)
- Consistent error format: `{"detail": "Error message"}`

---

## Database

- SQLite with r2d2 connection pooling
- Schema in `src/backend/database/schema.sql`
- Current-schema initialization only; no migration framework or compatibility DDL.
- Helper functions in `src/backend/database/mod.rs`:
  - `fetch_one()`, `fetch_all()` for queries
  - `execute_query()` for mutations
  - `insert_returning_id()` for inserts

---

## Key Dependencies

**Backend (Rust)**:
- axum (web framework)
- tokio (async runtime)
- rusqlite + r2d2 (SQLite)
- serde + serde_json (serialization)
- jsonwebtoken (JWT)
- argon2 + bcrypt (password hashing)
- image + kamadak-exif (image processing)
- reqwest (HTTP client)

**Frontend**:
- React 18
- React Router 7
- TanStack Query
- Axios
- Tailwind CSS
- react-leaflet
- react-virtuoso
