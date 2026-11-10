# Import, metadata, and prepared AI inputs

Read when changing local/WebDAV import, metadata lifecycle, or AI input preparation.

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

Duplicate sidecar publication carries the active content-hash claim token. Runtime journal
recovery excludes publishing and files-committed operations while that import claim exists;
cleanup remains eligible. Startup recovery runs before import claims are cleared and deliberately
ignores this exclusion, so interrupted sidecars can finish using their recorded file evidence.
Sidecar recovery also remains valid after an import releases its claim on failure.

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
remain photo-only. UI thumbnails are presentation assets and are
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
copy. All AI control and status endpoints require an administrator. The administrator work-status
table counts only the latest job for each media item and inference task, so a successful rerun
supersedes an older failure without deleting the historical job. A successfully inferred empty
result, such as no detected faces, is completed work rather than a failure. User-facing
duplicate-group and face-group browsing uses normal authenticated access and filters through `media_access`.
The LLM WebSocket uses the configured client ID and shared API key rather than a user JWT.

Metadata jobs use `queued`, `processing`, `cancelling`, `cancelled`, `completed`, and `failed`
states. Workers atomically claim rows, recover orphaned claims on startup, retry through
`available_at`, and verify every enabled task's prepared inputs before completion. Cancelling moves
queued jobs directly to `cancelled` and lets a processing owner settle from `cancelling` to
`cancelled` without committing metadata. Metadata cleanup removes generated metadata and related AI
data without scheduling regeneration; a later explicit generate action queues missing, failed, and
cancelled metadata work. Cleanup currently deletes matching AI job rows directly; do not claim that
cleanup uses the cancellation outbox unless that implementation is changed.
The durable cleanup operation is `metadata_clean_operations`, with its SQL grouped under
`queries::metadata_clean`; generation and cancellation SQL stays under `queries::metadata_jobs`.
Cleanup phases never enqueue regeneration. A new rerun requested after cancellation survives both
normal worker settlement and startup recovery; recovery consumes the rerun flag when it queues that
new attempt.

### Prepared input contract

Video preview decisions use FFprobe's content-derived container brand, codec, profile, pixel
format and audio channels, never filename extensions or declared MIME types. Compatible MP4
uses the original. Other containers receive a Journal-owned `preview.mp4`. H.264 with 8-bit
yuv420p/yuvj420p and Baseline/Main/High profile is copied. HEVC Main/Main10 with 8/10-bit 4:2:0
is also copied, preserving encoded pixels, bit depth and HDR metadata, with an `hvc1` tag
when producing MP4. Full-range `yuvj420p` is accepted for both codecs; stream copy preserves
the encoded range flags without relabeling or converting full range to limited range.
Playback of HEVC requires a capable browser/system decoder; this policy
does not guarantee support on older clients. Other video is encoded as H.264 High/yuv420p
(CRF 23, fast preset, 20 Mbit/s VBV). Audio selection always chooses the first AAC stream,
regardless of default flag, profile or channel count, and copies it. If none is AAC, the first
audio stream is encoded as stereo AAC-LC at 192 kbit/s, 48 kHz. Decode failures propagate;
the selection does not skip an undecodable first track when there is no AAC alternative.
The selected audio ordinal is carried to FFmpeg's explicit map, producing exactly one AAC track
when the source has audio. Sources without audio remain silent.
Multiple-audio originals are remuxed to expose only the selected track, even in native MP4.
No audio is synthesized for silent videos, and unsupported-only audio is not silently dropped. Only incompatible
streams are transcoded. Pixel dimensions are not scaled or padded; odd dimensions cannot be
encoded as yuv420p and fail explicitly. Stream-copy preserves rotation metadata; transcoding
uses FFmpeg's automatic orientation. Failures retain source context and do not publish partial output.
The fragmented MP4 supports immediate playback and HTTP
Range seeking. Image viewing and video playback both use `/preview`: the backend serves the
published preview when `preview_path` exists, otherwise the original, without client-side
format decisions. Invalid paths or missing published files return errors rather than silently
falling back. Preview tickets are scoped to that media's `preview` resource, never `original`.
Browser sessions and Android Bearer authentication also work on the same preview endpoint.
Download Original requests `original` and always receives the immutable original bytes;
AI frame extraction also still uses the original.
The preview shares metadata artifact publication, rollback and cleanup, and uses only mounted
storage. Copy reservations use the actual original size plus 10% and 16 MiB muxing headroom;
transcoding additionally reserves duration-based video/audio output headroom,
bounded by the executor's 32 GiB child-output ceiling. Empty input or remux failure
does not publish a partial preview. Existing completed metadata needs regeneration to gain it.

Generate Metadata queues ordinary incomplete work. A compatible original uses no generated preview.
If an authorized request finds a published video preview or thumbnail missing, the API can queue
one repair for that generation. The original must exist. The metadata worker regenerates both
thumbnail sizes, the required playback preview, and any missing prepared video frame.
The request still returns a missing-file response until the worker publishes the new generation.

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
verified handle. If a published video frame is missing, Momento queues metadata repair from the
original. The job waits without consuming a submission attempt. Momento keeps the saved input
descriptor and checks the regenerated frame against it before submission. A missing original or
changed input fails the job. File recovery skips frames that an active metadata claim is publishing.
The metadata worker waits for recovery to release a pending write claim before it reuses a frame.
The wire manifest contains no storage root or path: llm-service receives only descriptors and raw bytes,
persists those bytes below its own data root, and can run on another server with unrelated storage.
Input and job admission allow up to 32 GiB so large streamed media is not rejected by the previous
50 MiB cap. llm-service responds to admission with the exact input sequences it does not already
hold, and Momento streams only those sequences. Received source bytes live in a content-addressed
store below the llm-service queue and are hard-linked into job directories, so concurrent task types
share one durable copy without sharing storage with Momento. Supported camera RAW inputs are decoded
at full resolution by `dcraw_emu`/LibRaw inside llm-service; one TIFF normalization is cached per
source content hash and hard-linked into each active job. All source, normalized, and temporary RAW
files remain below the llm-service mounted data root. When Momento durably acknowledges a result,
llm-service deletes the job and removes content whose final job link is gone. Deployments still must
monitor durable queue space and configure `[scheduler].max_queue_bytes` plus
`working_space_reserve_bytes` for the llm-service filesystem.

Multi-input jobs preserve every descriptor's `sequence` and optional `frameTimestampMs` through
the queue, provider response, result message, and input-level persistence. Concurrency is across jobs;
inputs within one job are currently inferred sequentially in descriptor order. A new type must
define explicit aggregation and persistence semantics for all inputs rather than silently using
only the first result.

## Android backup completion

Android uploads at most 1 MiB per request and waits for the server to confirm each durable offset.
After a transient chunk failure, Android queries the durable offset before sending the chunk again.
A confirmed chunk advances the transfer even when its original response was lost.
Retries use bounded delays. Persistent failures retain the upload ID and snapshot for a later worker attempt.
Offsets and file sizes use 64-bit integers.

The completion endpoint queues the durable import task without reading the entire uploaded file.
The backup worker verifies the whole-file SHA-256 before publishing media.
A mismatch marks the backup as failed and does not create media.
Clients poll the existing status endpoint until processing finishes.
This keeps the completion request independent of video size without removing integrity verification.

A new backup operation for the same user, device, and client asset replaces the previous transfer.
The server cancels an uploading, queued, or expired transfer and journals staging cleanup in the same transaction.
It retains old operation responses for delayed retries and preserves completed media.
An active chunk write or import blocks replacement until that operation stops.
Android treats expired uploads as terminal and creates a fresh operation when backup resumes.
It retains the exact snapshot metadata across replacement so a lost create response remains retryable.

The backup session expiry is an idle deadline, not a limit on total transfer duration.
Each durably committed chunk renews it using the configured `backup.session_expiry_hours`.
Expiration skips active chunk writes. Completion or abandonment wakes session maintenance to recalculate its next deadline.
Failed chunks and status polling do not renew the deadline. Restart recovery releases interrupted writes before checking expiration.
