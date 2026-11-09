# Momento API Reserved Executor Refactor Plan

## Status

Approved plan implemented. Every in-repository phase is complete and the final release-mode,
plan-to-code, dependency-feature, and cross-project verification passes are green.

Current implementation checkpoint:

- The three-field worker configuration, checked runtime sizing, memory/fd preflight, r2d2 sizing and
  warmup, named fixed executors, two-thread network runtime, and CPU-built authentication dummy state
  are implemented.
- The current repository-wide verification checkpoint is green: all 47 common Rust tests, 11
  llm-service library tests, 93 llm-service integration tests, 8 Momento library tests, and 458
  Momento integration tests passed; optimized release builds for both Rust services and strict Clippy
  with `-D warnings` also passed, with no Clippy suppression attributes. All 90 executable Python
  llm-runtime tests passed (one environment-dependent case skipped), frontend Prettier, ESLint, all
  187 Vitest tests across 67 files, TypeScript, and the Vite production build passed, and
  `./build_android_client.sh verify` completed Android compilation, JVM tests, and lint in Docker.
  Final signed Docker packaging remains gated on operator-provided keystore material and is not treated
  as permission to invent or inspect signing secrets.
- Fresh SQLite schema creation now wraps the full source-owned schema in one atomic transaction before
  the existing file and parent-directory durability syncs. This removes per-DDL commit/sync overhead:
  the mounted-filesystem bootstrap performance test dropped from 2.22 seconds to 0.08 seconds without
  relaxing its two-second requirement, and the complete backend suite passes through the same path.
- Both service runtime layers were built independently. Momento's runtime installs
  `imagemagick-raw` and fails its image build unless ImageMagick registers readable DNG support;
  llm-service installs `libraw-bin` and verifies `dcraw_emu`. The temporary verification image tags
  are removed after the checks.
- `AGENTS.md` now describes the implemented scheduler, two-thread network runtime, closed CPU/File/SQLite
  executor domains, and r2d2 connection ownership; the obsolete single `available_workers` application
  pool description has been removed.
- Momento's Tokio dependency now declares only `io-util`, `macros`, `net`, `rt-multi-thread`, `signal`,
  `sync`, and `time`; the obsolete `fs` and `process` features have been removed, and the backend
  feature tree confirms that neither capability is transitively re-enabled. The unused `tower-http`
  dependency has been removed completely, while `tower` remains because the owned HTTP server and
  WebDAV contract tests use `ServiceExt`. The `image` crate is now test-only because production image
  normalization/cropping is performed by the supervised ImageMagick path. The Phase 6 dependency and
  cross-project verification work is complete.
- Runtime file-log draining now enforces both declared batch ceilings: at most 64 events and 256 KiB
  of encoded events per File-worker turn. If the byte ceiling is reached after a ring receive, one
  bounded event is retained by the writer and processed first on its next turn, preserving order
  without blocking or losing it; release tests cover both byte and event-count boundaries.
- Request-log capture is now a fixed source-owned 48 KiB limit rather than a configurable per-request
  allocation. The removed TOML field is rejected, binary/media and multipart streams retain only their
  omission marker, an oversized control body still reaches its handler unchanged, and base64/sensitive
  fields remain redacted on the CPU executor. Every JSON control request now uses one closed typed CPU
  parse selected from the exact request DTO registry; the same parse returns its redacted log payload,
  so Axum and logging no longer parse the body separately. The extractor retains the configured body
  limit and the prior 400 syntax/413 size/422 DTO-data status contract. Every successful non-streaming
  JSON response is likewise a closed response DTO serialized on a CPU worker through a four-MiB ceiling;
  the network lane only installs headers and sends completed bytes. Backup metadata JSON encoding and
  place-identity JSON/base64 conversion also use closed CPU operations. Release route coverage and
  dedicated CPU tests verify typed parsing, redaction, error classification, and response serialization.
  Error variants now create only a typed pending response DTO; the outer application middleware sends
  that DTO through the same CPU serializer before installing response bytes, status, and existing retry/
  connection headers. Its only non-CPU fallback is the fixed pre-encoded internal-error protocol frame
  used when the CPU executor itself cannot return a response.
- The unused `metadata.thumbnails_video_frame_quality` option has been removed from the typed config,
  defaults, generated template, playground config, function signatures, and callers. It had never
  influenced FFmpeg output; the old field is now rejected explicitly instead of presenting a no-op
  control, while the effective shared thumbnail quality remains intact.
- The old `src/backend/scheduler/` permit actor has been removed. Request and durable admission now
  use derived non-semaphore RAII counters owned by the runtime, and raw executor senders remain private.
- Authentication attempt buckets and the complete login/refresh/logout/password-change SQL path now
  use closed CPU/SQLite executor operations without holding an r2d2 connection across another domain.
  Password work uses the fixed bounded Argon2id contract.
- Pre-runtime configuration reads now enforce the 1 MiB and lexical path bounds, reject symlinks and
  non-regular files, capture file identity plus SHA-256, and are revalidated by a ready File I/O worker
  before any public runtime handle is published. Runtime cron updates and the one-shot administrator
  reset use File I/O identity CAS plus CPU TOML editing/full validation, atomic rename and parent sync;
  external edits leave both the file and live snapshot unchanged. File bootstrap removes only the exact
  reserved interrupted-update temporary.
- File bootstrap now requires an existing non-symlink data directory and holds a nonblocking exclusive
  lock on that directory descriptor for the complete executor lifetime, so a second Momento process
  cannot open the same storage. Listener binding occurs only after consistency-critical startup
  recovery. Already-committed `cleanup_pending` work is explicitly excluded from that barrier and is
  drained by the event-driven journal recovery worker after the listener is available.
- Scheduler control events are versioned and level-triggered. Durable admissions record their closed
  source and kind, shutdown rejects only new claims at quiescing, reports both active-work counts and
  the exact sorted `source:claim-token` identities still registered at the grace deadline, and waits
  for request/durable admissions before advancing through securing, draining, and stopped. Metadata
  claims, LLM result claims, LLM submission job IDs, and import content-hash claim UUIDs are registered
  for their complete durable SQLite ownership lifetime rather than only while one continuation runs.
- Default-administrator initialization, reset preparation, refresh-token maintenance, and schema
  initialization now use CPU/SQLite executor operations rather than executing SQL on the network lane.
- Authentication, users, albums, map, places, duplicate browsing, authenticated sharing, public-share
  lookup/content authorization, media batch lookup, and move-to-trash now use closed request-mode
  SQLite operations with owned DTOs. Their dynamically sized results are bounded during row mapping;
  ordinary HTTP work uses non-waiting request admission instead of the durable executor wait path.
- Every configured pooled connection now reapplies memory-only temporary storage, zero mmap, the fixed
  page cache, zero SQLite helper threads, busy timeout, and prepared-statement cache bounds. SQLite
  executor checkout is limited to five seconds and each operation installs a 30-second progress-handler
  deadline that is removed before returning the connection; interruption is a typed database timeout.
- Metadata queue/status/claim/completion and prepared-input descriptor loading now use closed SQLite
  operations; filesystem verification happens only after the owned descriptor DTO is returned, so no
  connection is retained during path access. Media binary authorization and persisted asset-reference
  lookup, plus editable metadata updates, likewise use request-mode SQLite operations; thumbnail and
  preview bytes are served only through the direct binary GET routes. GPS-derived
  geohash/reverse-geocoding is a separate CPU operation between a read-only
  preparation and one atomic final SQLite update/rtree transaction.
- Metadata job ownership now starts with a fresh UUID claim token stored under a fresh-schema
  processing-state constraint. The scheduler registers that exact bounded token while retaining the
  claim's durable admission; completion and retry compare both media ID and token, and a guard hands an
  abandoned or panicked claim back to SQLite before unregistering and releasing capacity. Pre-listener
  recovery requeues prior-process tokens after journal recovery, and release tests prove exclusive
  claims, stale-token rejection, rerun behavior, unbounded transient retries, registry uniqueness, and
  shutdown ownership. The same token is now stored on every generated-artifact Journal group and is
  rechecked at preparation, publication authorization, immediately before each file mutation,
  publication checkpoint, and completion; the final metadata/AI-input transaction also requires the
  same live token. A stale owner therefore cannot publish or persist. The normal thumbnail, tiny
  thumbnail and optional browser-compatible preview now share one product-targeted
  Journal group. They are written to claimed temporaries, published under a claim-specific versioned
  path, and exposed only when the same SQLite transaction finalizes the group and switches every
  persisted reference plus `artifact_version`. A failed run retains the prior generation; post-commit
  cleanup journals the prior paths, and startup cancellation detaches and cleans an interrupted new
  product before metadata claims are requeued.
- Metadata generation now loads its imported-media descriptor through one durable closed SQLite read.
  After all metadata and derived inputs are prepared, one bounded immediate SQLite transaction replaces
  raw sources, normalized metadata, thumbnail reference, content hash, rtree/geohash rows, and every AI
  input descriptor together. A failed AI-input preparation therefore leaves the previous complete
  database generation unchanged instead of committing metadata without its required descriptors. The
  completion verifier also opens and closes every required AI input through its typed
  descriptor-relative File session; it no longer canonicalizes absolute paths on the worker. The
  metadata generator now performs source reads and hashing through bounded File/CPU continuations and
  publishes thumbnails, previews, video frames, and embedded previews through the generic journal.
  Supplemental, exiftool, and ffprobe JSON now enter closed CPU operations and return typed DTOs;
  orchestration no longer traverses or serializes generic JSON. A metadata-specific streaming visitor
  validates and normalizes without constructing `serde_json::Value`, rejects duplicate keys, and
  enforces the exact 4 MiB encoded, 1 MiB normalized, 16-depth, 4,096-field, 8,192-item, and 256 KiB
  decoded-string limits before typed deserialization. Exiftool is invoked with an exact scalar/text
  whitelist, so binary and unrequested fields are neither persisted nor passed through business logic.
  Fractional integer fields, non-finite values, overflowing exposure denominators, malformed inputs,
  and allocation failures are explicit metadata errors. Release tests cover normalization, duplicate
  fields, every structural ceiling, typed supplemental/exiftool/ffprobe mapping, and binary-field
  exclusion.
- The generation path now validates each canonical image through a real generation-checked
  `ChildDescriptorLease`: a File worker opens and pins the Originals descriptor, the closed CPU child
  operation passes only its stable descriptor number to `identify`, and orchestration returns and
  closes the session only after the child process group is reaped. The common CPU child supervisor
  applies a closed tool identity, fixed argument/descriptor limits, one-thread native-library settings,
  per-tool address-space and runtime ceilings, `RLIMIT_FSIZE`, close-on-exec allowlisting, bounded
  nonblocking diagnostic draining, and SIGTERM/grace/SIGKILL group cleanup. Normal ImageMagick,
  FFmpeg, ffprobe, and metadata exiftool calls now retain one global CPU worker until completion rather
  than running on a network runtime thread. Binary embedded-preview output, ImageMagick crops/resizes,
  FFmpeg frames, and metadata inputs use pre-opened generation-checked child descriptors, while output
  descriptors target journal temporaries. The supervisor now lives under `executor/process.rs` and
  uses an explicit `fork`/`execvpe` startup handshake: a close-on-exec status descriptor distinguishes
  successful `exec`, exact startup errors, and a fixed 30-second start deadline; timeout kills and
  reaps the process group without adding a helper thread. Production raster work has no Rust full-image
  decoder: checked geometry is enforced before the supervised ImageMagick fallback, including the fixed
  262,144 dimension ceiling and checked pixel product. Its pixel cache is now a source-owned 256 MiB
  memory ceiling with map cache disabled and a fixed four-GiB disk scratch ceiling (the deployed
  ImageMagick build requires disk-backed decode scratch even for modest transforms),
  its process address space remains fixed at 512 MiB, and
  the former configurable cache controls are rejected. Face cropping requests only the calculated
  original-image region and never falls back to a thumbnail. Together with typed control/error response
  serialization, this closes the remaining Phase 4 network/decode items.
- The embedded GeoNames snapshot is now initialized by an explicit pre-listener CPU-executor command
  and retained as one immutable snapshot shared by the CPU facade. Decompression is streamed through
  a 16 MiB ceiling, record/field/string-arena allocations are checked, and nearest-neighbor lookup uses
  a packed in-place kd-tree instead of a whole decoded buffer plus `rstar`. Metadata extraction no
  longer performs reverse geocoding on its orchestration future; durable metadata generation requests
  location and geohash derivation from the CPU executor. `RuntimeBuilder` completes this initialization
  before it constructs the network runtime, so every published application handle has the immutable
  snapshot; the production startup path no longer performs a later duplicate initialization. The
  `rstar` dependency has been removed.
- Timeline pages and markers now run as bounded request-mode SQLite operations and return owned rows;
  pagination/group assembly retains no connection. Trash listing/restoration and every face browsing,
  representative lookup, and merge route also use closed SQLite operations. Face merging validates all
  groups, moves members, selects the representative, deletes sources, and returns counts in one
  immediate transaction. No HTTP route now checks out the legacy pool directly. Local, WebDAV, and
  backup import job accounting, hash claims, media allocation/finalization/failure, duplicate
  absorption, readiness revalidation/update, and paged interrupted-import recovery now also use
  closed SQLite operations; the backup worker no longer receives a pool. AI-result, deduplication, and
  face-grouping production entrypoints now also submit closed operations, and
  `ApplicationRuntime` no longer exposes or retains a clonable r2d2 pool outside the SQLite executor.
  Deduplication no longer executes comparisons or holds its cluster graph on a SQLite worker: its
  durable finalizer advances 64-row candidate, edge, label-pass, component, member, dirty-snapshot,
  publication, staging-cleanup, and retired-generation cursors. Embedding comparisons and representative
  metrics run as bounded CPU operations between short SQLite operations. New groups remain invisible
  until one active-generation switch; partial inference failures still publish the successful graph,
  and old generations are then deleted incrementally. Face grouping now uses its own durable
  64-row face snapshot, manual-anchor snapshot, manual/automatic comparison, representative-reduction,
  publication, restart, staging-cleanup, and retired-generation cursors. Embedding comparisons and
  representative scoring run on CPU workers between short SQLite operations. Building groups and
  automatic memberships stay invisible until the active-generation switch, manual group IDs and
  manual members remain outside generation cleanup, and a manual merge during construction advances
  a revision fence that discards and rebuilds only the invisible generation. Deduplication and face
  grouping are driven by separate durable scheduler loops under `DeduplicateFinalization` and
  `FaceGroupFinalization`; their shared wake is broadcast, so either finalizer may advance without
  waiting for or serializing behind the other feature.
- Startup recovery for deduplication and face grouping now uses closed durable SQLite operations.
  Face representative refresh advances through 64-group and 64-candidate continuations; SQLite returns
  only bounded owned candidate pages, CPU workers score them, orchestration retains one scalar best
  candidate, and a short SQLite write commits each group. This replaces both the former
  all-group/all-candidate in-memory transaction and the interim SQL scoring scan. Deduplicate
  finalization resumes from its persisted phase/cursors and has release tests for more than one page,
  invisible building generations, resumed work, partial failures, cancellation, atomic publication,
  and incremental cleanup. Face grouping has matching release coverage for cross-page grouping,
  invisible/resumable construction, partial failures, manual-anchor attachment, manual-edit restart,
  atomic publication, and staging/retired-generation cleanup.
- Authenticated originals, normal/tiny thumbnails, public-share originals/thumbnails, face/place
  thumbnails, and media previews now open one generation-checked
  `StorageFileSession` from a typed root-relative path. The File worker derives size, device, mount,
  inode, modification, and change-time evidence from that same descriptor; range seek and every
  response chunk reuse it. Each response read is at most 1 MiB and returns to the shared File FIFO
  before the next chunk, so a long transfer retains only its bounded stream-session slot rather than
  a File worker. The Base64 thumbnail/preview batch routes and their SQLite/DTO machinery are removed;
  web and Android clients load authenticated binary GET URLs directly. Missing paths retain 404
  semantics, executor saturation propagates, and
  the route tests now place assets under the exact per-database runtime data directory instead of the
  unrelated process-global test path. Static GET/HEAD serving has likewise replaced
  `ServeDir`/`ServeFile`: normalized paths open only below the read-only Static capability, media-style
  chunks reuse the opened descriptor, API/WebDAV paths never fall through, missing named assets remain
  404, and HTML navigation alone may reopen `index.html` for SPA fallback. The shared responder derives
  weak ETag and Last-Modified from the same descriptor, applies If-Match/If-Unmodified-Since before
  If-None-Match/If-Modified-Since, never accepts its weak tag for If-Range, accepts a current If-Range
  date, and preserves bounded GET/HEAD/range behavior. The backend no longer enables
  tower-http's filesystem service or depends on Tokio's file adapter for responses.
  Non-web-compatible image previews are now generated during the metadata stage at the persisted
  `Previews/media/<media-id>/v<version>-<claim>/preview.jpg` product path; HTTP never guesses or
  generates an artifact inline, and metadata
  reset journals that subtree together with all other derived trees. Consequently `src/backend/routes`
  and the application factory contain no direct filesystem calls or absolute storage-path lookup.
  Thumbnail and place-cover reads now require the normalized `thumbnail_path` persisted with the
  current metadata generation. The former fallback that guessed a thumbnail path from the canonical
  original path has been deleted together with the unused query/DTO columns; a missing or invalid
  reference is a missing thumbnail even when bytes happen to exist at the legacy guessed location.
  The metadata generator publishes those artifacts through one descriptor-bound product Journal
  operation. The restart-resumable WebDAV recursive-COPY cursor described below is implemented;
  repeated direct-filesystem audits currently find storage access only inside bootstrap and the
  File/SQLite/log executor owners.
- File I/O now also owns descriptor-relative directory sessions. Each continuation issues one fixed
  64 KiB `getdents64` read, validates every returned child with `fstatat(AT_SYMLINK_NOFOLLOW)`, rejects
  symlinks and unsupported node types, retains the directory offset in the generation-checked handle
  registry, and returns the session to the File FIFO before the next continuation. Local import and the
  Momento-owned WebDAV layer consume this interface; neither uses a direct recursive `read_dir` walk.
  Directory pages are capped at 256 entries even when one `getdents64` buffer contains more, with the
  descriptor offset rewound to the first unreturned record.
- Aggregate AI status and manual AI starts now use bounded closed SQLite operations. All feature
  starts, including deduplication run creation plus clustering-job/input snapshot creation and face
  run/job creation, commit atomically. The live scheduled occurrence path calls that same start
  operation in durable mode with its scheduled timestamp. Deduplication startup/catch-up now also
  reads its latest-run/last-schedule snapshot and starts its replacement through durable closed SQLite
  executor operations; the live cron path no longer checks out r2d2. The testable scheduled-occurrence
  helper is now asynchronous, accepts only `SqliteExecutorHandle`, and is the exact helper called by the
  live loop. The former synchronous pool-based cron/start helpers have been deleted.
- The seven independent cron futures have now been replaced by one scheduler-thread-owned timer
  state machine. It retains one next occurrence per task, reacts to the existing config watch,
  dispatches each due occurrence through the same durable start operation as manual runs, and bounds
  deduplicate catch-up scans to 256-occurrence CPU-executor continuations that re-enter the FIFO tail.
  Cron evaluation receives one
  immutable startup `SystemTimezoneSnapshot`; runtime code no longer reads `chrono::Local`.
- The live LLM submission and cancellation-delivery workers now use closed durable SQLite operations
  for stale-claim recovery plus input snapshotting, atomic bounded job claims, owned prepared-input
  descriptor reads, every post-network submission transition, bounded cancellation-batch reads, and
  acknowledgement cleanup. No database connection is retained while input files are verified or while
  the WebSocket is awaited. Prepared inputs now open through their typed Originals/Previews root and
  normalized relative path, validate descriptor size from the opened descriptor, hash in bounded
  File-to-CPU chunks, rewind that same generation-checked session, and send at most one 64 KiB frame per
  File operation. Neither verification nor transport owns a Tokio file or absolute path, and a verified
  descriptor survives later path removal/replacement. The legacy synchronous
  `queue_task`/`cancel_active_jobs` fixture paths have
  been deleted and their tests now drive the closed executor operations. AI result candidate selection,
  face-preparation context loading, and final persistence now use closed SQLite operations. The direct
  result-processing test path has a separate closed operation so it propagates precise validation,
  conflict, not-found, and database errors instead of applying inbox permanent-failure semantics.
  Face preparation now verifies the original descriptor, inspects oriented dimensions, and writes each
  crop through a descriptor-bound ImageMagick child into a journal temporary. Streamed face records use
  a typed preparation entrypoint and never rebuild the compatibility JSON result. They publish bounded
  versioned artifact groups below a job/product-version path; one token-fenced SQLite transaction
  switches all face rows and artifact groups together, and generic cleanup removes only Journal
  temporaries while retaining the published crops. Journal capacity ownership is keyed by the unique
  file-operation group rather than the repeatable business owner, so the receipt and every artifact
  batch have independent durable reservations.
- Momento's LLM WebSocket now has one connection-owned actor instead of separate reader/writer tasks,
  a generic outbound-message queue, and unbounded per-job event channels. Its bounded command mailbox
  admits submission/cancellation owners; urgent controls are separately bounded; each active
  submission sends at most one 64 KiB frame before rotating to the next job; waiting for inference
  retains no Momento file or durable admission; heartbeat expiry and ready/final acknowledgement
  deadlines are explicit. A concurrent-submission integration test proves binary-frame round-robin.
  The actor now also accepts manifest-first result streams, validates and credits one 64 KiB binary
  frame at a time, expires stalled receipts without imposing a total-progress timeout, and tracks
  result sessions independently beside submissions and cancellations. Before `resultReady`, the live
  path now atomically creates its token-fenced receipt, product-owned Journal group, path claims, and
  declared-byte reservation. Every credited frame is incrementally validated and written through a
  bounded File session; exact sync/hash/size completion precedes Journal publication and the atomic
  durable-inbox commit, and only that commit permits `resultReceived`. Cancellation, invalid input,
  timeout, disconnect, and restart hand the group to bounded rollback or forward cleanup; successful
  processing likewise detaches the inbox product, deletes its file through generic recovery, marks the
  receipt cleaned, and releases its Journal reservation. The transport no longer creates the
  compatibility JSON inbox row. The result worker reopens the Journal file through `FileIoExecutor`,
  rechecks its exact length and SHA-256 through bounded File-to-CPU chunks, validates the record stream,
  and idempotently stages records in at-most-256-row transactions with durable record/byte cursors.
  After typed persistence, it removes staging rows in at-most-256-row cleanup transactions; only the
  completed staging cleanup hands the inbox group to generic file cleanup. Pass one now uses the
  non-collecting stream validator; pass two reads staged rows in ordered at-most-4-row/4-MiB pages and
  performs the collecting validation from SQLite rather than from the Journal buffer. Pass two now
  persists typed staged values directly without constructing `JobResult`; face results additionally
  build and atomically switch bounded versioned artifact groups. The compatibility table, Legacy
  receipt path, direct `JobResult` executor operation, duplicate JSON parsers, and immediate face-crop
  preparation path are removed. Validation fixtures now construct the same typed result values used by
  staged-record persistence, while end-to-end transport tests exercise the complete Journal path.
  Pre-listener recovery now separately finds interrupted
  result products already in publication/forward states, generation-fences and detaches them one
  bounded page at a time, then lets ordinary Journal recovery finish rollback or forward cleanup;
  a release test covers this product-specific restart handoff.
- The shared Rust `common::llm` contract now implements the exact 20-byte little-endian
  `momento-result-chunk-v1` frame (`MRCH`, version/reserved bytes, job-ID length, offset, payload
  length), including the 128-byte hexadecimal job-ID limit, 64 KiB nonempty payload limit, checked
  exact-length decoding, and rejection of bad magic/version/reserved/truncation/trailing bytes. Common
  release golden-vector and maximum-boundary tests pass in addition to both service builds. Both live
  transports now use this frame with exact cumulative-offset acknowledgement and one-credit flow
  control.
- The shared Rust contract and the Python runtime module now implement the exact 24-byte little-endian
  `momento-result-records-v1` framing header and CRC32C coverage rule. The Python implementation uses
  its fixed source-owned 256-entry Castagnoli table and adds no package dependency; Rust and Python
  release tests share an exact cross-language golden vector and cover empty, maximum, truncated,
  unknown-kind, corrupt, and oversized records. Both implementations now share the closed ordered
  twelve-kind registry and enforce each kind's encoded-payload and normalized-heap ceiling before
  allocation; Rust compile-time assertions prove every table member stays within the 1 MiB/2 MiB
  global bounds. Exact field-level little-endian codecs now cover failure text, input boundaries,
  UTF-8 OCR chunks, length-prefixed tag chunks, fixed 768-float clustering embeddings and hashes,
  five-score aesthetics, fixed 512-float face records, and screenshot/document classifications.
  Both implementations reject invalid UTF-8, lengths/counts before allocation, non-finite or
  out-of-range scores, nonzero reserved bytes, and trailing data; their fixed payload golden vectors
  match. A shared bounded stream validator now enforces strictly ordered manifest inputs and record
  sequences, exact input start/finish correlation, task-compatible primary records, face-empty-result
  semantics, one unscoped failure record, at most four text/tag continuations, and the 2 MiB aggregate
  before a stream can finish; matching Rust and Python tests cover valid and invalid transitions. The
  shared durable result manifest now also validates correlation, task/status, exact encoding, minimum
  and maximum record bytes, SHA-256, and bounded model provenance (required only for completed work).
  Record consumers can now validate the exact 24-byte header and its kind-specific payload length
  before allocating or reading the payload, then validate CRC32C from the separate bounded payload;
  the complete-slice decoder delegates to that same incremental path in both Rust and Python.
  The live transport validates these records incrementally and durably stores the exact record bytes
  in Momento's Journal inbox. The live worker now reads and stages those exact records directly,
  persists typed staged values, and publishes face artifacts through versioned groups. Other
  multi-artifact product generations still require the same versioned switch.
- llm-service now converts every typed provider response into the shared bounded result records before
  durable completion. It writes and syncs `result-records.tmp`, atomically publishes and directory-syncs
  `result-records.bin`, then writes and syncs `result-manifest.tmp` and atomically publishes and
  directory-syncs the independently validated `result-manifest.json`; only then does it move `processing` to
  `callback_pending` with both parents synced; `result.json` has been removed. Provider task/input/model
  correlation is checked during encoding, invalid provider output becomes a bounded failed record, and
  OCR/tag values use at most four continuations. Startup promotes a complete, hash-valid, semantically
  valid manifest/record pair from `processing` without rerunning inference, while an incomplete pair is
  removed and requeued. WebSocket delivery reads the durable record file in at most 64 KiB frames after
  Momento accepts the manifest, waits for the exact cumulative-offset credit after every frame, and
  sends `resultFinished` only after the declared size has been transmitted. No monolithic result is
  reconstructed for the wire or retained on llm-service disk. Startup validation also scans the record
  file incrementally with one 64 KiB buffer and a non-collecting typed validator; it no longer reads the
  complete result or rebuilds a compatibility DTO in memory.
- Both live WebSocket handshakes now apply the shared direction-specific allocation ceilings instead
  of Tungstenite's large defaults: Momento accepts at most 128 KiB per message/frame, llm-service
  accepts `MAX_CONTROL_MESSAGE_BYTES + 1 KiB`, and both use a zero target write buffer with a 256 KiB
  maximum so sink backpressure is visible. Per-message compression remains absent.
- The 1..=128-byte hexadecimal job-ID rule is now one shared `common::llm` validator used by
  llm-service queue/cancellation/recovery admission and by both binary input and result-chunk codecs;
  the previous unbounded llm-service-local predicate has been deleted. Common boundary tests and all
  78 llm-service release integration tests pass.
- Submission manifest bounds are now one shared validator used immediately before Momento enqueues a
  transport session and before llm-service creates a staging admission. It enforces a positive media
  ID/correlation attempt, the closed seven-task registry, 1..=1024 strictly ordered inputs, path-free
  255-byte filenames, 127-byte image MIME types, the exact `image`/`video_frame` input kinds, nonzero
  per-input and checked aggregate 32 GiB limits, and 64-character SHA-256 hashes. Momento also proves
  that its prepared descriptor sessions match the validated sequence list. The duplicate local
  llm-service size/count checks have been removed, and common plus both service transport/admission
  release tests pass.
- The result-receipt transport no longer owns or receives an r2d2 pool and no longer accepts a
  monolithic WebSocket result message. It validates the manifest, loads at most the shared 1,024 input
  correlations through the closed SQLite executor, incrementally validates each credited record frame,
  verifies exact byte size/SHA-256/record completion, and only then commits the Journal-backed receipt
  without constructing or storing a compatibility JSON DTO. Terminal or unknown deliveries remain
  idempotently acknowledged. Before `resultReady`, one atomic SQLite operation validates the
  media/task/attempt against the current job and returns the same bounded input-correlation snapshot;
  a mismatch first marks the Momento job failed and is then rejected without transferring record bytes.
  Every correlated permanent protocol failure, including an invalid manifest, frame offset/size,
  record CRC/semantics, completion size/hash, or aggregate result, first atomically marks the matching
  active Momento job failed. Only that committed outcome emits `resultReceiptRejected`; a stale or
  already-terminal delivery is acknowledged idempotently, and a transient SQLite failure emits
  `resultReceiptDeferred` so llm-service retains the durable bytes. Duplicate active starts and
  finish markers without an active receive session are deferred for a clean replay rather than being
  misclassified as permanent corruption. Temporary
  scheduler/SQLite receipt failures use `resultReceiptDeferred`, and llm-service persists a retry time
  without consuming an attempt. llm-service records permanent rejection evidence before moving the job
  to `failed`. The Journal-owned inbox file, deterministic Journal reservation, receive token, and
  job-version fence are now live. The worker now performs direct paged record staging, a staged-row-
  driven typed persistence pass, paged staging cleanup from that inbox, and versioned face artifact
  publication. The result manifest now derives one page-aligned SQLite construction footprint from its
  task, record count, encoded bytes, final rows/indexes, and artifact plan before `resultReady`. The
  receipt atomically binds that provisional capacity to a durable SQLite parent; each staging page
  consumes a bounded child in the same cursor transaction, typed terminal persistence returns unused
  construction capacity and transfers ownership to `llm_result_cleanup`, and cleanup reuses the one
  retained rolling bound until the Journal product becomes terminal. The in-memory ledger publishes
  each consumed child against measured SQLite allocation, reconstructs only the remaining parent after
  restart, and releases it only after terminal file cleanup. Release tests prove task-derived/page-
  aligned bounds, restart reconstruction, owner transfer, cleanup shrink, and final release. The
  Import publication now uses the same product-switch boundary for its original plus optional sidecar.
  `llm_jobs` now also owns a positive monotonic `state_version`; every production claim, retry,
  recovery, submission, cancellation, completion, and failure transition advances it. Result receipt
  preparation snapshots that version, and every post-ready permanent disposition compares it before
  changing the job. A cancellation that wins after `resultReady` is therefore acknowledged as the
  final user intent instead of being overwritten by a late corrupt frame. Release tests cover both the
  version progression and that cancellation/result race. This token is the fencing basis for the
  live Journal receive group and result-product version.
  The fresh schema contains the bounded live `llm_result_receipts` owner and live
  `llm_result_staging` record table. Their checks enforce the closed task,
  result status/model correlation, exact record encoding, one-million-record/1-GiB manifest ceilings,
  4-KiB durable diagnostics, UUID receive/claim tokens, monotonic job/product versions, bounded
  progress cursors, the closed twelve record kinds, and at-most-1-MiB normalized staging payloads.
  The receipt is uniquely linked to its product-targeted Journal group and `(jobId, attempt)`; schema
  tests cover a valid continuation row and reject an oversized staging payload and completed receipt
  without model provenance. Transport tests prove the synced inbox exists before acknowledgement and
  that result persistence schedules bounded cleanup through to file deletion, a cleaned receipt, and
  released reservation. The `llm_job_results` compatibility table, its JSON insert/select/delete
  queries, executor operation, Legacy candidate branch, and receipt-only fixtures have been deleted;
  the Journal-backed record stream is now the only durable production inbox. Pass two persists the
  validated typed values for all seven tasks directly from
  staged rows; it no longer reconstructs JSON values or a compatibility `JobResult`. Face preparation
  consumes typed face payloads and publishes result-product artifact groups.
  A 100-input/300-record release integration test crosses the
  256-record staging boundary and proves ordered pass-two persistence plus complete paged cleanup.
  A separate streamed integration test proves clustering, aesthetics, and screenshot records persist
  through the typed path without any compatibility JSON inbox.
  Result candidate selection is now claim-token fenced. A bounded Immediate transaction atomically
  changes each selected receipt from `received` to `processing`, installs a fresh UUID, and excludes
  that row from competing selectors. The scheduler registers each UUID while its orchestration owns
  capacity. Every staging write, paged staging read, and final persistence transaction compares the
  same token; cancellation clears it, so a stale worker cannot publish. Transient preparation or
  persistence failure releases the token and returns the receipt to the durable FIFO tail without
  resetting its record/byte cursor. Startup recovery clears prior-process result claims before normal
  workers and the listener are started. Release integration coverage proves active claims are excluded,
  stale tokens cannot release a new owner, exact release restores eligibility, and restart recovery
  preserves the staging cursor.
- Feature-specific and all-feature AI cancellation now count active work, publish cancellation scopes
  and exact job outbox rows, transition jobs, and request deduplication/face-run cancellation within one
  SQLite transaction. Local-import job creation and the combined latest-job/actual-total-media status
  read are also closed request operations; import error rows and bytes are bounded during mapping.
- Administrator AI clean now runs through one closed request-mode SQLite operation per feature. Each
  operation checks active jobs and pending cancellation state inside the same transaction that removes
  the feature's database products. Face clean no longer calls `remove_dir_all`: it atomically records a
  generic subtree cleanup for `Previews/faces`, commits the database reset only when that path claim is
  acquired, and wakes journal recovery afterward. Deduplication clean also performs its active-run
  check and all index/run cleanup in that closed transaction. Result-inbox, result-artifact, metadata,
  and import-product cleanup now use the same journal boundary.
- Metadata reset now owns a fresh-schema singleton durable state machine. It first prepares and retains
  one five-entry generic cleanup group for the normal/tiny trees plus face crops, shared
  AI frames, and browser media previews; that group stays `prepared` while database work is incomplete.
  The reset invalidates metadata claim tokens, gates new metadata claims, and clears every metadata/AI
  product, result-inbox staging table, current and staged deduplication generation, and current and
  staged face generation in at-most-256-row transactions. It then rescans imported media in bounded
  pages to queue metadata and mark similarity work dirty. Only the final transaction removes the reset
  cursor and changes the retained cleanup group to `cleanup_pending`, so crash recovery can never delete
  the derived trees before the database reset is durable. The metadata worker advances one persisted
  page after restart before accepting normal work, while the administrator request drives the same
  operation to completion. Release coverage crosses the 256-row boundary, stops mid-reset, resumes with
  the original journal owner rather than a replacement ID, and proves the cleanup group is invisible to
  recovery until the terminal switch. AI reset retains the documented direct job-deletion semantics;
  no migration or compatibility path is added because deployments reset the database from scratch.
- The generic filesystem journal schema is now present from the fresh-schema source: bounded operation
  groups/entries, indexed path claims, durable Journal/SQLite/Log reservations, and idempotent retry
  receipts have source-level state/action/cardinality constraints. The closed 13-member
  `StorageRootId` registry and the single bounded relative-path normalizer now produce unambiguous
  component-length keys, all canonical ancestor keys, and indexed subtree upper bounds while rejecting
  absolute, traversal, empty, oversized-component, oversized-depth, and oversized-total paths.
  The same owner defines exact/subtree and read/write claim conflict semantics: different roots and
  overlapping reads coexist, while equal paths or component-boundary ancestor/descendant overlap
  conflict whenever either claimant writes. Logical root identifiers are distinct from their fixed
  on-disk directory names, including tiny thumbnail root.
- Generic journal plans now have one typed owner for the four entry actions, action-specific path
  shapes, bounded group/owner/product text, 1..=256 entries, 1..=512 claims, expected evidence, and
  Journal space reservations. Validation rejects internally conflicting claims before SQLite
  admission, rejects every source/temporary/destination/tombstone mutation path not covered by a
  same-root write claim, and requires a positive durable reservation exactly when a group publishes
  new bytes.
- The shared data-directory capacity ledger now exists as one generation-checked implementation for
  the non-borrowable Journal, SQLite, and Log classes. It derives the recovery floor and fixed class
  quotas from a fresh `fstatvfs` snapshot, checks the cross-class free-space invariant under one short
  lock, distinguishes temporary exhaustion from a hard class limit, and exposes explicit
  `RecoveryOnly`, `RecoveryReady`, and `Running` health snapshots. Startup pages active/releasing
  durable reservations 256 rows at a time, validates filesystem/owner/version evidence, measures
  retained SQLite and log allocation, reconstructs one epoch, and refuses ordinary runtime admission
  unless it is healthy. Move-only provisional tokens release on Drop; durable checkout Drop only
  releases exclusivity, and terminal journal transactions notify the SQLite worker's sole ledger
  release path so a long-running process does not leak reserved capacity. Journal preparation can no
  longer accept a caller-authored filesystem ID or byte count: it consumes a Journal token signed by
  the File facade and promotes that same obligation after the owner transaction commits.
- Every currently registered closed SQLite operation is now explicitly classified as read-only, a
  Fresh write with a nonzero source-owned `max_growth_bytes`, or one of the implemented LLM-result
  provisional/durable parent stages. A fresh writer reserves the SQLite class before
  connection checkout, checks the WAL peak and attempts one bounded passive checkpoint when required,
  and publishes measured main/WAL/SHM allocation before releasing the token on success, rollback,
  timeout, or panic. SQLite has no fixed database quota and Momento does not override SQLite's native
  `max_page_count`; startup rejects only a WAL beyond its bounded operational limit. The
  LLM result receipt/staging/persistence/cleanup now exercises the provisional parent, durable child
  consumption, owner transfer, rolling cleanup, and restart reconstruction protocol. That is the only
  paged writer that must preserve future SQLite capacity after accepting a non-reconstructible remote
  payload. Metadata reset, similarity/face finalization, Journal recovery, and directory-copy cursor
  updates retain reconstructible source state and deliberately acquire an independent Fresh bound for
  each short page; temporary exhaustion returns them to the durable tail without losing accepted data.
  r2d2 now uses a budget-aware manager: every replacement connection checks out the closed
  WAL/SHM/metadata opening peak before `open`, runs
  its validation query, publishes measured main/WAL/SHM allocation, and only then becomes available.
  Capacity failure leaves the pool below target and follows normal r2d2 timeout/retry behavior.
- File bootstrap now proves whether the official SQLite main/WAL/SHM set is fresh or existing before
  it mutates anything: orphan side files and an empty/truncated main file fail. A fresh database schema
  is built with rollback journaling in the exact private same-directory bootstrap name, checked for
  integrity and an empty reservation table, file-synced, atomically published without replacement, and
  parent-synced before r2d2 opens the official path and enables WAL. Existing databases are never run
  through `CREATE ... IF NOT EXISTS` as a migration; bootstrap compares their complete bounded
  `sqlite_master` manifest with a schema built from the current source and tells the operator to reset
  on any difference. Opening an existing database is guarded by the single RecoveryOnly token and the
  observed post-open growth is checked before durable reservation reconstruction. The former fixed
  64 MiB recovery request has been replaced by `SqliteRecoveryFootprintSpec`: it validates the SQLite
  main and WAL headers, derives the exact page size and complete WAL frame count, measures allocated
  main/WAL/SHM extents, and checks a simultaneous checkpoint-main-growth, WAL activation, SHM-region
  recreation, and directory-metadata peak against both current free space and the recovery floor.
  Existing startup now opens the official database with SQLite `READ_ONLY`, installs `query_only` and
  the bounded connection pragmas, validates the exact schema and reservation table, and reconstructs
  the ledger directly from that connection without using the recovery floor. Only a failed probe with
  a structurally valid nonempty WAL may acquire the one exact recovery token; it performs one FULL-sync
  TRUNCATE checkpoint, syncs every surviving main/WAL/SHM file and the parent, publishes allocation,
  destroys the token, and must pass a second read-only probe. The ordinary read-write pool is created
  only after reconstruction/Log recovery under a normal SQLite-class bootstrap reservation. A release
  unit test now retains a live WAL writer, snapshots device/inode/allocation/timestamps and every byte
  of main/WAL/SHM, runs the complete read-only probe, and proves all three files remain identical.
  Fresh schemas also record the source-owned SQLite `application_id` plus schema `user_version`;
  validation requires both before the exact schema manifest, so a schema-shaped crash-left temporary
  without Momento identity is preserved and startup fails. The injected executor-runtime test entrypoint
  now rejects in-memory databases and canonical path mismatches, while shared integration fixtures use a
  real file-backed database in the exact runtime data directory and reuse one runtime per database; the
  old unrelated in-memory-pool/data-directory topology is gone. Fresh
  creation no longer uses a fixed 64 MiB
  reservation: `SqliteBootstrapFootprintSpec` builds the current source schema in memory, derives its
  page size/count, and reserves the simultaneous private database, rollback journal, first WAL frame,
  SHM recreation, and directory-metadata peak. A crash-left exact bootstrap temporary is opened
  read-only and removed only when its integrity and complete current schema validate and every
  application table is empty; invalid or nonempty evidence remains in place and startup fails for
  inspection. Recovery therefore no longer trusts the exact private filename plus an empty schema
  shape alone.
- Journal preparation is now one closed SQLite write operation. It revalidates the complete plan,
  checks equal, subtree-ancestor, and subtree-descendant conflicts through the covering path index,
  and atomically inserts the group, ordered entries, claims, and active Journal reservation. A
  conflict rolls back the complete transaction and returns a typed `PathConflict`; no partial group or
  reservation is left behind.
- The private File bootstrap now locks the existing non-symlink data directory, creates and validates
  every fixed writable child root before SQLite/CPU/network publication, and publishes one shared
  fixed-capacity capability registry to every File worker. Required roots open with
  `O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC`; the optional Static root is never created and is rejected when it
  is unsafe, unreadable, symlinked, equal to, contains, or is contained by the private data root. Main
  no longer creates storage directories after runtime publication. Before publishing the registry it
  now compares Linux `statx` device and mount identities for Journal plus every writable root, creates
  and syncs one private Journal probe, atomically renames it into every writable root and back with
  both parents synced, and deletes/syncs the probe; Static remains excluded and tests verify no probe is
  left behind. The capacity ledger is derived first; SQLite reservation reconstruction then reaches
  `RecoveryReady`; only afterward does each missing root consume its own bounded Journal token, create
  the directory, sync the data-directory parent, publish allocation, and release. The rename probe
  likewise reserves its content plus simultaneous directory-entry metadata peak and publishes the
  post-cleanup allocation before release. Public File I/O senders are created only after a
  named bootstrap worker consumes exactly one
  command from a private capacity-one channel, revalidates the config identity, acquires the lifetime
  data-directory lock, validates/creates roots, completes the mount/rename probe, returns the retained
  capabilities, and exits. Public workers start only from the already-filled immutable root registry;
  bootstrap-only operations no longer exist in the public File operation enum. Reconstruction now
  permits only `Healthy` or `LogOverQuota` to continue. In the latter state the private bootstrap File
  path repeatedly retains a fixed 256-entry oldest-candidate heap, deletes only exact closed
  `momento-api.YYYY-MM-DD.log` rotations, revalidates each inode and allocated extent immediately before
  descriptor-relative unlink, syncs the Logs parent, remeasures all retained regular entries in bounded
  continuations, and publishes a decreasing Log allocation until health is restored. Unowned names,
  symlinks, non-regular entries, or an overage that cannot be removed fail startup. Fresh schema
  creation waits for this recovery; existing databases perform it only after the recovery token and
  durable reservation reconstruction. Common logging now contains formatting/validation only.
  llm-service owns its appender and guard in `src/backend_llm/logging.rs`; both services use a fixed
  source-level INFO filter and neither reads `RUST_LOG`. Momento no longer owns `tracing-appender`, a
  logger thread, a synchronous file writer, or a data-directory path in its logging layer. Formatted
  events enter the runtime-sized fixed-capacity ring, queue/oversize failures are counted by severity,
  and the existing File workers alone append, sync, and rotate descriptor-relative files below the
  typed Logs root after acquiring Log-class capacity. Runtime shutdown drains the bounded ring before
  File workers exit. Request-body capture now owns its bounded buffer inside the body stream, sends it
  exactly once after downstream consumption, and never shares an `Arc<Mutex>` with the network lane.
  Redaction, JSON parsing, compaction, and serialization run in one bounded CPU-executor operation;
  multipart/binary bodies remain fixed omission markers and oversized captures never truncate the
  handler body. The capture-boundary tests cover recursive redaction, normal JSON, oversize, binary,
  multipart, and non-POST behavior.
- The first journal publication state-machine slice is executable end to end. SQLite version-CAS
  changes `prepared -> publishing`; only the resulting move-only grant can acquire the scheduler's
  exclusive generation lease. The lease registry is fallibly preallocated to the derived File queue
  bound and cannot grow past it. SQLite snapshots the bounded ordered entry descriptors into the
  move-only grant; File APIs accept only a recorded sequence and never caller-supplied paths. Each
  lease permits exactly one operation, and its durable stage restricts Publication to
  Publish/Move/Tombstone and Cleanup to deletion. The File worker revalidates group/version/generation/
  stage immediately before descriptor-relative, no-symlink `renameat2` or `unlinkat`, validates regular
  file size when declared, and syncs every affected parent. Publication and cleanup recognize their
  source-absent/destination-present or already-absent crash states idempotently. Entry checkpoints
  advance through `files_committed`, `cleanup_pending`, and `cleaned`; the generic no-product finalizer
  releases path claims and the Journal reservation at product commit. Startup now enumerates generic
  nonterminal groups one entry at a time and a restart integration test proves recovery after rename
  but before its SQLite checkpoint. Permanent publication and cleanup failures are classified into
  durable failed states without blocking unrelated groups; transient failures release their worker
  admission and return to the recovery tail. An administrator retry is one version-CAS transaction
  with a seven-day, per-operation-count-bounded idempotency receipt, and only a newly committed retry
  wakes the permanent journal recovery loop. The administrator-only typed list/get/retry endpoints
  are present; list requires explicit state filters and bounded cursor pagination, while full detail
  returns ordered entries and active claims using only logical storage roots and normalized relative
  paths. Maintenance deletes at most 256 independently expired retry receipts, compacts one fully
  cleaned/rolled-back group into immutable action/state/cleanup counts, and prunes one compacted group
  after seven days; compacted detail omits entry/claim arrays and returns those real summaries instead.
  Cancellation now fences the current in-memory generation and drains any mutation that already
  crossed the worker's atomic start check before committing intent. Mutation authorization uses a
  pre-SQLite move-only candidate ticket: SQLite grant APIs require that ticket, cancellation advances
  its epoch, and ticket-to-lease conversion rechecks group/version/epoch. A stale queued lease or a
  delayed pre-cancellation grant therefore cannot mutate, including after terminal fence retirement.
  Repeated cancellation first reads the bounded durable cancellation status and returns the original
  state/version without installing another fence; a concurrent version race is reconciled by another
  durable read. Terminal rollback/finalization/cleanup invalidates the last epoch and retires the gate
  as soon as all invalidated candidate tickets have dropped, so cancelled operation history cannot
  exhaust the bounded registry.
  Prepared groups enter `rollback_pending`, delete only recorded Publish temporaries in reverse order,
  leave unstarted Move/Tombstone sources unchanged, then release claims/reservations at `rolled_back`.
  Cancellation after the forward point finishes publication with `completion_outcome = discarded` and
  reuses the recorded Publish destination as ordinary cleanup work. Rollback failures persist both
  entry and group diagnostics, release their current mutation/worker resources, and return to the
  recovery tail without failing startup. SQLite constraint/schema/corruption-class finalization errors
  enter `finalize_failed`; transient database errors remain `files_committed`, and administrator retry
  resumes only the same versioned finalizer. A failed cancellation transaction deliberately retains
  its closed fence until the same request is retried. This is deliberately resolved only by retry; no
  operator-abandon/reopen endpoint is exposed because the current schema has no stronger durable
  no-intent proof than completion of that same cancellation transaction. Recursive cleanup is
  descriptor-relative and removes at
  most 256 nodes per File operation. An incomplete tree advances a monotonic durable recovery order,
  releases its mutation lease, self-wakes recovery, and resumes from the FIFO tail; a missing ancestor is
  idempotently already absent. Journal authorization now loads the persisted SHA-256 evidence instead
  of dropping it: orchestration streams the claimed source through File and CPU continuations, rejects
  a mismatch as a durable stage failure, and carries the observed identity version into the bounded
  mutation so a post-hash replacement cannot be renamed. Restart recovery accepts either the original
  source or the already-renamed destination, preserving idempotence. Cancellation detaches every
  discardable metadata, LLM-result, and import product before generic forward cleanup, and their
  product-specific SQLite finalizers atomically switch visible references before releasing recovery.
- Phase 1 and Phase 2 implementation is complete. The direct business database/filesystem migration,
  exact active-claim shutdown reporting, explicit per-operation SQLite capacity classification,
  LLM-result durable parent, and budget-aware r2d2 replacement path are implemented and covered by
  release tests.
- Momento now owns its inbound HTTP/1.1 accept loop and connection admission. The fixed builder applies
  a 128 KiB read buffer, 128-header ceiling, 15-second header deadline, no pipelining flush, and no
  HTTP/2 path before Axum routing. URI length and request content encoding are rejected at the common
  boundary, including static fallback routes. Request admission converts once into bounded stream
  admission, and media/static/WebDAV/backup transfers charge each live 1 MiB file chunk globally while
  retaining the stream slot rather than a request slot. Release tests cover oversized headers,
  protocol rejection, conversion ownership, file streaming, WebDAV, and backup uploads. The HTTP owner
  now starts scheduler handoff and graceful connection draining under the same 30-second deadline,
  aborts residual connection tasks at expiry, and reports the exact unresolved admission counts rather
  than waiting before the grace clock starts. The connection owner also enforces the source-owned
  60-second request-body, 30-second keep-alive, and 60-second response-write idle deadlines from actual
  socket activity. Request-body and response wrappers switch the timer to a no-deadline paused phase
  while a handler awaits File/CPU/SQLite continuations, so internal executor backpressure does not age
  a progressing transfer. Release tests prove partial-body expiry and that handler work longer than a
  test-scaled socket deadline is not disconnected. Shutdown reports exact registered metadata, result,
  submission, and import claim identities in addition to per-source admission counts.
- The live WebDAV readiness boundary now invalidates all affected paths in one closed immediate
  SQLite operation before a staging mutation and records successful completion through a second
  closed request operation; the WebDAV request path no longer checks out r2d2. `dav-server`/`LocalFs`
  and their dependency have been removed. The Momento-owned layer streams GET/HEAD, PUT/PATCH, file and
  recursive-directory COPY, MOVE, DELETE, and real MKCOL through typed File operations and the generic
  journal. PROPFIND honors depth zero, one, and infinity and streams 256-entry XML batches. Empty files,
  multi-mebibyte single body frames, directory replacement/move/delete, nested traversal, and startup
  rollback of interrupted prepared operations have release tests. Recursive COPY measures and
  revalidates a bounded traversal fingerprint and atomically stores its expected measurement with at
  most one durable cursor row per path depth. Each synced file or completed directory advances that
  cursor in a short SQLite operation; startup resumes prepared directory copies before generic prepared
  rollback, verifies the final accumulated byte/count/fingerprint measurement, and only then publishes
  the private root. Cursor checkpoint and restart-resume release tests cover both boundaries.
- Backup upload cancellation is the first production DB-plus-file workflow migrated onto the generic
  journal. One closed SQLite operation now changes the asset/session to `cancelled` and inserts an
  exact Backups-root `cleanup_pending` group plus write claim in the same immediate transaction. The
  route no longer opens a connection or removes the staged file directly; it wakes generic recovery,
  repeat cancellation reuses the existing result/group, and recovery performs the descriptor-relative
  delete, parent sync, terminal checkpoint, claim release, and fence retirement. An integration test
  verifies the product/journal atomic boundary and end-to-end cleanup. Device registration, upload
  creation/idempotent legacy-manifest upgrade, status, completion preparation/queueing, chunk
  claim/finalization/abandonment, and cancellation now also use closed SQLite operations; no backup
  route checks out r2d2 directly or retains a connection across file work. Route-level chunk writes,
  rollback truncation, sync, and completion hashing now use descriptor-relative Backups-root File I/O
  operations in at-most-1-MiB FIFO continuations, so a multi-gigabyte upload yields between chunks.
  The route now retains one move-only, generation-checked descriptor token across those continuations
  in the fixed `RuntimeSizing.file_registry_capacity` registry. A bounded coalesced close channel wakes
  a File worker when an abandoned token needs physical rollback/sync/close; write errors and explicit
  aborts restore the durable offset, successful commit syncs before releasing the slot, and reads keep
  the same descriptor even if the path is renamed. Route upload and completion SHA-256 now use a
  move-only incremental CPU-executor state, returning each at-most-1-MiB buffer for the next file stage
  instead of hashing on the network/orchestration lane or buffering the complete file. Backup worker
  recovery, resumable-file and processing-asset enumeration, expiration, claim/transition, recovered-media
  lookup, and content-hash persistence now use bounded closed SQLite operations. Recovery truncation,
  existence checks, source modification time, staged-file hashing, and supplemental-metadata validation
  plus atomic publication now use durable File/CPU operations. Every terminal finalization transition
  commits the staged-file and sidecar cleanup journal in the same immediate transaction, so retry and
  recovery are idempotent. The shared local/WebDAV/backup `import_staged_file` boundary now receives
  only a typed storage root plus normalized relative path. It opens one generation-checked File session,
  streams its hash through bounded File/CPU continuations, retains and rewinds that same descriptor
  through content-hash claim deferral, and streams publication into a Journal-owned Originals temporary.
  Local recursive discovery now uses resumable descriptor-relative directory sessions. WebDAV readiness
  revalidation opens the exact typed path while holding its request mutation barrier. Successful
  local/WebDAV imports commit exact identity-fenced source and selected-sidecar cleanup entries with the
  media transaction; backup deliberately leaves source cleanup to its existing asset-transition cleanup
  group so the two owners cannot conflict. The obsolete process-local `.processing` rename/restore tree
  and its startup migration path have been removed, consistent with the reset-from-scratch schema contract.
  The process-local content-hash mutex map has been removed and replaced by the fresh-schema
  `import_content_hash_claims` table with token-fenced acquire/release and pre-listener orphan recovery.
  A collision returns a typed deferred value; Local, WebDAV, and backup orchestration release their
  durable admission, yield to the scheduler FIFO tail, and reacquire only before resuming. The deferred
  value retains its computed preparation instead of hashing the source again. Media allocation/
  finalization, access, metadata-job creation, duplicate absorption, content-hash claims, interrupted-row
  reconciliation, and source-cleanup journal creation are closed SQLite operations. Supplemental sidecar
  publication is also Journal-owned: a replacement first identity-fences and moves the previous canonical
  sidecar to a recorded tombstone, publishes the bounded replacement, then schedules tombstone cleanup;
  a first sidecar uses the same journal without the replacement move. Staged cleanup ownership is now
  explicit for the primary source and supplemental sidecar: local/WebDAV imports own both, ordinary
  direct imports may retain the source while consuming the sidecar, and backup owns neither because its
  asset transition commits both cleanup entries. Backup worker errors now propagate after all concurrently
  claimed assets release their resources instead of being swallowed; this exposed and removed a duplicate
  sidecar-claim conflict that could leave an asset in `processing`. New-media import now prepares one
  product-targeted Journal group containing the canonical original and optional sidecar, publishes both
  to `files_committed`, and atomically switches the media row, access, metadata job, product target, and
  source-cleanup journal in one SQLite transaction fenced by the live content-hash claim. A crash or
  database failure before that switch detaches the import product and forward-cleans every published
  destination; startup never accepts an unjournaled original merely because a filename exists. Existing-
  media duplicate sidecar replacement remains a separate Journal operation because it updates an already
  visible canonical product rather than participating in a new media-row publication.
- Permanent trash deletion is now another production committed-cleanup workflow. Removing a trashed
  access, deciding whether it was the final owner, deleting the media/cascade rows, removing empty face
  groups, and inserting every exact/subtree cleanup entry and write claim commit in one immediate SQLite
  transaction. Shared media loses only the requesting user's trashed access. The cleanup journal owns the
  canonical original, canonical sidecar, three thumbnail variants, deterministic
  `previews/media/<media-id>/`, face crops, AI inputs, and the legacy clustering frame. Routes and expired
  startup cleanup use bounded closed SQLite pages and wake generic recovery; the old direct deletion
  helper, filename glob, and `glob` dependency are gone. Non-browser-compatible image previews now use
  one deterministic media-ID path instead of per-user filename copies. Release tests cover atomic
  database/journal publication, shared ownership, invalid active access, similarity dirty propagation,
  and multi-continuation recursive cleanup.

## Goal

Momento API will have one scheduler thread and three fixed execution domains:

1. CPU workers for CPU-heavy application work.
2. I/O workers for network and filesystem work.
3. SQLite executor workers that run every SQL operation using r2d2 connections.

The scheduler routes work but does not execute business logic. A worker from one domain never blocks
while waiting for another domain. The async orchestration future submits one bounded operation, awaits
its owned result, and then submits the next operation.

Business jobs are independent: one job never owns or waits on another job's in-memory mutex, permit,
future, or feature queue. Their only mutable product-level shared state is the bounded log sink and
SQLite state reached through their executors, plus the explicitly serialized administrator config-file
publication path. Scheduler admission counters, executor FIFOs, the file
handle/capacity registries, and immutable configuration/geocoder snapshots are infrastructure ownership,
not cross-job business coordination; their critical sections are bounded and never span I/O or another
executor. Content identity, authentication attempts, path conflicts, cancellation, grouping progress,
and recovery coordination therefore live durably in SQLite instead of process-global keyed maps.

This design deliberately replaces the old model in which one worker permit tried to represent HTTP,
CPU, filesystem, database, and durable-job capacity at the same time.

## Required Configuration

Replace `[thread_pool].available_workers` with three required values:

```toml
[thread_pool]
cpu_workers = 8
io_workers = 8
sqlite_workers = 4
```

- `cpu_workers >= 1`.
- `io_workers >= 4`.
- `sqlite_workers >= 1`.
- The hard parser limits are `MAX_CPU_WORKERS = 256`,
  `MAX_IO_WORKERS = 256`, and `MAX_SQLITE_WORKERS = 64`. Startup rejects values outside those closed
  ranges before creating a channel, buffer, runtime, r2d2 pool, or thread. `RuntimeSizing` computes
  every derived count with checked arithmetic in a wider integer type, performs fallible conversion to
  platform `usize`, and validates every allocation before the first worker is spawned. Overflow,
  conversion failure, or allocation failure is a typed startup error rather than a panic or partial
  runtime.
- The worker maxima are syntax guards, not permission to reserve arbitrary memory. Runtime-owned
  threads use `WORKER_STACK_BYTES = 2 MiB`, and `RuntimeSizing` also computes the deterministic
  worst-case reservation for thread stacks, every 1 MiB stream/pipeline chunk, retained child
  diagnostics, bounded request/log capture, the complete bounded log-event ring, executor and scheduler channel envelopes, file registry,
  and bounded LLM transport state. The source-owned limit is
  `MAX_DERIVED_RUNTIME_BYTES = 1 GiB`; a configuration whose checked worst case exceeds it fails before
  worker creation even when each individual worker count is below its maximum. External model-process
  memory and unavoidable allocator bookkeeping are reported separately, but fixed SQLite page/statement
  caches for `sqlite_workers + 1` possible live/replacement connections are included. No queueable application DTO or byte buffer may have an unbounded payload omitted
  from this calculation; the declared 8 MiB per-operation SQLite temporary heap is included. A count inside its parser range is usable only when the combined derived
  reservation also passes; a rejection reports the complete per-category byte breakdown and the
  maximum feasible value for each worker field while the other two configured values remain fixed.
- `RuntimeSizing` models ownership phases, not the impossible sum of mutually exclusive payloads. Its
  top-level checked value is `max(bootstrap_peak, pre_listener_initialization_peak, running_peak)`; each
  peak includes every thread/long-lived object alive in that phase. Within `running_peak`, independent
  owners add, while one owner's closed state machine contributes the maximum of its legal states:

  ```text
  per_request_peak = max(encoded_body + parse_scratch + normalized_request_dto + log_capture,
                         business_request_dto + db_output + log_capture,
                         db_output + encoded_response + log_capture,
                         bounded_stream_setup + log_capture)
  per_cpu_worker_peak = max(OperationSpec::CPU_TEMP_AND_OUTPUT_BYTES for every CpuOperation)
  per_durable_owner_peak = max(result_record + normalized_record + persistence_batch + envelopes,
                               metadata_source + normalized_metadata + envelopes,
                               every other DurableOperationSpec phase)
  running_peak = long_lived_runtime_bytes
               + active_requests * per_request_peak
               + cpu_workers * per_cpu_worker_peak
               + durable_orchestrations * per_durable_owner_peak
               + independently admitted connection/stream/file/SQLite/transport states
  ```

  A phase transition is executable only when orchestration drops/returns the prior phase's large owned
  object before constructing the next one; type-state APIs consume the old state so two mutually
  exclusive buffers cannot coexist. Buffers that really may overlap remain in the same state formula.
  Compile-time state enumeration and allocation instrumentation fail if implementation-observed live
  ownership exceeds a declared state. This keeps the bound exact without either double-counting or
  assuming unrelated owners cannot peak together.
- The embedded reverse-geocoding asset is an explicit fixed runtime category rather than a lazy hidden
  allocation. `MAX_GEONAMES_COMPRESSED_BYTES = 4 MiB`,
  `MAX_GEONAMES_DECOMPRESSED_BYTES = 16 MiB`, `MAX_GEONAMES_RECORDS = 250,000`, and
  `MAX_GEONAMES_FIELD_BYTES = 255` are compile-time/source-owned bounds verified against the embedded
  asset. `RuntimeSizing` includes its at-most-4 MiB read-only image, one
  `MAX_GEONAMES_RUNTIME_BYTES = 48 MiB` persistent packed index/string arena, and
  `MAX_GEONAMES_INIT_TEMP_BYTES = 4 MiB` initialization scratch at the startup peak. Initialization
  streams gzip and TSV directly into fallibly preallocated fixed-layout nodes/string storage, counting
  decoded bytes, records, and fields before growth; it never uses `read_to_end` or a complete decoded
  copy. A source-owned packed 3D kd-tree is built in place with bounded stack depth, replacing `rstar`
  so a third-party bulk loader cannot allocate an unmeasured second index. Any asset bound/hash/format
  violation is a pre-listener startup error.
- Authentication cryptography is another explicit CPU category. Route validation enforces
  `MAX_AUTH_PASSWORD_BYTES = 1,024`; hashing and verification accept only the source-owned Argon2id
  parameters `m = 19,456 KiB`, `t = 2`, and `p = 1`. A stored encoded hash with different/overflowed
  parameters is unusable and takes the same prebuilt dummy-verification path rather than controlling
  process memory or native parallelism. The Argon2 `CpuOperation` contributes the library-proven
  `ARGON2_WORKSPACE_BYTES` plus bounded input/output scratch to `per_cpu_worker_peak`; all CPU slots may
  therefore verify concurrently without adding that mutually exclusive workspace to image-decode
  scratch. Startup constructs one dummy hash in a CPU operation before listener
  publication and stores it in immutable authentication state; there is no first-request `LazyLock`,
  hidden worker, or Rayon feature.
- Local metadata control files are bounded independently from original media size.
  `MAX_METADATA_SOURCE_BYTES = 4 MiB`, `MAX_NORMALIZED_MEDIA_METADATA_BYTES = 1 MiB`,
  `MAX_METADATA_FIELDS = 4,096`, `MAX_METADATA_NESTING_DEPTH = 16`, and
  `MAX_METADATA_STRING_BYTES = 256 KiB` apply separately to the selected Google supplemental sidecar
  and exiftool's non-binary JSON output. Metadata parses the two sources sequentially into one typed DTO,
  so at most one encoded buffer plus one normalized DTO coexist. That pair is the metadata member of
  `per_durable_owner_peak`; the enclosing maximum permits every durable slot to hold it while waiting
  for CPU FIFO admission without also charging the mutually exclusive result-processing member. The
  exiftool child has matching `RLIMIT_FSIZE`; sidecar reads validate descriptor size
  before allocation. Typed depth/count/string visitors replace `serde_json::Value`, and oversize,
  malformed, or allocation failure becomes an explicit metadata-job failure with bounded logging—not
  truncation or thumbnail/original fallback.
- `RuntimeSizing` also calculates `required_open_files` with checked arithmetic before thread creation:
  every admitted inbound connection; the maximum outbound sockets implied by request, durable, and
  outbound-session admissions; the complete dynamic file registry and fixed typed-root/process-lock
  descriptors; `3 * (sqlite_workers + 1)` for worst-case main/WAL/SHM SQLite descriptors including
  the one permitted replacement-overlap connection; listener and LLM
  transport sockets; one close/wakeup descriptor per executor worker where the platform implementation
  uses one; and the audited parent/child diagnostic, pidfd, and allowlisted-descriptor spawn peak for
  every CPU worker. A closed `PlatformFdSpec` owns the small fixed epoll/eventfd/timer/signal descriptors
  used by the two-thread Tokio runtime and scheduler; adding a platform handle changes the formula/test.
  A separate closed `BootstrapFdSpec` enumerates the startup-only root capability, resolver and
  timezone discovery descriptors, the config bootstrap/revalidation descriptors, private
  fresh/recovery database files, schema/WAL activation, mount/rename probe, and
  temporary child/process setup peaks in their actual phase order. Startup reads the soft
  `RLIMIT_NOFILE` without raising it and fails before spawning when it is below
  `required_open_files`, reporting required, available, and each category. No undocumented percentage
  headroom substitutes for an enumerated owner, and runtime admission counts prevent exceeding the
  proven maximum. The source formula is:

  ```text
  active_outbound_network_operations = active_requests + durable_orchestrations
                                     + active_outbound_stream_sessions
  max_outbound_sockets = 2 * active_outbound_network_operations  # service + async DNS worst case
                       + 1                                      # shared LLM WebSocket
  runtime_required_open_files = active_connections + max_outbound_sockets + 1  # HTTP listener
                              + FILE_REGISTRY_CAPACITY
                              + StorageRootId::COUNT + 1       # data-dir lock capability
                              + ConfigFileCapability::FD_COUNT # exact config-parent capability
                              + 3 * (sqlite_workers + 1)       # one replacement-overlap connection
                              + CHILD_SUPERVISION_FD_PEAK * cpu_workers
                              + PlatformFdSpec::FIXED_FDS
  required_open_files = max(runtime_required_open_files, BootstrapFdSpec::PEAK_FDS)
  ```

  The optional Static root is a real `StorageRootId` variant and is included even when absent, making
  the result conservative without a runtime branch. `CHILD_SUPERVISION_FD_PEAK` is the compile-time
  maximum additional parent descriptors for the closed process-descriptor enum, not a configurable
  estimate.
- `RuntimeSizing` charges one long-lived at-most-`MAX_NORMALIZED_CONFIG_BYTES` current ConfigManager
  snapshot plus one serialized update state of
  `2 * MAX_CONFIG_FILE_BYTES + 2 * MAX_NORMALIZED_CONFIG_BYTES` (raw current bytes, output bytes,
  edited TOML arena, and prospective typed snapshot), its bounded request DTO/lease, and the exact
  config capability/temporary metadata envelopes. The mutation lease
  permits only one such state, so active HTTP request cardinality cannot multiply this amount.
- The LLM-result member of `per_durable_owner_peak` is
  `MAX_LLM_RESULT_RECORD_BYTES + MAX_NORMALIZED_RESULT_RECORD_BYTES +
  MAX_LLM_RESULT_PERSIST_BATCH_BYTES + RESULT_EXECUTOR_ENVELOPE_BYTES`, with
  `MAX_NORMALIZED_RESULT_RECORD_BYTES = 2 MiB` and
  `MAX_LLM_RESULT_PERSIST_BATCH_BYTES = 4 MiB`. `RESULT_EXECUTOR_ENVELOPE_BYTES` is not an estimated
  literal: it is the compile-time sum of `size_of::<SchedulerEnvelope>()`, the maximum of
  `size_of::<FileReadResultRecordCommand>()`, `size_of::<NormalizeResultRecordCommand>()`, and
  `size_of::<PersistResultRecordCommand>()`, plus `size_of::<ResultCompletionEnvelope>()`. Heap-owned
  record, normalized DTO, and persistence-batch bytes are excluded from that formula because the other
  three terms charge them explicitly. Compile-time assertions and the startup sizing report use the
  same formula, so adding a field to any envelope changes the enforced budget. The enclosing durable
  maximum deliberately permits every global durable slot to be in this LLM-result phase; there is no
  hidden per-feature window. Inbound receive
  frame buffers are counted separately by `active_inbound_durable_streams * 64 KiB`. Checked startup
  sizing rejects a worker combination whose complete worst case exceeds `MAX_DERIVED_RUNTIME_BYTES`.
- r2d2 `max_size` is exactly `sqlite_workers`.
- r2d2 receives one explicitly constructed service-owned scheduled pool with
  `R2D2_MAINTENANCE_THREADS = 1`. This source constant is not a fourth config worker count. The thread
  may create/validate replacement connections through the budget-aware manager but never executes
  business SQL. Runtime status reports it separately from the configured scheduler/CPU/I/O/SQLite
  counts; its implementation-owned stack/allocator reservation is reported in the infrastructure
  section rather than falsely attributed to an application worker.
- The three worker fields are startup-only. `RuntimeBuilder` copies them into one immutable
  `RuntimeSizing`/runtime status snapshot; no scheduler or executor rereads them from a later
  `ConfigManager` revision. Runtime configuration endpoints do not accept these fields, and status/help
  reports that changing the TOML requires process restart. This refactor adds no partial live resize,
  pending mixed topology, or compatibility alias for `available_workers`.
- Queue capacity is derived in code as four queued operations per worker; it is not configurable.
  Every executor queue is strict FIFO. A completed operation that has more work must submit a new
  operation at the queue tail rather than retaining its worker or inserting at the head.
- Network admission limits are also derived in code rather than exposed as tuning knobs:
  `active_connections = 16 * io_workers`, `active_requests = 8 * io_workers`, and
  `active_stream_sessions = 8 * io_workers`. At most half the connection budget may become long-lived
  upload/download sessions, leaving derived connection capacity for ordinary API/control traffic.
  A request consumes request admission only through parse, authentication, validation, and
  pre-stream journal/response setup; it then atomically converts that admission into a stream-session
  guard and releases the request slot. Session count bounds long-lived orchestration futures but does
  not reserve a file worker or file-chunk slot.
- The HTTP parser has source-owned amplification bounds included in `RuntimeSizing`:
  `MAX_HTTP_HEADER_BYTES = 64 KiB`, `MAX_HTTP_HEADER_COUNT = 128`,
  `MAX_HTTP_URI_BYTES = 8 KiB`, and `MAX_BUFFERED_HTTP_BODY_BYTES = 1 MiB`. Hyper rejects headers/URI
  before Axum allocation exceeds those limits. JSON, form, multipart-control, PROPFIND, and every other
  non-media control extractor uses the one buffered-body limit with checked preallocation. Declared or
  chunked media upload/download bodies never pass through that extractor: after auth/validation they
  convert to stream-session admission and use the existing 1 MiB chunk path, so this limit cannot reject
  a progressing 20 GiB media transfer. A route is audit-classified as `BufferedControl` or
  `StreamingMedia`; no handler may silently call an unbounded body collector.
- Decoded control structure has its own amplification bound:
  `MAX_CONTROL_NESTING_DEPTH = 32`, `MAX_CONTROL_COLLECTION_ITEMS = 8,192`,
  `MAX_CONTROL_OBJECT_FIELDS = 1,024`, `MAX_CONTROL_STRING_BYTES = 256 KiB`, and
  `MAX_NORMALIZED_CONTROL_DTO_BYTES = 1 MiB`. Route-specific typed specs may be smaller but never
  larger. CPU parsers use depth/count/length-checking visitors and a fallible charged arena; they do
  not first construct `serde_json::Value`, an unbounded map/vector, or a complete XML DOM. Form and
  multipart-control field/part counts use the same registry. XML DTD/entity/external-resource expansion
  is disabled, duplicate JSON/form object fields are rejected rather than last-write-wins, and total
  decoded text remains within the string/DTO bounds. Exceeding a structural
  bound returns typed 400/413 before the offending allocation. The request parse state charges the
  encoded body, maximum normalized DTO, parser scratch, and log capture together; the encoded body is
  consumed before a later database/response state.
- Pre-handler HTTP connection state is closed and budgeted as well. The existing server remains
  HTTP/1.1-only; this refactor does not enable Axum's optional HTTP/2 feature, h2c upgrade, or
  prior-knowledge path. `HTTP1_READ_BUFFER_BYTES = 128 KiB` covers the independent URI/header limits
  plus framing, and each connection admits only one parsed/in-flight request; it does not read ahead or
  retain a pipeline of later requests. `RuntimeSizing` charges that complete buffer/protocol state for
  every admitted connection in addition to global active-request DTOs. The no-idle-pool outbound HTTP
  connector and LLM WebSocket upgrade are also forced to HTTP/1.1, so a negotiated HTTP/2 connection
  cannot introduce hidden multiplexed stream/window state. Request `Content-Encoding` other than absent/`identity` receives 415 before body
  consumption, and automatic response-compression middleware is removed; no transparent decoder or
  encoder may create an uncharged buffer or CPU workload on a network thread. A large upload's small
  socket backpressure is released as its charged chunks complete, so this is a memory bound rather than
  a total-size or duration limit.
- Ordinary API responses use `MAX_BUFFERED_HTTP_RESPONSE_BYTES = 4 MiB`, represented in every active
  request's response phase rather than added to its earlier body/parse phase. Collection endpoints
  declare a source-owned page maximum and cursor and serialize through a
  fallible bounded writer; reaching the bound before the page is complete is an invariant/test failure,
  not silent truncation. Media, static files, backups, and explicit exports are `StreamingResponse` and
  emit bounded chunks without first constructing a complete body. The route audit updates frontend/API
  callers in both the web GUI and `src/android/app` for any newly paginated endpoint and rejects a
  handler or client accumulator with an unbounded `Vec`/JSON collection. Android request/response
  contract changes use the mandatory root Docker build entrypoint for verification.
- A WebDAV PROPFIND request remains bounded control input, but its multistatus response is always a
  `StreamingResponse`. The replacement WebDAV implementation enumerates at most
  `FILE_IO_ENTRY_BATCH = 256` entries per file operation, serializes each validated response element
  directly into the ordinary charged streaming chunk, and retains only the bounded traversal cursor;
  it never materializes a complete directory tree or XML document. `Depth: 0`, `Depth: 1`, and the
  existing infinity behavior share that path. Client disconnect cancels the read-only traversal and
  releases its stream admission without creating recovery work.
- File streaming capacity is separately derived as
  `active_file_chunks = 2 * (io_workers - 2)`. A normal progressing stream may own at most two 1 MiB
  chunk objects total: one executing and one queued. Each upload chunk acquires and retains a global
  file-chunk debit before the network read allocates its buffer and through the queued/write operation;
  it releases that debit with the buffer after completion. A download chunk holds the debit while
  queued/reading, then atomically transfers its buffer into the session response state and releases the
  debit. A response buffer waiting on a slow peer prevents that session from prefetching another chunk,
  so a slow peer retains one bounded buffer but no file-chunk debit. There is no additional uncharged
  session byte buffer. Runtime sizing charges all `active_file_chunks` buffers plus one possible
  detached response buffer per stream session, making the two-chunk per-stream and global bounds
  simultaneously conservative.
- Long Momento-originated streams use a separate generic, source-derived admission count,
  `active_outbound_stream_sessions = 2 * (io_workers - 2)`. It is shared by all durable outbound
  byte-stream operations rather than configured per feature; LLM submission is its current user. One
  outbound session follows the same charged two-chunk ceiling and owns one scheduler ingress credit,
  but retains at most one detached bounded buffer and no CPU, SQLite, file-chunk, or
  durable-orchestration slot while waiting for network progress.
- Inbound durable WebSocket byte streams use
  `active_inbound_durable_streams = cpu_workers + sqlite_workers`. `MAX_CONTROL_MESSAGE_BYTES = 1 MiB`
  is the llm-service inbound bound needed for Momento submission manifests and cancellation chunks;
  Momento's reverse direction has
  `MAX_MOMENTO_INBOUND_LLM_CONTROL_BYTES = 64 KiB` because ready/ack/resultStart/receipt controls carry
  no ID array or media bytes. Both services validate direction and exact message-type encoded size
  before JSON allocation. Neither value is a result-size limit. Each admitted stream owns one
  scheduler ingress credit, one `StreamSession` file-token debit, and at most one
  `LLM_STREAM_FRAME_BYTES = 64 KiB` body buffer while it writes unchanged bytes into Momento-owned
  journal storage. A source-owned `MAX_LLM_RESULT_BYTES = 1 GiB` bounds one declared result and is
  validated before `resultReady`; `MAX_LLM_RESULT_RECORD_BYTES = 1 MiB` bounds each independently
  parseable length-delimited record. `momento-result-records-v1` is an exact endian-stable binary
  format: each record begins with a 24-byte header containing little-endian `u32 totalLength`, `u8
  version`, `u8 kind`, `u16 flags`, `u32 recordSequence`, `u32 inputSequence`, `u32 payloadLength`, and
  `u32 crc32c`; the declared payload follows immediately, and `totalLength = 24 + payloadLength`.
  CRC32C covers the header except its `totalLength` and `crc32c` fields plus the payload. Payloads use
  only fixed-width little-endian numbers and length-prefixed UTF-8 or byte fields; there is no serde,
  native-width integer, implicit padding, or recursive container on disk or wire. Therefore
  `MIN_LLM_RESULT_RECORD_BYTES = 24`,
  `MAX_LLM_RESULT_RECORDS = 1_000_000`, and
  `MAX_LLM_RESULT_CONTINUATIONS_PER_VALUE = 4` bound scheduling/row amplification as well as bytes.
  OCR/text/tag tasks additionally enforce
  `MAX_LLM_LOGICAL_TEXT_AGGREGATE_BYTES = MAX_NORMALIZED_RESULT_RECORD_BYTES = 2 MiB` across all inputs
  for one job. Logical values larger than one record use explicit ordered continuation records rather
  than truncation, but a provider aggregate above 2 MiB is an explicit failed inference result before
  transport commit. The 1 GiB total-result allowance remains available to large cardinalities of
  independently persistable bounded records such as faces; it never implies a 1 GiB SQL cell or DTO.
  Each task has one closed record-kind and
  ordering state machine; unknown kinds, duplicate terminal markers, invalid input transitions, and
  task-incompatible records are rejected before durable receipt. Before allocating, decoding validates
  every field length and element count with checked arithmetic and a fallible reservation. One
  exhaustive `RecordKindSpec` table defines each kind's legal payload fields,
  `MAX_ENCODED_PAYLOAD_BYTES`, and `MAX_NORMALIZED_HEAP_BYTES`; compile-time checks prove
  `24 + MAX_ENCODED_PAYLOAD_BYTES <= MAX_LLM_RESULT_RECORD_BYTES` and
  `MAX_NORMALIZED_HEAP_BYTES <= MAX_NORMALIZED_RESULT_RECORD_BYTES` for every kind. The global 2 MiB
  normalized bound is the maximum of that table, not an assumed relationship to the 1 MiB encoding.
  This accommodates bounded multi-input text and high-cardinality face-embedding results without
  buffering the complete result or silently truncating an ordinary result
  cardinality. LLM results are the current user, but the admission class is transport-generic rather
  than model concurrency. Every control envelope, frame, record DTO, stream state, file token, and
  scheduler credit is included in `RuntimeSizing`.
- The existing outbound LLM submission contract becomes explicitly sizing-safe as part of the same
  shared DTO audit: `MAX_LLM_INPUTS_PER_JOB = 1_024`,
  `MAX_LLM_JOB_INPUT_BYTES = 32 GiB` across all descriptors, `MAX_LLM_INPUT_FILENAME_BYTES = 255`,
  `MAX_LLM_MIME_TYPE_BYTES = 127`, and `MAX_LLM_INPUT_KIND_BYTES = 63`. Each individual and the checked
  sum must be nonzero/within 32 GiB. Both services validate JSON array count and every string length
  before allocating descriptor storage, preserve ordered sequence uniqueness, and reject overflow or an
  input sequence outside the submitted manifest. Current metadata still emits one original/full-
  resolution-frame input; the bound exists for the wire/runtime proof, not to create task copies. Inputs
  are streamed/inferred sequentially within a job as today, so the 32 GiB total is never buffered.
  Admission returns the exact input sequences absent from llm-service's content-addressed store, so
  Momento does not retransmit a large source already held for another active task. llm-service stores
  one unchanged source per SHA-256 hash and hard-links it into every job. Supported camera RAW formats
  are normalized once at full resolution through LibRaw into a content-hash-cached TIFF below the
  llm-service mounted data root; active jobs hard-link that normalization, and final job cleanup removes
  the source and normalized cache when their last reference disappears. RAW conversion errors become
  durable failed inference results delivered to Momento, not corrupt-queue failures.
- Disk admission has no per-feature concurrency or queue-size knob. Bootstrap derives one shared
  `DataDirSpaceBudget` from the filesystem containing `server.data_dir`. It has Journal and SQLite
  allocation owners sharing one data-capacity domain, a quota-bounded Log owner, and a
  normal-runtime-unallocatable recovery floor; it is source-owned
  and adds no config fields:

  ```text
  recovery_floor = min(5 GiB, filesystem_total_bytes / 20)
  operational_bytes = filesystem_total_bytes - recovery_floor
  log_quota = min(2 GiB, operational_bytes / 100)
  shared_data_hard_limit = operational_bytes - log_quota
  sqlite_wal_limit = min(2 GiB, shared_data_hard_limit / 4)
  ```

  The budget is one common reservation implementation, not three counters that are later reconciled.
  Every block-allocating Journal, SQLite, or log operation must first acquire a generation-checked,
  move-only (non-`Clone`)
  `SpaceReservationToken { reservation_id, class, peak_additional_bytes, generation }`. The Journal
  class covers every write-producing journal group: HTTP/WebDAV uploads, local and
  backup import copies, originals, metadata/thumbnail/preview outputs, external-tool outputs, AI result
  receipt, face crops, tombstones that require new bytes, and cleanup/rollback staging. Rename-only
  operations are not assumed to be allocation-free: each newly created destination/tombstone/temp
  directory entry reserves
  `FILESYSTEM_ENTRY_METADATA_BYTES = 4 * validated_statvfs_fragment_size`, rounded and checked in the
  same Journal peak. A bounded group multiplies this by its maximum simultaneously retained new entries;
  incremental tree work extends before each next entry. The factor is a source-owned supported-
  filesystem safety bound verified by the startup mutation probe and stress tests. Only an exchange
  where both directory entries already exist, a same-entry metadata-neutral rename proven by its closed
  action spec, or pure unlink/rmdir cleanup reserves zero additional filesystem bytes. SQLite/database
  growth and Journal file growth compete for the same currently available data capacity while retaining
  distinct owner accounting; neither may consume `recovery_floor` or the retained Log remainder.
  Bootstrap prunes expired/oldest rotated logs to `log_quota` before admission; failure to make
  them fit is also a startup error. External filesystem use is not classified as Momento capacity and
  automatically reduces admission through the fresh free-space snapshot.
- Reservation capability lifetime has two explicit, non-interchangeable states. A newly issued
  `ProvisionalSpaceToken` is in-memory only, permits no bytes before owner preparation, and releases its
  unused aggregate on Drop. The owner-prepare SQLite transaction consumes it through
  `commit_to_durable_owner`, creates the matching `data_dir_space_reservations` row, and returns a
  move-only `DurableSpaceCheckout { reservation_id, owner_kind, owner_id, generation }`. At most one
  checkout exists for that reservation. It can be moved through bounded file/SQLite operations and
  reacquired only after validating the durable owner/version. Dropping it—reply loss, panic,
  cancellation, or ordinary stage yield—only clears the in-memory exclusive-checkout flag; it never
  reduces ledger outstanding bytes or deletes the durable row. A later owner/recovery continuation can
  reacquire it. The sole release path consumes a checkout after one SQLite transaction has verified
  the terminal owner/cursor and recorded release intent and after file workers have published the
  required allocation/deletion/parent-sync evidence; it then removes the durable obligation and
  advances the ledger epoch exactly once. Short SQLite and Log reservations that never become durable
  remain provisional/ephemeral and may release on Drop only after any physical allocation has first
  been published. `SpaceReservationToken` is the private common implementation detail of these typed
  wrappers, not a caller-visible state that can silently switch semantics.
  Incremental unknown-size growth uses the analogous `extend_durable_owner`: it acquires a provisional
  delta, consumes that delta in the same compare-and-set transaction that increases the durable row's
  peak/generation, and only then returns the next checkout allowed to read/write that chunk. Transaction
  rollback or a lost pre-commit reply releases the provisional delta; a committed-but-lost reply leaves
  the increased durable row authoritative for idempotent reacquisition, so neither branch leaks or
  double-spends capacity.
- `DataDirSpaceBudget` owns one short critical section shared by the file executor, SQLite executor,
  and logger infrastructure. Its in-memory `LedgerSnapshot` stores checked O(1) aggregates for SQLite
  allocated/outstanding bytes, Log allocated/outstanding bytes, and Journal outstanding additional
  bytes. Capacity observation runs `statvfs`, reads that fixed-size snapshot, and
  issues/extends/materializes/releases a token by checked aggregate delta under one lock and epoch; it
  never scans reservation rows or journal entries on an admission path. The lock may cover the bounded `statvfs`/metadata snapshot but is never held across a database
  transaction, file read/write/sync, checkpoint, or log append. An owner keeps its full outstanding
  reservation while performing I/O; after I/O, it reacquires the lock, publishes the new allocated
  blocks, and only then reduces or releases the reservation. If an admission snapshot lands between the
  write and that publication, it still sees the full reservation and is conservative. File workers
  perform Journal/Log capacity observation. SQLite workers and the budget-aware r2d2 manager may invoke
  only the ledger's SQLite-class `statvfs` plus main/WAL/SHM allocation observation as the declared
  database-infrastructure I/O exception; they never call `FileIoExecutor` or wait on another executor
  to begin/finish a database operation. No business SQL, route, processor, or event-producing logging
  code can access the ledger internals or use this exception for an application path.
- Ledger methods do not log, call user/business callbacks, allocate an unbounded collection, or acquire
  a scheduler, SQLite connection, file-registry, or logger lock while holding the budget lock. They
  return a typed owned error, release the lock, and only the caller's boundary logs it. This prevents
  the logger's own reservation failure from recursively entering the ledger. Startup reconstruction is
  the only full durable scan: while admission is closed, SQLite pages at most 256 reservation rows and
  file workers stat at most 256 referenced entries per continuation, accumulate checked totals, and
  publish one final epoch only after owner evidence and the recomputed sums match. Runtime row changes
  adjust the aggregates in the same typed transition that changes the token; periodic audit may page
  durable rows outside the lock and compare against a captured epoch, but cannot repair live counters
  or block admissions with an unbounded critical section.
- Reconstruction uses checked aggregate arithmetic and publishes explicit ledger health:
  `Healthy`, `LogOverQuota`, or `ExternalDeficit`. Existing SQLite allocation has no independent quota;
  it is ordinary already-consumed filesystem space, while reconstructed unfinished SQLite reservations
  remain protected in the common data-capacity invariant.
  `LogOverQuota` admits only zero-growth deletion of oldest closed rotations plus affected-parent sync,
  remeasures after every bounded batch, and cannot issue a Log append/rotation or Journal token. Once
  measured log allocation is at or below quota, the ledger publishes a new healthy/deficit epoch and
  only then uses checked nonnegative class remainders. `ExternalDeficit` likewise allows only declared
  zero-growth cleanup/recovery; normal allocation remains closed until the invariant is restored.
- `RecoveryOnly` has one exception that gives the floor a real recovery purpose without making it an
  operational fourth class. At most one move-only `RecoverySpaceToken` may exist, it is issued only by
  bootstrap, and it authorizes only the exact checked `SqliteRecoveryFootprintSpec` needed to recover,
  checkpoint, and sync a pre-existing database/WAL/SHM enough to read durable reservation ownership.
  The spec is derived from page size plus allocated main/WAL/SHM extents, includes simultaneous
  checkpoint peak and SHM recreation, and must fit both current free space and `recovery_floor`; otherwise
  startup reports the exact additional bytes required and performs no write. No journal, log, schema,
  business, or runtime operation can obtain this token. After recovery, bootstrap republishes measured
  SQLite allocation, destroys the token, loads the reservation rows, and reconstructs the ordinary
  ledger before entering `RecoveryReady`. A crash during this bounded recovery repeats the same
  filesystem-derived calculation; it never assumes the prior in-memory token survived.
- The invariant checked before every new allocation is:

  ```text
  free_bytes >= recovery_floor
    + (log_quota - log_allocated_blocks)
    + sum(journal_reserved_peak_additional_bytes - journal_newly_allocated_blocks)
    + sum(sqlite_reserved_peak_additional_bytes - sqlite_newly_allocated_blocks)

  log_allocated_blocks + log_outstanding_reservations <= log_quota
  ```

  Journal and SQLite reservations increase their respective terms and therefore cannot spend the same
  free blocks. Log reservations consume only the protected Log remainder. If external use has already
  broken the first invariant, every new allocation is
  rejected until deletion or capacity restoration; non-allocating cleanup remains eligible. Checked
  arithmetic and one epoch make a mixed old-free/new-counter snapshot impossible.
- Capacity admission returns one exhaustive outcome:

  ```text
  Fits(token)
  TemporarilyUnavailable(required_bytes, available_bytes)
  ExceedsHardLimit(required_bytes, class_limit_bytes)
  ```

  Current free space, another live reservation, a blocked checkpoint, or external consumption is
  temporary. Checked overflow, a Log request larger than its fixed quota, or one Journal/SQLite peak
  larger than `shared_data_hard_limit` is permanently impossible
  for this process/filesystem sizing. HTTP maps these explicitly to its request error policy; durable
  work retries only Temporary, and LLM receipt maps Temporary to Deferred and HardLimit to Rejected.
- The SQLite executor owns bounded growth for `database.sqlite`, its WAL, and SHM files. SQLite has no
  fixed storage quota and Momento leaves the native `PRAGMA max_page_count` unchanged. The source-owned
  `sqlite_wal_limit = min(2 GiB, shared_data_hard_limit / 4)` bounds transient WAL growth. Every write `DbOperation` declares a checked
  `max_growth_bytes` derived from its already bounded input DTO; operations without a finite bound do
  not compile into the closed operation enum. Before a potentially growing transaction, the executor
  acquires a SQLite-class `SpaceReservationToken`, so concurrent SQLite and Journal writers cannot all
  pass the same physical-capacity check. It then checks main/WAL/SHM allocated blocks and the declared peak. If the WAL would cross its limit, it
  runs a bounded checkpoint before starting; a blocked checkpoint or insufficient free capacity is a typed
  transient capacity error, so the operation releases its connection/resources and returns to the
  source tail. Transactions remain page/batch bounded, and a post-transaction allocation check treats
  an exceeded declaration as an invariant failure. After every write/checkpoint it atomically updates
  the shared ledger's SQLite allocation and releases the matching token; it does not publish an
  independently timed counter.
- r2d2 uses a budget-aware connection manager. Bootstrap eagerly opens, configures, and validates exactly
  `sqlite_workers` connections under a checked `SqliteConnectionBootstrapFootprintSpec` before
  readiness, so no first request performs lazy database initialization. Any later broken-connection
  replacement first acquires a short SQLite-class token covering connection-open WAL/SHM/cache metadata
  growth, opens/configures the connection, publishes measured allocation, and releases the token before
  making the connection available. Temporary capacity/open failure leaves the pool below target and
  produces ordinary checkout timeout/retry; it cannot create an unbudgeted connection or spin in an
  application worker. r2d2's permitted maintenance context may call only this manager and ledger API,
  never business SQL, the scheduler, logger file sink, or another executor.
- `DbOperation` declares one explicit `SqliteCapacitySource`; there is no default or inferred branch:

  ```text
  Fresh { token }
  ProvisionalParent { token, first_child_max_growth_bytes }
  DurableParentConsume { reservation_id, parent_generation, child_max_growth_bytes }
  DurableParentRolling { reservation_id, parent_generation, child_max_growth_bytes }
  ```

  `Fresh` adds a new SQLite-class obligation as above. `ProvisionalParent` is used only by the transaction
  that creates a durable owner: its already-counted in-memory token authorizes the bounded first child,
  and commit atomically writes owner evidence, persists the parent reservation with its post-child
  remaining bound, and advances the first cursor. Rollback releases the entire provisional parent and
  leaves no durable row. `DurableParentConsume` atomically splits a bounded child
  from that parent's remaining reservation without increasing class outstanding bytes, and only the
  matching owner/job/version may use it. The same SQLite transaction that commits the bounded business
  rows also advances the durable processing cursor and decreases the parent's worst-case remaining
  bytes by that stage's declared bound. Rollback returns the entire child to the parent. A committed
  operation publishes resulting allocation before another child is issued; unused child capacity
  becomes ordinary unreserved SQLite capacity.
  `DurableParentRolling` is permitted only after a result has transactionally shrunk to
  `RESULT_CLEANUP_RECOVERY_MAX_GROWTH_BYTES`. It lends that same bound to one cleanup transaction at a
  time, commits the cleanup cursor but does not consume the next-batch reserve, and publishes any
  resulting allocation before lending again. Because allocation plus the still-outstanding rolling
  peak is checked against shared physical capacity, later batches remain safe even if a prior batch left pages in
  main/WAL. The matching owner/version and one-live-child invariant prevent concurrent reuse; terminal
  cleanup releases the rolling parent. No construction/finalization operation may use Rolling, and no
  cleanup operation may use Fresh or Consume to silently reacquire capacity.
  Crash recovery derives the parent's exact remaining
  bound from its durable cursor and `ResultSqliteFootprintSpec`; an uncommitted WAL is checkpointed or
  rolled back under `RecoveryReady` before new children/admission. A provisional token used by any later
  operation, a missing parent, wrong generation,
  exhausted remainder, rolling-owner/cursor mismatch, concurrent rolling child, or child larger than
  the declared consume/rolling stage bound is an invariant
  failure rather than a second reservation or unbudgeted write.
- The logging infrastructure owns `log_quota` but owns no thread and performs no filesystem syscall on
  an event-producing thread. Request-body capture remains enabled but is source-bounded by
  `MAX_REQUEST_LOG_CAPTURE_BYTES = 48 KiB`; base64 payloads are redacted before the retained event is
  encoded. Buffered control requests do not parse the body twice: the same `CpuOperation` that parses
  the at-most-1 MiB JSON/form/XML body returns both its typed DTO (or typed parse error) and an at-most-
  48 KiB compact/redacted `RequestLogCapture`. Sensitive/base64 values are replaced while the parsed
  structure is already owned by that CPU worker. Binary/media and multipart streams retain only their
  fixed omission marker; they never copy a prefix of a 20 GiB body. The response middleware retains the
  already prepared capture and, after status/duration are known, performs only checked fixed-buffer
  event encoding plus a non-waiting ring offer—no JSON parsing, body mutex, CPU executor wait, or file
  I/O occurs on the response/network path. A structured event is encoded into a checked `MAX_LOG_EVENT_BYTES = 64 KiB` buffer and
  offered without waiting to the fixed
  `LOG_EVENT_QUEUE_CAPACITY = 32 * io_workers` ring. The existing bounded request-body capture and
  base64 redaction define that event bound; oversize fields are represented by their explicit
  truncation metadata rather than allocating a larger event. Queue saturation drops only the new log
  event, increments atomic dropped-by-level counters, and exposes one coalesced health diagnostic; it
  never blocks or fails business work. The ring's empty-to-nonempty transition increments one
  executor-local, non-lossy, coalescing wake counter and signals an existing sleeping file worker;
  producers neither wait nor enqueue a second wake while one is pending. The awakened worker consumes
  the observed generation before draining, and rechecks the ring before sleeping, so the signal/ring
  race cannot strand events. This wake primitive is included in the file executor's closed
  `PlatformFdSpec`; it is not a logger thread, scheduler command, or business queue entry. File workers
  attempt one non-waiting move-only `LogSinkLease` before drain; exactly one can own the append/rotation/
  console state, and a loser immediately returns to the business FIFO instead of waiting. The owner
  releases it after this one bounded batch, so no worker reserves future log work or interleaves file
  rotation. This internal sink-serialization capability is not a configured/semaphore business
  concurrency window. File workers drain at most `LOG_DRAIN_EVENTS = 64` and
  `LOG_DRAIN_BYTES = 256 KiB` after a business file operation and before sleeping, so every legal event
  fits one drain batch, sustained logging
  cannot starve the executor FIFO and sustained file work cannot starve logs indefinitely. Plain daily logs additionally rotate at the
  source-owned `LOG_ROTATION_BYTES = 128 MiB`, and the sink deletes the oldest closed rotations before an append that would
  exceed its quota. Request-body logging retains the existing bounded capture and base64 redaction; the
  quota changes retention, not the logging policy. Each append/rotation first reserves its bounded
  append plus simultaneous old/new rotation peak in the Log class, then publishes allocation and
  releases that token. If pruning or a real disk error cannot make a file append preserve its
  quota and `recovery_floor`, the file sink stops appending, emits one rate-limited console/fatal-health
  diagnostic, and never blocks a business worker or consumes recovery space. Bootstrap/fatal console
  output is the only direct logging I/O exception and is rate-limited; there is no
  `tracing_appender`/logger worker thread.

Console output uses the same ring rather than a synchronous tracing layer on event-producing threads.
Before the first bootstrap message, main leaves inherited stdout/stderr untouched and attempts to open
process-owned `O_NONBLOCK | O_CLOEXEC` handles through exact `/proc/self/fd/1` and `/proc/self/fd/2`.
It accepts only a validated TTY, pipe, or socket target; a regular-file target, missing `/proc`, unsafe
type, or open failure disables that console side rather than introducing a second blocking disk sink.
The at-most-two owned handles and their bootstrap peak are recorded in `PlatformFdSpec` and
`BootstrapFdSpec`; they close at shutdown and are never inherited by media children. A file-worker drain writes each already encoded event
to the daily file without ANSI and to stdout/stderr with the existing severity color policy when that
descriptor is a TTY. The ring stores one plain `EncodedLogEvent` plus validated timestamp/level/message
byte offsets; console coloring uses bounded `writev` ANSI prefix/reset segments around those same bytes,
so it does not allocate a second event string. Console writes are bounded to the event, handle partial writes only until the next
`EAGAIN`, and then increment dropped-console-by-level/bytes counters; a stalled pipe never blocks the
file worker or changes business/file-log success. Bootstrap, coordinated-fatal, and after-join final
diagnostics may bypass the ring only through the same bounded nonblocking write helper; failure is
counted when counters still exist and otherwise silently exits. No other `println!`, `eprintln!`,
tracing console layer, or direct stdout/stderr writer remains.
- A file worker issues a Journal-class `ProvisionalSpaceToken` after one serialized budget check; its
  reservation ID and peak-byte commitment are then consumed into durable ownership by the SQLite
  transaction that prepares the journal group before any bytes are read from a peer or written. Each
  later bounded mutation carries the matching `DurableSpaceCheckout`, never a newly inferred or
  release-on-Drop token. Known-size sources reserve their
  complete additional peak. Undeclared chunked bodies extend the reservation by one bounded chunk
  before reading that chunk; an extension becomes spendable only after the matching reservation-row
  version commits. Copy/replace accounts for simultaneous old, temporary, and final bytes.
  Derived/external outputs use a conservative source-owned maximum calculated from validated media
  geometry/format; the child receives `RLIMIT_FSIZE` for exactly that reservation and fails rather than
  exceeding it. An operation that cannot establish a safe bound is rejected before spawn or mutation.
- Each live reservation tracks `reserved_peak_additional_bytes` and
  `newly_allocated_blocks` beneath its recorded journal temporaries or newly created destinations.
  Pre-existing originals, sources, destinations, and tombstoned blocks are never reclassified as
  reservation materialization; rename does not change their ownership category. Only blocks allocated
  after the reservation was activated consume it. The common epoch formula above counts existing media
  and arbitrary external use through real free space, preserves the recovery floor and Log remainder,
  and counts each newly allocated Journal or SQLite block exactly once. File workers update
  `newly_allocated_blocks` after bounded write checkpoints under the short ledger lock, never while
  performing the write itself. On product commit, actual published bytes become ordinary used
  space as the reservation is released; on rollback/cleanup, the reservation remains associated with
  the group until deletion and parent sync complete. A real `ENOSPC` despite reservation is transient
  external interference, triggers durable handoff/rollback, and never permits another unreserved write.
  Startup stats every live journal entry, reconstructs the ledger from journal/inbox rows, and completes
  recovery before enabling any disk-producing admission.
- Network deadlines are fixed source-owned safety invariants rather than concurrency configuration:
  `HTTP_HEADER_TIMEOUT = 15 seconds`, `HTTP_BODY_IDLE_TIMEOUT = 60 seconds`,
  `HTTP_KEEP_ALIVE_IDLE_TIMEOUT = 30 seconds`, and `HTTP_RESPONSE_WRITE_IDLE_TIMEOUT = 60 seconds`.
  Idle deadlines reset only when bytes make progress, so a progressing 20 GiB transfer has no total
  duration limit. Protocol-owned long-lived WebSockets use heartbeat/liveness deadlines instead of
  the HTTP keep-alive deadline.
- Network idle time counts only while an orchestration future is actively waiting for progress from
  its peer. The body-read and response-write clocks pause while the future waits for an executor FIFO
  reservation or an internal CPU, file, or SQLite result, then resume when network polling resumes.
  Internal backpressure can therefore slow a healthy transfer without falsely classifying the client
  as idle; a client that is actually being polled but sends or receives no bytes still expires.
- Metadata, LLM submission, and LLM result use versioned event signals and exact durable retry deadlines;
  they have no configurable polling interval. Metadata `lease_seconds` and `max_attempts`, and backup
  `max_active_uploads_per_user`, remain removed.
- This config rewrite preserves every non-concurrency AI setting directly: the seven `[llm]`
  `*_cron` expressions remain beside their features and use the immutable system timezone; `[llm].enabled`
  remains the single AI gate; `[face_group]` remains the section owner of
  `similarity_threshold = 0.50` and representative-selection weights. The removed
  `[face_group_representative]` section, per-feature `*_enabled` flags, an explicit timezone field, and
  old concurrency keys remain unknown rather than aliases. These values do not create workers,
  semaphores, or executor capacity.
- Metadata no longer uses a wall-clock lease or live expiry scan. This process has one scheduler and
  is the only supported owner of its SQLite/data directory, so time-based reclaim adds a duplicate-work
  race without detecting anything the scheduler cannot already observe. Claiming writes a new
  cryptographically random `claim_token` and registers it in the scheduler's bounded active-claim
  registry. Every success, failure, panic, timeout, cancellation, and dropped orchestration path owns
  a guard that durably completes or requeues the row before unregistering the token.
- Startup first recovers every forward or rollback journal to a stable recorded state, then runs one
  `DurableClaimRecovery` SQLite transaction before normal dispatch. The closed recovery registry has
  one explicit entry for every `DurableSourceId`: media processing; local, WebDAV, and backup import;
  metadata; LLM submission and cancellation; LLM result inbox processing; deduplicate and face-group
  finalization; journal recovery; file cleanup; and maintenance. It clears prior-process claim tokens
  and moves every nonterminal claimed state (`processing`, `submitting`, or its source-specific
  equivalent) to that source's ready/retry tail. Journal-owned file state is reconciled first and is
  never blindly reset by this transaction. A durable external-wait state such as LLM `submitted` is
  not an active claim and remains unchanged.
- Claiming an LLM submission durably snapshots its correlation attempt before releasing the durable
  slot. Startup recovery of `submitting` preserves the same `jobId`, client ID, and attempt when it
  returns the row to `queued`; replay is therefore the same idempotent llm-service admission, not a new
  attempt. A crash after llm-service admission but before Momento's local transition is resolved by
  that replay or by the pending result on reconnect. Runtime never reclaims a token still in the active
  registry. Cooperative Rust CPU work checks cancellation at bounded checkpoints, and supervised
  children terminate as a process group on cancellation or timeout.
- Journal preparation, first forward publication authorization, and final product persistence compare
  the same claim token. A stale execution may clean uncommitted temporaries but cannot commit product
  state. If it already crossed the forward-only file commit point, journal recovery owns completion;
  replacement work sees the recorded product/path claim and current product version instead of
  publishing a second version.
- Transient metadata failures retry without an attempt limit after releasing every resource and
  returning to the durable-source tail; typed permanent input or invariant failures fail immediately.
- Remove `[thread_pool].available_workers` and every old per-feature concurrency setting, and reject
  all of them as unknown configuration. There is no compatibility loader because the deployment
  starts with a reset database.

`io_workers` is one configured I/O budget. Internally it is divided into two fixed lanes:

- two Tokio network-event-loop workers;
- `io_workers - 2` blocking filesystem workers.

This small internal split is necessary because a blocking disk syscall must not stop Axum, WebDAV,
media streaming, or the LLM WebSocket from polling network readiness. Both lanes belong to the same
I/O executor and their sum never exceeds `io_workers`. The minimum therefore provides two workers in
each lane: one long file write/sync still leaves one file worker for LLM input, metadata, log, or other
short disk progress, while the two network workers remain nonblocking. Progress guarantees are stated
against capacity in the operation's required lane; an idle network worker is not falsely counted as
available capacity for a blocking filesystem syscall.

## Thread Ownership

```text
momento-scheduler                  1 thread
momento-io-network-*              2 threads
momento-io-file-*                 io_workers - 2 threads
momento-cpu-*                     cpu_workers threads
momento-sqlite-*                  sqlite_workers threads
r2d2 maintenance                  exactly 1 r2d2-owned infrastructure thread
```

No business or logging module creates an OS thread, Tokio runtime, Rayon pool, `spawn_blocking` task,
semaphore, or private concurrency window. Only startup constructs the scheduler and executors; r2d2's
own connection-maintenance internals remain the explicitly permitted second-pool exception.

External media programs are started by a CPU worker and count as that CPU operation until the child
exits. `FileIoExecutor` stages their explicit input/output paths; the external process may perform its
own internal file I/O, and `FileIoExecutor` validates/publishes its output afterward. Their process
group, timeout, and termination are supervised. Model/provider concurrency in the separate llm-service
is unchanged.

Each supervised Momento child process is restricted to one computation thread. ImageMagick receives
its explicit one-thread resource limit, FFmpeg receives one codec/filter/complex-filter thread, and
fixed one-thread OpenMP/native-library child settings are supplied by the process descriptor rather
than read as application configuration. exiftool is verified to remain single-threaded. A child that
cannot honor the required limit fails that CPU operation instead of running unbounded. Because every
live child retains exactly one CPU worker until exit, the CPU executor admits at most
`max(1, cpu_workers - 1)` such single-threaded children at once. The admission is executor-owned,
derived from the existing worker count, and creates neither another thread nor another configurable
pool. With two or more CPU workers, one worker therefore remains able to drain bounded control parsing,
status, and response-serialization operations while recovery is supervising long-running media tools.
The admission token remains owned by the submitted CPU command until the worker has reaped the child,
so cancellation of the calling future cannot release capacity early. A one-worker configuration
cannot provide this responsiveness guarantee and still permits one child at a time. llm-service
processes are outside this Momento pool and unchanged.

Every closed `ChildProcessSpec` also supplies mandatory source-owned resource/deadline policy; no
caller selects or disables it. `CHILD_EXEC_START_TIMEOUT = 30 seconds` bounds spawn through successful
exec detection, and `CHILD_TERMINATION_GRACE = 10 seconds` bounds SIGTERM before whole-process-group
SIGKILL and reap. ImageMagick uses `RLIMIT_AS = 512 MiB` and a 30-minute total runtime; exiftool uses
`RLIMIT_AS = 256 MiB` and a 10-minute total runtime. FFmpeg uses `RLIMIT_AS = 1 GiB` and a checked total
runtime of `max(30 minutes, 2 * validated_media_duration + 10 minutes)`, capped at 24 hours; missing,
non-finite, negative, overflowed, or over-cap duration metadata selects the conservative 24-hour cap
instead of an arithmetic fallback. These are safety ceilings, not idle deadlines. A several-hour video
therefore has a duration-derived processing window, while a wedged tool eventually releases its CPU
slot. The supervisor uses monotonic time, and timeout/cancellation follows the same process-group
termination, durable error propagation, and rollback handoff as another child failure. Runtime status
reports `cpu_workers * max(ChildProcessSpec::RLIMIT_AS)` as the worst-case external-child address-space
exposure; it is not charged as Momento heap.

Process supervision never uses `Command::output()` and never buffers a complete pipe. Small textual
stdout/stderr diagnostics are drained with nonblocking pipes by the supervising CPU worker using a
fixed 16 KiB scratch buffer; only the declared diagnostic limit is retained and later bytes are
discarded while the pipe continues to drain. This pipe polling is the explicit bounded supervision
exception to executor-domain I/O and creates no task or thread. Binary or potentially large output is
never sent through a captured pipe: `FileIoExecutor` prepares an opaque inheritable output descriptor
inside a journal temporary entry, and the supervisor passes that descriptor directly to the child.
Input descriptors are likewise pre-opened beneath a storage root. A tool that cannot consume an input
descriptor receives only a path inside an immutable journal staging directory created from the
verified input by `FileIoExecutor`; if neither descriptor input nor immutable staging can be enforced,
the operation fails instead of exposing an ordinary Momento path.

The cross-executor descriptor handoff is explicit rather than a raw descriptor escape.
`FileIoExecutor` alone creates a private, move-only, non-`Clone`
`ChildDescriptorLease { handle_id, generation, raw_fd, access, child_fd }` and changes that registry
slot to `ChildPinned`. The lease owns no close authority, but its live generation prevents the file
worker from closing or reusing the process-wide descriptor. A closed `ChildProcessSpec` declares the
maximum lease count and exact read/write access and stable child descriptor number for each media
program. `CpuExecutor` may inspect `raw_fd` only while applying that spec's async-signal-safe spawn
file actions; it cannot read, write, seek, sync, duplicate for application use, or close the
descriptor. After spawn, the child owns only its declared duplicates while the parent registry entry
remains pinned. The CPU reply returns every lease after the process group has exited; orchestration
then moves each output lease into a `FileIoExecutor` sync/validate operation. That file worker validates
the generation, changes `ChildPinned -> InFlight`, syncs and validates the output, and either returns a
normal `FileToken` or consumes it into `ClosePending`; input-only leases are consumed directly into
close after supervision. Dropping a lease instead of returning it, including reply loss or panic
cleanup, atomically changes only the matching live slot to `ClosePending` and wakes a file worker; it
does not unpin to reusable `Idle`. The child-process guard always terminates and reaps the process group
before any lease can drop, so registry close cannot race a child that still depends on the descriptor.

Every listener, database, storage-root, journal, input, output, and pipe descriptor is opened with
close-on-exec. A typed process descriptor declares the exact child descriptor allowlist and stable
descriptor numbers. Child setup uses only async-signal-safe descriptor operations: it duplicates the
declared descriptors, closes or marks close-on-exec for everything outside stdio diagnostics and that
allowlist, then executes the program. Parent-side duplicates close on spawn failure, immediately after
successful spawn, and after exit. No child can inherit an HTTP listener, SQLite/r2d2 connection, root
directory handle, unrelated journal entry, or another request's file. The CPU worker retains the
process operation until exit, timeout, or whole-process-group termination, then returns only bounded
diagnostics and descriptor metadata.

## Scheduler

The scheduler owns:

- one bounded command channel;
- typed handles to the independently owned bounded FIFO of each executor;
- the derived connection, request, inbound/outbound stream-session, and file-chunk admission policy;
- round-robin durable-source wakeups;
- cron and recovery timers;
- job cancellation and shutdown state.

Each executor owns its operation queue and workers; the scheduler owns routing, admission, and durable
source ordering but never the queue storage or a worker. It does not own CPU permits, database permits,
filesystem permits, or the previous nested resource graph. A queue slot and its executor worker are
the only execution capacity for that domain.
Admission uses source-owned counters and move-only RAII guards, not semaphores or per-feature windows.
Guard fields are private and the guard types do not implement `Clone`; ownership may only be moved by
the explicitly defined request-to-session and receipt-credit conversions. Releasing an admission
updates an atomic counter and wakes the scheduler directly; it never depends on placing
a release command into a bounded queue, so a full command channel cannot leak capacity.

The raw executor queue senders remain private to the scheduler/runtime builder. `AppState`, routes,
business modules, and durable dispatchers receive admission handles that can only ask the scheduler to
submit; they cannot call `try_send` on an executor. Scheduler ingress capacity is derived exactly as:

```text
active_requests
+ active_stream_sessions
+ active_outbound_stream_sessions
+ active_inbound_durable_streams
+ EXECUTOR_COUNT * DurableSourceId::COUNT
+ SchedulerControlSource::COUNT
+ LLM_TRANSPORT_INGRESS_CREDITS
+ CronTaskId::COUNT
```

Each admitted request, inbound or outbound stream session, and inbound durable stream owns one
reusable ingress credit and may have at most one outstanding cross-domain operation. Every durable
source owns one permanent credit per executor, and each closed-enum scheduler control source owns one
coalescing credit. Credits reserve bounded command
channel capacity before business work exists, so a legal post-journal or durable submission never
waits outside the scheduler channel and never loses its place to a later `try_execute`. Successful
bounded-channel enqueue is the ordering linearization point; the single scheduler assigns its
monotonic sequence in receive order and is the only executor admission arbiter. A worker completion
returns queue capacity through the existing non-lossy atomic wakeup, after which the scheduler admits
the oldest registered request for that executor. A violated one-command-per-credit invariant is an
internal failure, not a dynamically growing fallback queue.

`LLM_TRANSPORT_INGRESS_CREDITS = 1` is the connection loop's permanent reusable credit for its sole
in-flight inbound-control CPU parse. It never owns a second executor command; queued raw frames remain
inside the separately sized socket-loop FIFO until the prior parse reply returns.

`CronTaskId` is the exact seven-task enum for OCR, tagging, deduplication, aesthetics, face,
screenshot, and document scheduling. The scheduler owns one timer state and one permanent reusable
ingress credit per ID; it does not run cron parsing/calendar iteration or create one Tokio task per
feature. On startup, config generation change, or a due timer, it dispatches a token-fenced
`CronEvaluation` orchestration. `CpuExecutor` parses the five-field expression with the immutable
`SystemTimezoneSnapshot` and examines at most `CRON_OCCURRENCES_PER_CPU_OPERATION = 256` occurrences.
A long catch-up persists/retains only its bounded `(task, config_generation, after, latest_due, now)`
cursor, releases the CPU operation, and resubmits at the ordinary FIFO tail until it finds the latest
due occurrence and first next occurrence after `now`. It then uses one bounded SQLite operation to invoke the same feature-start path as the
manual API and store the scheduled cursor. A stale config generation or already-recorded occurrence is
Superseded; crash/restart recomputes idempotently from durable feature-start evidence. Thus no calendar
loop blocks the scheduler/network thread, no missed interval creates one job per occurrence, and each
feature creates at most the same single catch-up run the current semantics intend.
After the token-fenced SQLite outcome, the scheduler installs that returned future occurrence only if
the config generation still matches; otherwise it immediately starts a new evaluation.

`SchedulerControlSource` is the exact closed enum `CpuCapacity`, `FileIoCapacity`,
`SqliteCapacity`, `AdmissionReleased`, `ConfigChanged`, `CancellationChanged`, and
`ShutdownRequested`; its count is seven. A coalesced control command contains only its source and the
latest observed monotonic version. The authoritative state and payload live in the executor counters,
admission atomics, `ConfigManager`, SQLite cancellation state, or scheduler shutdown state rather than
inside the command. On receipt, the scheduler itself reads only lock-free counters/version state,
immutable `ConfigManager` snapshots, and its own shutdown state; it never performs or awaits file,
network, or SQLite I/O. `CancellationChanged` advances an atomic version and wakes the
`LlmCancellation` durable source, which reads the authoritative outbox through its existing SQLite
ingress credit. Capacity and admission sources reread their atomic counters, while configuration and
shutdown reread their in-memory owners. Each owner processes until its observed version catches the
current version before returning the credit. Multiple completions or changes can therefore collapse
into one wakeup without losing a cancellation, configuration revision, shutdown, or available
executor slot.

The scheduler also owns a global durable-orchestration counter derived as:

```text
cpu_workers + (io_workers - 2) + sqlite_workers
```

A durable dispatcher must atomically acquire one slot before claiming a database row and normally
holds it until that Momento-side orchestration completes, requeues, cancels, is superseded, or hands
off to a separately bounded external phase. When no slot is available, the source leaves the row
unclaimed and returns to its durable tail. This is scheduler-owned admission derived only from the
three configured pools, not a new thread, semaphore, or feature-specific concurrency setting.

A durable outbound transfer uses an explicit phase-ownership protocol so a slow 20 GiB network send
cannot consume every durable slot. After a non-mutating eligibility peek, its dispatcher first
acquires an outbound-stream-session guard, then a durable-orchestration guard, and only then atomically
claims the row and snapshots its immutable input descriptors. It then releases the durable guard
before file verification or network transfer, while retaining the claim token and outbound guard;
opening the input adds one `StreamSession` file token and the transfer keeps one bounded buffer.
During progress it may pipeline a second charged file chunk under the global/two-per-stream rule; while
waiting for network progress it retains only the one detached buffer.
After the remote peer durably admits or rejects the transfer, the session immediately drops its
logical `StreamSession` token, requests executor-owned close, and drops its byte buffer before waiting
for another capacity. It retains
only the outbound guard, claim token, correlation fields, and small typed outcome, then reacquires a
durable guard before any local SQLite state transition and finally releases the outbound guard. If the
row disappeared between peek and claim, both guards are released. Acquiring the outbound guard before
the claim and releasing the durable guard before network waiting prevents completed transfers from
deadlocking behind new claims.
The active-claim registry is bounded by
`durable_orchestrations + active_outbound_stream_sessions`, not by durable admission alone. LLM
inference waiting starts only after the submitted state is durable and owns neither guard nor a file
token; result receipt later acquires a new durable slot.

### Submission behavior

- HTTP and WebDAV orchestration runs on the I/O network runtime. The accept path acquires connection
  admission before assigning a socket to the runtime. A parsed request acquires request admission.
  After a streaming route has completed authentication and validation but before journal creation,
  body consumption, or response production, it atomically converts request admission to
  stream-session admission; it never holds both while waiting.
  File-chunk admission is acquired and released per bounded chunk as described above. Admission is
  non-waiting before a durable boundary: excess connections are closed, and excess parsed requests or
  stream-session conversions receive 503 before body consumption or mutation. A rejected HTTP/1 request with an unread
  body closes its connection, so unread bytes cannot be interpreted as another request. Header timeout
  is enforced at the accepted-socket HTTP builder before Axum routing;
  body, keep-alive, response-write, and WebSocket liveness deadlines release all admissions on expiry.
- Read-only requests use non-waiting scheduler admission and may return 503 whenever a required queue
  is full or any older reservation already exists for that executor. They never take a newly released
  slot ahead of an older durable or HTTP waiter. A mutating request also uses non-waiting admission
  until its generic file-operation group,
  entries, and path claims have committed. Group preparation is its first externally durable mutation;
  if that SQLite submission cannot be admitted, the request returns 503 without creating a temporary
  file or changing product state.
- After a journal group exists, queue saturation is backpressure rather than rejection. The accepted
  orchestration future enters the executor's shared bounded FIFO reservation path while retaining only
  its bounded request or stream-session admission and owned operation state. If the client disconnects, the
  journal is handed to the recovery source before that future exits. It is never abandoned or
  converted to 503 after the durable boundary.
- Streaming requests apply socket backpressure and keep at most two file chunks in memory: one chunk
  may be executing and one may be queued. A stream cannot enqueue a third chunk. When the queued
  chunk completes, the next chunk is submitted at the strict FIFO tail. Waiting for queue capacity is
  asynchronous and pauses socket reads; it never blocks a network event-loop thread.
- Durable outbound streaming follows the same bounded-buffer and per-chunk file FIFO rules. It obtains
  outbound-session admission before claiming work, transfers ownership out of the global durable slot
  before waiting on the peer, drops its logical file token, requests close, and drops its buffer as soon
  as the peer outcome is known, and reacquires durable admission only to persist admission/rejection or requeue state. A retry
  releases the outbound session and rejoins the source tail.
- Every admission handle has exactly two submission modes. `try_execute` is used for pre-journal
  mutation admission and read-only requests that explicitly permit load shedding; it fails immediately
  when the executor has no slot or an older reservation exists. Accepted bounded control-body parsing,
  status snapshots, and response serialization instead enter the waiting FIFO using their already
  bounded active-request credit, so recovery load cannot turn status polling or a completed operation's
  response into an executor-overload error. A valid ingress-credit owner always has scheduler-channel capacity;
  command-channel exhaustion is an internal invariant failure rather than a normal 503 path.
  `execute_durable` enters the same FIFO
  reservation order for post-journal requests, durable jobs, and journal recovery. Each durable source
  may own at most one outstanding reservation per executor. A normal durable flow first acquires global
  durable-orchestration admission, then the next required executor reservation, and only then claims
  its next SQLite row. A durable outbound flow uses the outbound-then-durable acquisition and phase
  transfer defined above. Therefore waiter count is bounded by active HTTP/stream-session admissions plus the fixed number
  of durable sources, and continuous HTTP traffic cannot starve recovery or background
  work outside the executor FIFO. A completed or failed durable operation releases its resources and
  rejoins only at the FIFO tail when more work remains.
- Durable source identity is a closed `DurableSourceId` enum:
  `MediaProcess`, `LocalImport`, `WebDavImport`, `BackupImport`, `Metadata`, `LlmSubmission`,
  `LlmCancellation`, `LlmResult`, `DeduplicateFinalization`, `FaceGroupFinalization`,
  `JournalRecovery`, `FileCleanup`, and `Maintenance`. A job ID, media ID, user, AI task type, cron
  invocation, or retry is never a new source identity. Adding a durable worker requires extending this
  enum and its fairness tests.
  Each source dispatcher may coordinate many active jobs, but it may have only one pending FIFO
  reservation per executor; admitted work releases that reservation before the dispatcher selects
  the next ready job at its own durable tail.
- Scheduler work admission is an exact closed enum:
  `SchedulerAdmissionKind::NewClaim`, `ExistingClaimCompletion`, and `RecoveryHandoff`.
  `NewClaim` is the only kind that may allocate a new active claim. An
  `ExistingClaimCompletion` retains the original claim identity and enters through the credit already
  owned by its request/stream session or its closed durable source while it persists that claim's
  terminal, retry, cancel, or superseded outcome; it never creates another row or claim. Multiple
  completions from one durable source remain durable by row ID and register one reservation at a time
  through that source's permanent credit. `RecoveryHandoff` uses the current admitted owner's credit to
  transfer an already journaled mutation or existing claim into its durable recovery state. During
  quiescing the scheduler rejects `NewClaim` but continues accepting the latter two kinds until every
  registered claim is resolved or the shutdown deadline expires. Shutdown stops
  registering ordinary work first, so completion/handoff becomes the remaining traffic, but each such
  reservation receives its normal monotonic sequence and may not bypass a waiter that was already
  registered. No shutdown-only priority queue or unbounded completion channel exists.
  The quiescing transition atomically removes every not-yet-linearized `NewClaim` waiter from the
  bounded registry, returns typed `ShuttingDown`, releases its credit, and leaves its durable row
  unclaimed. A `NewClaim` whose executor command was already enqueued before the transition has crossed
  the admission linearization point and is treated as admitted existing work even if its SQL closure
  runs later; it must complete or hand off through `ExistingClaimCompletion`. FIFO is preserved among
  executor commands already enqueued and among the remaining admissible waiters, so no completion is
  blocked behind a rejected waiter and no pre-quiesce admitted command is silently removed from an
  executor-owned queue.
- Manual and cron AI starts only create durable jobs through the SQLite executor. Existing LLM jobs
  are submitted independently when I/O network capacity is available.
- OCR, tagging, clustering, aesthetics, face, screenshot, and document jobs remain independent and
  may all wait for llm-service concurrently. Momento does not determine llm-service inference order.

### LLM WebSocket arbitration

One connection-owned `LlmTransport` orchestration future on the network runtime exclusively owns both
halves of the one authenticated WebSocket. Business submission futures never own the socket sink and
never create writer/reader tasks. Each admitted outbound session registers a small explicit state
machine (`starting -> ready -> sending -> finishing -> admitted | rejected`) and may expose at most one
64 KiB binary frame at a time. The transport sends one frame from each ready session in round-robin
order, then returns that session to the tail. It never drains one multi-gigabyte job before considering
another, so all AI types can reach llm-service independently; this transport fairness does not select
llm-service inference order.

Both handshake builders configure the WebSocket library before upgrade. Momento uses
`MAX_MOMENTO_WS_MESSAGE_BYTES = 128 KiB` for inbound complete/fragmented messages, which covers its
64 KiB control limit and one 64 KiB result chunk plus fixed/job-ID header. llm-service uses
`MAX_LLM_SERVICE_WS_MESSAGE_BYTES = MAX_CONTROL_MESSAGE_BYTES + 1 KiB` for Momento's largest JSON
control and the smaller submission binary frame. Each side sets the same value as its maximum frame,
uses `MAX_WS_WRITE_BUFFER_BYTES = 256 KiB`, rejects a fragmented aggregate before it crosses the
message bound, and disables per-message compression. The single sink arbiter never queues another
application message after the write buffer reports backpressure. These library buffers and fragment
state on the Momento side are charged once in `RuntimeSizing`; llm-service uses the same constants in
its own closed transport allocation tests. A library-level oversize closes the connection and triggers
the same independent durable session dispositions as another protocol failure.

Connection-control traffic produced outside the socket loop has a separately reserved bounded mailbox
whose capacity is derived as
`active_requests + active_outbound_stream_sessions + active_inbound_durable_streams +
DurableSourceId::COUNT`. Asynchronous `resultReady`, `resultChunkReady`, result receipt/defer/reject,
and cancellation outcomes use this mailbox; binary payload bytes and submission protocol markers never
enter it. Every request, outbound session, inbound result stream, and durable source owns exactly one
non-duplicable urgent credit and may have at most one outstanding mailbox item. An inbound result uses
that same credit sequentially for each ready/chunk/terminal response, and the final inbox-commit phase
transfers its ownership without a release gap. The connection loop is the sole mailbox consumer and
never enqueues into or awaits the mailbox it drains. Loop-originated Pong, Close, connection error, and
immediate pre-admission `resultReceiptDeferred`/`resultReceiptRejected` responses instead occupy
separate one-item connection-local pending slots and are sent directly by the loop. A cancellation
targeting an active upload first invalidates and removes that session from the data rotation, then its
externally owned urgent cancel message may overtake unrelated data.

`submissionStart`, `inputFinished`, and `submissionFinished` are session-ordered markers owned by the
per-session state machine rather than the urgent mailbox. `submissionStart` must complete its sink send
before the session waits for `submissionReady`. An `inputFinished` becomes eligible only after the
sink send of that input's final binary frame completes; `submissionFinished` becomes eligible only
after every ordered `inputFinished` send completes. All frames share the same sink, so these dependency
edges preserve exact per-job wire order even when urgent connection messages and other jobs interleave.

The connection loop handles due Ping/Pong and result-receipt deadlines first, processes at most
`LLM_CONTROL_BURST = 32` other urgent messages, then advances one eligible session frame or marker when
session work exists, preventing both session-data starvation and urgent-control starvation. The reader
is polled in the same `select` loop while writes are pending. Every mailbox, per-submission event slot,
cancellation waiter, ready-frame slot, and ordered marker is bounded by request, inbound-stream,
outbound-session, or durable admission; the existing unbounded per-job event channels and generic
binary-message queue are deleted rather than wrapped.

Inbound WebSocket application JSON follows the CPU-domain rule without stopping protocol liveness.
The connection loop validates only the WebSocket frame type and
`MAX_MOMENTO_INBOUND_LLM_CONTROL_BYTES`, then places
the owned raw control frame into a FIFO of
`LLM_INBOUND_CONTROL_SLOTS = active_outbound_stream_sessions + active_inbound_durable_streams +
DurableSourceId::COUNT`. Exactly one frame at a time is submitted through ordinary global
`CpuExecutor` FIFO admission for JSON parse/schema validation; its ordered result returns to the socket
state machine. The loop continues polling/sending Ping, Pong, Close, credited result binary frames, and
ready outbound bytes while that parse is pending. It never parses JSON on a network worker and has no
reserved CPU worker or control-specific concurrency window. A peer that fills every derived raw slot
receives a connection-level overload close; registered jobs independently take their normal durable
disconnect/requeue path, and no extra frame is read or retained. `RuntimeSizing` charges every slot at
the 64 KiB Momento-inbound bound plus its exact queue/output envelopes. Binary result chunks use their
fixed 20-byte framing/credit path and do not enter this JSON FIFO.

Results use one shared streamed wire and durable-record contract implemented in `src/common/llm.rs` by
both services. The old monolithic result-body representation is replaced, not retained as a fallback.
llm-service durably stores a bounded result manifest plus one `result-records.bin` file containing
length-delimited, checksummed `momento-result-records-v1` records. Each record carries its kind and
ordered correlation. OCR/tagging/aesthetics/screenshot/document/clustering records are bounded per
input; face detection emits bounded input markers and one bounded face record per detection; aggregate
and continuation records preserve first-input and split-value semantics. Failed inference emits one
bounded failure record. Local model runtimes stream the same record form to llm-service, which validates
and writes it incrementally instead of constructing a second complete response body.

llm-service commits a completed runtime stream with one explicit filesystem state machine. It writes
only `result-records.tmp` while incrementally validating framing/count/hash, syncs it, atomically renames
it to `result-records.bin`, and syncs the job directory. It then writes and syncs
`result-manifest.tmp`, renames it to `result-manifest.json`, syncs the directory again, atomically moves
the job directory from `processing` to `callback_pending`, and syncs both queue parents. Result delivery
selects only `callback_pending` jobs with a matching complete manifest and record file. Startup validates
interrupted `processing`: a complete synced manifest/records pair moves to `callback_pending` without
rerunning inference; any temporary, missing, or mismatched pair is removed and the original durable
inputs return to `queuing` under the existing runtime-attempt policy. Interrupted
`callback_pending` publication finishes forward from the same evidence. No state treats the existence
of a partial file as completed inference.

llm-service sends a bounded `resultStart` envelope containing `jobId`, `mediaId`, `task`, `attempt`,
`status`, `modelType`, `modelVersion`, `encoding`, `recordCount`, `byteSize`, and `contentHash`.
Completed results require the bounded model fields; failed results omit both and carry their reason in
the failure record. This preserves the existing explicit model provenance without repeating it in
every per-input record. Momento validates the envelope and
the exact total/record/continuation/task-state bounds, then acquires inbound-durable-stream admission.
Before capacity reservation it runs a bounded read-only `ResultCorrelationProbe` keyed by
`(clientId, jobId, attempt)`. A terminal/removed job or an already committed matching inbox receipt is
answered immediately with `resultReceived`; llm-service sends no body and deletes its durable result.
An unknown/unparseable correlation with no matching active job is immediately Rejected because it owns
no Momento state. An active job whose media/task/attempt fields mismatch is a correlated permanent
failure, not a stateless protocol response. Before sending Rejected, Momento acquires the small
source-owned `RESULT_REJECTION_MAX_GROWTH_BYTES` SQLite token and atomically rechecks the job version,
records its terminal failure/reason, and clears submission eligibility. Temporary inability to commit
that disposition sends Deferred; a race that made the job terminal sends Received. Thus duplicate/late
disposal needs no Journal or long-lived SQLite reservation, while no active Momento job is stranded
after llm-service accepts a terminal Rejected. A transient probe failure Deferreds without retaining
bytes.

The two "small" SQLite bounds are executable schema-derived values, not placeholders.
`MAX_DURABLE_ERROR_BYTES = 4 KiB` bounds every error/reason persisted in a durable row; longer source
diagnostics remain in the already bounded structured log event and the database stores explicit
truncation metadata. At bootstrap the centralized `SqliteFootprintRegistry` uses the actual validated
page size and the closed affected-table/index registry to calculate:

```text
RESULT_REJECTION_MAX_GROWTH_BYTES = page-aligned worst-case pages for one token-fenced llm_jobs
                                    terminal update, its <= 4 KiB error value, every affected index,
                                    and its bounded commit/checkpoint WAL peak
RESULT_CLEANUP_RECOVERY_MAX_GROWTH_BYTES = page-aligned worst-case pages/WAL for one cleanup cursor
                                           update plus deletion/update of <= 256 rows and <= 4 MiB
                                           staged payload across every affected table/index
```

Both values are immutable fields in `RuntimeSizing` and every relevant `DbOperation` references the
same registry entry. Schema/query tests insert maximally fragmented rows and verify observed main/WAL/
SHM allocation never exceeds the calculated footprint. Adding an affected column, table, index, or
larger error/payload bound fails the exhaustive registry test until the formula is extended.

Only an active matching probe returns the exact job version needed for preparation. Momento then uses
the exhaustive `ResultSqliteFootprintSpec` for that task to calculate a page-aligned worst-case
SQLite growth from `recordCount` and `byteSize`. The formula includes fixed inbox/progress/product rows,
one worst-case staging row and every affected index entry per record, task-specific final rows,
artifact-plan/group links, SQLite record headers, page-split overhead, and the receive/finalization WAL
peak plus one bounded cancellation/permanent-failure cleanup batch's WAL peak. It uses the configured SQLite page size and checked arithmetic; each affected table/index declares
its maximum pages per bounded record, and a compile-time exhaustive task table plus fragmentation tests
prove the bound. Momento then atomically acquires both a Journal-class reservation for the declared
result bytes and a durable SQLite-class reservation for this calculated footprint before requesting the
body. `TemporarilyUnavailable` in either class sends `resultReceiptDeferred` without creating a receive
group or consuming a llm-service delivery attempt. `ExceedsHardLimit` or checked footprint overflow is
a permanent pre-body result condition: Momento releases any already-issued provisional token, then uses
the same bounded correlated-rejection transaction to mark the exact job failed before sending
`resultReceiptRejected`. If that small transaction is temporarily unavailable it sends Deferred
instead; after its commit llm-service records the normal failed transition and no receive group is
required. A valid result can therefore wait
for reclaimable capacity, but no result can remain `callback_pending` for a requirement this deployment
can never satisfy.

With both provisional tokens held, Momento runs one SQLite prepare transaction that rechecks the exact
correlation and probed job version, closing the probe/reservation race. If cancellation, terminal
transition, duplicate receipt, or mismatch won meanwhile, it commits no receive state, releases every
provisional token, and returns Received or runs the same bounded correlated-rejection transaction before
Rejected, exactly as a fresh probe would. An
active match uses `SqliteCapacitySource::ProvisionalParent` for the preparation footprint, atomically
promotes/persists both tokens in `data_dir_space_reservations`, advances the result cursor past that first
bounded stage, and prepares a deterministic generic
journal group, product claim,
receive token tied to the current job version, and `Publish` entry from a Journal temporary to a Journal inbox path. A prior incomplete receive for the same correlation is
handed to journal recovery and deferred until it is rolled back or finalized. Only after the prepared
temporary descriptor is open does Momento send
`resultReady { jobId, nextOffset: 0, maxBytes: LLM_STREAM_FRAME_BYTES }`.

`resultReady` grants exactly one frame credit. llm-service may send one job-tagged binary frame at the
granted offset and no second frame for that result until Momento has written the first through
`FileIoExecutor` and sent `resultChunkReady { jobId, nextOffset, maxBytes }`. Every active result owns
at most one frame and one file operation. llm-service has one connection sink arbiter, not one writer
per result; it sends at most one credited frame from each result stream in round-robin order and
interleaves its bounded urgent-control burst. Consequently a large result or a saturated Momento file
FIFO can impose backpressure on only its credit while the single WebSocket continues to carry Pong,
cancellation, submissions, and other admitted results. After the last credited frame, llm-service sends
`resultFinished`.

The job-tagged frame is not implementation-defined. `momento-result-chunk-v1` is exactly a 20-byte
little-endian header followed by UTF-8 job ID and raw result bytes: four-byte magic `MRCH`, `u8 version
= 1`, `u8 reserved = 0`, `u16 jobIdLength`, `u64 offset`, and `u32 payloadLength`. The total WebSocket
message length must equal `20 + jobIdLength + payloadLength`; job ID is nonempty hexadecimal and at most
`MAX_LLM_JOB_ID_BYTES`, payload is nonempty and at most the granted
`LLM_STREAM_FRAME_BYTES = 64 KiB`, and offset must equal the stream's next credited offset. Unknown
magic/version/reserved bits, integer overflow, trailing bytes, wrong job/attempt state, or an uncredited
frame is a protocol failure for only that result stream. Whole-file SHA-256 plus record CRCs provide
integrity, so the chunk header adds no competing checksum. The shared Rust encoder/decoder used by both
services and `RuntimeSizing` use this same constant header and maximum total frame size; Python model
runtimes produce result records, not WebSocket chunk envelopes.

Cancellation fences an active receive as well as the later result worker. Cancellation first receives
the scheduler's reserved acknowledgement that every matching mutation gate is closed and generation
invalidated; it then runs the cancellation transaction, which advances the job version, records the normal cancellation outbox, and sets the matching receive group's
durable `cancel_requested` intent flag plus one result-product cancellation marker; result artifact
groups inherit cancellation from their recorded `(jobId, jobVersion, resultProductVersion)` rather
than requiring one unbounded transaction to update every group. Cancellation does not introduce
another journal state. Its
versioned in-memory notification removes that result from the chunk-credit rotation, so no new
`resultChunkReady` is sent. Every credit grant, Journal Publish, and inbox finalization checks the
receive token; the durable token/version check, not the notification, is authoritative. Before the
first forward rename, the current mutation owner or `JournalRecovery` moves
`prepared -> rollback_pending` only after the active bounded file operation has returned and its logical
token has entered close; the actor must hold the group's exclusive `JournalMutationLease` while it
records that handoff, then releases the lease. If any forward rename
has occurred, generic recovery must still complete `publishing -> files_committed`; finalization then
records the group's `completion_outcome = discarded`, creates no inbox/business reference, marks the
group `completed`, and durably schedules ordinary `cleanup_pending`. Once rollback or forward-cleanup
ownership is durable, Momento sends `resultReceived` so llm-service deletes its completed task, then
releases the receive admission. Thus cancellation during any frame, sync, publish, or finalization
boundary uses only the existing journal graph and cannot insert an inbox row or resurrect work.

Momento hashes and counts the unchanged record bytes while writing into its prepared Journal temporary;
it never reads llm-service storage or assumes the services share a filesystem. It accepts only the exact
declared byte count, record count, framing, and SHA-256 hash. It syncs the temporary, atomically executes
the prepared `Publish` into the Journal inbox path, syncs the affected parents, and in one SQLite
transaction rechecks the receive token, inserts or reuses the inbox reference, and completes the receive group while retaining its
disk reservation. A crash at any earlier point leaves a prepared group that generic journal recovery
can roll back; a crash after publish finishes forward through the recorded product claim. The later
result worker reads records incrementally, and cleanup removes the file through the same group journal
before releasing its reservation. llm-service retransmits its existing durable record file after a
disconnect, so inference never reruns for delivery failure.

If no inbound stream admission exists when a valid `resultStart` arrives, Momento retains no result
bytes and sends `resultReceiptDeferred { jobId, attempt, retryAfterMs }`. A transient file, scheduler,
or SQLite failure before durable receipt stops chunk credit, closes the logical file token, and durably
marks or leaves the prepared group in a state owned by `JournalRecovery` (`prepared`,
`rollback_pending`, or the applicable forward state). It does not wait for physical close or deletion.
After that bounded handoff it logs once at the boundary, sends `resultReceiptDeferred`, and releases the
frame buffer, inbound admission, and urgent owner credit. The file registry debit and journal disk
reservation remain with their durable owners until physical close/cleanup. Insufficient unreserved disk
capacity before group preparation behaves identically without creating a group. This response
leaves the llm-service job in `callback_pending`, does not increment or exhaust its result-delivery
attempt count, and atomically persists `nextDeliveryAt` in `callback.json` before another result is
selected. `retryAfterMs` is the earliest retry time; reconnect and startup recovery honor the durable
timestamp, while llm-service remains free to delay it further. `resultReceiptRejected` is reserved for
an invalid envelope/record, size/hash/count mismatch, permanent durable-receive failure, or active-job
correlation violation, and every correlated pre-body case is sent only after its Momento terminal
disposition commits. Once `resultReady` is sent, the admitted stream may wait asynchronously on the
bounded file and SQLite FIFOs; ordinary saturation alone never turns it into a rejection.

Every failure discovered after `resultReady` uses the same resource and cleanup handoff regardless of
its wire response: stop further chunk credit, drop the frame buffer, close the logical file token,
and durably transfer the prepared group to `rollback_pending` or its required forward-completion path
before releasing inbound/network admission. A transient capacity/I/O/database failure then sends
`resultReceiptDeferred`; a permanent framing/hash/record/business-validation failure first commits the
exact Momento job's terminal failure and permanent cleanup target, then logs once and sends
`resultReceiptRejected`. If that commit is temporarily unavailable it sends Deferred and preserves the
remote copy. Cancellation completes the discard path and sends `resultReceived`.
None waits for physical deletion, and none leaves a temporary, reservation, registry debit, or
published inbox file without its generic recovery/cleanup owner.

Every receive preparation also creates one durable `llm_result_cleanup` owner keyed by the same
correlation/version and linked to both space reservations, the receive journal group, staging cursor,
artifact-plan cursor, and result-product marker. Success, cancellation, supersession, and permanent
failure set its target outcome but never delete rows inline. The permanent-failure target transaction
also rechecks the correlation/version and marks the exact Momento job failed before Rejected is eligible.
`LlmResultCleanup` deletes at most
`MAX_LLM_RESULT_CLEANUP_ROWS = 256` and at most
`MAX_LLM_RESULT_PERSIST_BATCH_BYTES = 4 MiB` of staging/plan rows per SQLite transaction, using a
`DurableParentRolling` child that reuses the source-owned cleanup WAL bound, then returns to the common tail.
It is a stage of the existing `LlmResult` durable source, not another worker, thread, admission class,
or `DurableSourceId`.
Artifact groups use their existing 256-entry journal cleanup. The Journal receive reservation releases
only after inbox bytes are durably removed; the SQLite parent releases only after every removable
staging/plan/cleanup row is gone, all persistent final rows have consumed their declared footprint, and
the cleanup terminal cursor commits. A post-`resultReady` Deferred/Rejected/Received response is
eligible only after the cleanup owner durably records which rollback/forward work and reservation it
owns. Before a success, cancellation, supersession, or permanent-failure outcome enters cleanup, the
same transaction advances the parent cursor and shrinks its outstanding SQLite reservation to the
exact source-owned `RESULT_CLEANUP_RECOVERY_MAX_GROWTH_BYTES`. Allocation already consumed by durable
rows remains measured as SQLite allocation; only unused result-construction capacity is released. That
cleanup reserve covers one bounded delete/checkpoint transaction and is reused by every automatic or
administrator retry, so a permanently bad result cannot retain its former worst-case footprint and
starve unrelated database work. Transient cleanup failure retries without pinning a worker. Permanent
evidence/schema failure is administrator-visible, stops automatic retry, and retains only that bounded
cleanup reserve plus durable evidence; administrator retry resumes from the recorded cursor without
reacquiring a large parent reservation.

`resultReceiptRejected` has one explicit llm-service terminal transition. Only after the Rejected
control message is received and correlated does llm-service stop delivery. It writes the bounded
permanent rejection kind/reason to `rejection-failure.tmp`, syncs the file, atomically renames it to
`failure.json`, syncs the `callback_pending/<job-id>` directory, atomically moves that directory
`callback_pending -> failed`, and syncs both queue parents. It neither retries callback delivery nor
reruns inference. Startup treats a callback job with only `rejection-failure.tmp` as not durably
rejected: it deletes/syncs the temporary and redelivers the unchanged result. A callback job with a
complete validated `failure.json` finishes the forward directory move without redelivery. A job already
under `failed` must have a complete matching failure file; missing/malformed evidence is an explicit
queue invariant failure retained for administrator inspection, never guessed or rerun. Disconnect
before llm-service receives Rejected leaves the job in `callback_pending` for idempotent redelivery.
Momento must complete the rollback/forward-cleanup handoff above before sending Rejected, so either side
can crash after the message without orphaning Momento bytes. Failed queue data retains the existing
administrator inspection/cleanup policy; Rejected never silently deletes failure evidence.

After the inbox transaction commits, the Journal reservation remains with the inbox file and the
SQLite reservation remains with the exact result correlation. llm-service may therefore delete its
copy without making later staging dependent on currently unreserved database space. Each result-worker
write explicitly splits a `DurableParentConsume` child for its bounded `max_growth_bytes`, commits its cursor
and parent-remaining update in the same transaction, then publishes allocation; no
unrelated SQLite operation may spend it. Terminal persistence or durable cancellation/permanent-failure
cleanup releases unused reserved pages only after all associated rows/artifacts have reached their
recorded terminal state. Startup reconstructs these reservations before result dispatch or any new
disk admission.

After the inbox transaction commits, the inbound admission guard is atomically converted in memory
into that same stream owner's one urgent-receipt credit: the frame buffer is dropped, the durable inbox
file remains owned by the row, and no release/reacquire gap can let another producer consume the
receipt capacity. The connection loop sends `resultReceived` and releases the urgent credit only after
the sink confirms the send. Disconnect before confirmation releases the in-memory credit; llm-service
replays from `callback_pending`, and the idempotent inbox transaction produces the same receipt rather
than another row. SQLite/result-worker saturation therefore cannot create an in-memory result backlog
or stop WebSocket reads, heartbeat, cancellation, and other submissions.

`LlmResult` never deserializes a complete result file and uses two explicit passes. In validation/stage
pass one, its token-fenced claim stores the next durable record sequence/offset; `FileIoExecutor` reads
one framed record of at most 1 MiB, `CpuExecutor` validates and normalizes that owned record, and
`SqliteExecutor` upserts idempotent staging rows keyed by `(jobId, attempt, recordSequence)`. This pass
does not create a crop, thumbnail, preview, or any other result artifact. Ordered continuation records
remain as staging chunks until their complete ordering/total is known. Pass two assembles only the
explicitly capped 2 MiB text/tag aggregate in one charged normalized buffer, inserts that bounded SQL
value, and releases it; no other record family concatenates the complete result in memory. A source-owned
`MAX_LLM_RESULT_PERSIST_BATCH_BYTES = 4 MiB` bounds optional bulk
inserts and is included in `RuntimeSizing`.

Only after every expected record is durably staged does pass two validate the complete staged record
state and construct durable artifact-plan rows. It partitions that plan into generic journal groups of
at most `MAX_RESULT_ARTIFACT_ENTRIES_PER_GROUP = 256`. Each complete group, including every entry, path
claim, product claim, additional-byte reservation, and expected version, is prepared in one SQLite
transaction before that group's first file mutation. File/CPU work then creates, syncs, validates, and
publishes the group's complete outputs under the normal journal state machine. Every group and artifact
row carries the same `(jobId, attempt, jobVersion, resultProductVersion)`; completed groups remain
invisible staging products. After all planned groups reach `completed`, one short token-fenced SQLite
transaction validates record counts/order and group completeness, switches every task-specific product
reference to `resultProductVersion`, and marks the Momento job terminal. A failure or cancellation
before that reference switch leaves the previous product intact. The product cancellation marker
invalidates every artifact group's mutation lease: a prepared group rolls back after its current bounded
operation drains, a publishing group finishes forward with a discarded outcome, and an already
completed invisible group receives ordinary durable cleanup. Enumeration and cleanup are paged, so
cancellation never updates an unbounded group set in one transaction. Transient failures replay the same record or group, release all resources, and return to the
source tail; uniqueness makes replay idempotent. The inbox file, staging rows, plan rows, and invisible
artifacts remain until terminal commit, cancellation discard, or administrator-visible permanent
failure cleanup has completed durably.

A connection loss fails every registered session with one typed retryable outcome, drops all ready
frames, drops its logical input token and requests close, and lets each durable boundary independently requeue at the shared
tail. No transport failure combines the durable state of different jobs. Heartbeat, cancellation, and
`resultReceived` delivery remain possible while one or more 20 GiB submissions are progressing.

Result receipt is also the durable arbiter for the startup replay race. In one SQLite transaction, a
matching result for a `queued`, `submitting`, or `submitted` job inserts the idempotent inbox row, clears
and invalidates any submission claim token, normalizes the job to the externally admitted `submitted`
state, and records that submission eligibility is blocked by the inbox row. The submission claim query
requires `NOT EXISTS` for that correlation in `llm_result_receipts`. An active outbound session compares its
token before every file read and protocol transition; once superseded by receipt, it requests close,
drops its buffers and admissions, and makes no job-state write. Repeated matching results reuse the
same inbox receipt, while a mismatched attempt remains a typed rejection. A pending result may
therefore win against startup replay without duplicate durable work.

Cancellation is the final user intent in every cancellation/result race. The result inbox row has an
explicit processing state and claim token tied to the job state version. If cancellation commits
before receipt, a later envelope for a cancelled, completed, failed, or removed job is safety-validated,
durably acknowledged, and discarded without inserting an inbox row or recreating work. If receipt
commits first, the cancellation transaction advances the job version, writes the requested terminal
state, and marks the inbox row `discarded`; a result worker that already claimed it must fail its token
check before file publication or business persistence. Discarded inbox bytes are removed through the
journal cleanup path, after which the inbox row may be deleted. If result persistence commits first,
the later cancellation/reset transaction performs its normal product cleanup and remains the final
state. Thus SQLite transaction order is explicit, no late result can reverse cancellation, and terminal
or deleted work is acknowledged without being resurrected.

Durable SQLite state remains the cancellation/submission authority; the scheduler gate is the bounded
pre-commit mutation fence, never a replacement for that state. The transaction that
initiates an all-task or task-specific cancellation is preceded by one bounded scheduler control
operation. It closes the in-memory scope gate for new matching submission/receive mutations and
invalidates every currently matching `JournalMutationLease`; the active registry is bounded by the
derived admissions, so this is not a database/job scan. Only after that acknowledgement does the
transaction store a stable request ID, scope, and
`snapshot_max_job_rowid`; that scope immediately gates matching submission claims. The same
`LlmCancellation` source then pages at most `CANCELLATION_DB_BATCH = 256` matching rows at or below the
cutoff per transaction, applies Momento cancellation, and inserts exact IDs into
`llm_cancellation_items(request_id, sequence, job_id)`. The paged selection uses centralized covering
indexes and canonical bytewise `job_id` order (with row ID only as a unique tie-breaker), so the stored
sequence is deterministic without an unbounded in-memory sort. New replacement rows may be prepared but
cannot be submitted until acknowledgement and are above the cutoff, so they are not accidentally
cancelled. Each canonical cancellation record is exactly 129 bytes: `u8 jobIdLength` followed by the
job ID and zero padding to 128 bytes. After materialization, the same source hashes at most 256 records
per CPU operation and persists a versioned `CancellationHashCheckpoint` after each page: canonical
job-ID cursor, processed count/bytes, the eight standard SHA-256 chaining words, and its at-most-63-byte
partial block. The checkpoint encoding is endian-stable and validated against cursor/count on resume;
it contains no implementation-private hasher memory. A crash resumes that page idempotently instead of
rescanning the whole request. After each checkpoint the orchestration releases its workers/admission and
returns `LlmCancellation` to the shared source tail. Only after the final page does one short transaction finalize/store exact
`totalCount` and SHA-256 over the concatenated ordered 129-byte records, eliminating variable-length
concatenation ambiguity. `MAX_LLM_JOB_ID_BYTES = 128` is enforced at job creation and on both services.

If the initiating transaction fails, the scheduler scope gate and invalidated generations remain
closed while the durable request retries; they reopen with a new generation only after a bounded
read proves that no cancellation intent committed, using the same rule as an individual group gate.
After commit, any receive/submission path checks both the scheduler scope generation and durable scope
before acquiring a mutation/session claim. Per-page job cancellation then uses the already closed scope
plus the individual lease handoff, so no result can start between scope publication and its row's later
256-row materialization batch.

The transport never places every ID in one control envelope. It sends
`cancelJobsStart { requestId, scope, totalCount, contentHash }`, then at most
`MAX_CANCELLATION_IDS_PER_CHUNK = 4096` ordered IDs in a bounded `cancelJobsChunk` after one matching
`cancelJobsChunkReady` credit, and finally `cancelJobsFinished`. At 128 bytes per hexadecimal ID plus
JSON framing, a chunk remains below `MAX_CONTROL_MESSAGE_BYTES`; runtime uses the encoded-size check as
the authority and may send fewer. llm-service stages the manifest/ID stream incrementally under its own
data root, checks sequence/count/hash, syncs it, and atomically publishes one client/request-scoped
cancellation set before sending `cancelJobsAcknowledged`. Partial staging is not active and is removed
and parent-synced on disconnect/startup; Momento replays from `cancelJobsStart`. The published ID file
uses those same fixed 129-byte records in canonical bytewise job-ID order, so membership is
a bounded binary search and publication never builds an unbounded in-memory set. Before acknowledgement,
llm-service makes that set authoritative for
duplicate/delayed admission, queued-job selection, the immediately-before-inference check,
post-inference result publication, callback selection, and every active delivery's next credit. A
listed queued or callback job is not run or delivered, and a listed processing job may finish its
current inference but discards its result. Startup loads published-set descriptors before queue
recovery, admission, inference, or result delivery, so restart cannot create a cancellation gap.

Bounded background conversion writes and parent-syncs each exact-ID marker before advancing the set's
durable conversion cursor, then removes non-running queue data in bounded batches. Admission and every
scheduler/delivery check consult the published set until all markers and cleanup cursors are durable;
after that barrier the compact set file may be deleted and parent-synced because exact markers are the
permanent delayed-replay fence. A crash before the barrier retains/replays the set; a crash after it
uses the markers. None of this delays `cancelJobsAcknowledged`, which depends only on the synced atomic
set publication and activation of the authoritative lookup. llm-service permits at most one published,
not-yet-marker-converted set per authenticated client. A different request's `cancelJobsStart` receives
a retryable busy response until conversion reaches its barrier, while replay of the same request remains
idempotent. Therefore membership consults at most that client's one compact set plus the exact marker
path, regardless of cancellation-request history or other connected clients.

`LlmCancellation` reuses the same request ID, cutoff, ordered IDs, chunks, and hash across disconnect
and startup recovery. Only a matching acknowledgement transaction deletes the outbox/items and permits
replacement submissions. Its post-commit scheduler acknowledgement reopens the scope gate with a new
generation; a crash before that acknowledgement reconstructs open/closed gates from the remaining
durable scopes before transport or claims start. Hash/request reuse with different content is a
permanent protocol error. An
active outbound claim invalidated by cancellation stops producing frames before
`cancelJobsStart` is eligible. No scheduler priority, process-local flag, oversized message, unbounded
transaction, or relative dispatcher startup order substitutes for this gate.

The scheduler never waits synchronously for an executor result. It dispatches the orchestration future
to the network runtime; that future awaits executor replies without retaining an executor worker.

## Executor Interfaces

Each executor exposes one typed asynchronous admission handle backed by the scheduler command channel;
only the scheduler holds its private bounded operation-queue sender. Completion uses a oneshot reply.
Business code cannot submit a closure or a trait object. Each domain has one non-exhaustive-to-callers,
closed command enum and one closed owned-output enum; their constructors and raw variants are private
to that executor's typed facade:

```text
CpuCommand { operation: CpuOperation, reply: Result<CpuOutput, ExecutorError> }
FileCommand { operation: FileOperation, reply: Result<FileOutput, ExecutorError> }
DbCommand { operation: DbOperation, reply: Result<DbOutput, ExecutorError> }
```

Facade methods such as `parse_control_body`, `read_chunk`, or `load_media_page` construct one exact
variant, submit it through the scheduler, match the corresponding output variant, and return its
concrete owned DTO. A mismatched output is an infrastructure invariant failure. One exhaustive
`OperationSpec` registry declares every variant's maximum inline/heap input and output bytes, temporary
heap, descriptor/access needs, cancellation checkpoints, and—where applicable—disk/SQLite growth
source. `RuntimeSizing` and compile-time/exhaustive tests consume that registry. Adding a command or
output variant cannot compile/pass tests until its size and resource policy is registered. No command
may carry an arbitrary function, absolute path, r2d2 connection, OS descriptor, executor handle, or
unbounded collection.

The three handles share the same scheduler-owned `try_execute` and `execute_durable` implementation;
only their operation types and workers differ. A monotonic sequence defines strict FIFO across both
modes. `execute_durable`'s bounded waiter registry contains at most one entry for each admitted HTTP
operation and one entry per durable source/executor pair; duplicate registration is rejected as an
internal invariant failure. `try_execute` never registers a waiter and succeeds only when it is already
the oldest admissible request.

Executor workers never invoke another executor. Cross-domain sequencing exists only in the calling
orchestration future. Queue order is strict FIFO for CPU, file I/O, and SQLite. No stream, durable
source, or retry path may bypass the tail or reserve the next worker before its current operation has
returned.

Each executor invokes the selected closed operation inside its narrow `catch_unwind` boundary. An operation
panic unwinds its owned database transaction, descriptor checkout, child-process guard, and other
operation-local RAII state, then returns `ExecutorError::WorkerPanic` through the normal reply. The
worker continues only after executor-specific cleanup verifies that its connection, registry slot,
and queue invariants are intact. A panic in the executor loop itself, the scheduler, a registry
invariant, cleanup verification, or runtime infrastructure initiates coordinated fatal shutdown; the
process never silently continues with fewer configured workers or potentially corrupted shared state.

Every durable orchestration future has a top-level async `catch_unwind` around business execution. A
business panic becomes `WorkerPanic` and runs the same awaited durable disposition path as another
operation failure before the claim token is unregistered. Claim guards do not attempt asynchronous SQL
from `Drop`: their destructor only releases already-resolved in-memory ownership and asserts that the
durable terminal/requeue handoff occurred. The scheduler never force-drops a claimed future during
normal operation or graceful shutdown; cancellation is cooperative and awaited. Process-level failure
is recovered from the journal and orphaned durable rows on the next startup.

### CPU executor

CPU workers handle:

- bounded JSON/form/XML control-body parsing and response serialization;
- authentication signature/password cryptography after lightweight header extraction;
- bounded system-timezone cron parsing and occurrence continuation;
- image decode, orientation, crop, and resize;
- hashing and media-content analysis;
- metadata parsing that does not perform filesystem access;
- face-result normalization;
- deduplicate and face comparison/group calculation;
- one pre-listener bounded reverse-geocoder decode/index build and later zero-copy nearest-location
  lookups against its immutable published snapshot;
- Argon2 password hashing;
- starting and supervising ImageMagick/FFmpeg/exiftool child processes.

CPU operations receive bytes, owned DTOs, or explicit child-process descriptors. They do not open
files, access sockets, or query SQLite.

Authentication attempt state is not a second shared in-memory job structure. The current
`AuthenticationProtection` mutex/map is replaced by `auth_attempt_buckets` rows keyed by a fixed
namespace plus SHA-256 digest. One closed SQLite operation atomically prunes at most 256 expired rows,
checks/updates the source and identity buckets, and returns the retry decision before Argon2 runs;
success deletes those exact two buckets in another bounded operation. The table has the source-owned
hard ceiling `MAX_AUTH_ATTEMPT_BUCKETS = 65,536`. At the ceiling, a request for an unseen bucket fails
closed with 429 until bounded `Maintenance` expiry pruning creates room—it never evicts a live bucket or
bypasses rate limiting. Keys store no raw password, identity, forwarded header, or IP string. Its fixed
row/index/WAL growth and 256-row prune peak are registered in `SqliteFootprintRegistry`; database busy,
timeout, or capacity failure returns a typed temporary HTTP failure, not authentication success.

For large media, orchestration uses a bounded chunk pipeline:

```text
file I/O read chunk -> CPU transform/hash chunk -> file I/O write chunk
```

At most two chunks are live per operation. No multi-gigabyte media is buffered in memory.

Compressed images require a separate decoded-memory rule: byte chunks alone do not bound raster
expansion. Header parsing first validates checked width/height/channel arithmetic and the closed codec's
geometry limits (`MAX_IMAGE_DIMENSION = 262,144` and `MAX_IMAGE_PIXELS = 1,000,000,000`) before
allocation. Exceeding either limit is a typed permanent media failure. Rust image operations use
incremental scanline/region decoding and
`IMAGE_TILE_BYTES = 16 MiB`, with at most two tiles charged per active CPU pipeline; face crops decode
only the required bounded region and no path expands the full original merely to crop/resize it. A codec
without a proven incremental/region implementation uses the supervised single-thread ImageMagick child
with `MAGICK_MEMORY_LIMIT = 256 MiB`, map pixel cache disabled, a fixed four-GiB disk scratch ceiling, and
the ImageMagick process spec's `RLIMIT_AS = 512 MiB`. Its product output still goes only to the pre-reserved journal descriptor and
`RLIMIT_FSIZE`; inability to process within those limits is a typed media failure, not a fallback to a
thumbnail or unbounded scratch spill. Runtime sizing includes two tiles per CPU pipeline and separately
reports the maximum `cpu_workers * RLIMIT_AS` external-child address-space exposure. The per-child
address-space limit is mandatory and enforced by the process supervisor; the aggregate is a reported
worst-case child-process bound, not memory reserved inside the Momento process.

### I/O executor

The network lane handles:

- Axum/Hyper protocol framing, sockets, and already-serialized response bytes;
- WebDAV request and response streaming;
- media download streaming;
- outbound HTTP and the Momento-to-llm-service WebSocket;
- request timeouts and client disconnects.

Network workers perform only the protocol parser's bounded framing, lightweight method/path/header
extraction, admission conversion, and byte movement. They do not parse a complete JSON/form/XML DTO,
serialize a bounded API response, verify an authentication signature, hash a password, decode media,
or perform another input-size-dependent CPU loop. The orchestration future transfers the already
bounded owned body/DTO to `CpuExecutor`, awaits it without blocking the network worker, and later gives
the resulting bounded serialized buffer back to the network lane. Authentication database lookup
remains a separate SQLite operation. Hyper's unavoidable fixed protocol parsing is covered by the
connection-state limits above.

Before the network runtime starts, one file worker performs the only system-resolver configuration
read through a bootstrap-only capability: exact `/etc/resolv.conf` and `/etc/hosts`, read-only,
no-symlink, at most `MAX_SYSTEM_RESOLVER_FILE_BYTES = 64 KiB` each. It returns an owned validated
`ResolverConfigSnapshot` with at most 8 nameservers, 6 search domains, 4,096 host entries, and 255 bytes
per hostname, closes the descriptors, and never
exposes the `/etc` capability to `AppState`. Missing/malformed/oversize configuration is a typed startup
error; there is no implicit fallback or runtime reload. This fixed bootstrap descriptor peak is included
in `PlatformFdSpec` and all reads execute on an existing file worker.

The same bootstrap owns system-timezone discovery so `chrono::Local` can never perform lazy file I/O on
the scheduler. A bounded `SystemTimezoneSnapshot` contains one validated IANA name and compiled
`chrono_tz::Tz`. The file worker first resolves an at-most-4 KiB `/etc/localtime` symlink only when its
normalized target is beneath `/usr/share/zoneinfo/`, strips that fixed prefix, limits the name to 255
bytes, and validates it against `chrono-tz`. If `/etc/localtime` is a regular file, it instead reads an
at-most-256-byte `/etc/timezone` name and verifies the at-most-1 MiB localtime bytes exactly match that
named zoneinfo file before accepting the name. Missing, escaping, mismatched, ambiguous, or unknown
timezone evidence is a pre-listener startup error; there is no UTC guess, `TZ` environment fallback, or
new config field. The exact file descriptors are in `BootstrapFdSpec`, close before runtime, and the
immutable snapshot is passed to the scheduler. Every cron calculation takes that explicit `Tz`; no
runtime code calls `chrono::Local`, libc `localtime`, reads timezone files, or reloads a changed system
timezone until process restart.

All hostname resolution then uses one bounded asynchronous state machine polled by the two network workers; it
must not call `getaddrinfo`, Tokio's blocking resolver, `spawn_blocking`, or create helper threads. Its
TTL cache has `DNS_CACHE_ENTRIES = 4 * io_workers`, fixed-size host/address records, checked eviction,
and is included in `RuntimeSizing`. Resolution uses `DNS_ATTEMPTS = 2` and
`DNS_ATTEMPT_TIMEOUT = 5 seconds`; UDP truncation closes that socket before bounded TCP fallback, so no
operation owns two resolver sockets. Resolution supports only A/AAAA for an ASCII/punycode hostname of
at most 255 bytes with 63-byte labels. It bounds UDP DNS messages at 1,232 bytes, length-prefixed TCP
fallback at `MAX_DNS_TCP_MESSAGE_BYTES = 64 KiB`, compression-pointer hops and CNAME depth at 8,
returned addresses at 16, and the already snapshotted search suffixes at 6. All counts/lengths are
validated before allocation; malformed pointer cycles, trailing data, unsupported records, or excess
cardinality fail that resolution. Search candidates, retry attempts, TCP fallback, and candidate-address
connection attempts are sequential under `DNS_OPERATION_TIMEOUT = 30 seconds`; addresses never use a
parallel Happy-Eyeballs socket. TTL is clamped to 30 seconds through 1 hour before entry into the fixed
cache. `RuntimeSizing` charges one maximum DNS message/state per active outbound operation. Every outbound network operation is already bounded by its request,
durable-orchestration, or outbound-session guard and may own at most one resolver socket followed by one
service socket. The shared HTTP connector disables idle connection pooling; redirects are capped at
`MAX_HTTP_REDIRECTS = 5` and close the prior response/socket before resolving/connecting the next hop.
The authenticated LLM WebSocket is the one separately counted persistent socket. Thus no DNS task,
idle pool, per-host cache, or redirect chain creates a hidden thread, future, or descriptor population.

The filesystem lane handles:

- open, read, write, seek, sync, rename, copy, and delete;
- directory enumeration and metadata/stat calls;
- upload temporary files;
- originals, previews, thumbnails, imports, backup, WebDAV, and log storage.

All filesystem calls, including small `std::fs` calls, go through `FileIoExecutor`. Application code
does not call `tokio::fs`, `std::fs`, or Tokio's implicit blocking filesystem pool directly.
Every operation that can allocate blocks also requires a live generation-checked
Journal-class `DurableSpaceCheckout`, except the explicitly enumerated bootstrap/owner-preparation
operation that consumes a `ProvisionalSpaceToken`; `FileIoExecutor` rejects an absent, stale,
wrong-owner, wrong-group, or exhausted capability
before writing. The shared `DataDirSpaceBudget` owns the one short-lived ledger lock, but never holds
it across stat/read/write/sync/rename/delete syscalls. Reservation issue, bounded extension,
new-allocation update, journal-owner transfer, product conversion, and cleanup release are the only
typed transitions. Business code cannot adjust counters or invent a reservation ID.

Every opened file remains in a generation-checked, bounded handle registry owned only by
`FileIoExecutor`. Orchestration receives an opaque, move-only, non-`Clone`
`FileToken { handle_id, generation }`; it never owns
the OS descriptor. Read/write/seek/sync operations validate the generation and operate on the registry
entry. The registry lock protects only slot state (`Idle`, `InFlight`, `ChildPinned`, or
`ClosePending`): a worker
takes the descriptor from a slot, releases the lock, performs the blocking syscall, then briefly locks
again to return or close it. No registry mutex is held across filesystem I/O.

`ChildPinned` is the only additional registry state. It is entered by a file worker while converting a
`FileToken` to the child lease described above, and only the matching returned/dropped lease generation
can leave it. Ordinary file operations reject a pinned slot. The CPU executor never locks or otherwise
accesses the registry; it carries the validated immutable descriptor numbers in the lease and returns
ownership after spawn supervision.

Dropping a token atomically marks its entry `ClosePending` and signals an executor-local condition
variable/event counter whose notification cannot be rejected by scheduler or queue saturation; it
performs no `close`, sync, or other filesystem syscall on the network, scheduler, CPU, or SQLite
thread. File workers sweep a bounded number of close-pending slots after every file operation, before
blocking for work, whenever the local signal fires, and during shutdown. A continuously non-empty FIFO
therefore cannot postpone close forever. Reply-channel failure is handled by the file worker before it
returns the handle, and handle IDs are not reused until the previous generation is closed.
The token's non-interchangeable budget-class debit and registry slot remain occupied throughout
`ClosePending`; they are released only after the file worker consumes descriptor ownership with its
single close attempt and invalidates the generation. That release increments a non-lossy executor-local counter and wakes
admission. Orchestration never waits for physical close and cannot spend the credit early, so a burst
of cancelled or completed streams cannot overcommit the registry while close work is backlogged.

On the supported Linux runtime, durability comes from the prior explicit file/parent `fsync`, never from
interpreting `close` as a commit. A worker removes the `OwnedFd` from the slot and invokes close exactly
once; the call consumes ownership and the generation is invalidated whether it returns success or an
error such as `EINTR`. It never retries the numeric descriptor, which could already have been reused.
The close error is returned/logged against the completed operation or health counter, while the registry
slot/debit is released after that one consumption. A required sync error was already handled before
close and remains a durable operation failure; close ambiguity cannot leak a slot or close another
request's descriptor.

Dynamic registry capacity is a source-derived invariant:

```text
active_stream_sessions
+ active_outbound_stream_sessions
+ active_inbound_durable_streams
+ 2 * cpu_workers
+ 2 * DurableSourceId::COUNT
+ (io_workers - 2)
+ LOG_FILE_HANDLE_SLOTS
```

`LOG_FILE_HANDLE_SLOTS = 2` is source-owned: one active append descriptor and one simultaneous
rotation/publication descriptor. They are private to file workers, follow the same generation/close
state, and cannot be borrowed by a business stream. The fixed root capabilities and process-instance
lock are counted separately. Every other open declares one
non-interchangeable budget class:

- `StreamSession` uses the first three terms and retains exactly one primary token for each admitted
  HTTP inbound/outbound stream, Momento outbound stream, or inbound durable result stream from open
  through terminal close. Chunk operations use that existing token and `active_file_chunks`; they do
  not open a new descriptor, consume another registry slot, or reopen a path between chunks;
- `CpuPipeline` uses `2 * cpu_workers` and may retain at most two tokens for each globally admitted CPU
  pipeline, allowing multiple metadata/import jobs from the same source up to `cpu_workers`;
- `SourceMaintenance` uses `2 * DurableSourceId::COUNT` and permits one two-token file-only stage per
  durable source;
- `FileWorkerHandoff` uses `io_workers - 2` for a descriptor temporarily owned before registry
  insertion.

A token cannot change class, and one operation cannot debit two classes. An admitted HTTP stream,
outbound transfer to llm-service, or inbound result transfer retains its one `StreamSession` token
while waiting on its peer. A result stream releases that token only after its staging descriptor is
physically closed; its committed inbox file is later reopened under `SourceMaintenance` for
parsing/cleanup rather than retaining the receive descriptor. Jobs waiting for inference, SQLite,
timers, or unrelated network work retain no token. Opening reserves the matching atomic class slot
before the syscall; exhaustion follows the same pre-journal rejection,
post-journal durable-tail, or result-deferred behavior defined for that boundary and never borrows
from another class or grows the registry.

`FileIoExecutor` has an explicit lifecycle:
`bootstrapping -> recovery_only -> ready -> shutting_down -> stopped`. Startup first creates the
file-worker threads. One file worker receives exactly one private `FileBootstrapCommand`; the configured
data directory must already exist as a directory. The worker opens two independently owned
close-on-exec descriptors for that exact directory. `DataDirLockFd` is used only to take the nonblocking
exclusive advisory lock and is retained for the process lifetime; `BootstrapRootFd` is the temporary
capability used for descriptor-relative inventory, root creation, fresh-database publication, and
parent sync, then closed before runtime. No lock file or other lock entry is created. A second Momento API pointing at the same data directory fails startup before SQLite or HTTP
admission. While capacity remains unknown, the worker performs only a read-only inventory of existing
source-owned child roots and opens the handles that already exist:
`Originals`, `Thumbnails`, `TinyThumbnails`, `Previews`, `Imports`, `Albums`,
`Trash`, `WebDav`, `Backups`, `Logs`, and the independent `Journal` staging/tombstone root. It then closes
`BootstrapRootFd` only after recovery has budget-created/validated all
missing roots, so runtime code cannot bypass the typed roots. The configured `server.static_dir` is
opened as an optional, read-only `Static` root: an absent directory disables
the frontend fallback as it does today, while an existing but unsafe or unreadable directory is a
startup error. Descriptor/inode ancestry checks reject a `Static` root that is equal to, contains, or
is contained by the data directory or any writable storage root, so unauthenticated static serving
cannot expose private data. In this read-only phase it records mount/device identities for existing
roots but creates no directory, probe, log, database file, or cleanup mutation. No business or HTTP
operation is accepted until this bootstrap reaches `ready`.

Process startup first installs a console-only `BootstrapLogger`; no file log is opened or appended
before storage capacity is known. The runtime builder creates file workers with only that one-shot bootstrap channel; the public file
queue sender does not enter `AppState` or the scheduler until bootstrap succeeds, and the bootstrap
channel is then permanently closed. The file bootstrap derives `DataDirSpaceBudget` in
`RecoveryOnly` mode and measures existing root/log/database allocation without modifying it; it cannot
issue a block-allocating Journal or Log token yet.

SQLite bootstrap then follows one of two explicit paths. A database is fresh only when
`database.sqlite`, `database.sqlite-wal`, and `database.sqlite-shm` are all absent at the locked
file-bootstrap snapshot. Orphan side files or a zero/truncated main file are inconsistent existing
state and fail startup rather than being deleted or treated as fresh. For a proven fresh database there
can be no prior durable reservation: the budget records that fresh-db
fact, enables only a schema-sized SQLite bootstrap reservation calculated by
`SqliteBootstrapFootprintSpec`, and creates the complete schema in one source-owned private database
name in the same data directory using a single-file rollback mode. It validates the full schema and
empty reservation table, checkpoints/closes and syncs the file, atomically renames it to
`database.sqlite`, and syncs the data-directory descriptor before the official path is considered
published. It then enables WAL/SHM with the remaining declared bootstrap bound and publishes measured
allocation. The spec includes temporary auxiliary-file and WAL/SHM activation peaks. A crash before
rename leaves the official three paths absent; the next proven-fresh bootstrap removes and parent-syncs
only the exact source-owned bootstrap names under a new bootstrap token, then recreates them. A crash
after rename resolves to either an absent fresh path or a complete schema; an official partial/truncated
file is never guessed or deleted. If both an official database and a bootstrap temporary exist, startup
fails for inspection rather than choosing one. If the database existed, one SQLite
worker first opens it read-only with schema initialization, checkpoint, and every write disabled; the
required reservation/schema tables must already exist because this design has no migration path. If
that probe reports that a valid SQLite database needs WAL/SHM recovery before those tables can be read,
bootstrap closes the read-only handle, acquires the single bounded `RecoverySpaceToken`, performs only
the specified SQLite recovery/checkpoint/sync, publishes the resulting allocation, destroys the token,
and repeats the read-only probe. It then loads all nonterminal `data_dir_space_reservations` and their
owner evidence. Corruption, missing schema, inconsistent ownership, recovery beyond the floor, or any
second failure after bounded recovery is a pre-listener startup error; startup never uses an ordinary
class or guesses capacity to break this dependency.

File workers then stat every referenced Journal entry, SQLite workers read main/WAL/SHM allocation,
and the budget reconstructs one complete ledger epoch. Only after that barrier does it transition
`RecoveryOnly -> RecoveryReady`. If the reconstructed epoch is `LogOverQuota`, file workers first use
only zero-growth typed `Logs` cleanup operations to delete/parent-sync oldest closed rotations in bounded
batches and republish allocation until the class fits; failure to do so fails startup. Only from a
healthy epoch does the file bootstrap acquire bounded Journal-class bootstrap tokens to create and
parent-sync each missing fixed child root, remove only source-owned stale probe names, and
run the real rename probe: create/sync a unique probe under Journal, rename it into a private bootstrap
location under every writable root and back, sync both parents at every move, then delete/sync it.
`statx` verifies that every writable root and Journal share the same mount and device. Actual allocation
is published before each token is released; any `EXDEV`, capacity, permission, read-only, sync, or
cleanup failure fails startup. `Static` is read-only and excluded from this rename requirement.

`RecoveryReady` also permits budgeted SQLite recovery/schema validation. Only after retained logs fit
`log_quota` and every bootstrap allocation preserves a healthy ledger does startup activate the quota-controlled
file sink. Activation failure is reported through the console logger and fails startup. Every later
append/rotation uses a Log-class reservation; there is no unbudgeted file-logging fallback. The temporary
`BootstrapRootFd` and the SQLite bootstrap channel close before the public
file/SQLite handles are published; `DataDirLockFd` remains lock-only. Startup then starts CPU workers,
constructs the scheduler from the three ready handles, starts the network runtime, and runs the
consistency-critical Journal/claim recovery required to resolve `publishing`, `files_committed`, and
`rollback_pending` work. Ordinary `cleanup_pending` entries are safe post-commit cleanup and never form
a pre-listener backlog; the event-driven generic recovery worker drains them in FIFO order after the
listener is available. Startup then transitions the budget `RecoveryReady -> Running`, constructs Axum
through the application factory, and opens the HTTP listener. Any
bootstrap failure closes retained descriptors, shuts down and joins already-started workers, and exits.
Durable sources cannot claim work before all required root handles and SQLite are ready. Direct,
rate-limited console bootstrap/fatal output remains the declared logging-infrastructure exception.
Root creation and validation are file-worker operations, never direct main-thread filesystem calls.
After activation, event producers only enqueue bounded encoded records; file workers exclusively open,
append, sync, rotate, prune, and close files below the typed `Logs` root.

The required CLI config creates one unavoidable pre-runtime read before worker counts/data-dir paths are
known. `runtime/config_bootstrap.rs` is the sole exception. It accepts an absolute or cwd-relative path
only when its normalized encoded length is at most `MAX_CONFIG_PATH_BYTES = 4,096`, it has at most
`MAX_CONFIG_PATH_COMPONENTS = 256`, every parent component is at most
`MAX_CONFIG_PATH_COMPONENT_BYTES = 255`, and its final filename is at most
`MAX_CONFIG_FILENAME_BYTES = 200`; empty, dot, dot-dot, repeated-separator, NUL, and non-UTF-8
components are rejected. Starting from an explicit root/cwd directory descriptor, component traversal
uses `openat2` with `RESOLVE_NO_MAGICLINKS | RESOLVE_NO_SYMLINKS` and, for a relative path,
`RESOLVE_BENEATH`; no parent or final symlink is accepted. These limits guarantee that both reserved
temporary names remain below the filesystem's 255-byte component limit. Main opens the exact
`-c|--config` regular file and bounds it by `MAX_CONFIG_FILE_BYTES = 1 MiB`. Before TOML allocation
it runs a linear lexical preflight with `MAX_CONFIG_NESTING_DEPTH = 8`,
`MAX_CONFIG_VALUES = 4,096`, `MAX_CONFIG_KEY_BYTES = 255`,
`MAX_CONFIG_STRING_BYTES = 256 KiB`, and `MAX_NORMALIZED_CONFIG_BYTES = 2 MiB`. The preflight counts
tables/arrays/inline values and rejects overlong/overflowed decoded strings before `toml_edit`/typed
parsing; parsing uses a fallible charged arena and duplicate keys remain invalid. Main then parses once
and closes the file before `RuntimeSizing`. It records the normalized parent path, filename, mount/inode/change
identity, size, and SHA-256. During private file bootstrap, a file worker opens a dedicated
`ConfigFileCapability` for that parent/file, re-reads the same bound, and requires the identity/hash to
match before readiness; a change in the gap aborts startup for retry. The capability is counted
separately from `StorageRootId`, survives runtime, and exposes only read-current and atomic-replace of
that one filename—not arbitrary parent traversal.

Runtime configuration updates first acquire the config mutation lease and have `FileIoExecutor` read
the current at-most-1 MiB official bytes plus identity/hash. `CpuExecutor` uses `toml_edit` to change
only the typed requested fields, preserves unrelated values/comments, validates the prospective
complete `Config`, and serializes at most 1 MiB. `FileIoExecutor` rechecks the official identity/hash
immediately before mutation; an external edit returns a typed conflict and does not overwrite it. It
then writes the one reserved same-directory name
`.<config-filename>.momento-update.tmp` through the
config capability, syncs it, atomically renames it over the exact config filename, and syncs the parent
before `ConfigManager` publishes the new immutable snapshot. Failure before rename removes the temp and
leaves file/live state unchanged. Rename is the commit point; an unexpected parent-sync failure after
it initiates coordinated fatal shutdown rather than returning a normal failed update with divergent
file/live state, and restart accepts only the atomically complete official file. A crash before rename
leaves only that exact source-owned regular non-symlink temp. The next bootstrap revalidates the
official file first, then deletes/parent-syncs the temp through a bounded zero-growth config operation
(after `RecoveryReady` when it shares the data filesystem); any wrong type/identity fails visibly. A
crash after rename reads the complete new file. Updates are serialized by one generation-checked config
mutation lease, so two administrators cannot lose fields/comments.

If the config parent shares the data filesystem, the temporary plus one directory-entry metadata peak
uses a short Journal-class provisional reservation; if it is on another filesystem, a fixed
`ExternalConfigSpaceToken` serializes the sole update and checks that filesystem's fresh free space for
the same bounded peak. The external token is not a general fourth data-dir class and cannot authorize
another path. Real external `ENOSPC` before rename returns the typed update error and rolls back. All
post-bootstrap config reads/writes, including the AI schedule endpoint, use this capability and never
call `std::fs`/`tokio::fs` directly.

`--init-config` is the only pre-runtime write mode. It is a terminal CLI action performed inside the
same `config_bootstrap` module because no config, data directory, or worker count exists yet. It renders
the existing source-owned at-most-1 MiB template, refuses to replace an existing/non-regular/symlink
target, applies the same config path/filename/component policy, writes/syncs the one exact same-directory
name `.<config-filename>.momento-init.tmp`, atomically renames it to the absent target,
syncs the parent, and exits without constructing `RuntimeBuilder`. A failure before rename removes the
temp; ambiguous post-rename sync failure exits fatally and the next invocation accepts only a complete
valid official template. If the official target is absent but the exact reserved temp survives a crash,
the next invocation validates its regular-file type, bound, and exact rendered-template hash and either
finishes the rename/parent sync or fails visibly; it never accepts or deletes an unrelated entry. This
exception cannot be called from normal startup or runtime.

Normal startup's one-shot `[server].reset_admin_password` consumption is not a bootstrap exception.
After `RecoveryReady`, config capability publication, and CPU availability—but before scheduler/AppState/
listener publication—it uses the same leased read/`toml_edit`/validate/CAS/atomic-replace path to write
the flag to `false` while preserving all other text. Its file update must complete before the process
enables the temporary authentication override. An update failure aborts startup; an environment-only
`RESET_ADMIN_PASSWORD=true` request changes process authentication behavior but does not invent another
config write unless the stored TOML flag itself must be consumed.

Static assets and SPA fallback stop using `tower_http::ServeDir`. One shared static-file responder in
the Axum application factory accepts only GET and HEAD, resolves normalized paths below the read-only
`Static` root, opens them through `FileIoExecutor`, and streams bounded chunks through the network
lane. After open, the file worker obtains size, mount ID, inode, modification nanoseconds, and change
nanoseconds with `statx` on that same descriptor. Content-Length, Last-Modified, an opaque weak ETag
derived from that tuple, every precondition, range offset, and response chunk use this one open-handle
snapshot; the responder never stats or reopens the path after validation. Static deployment supports
only immutable files or atomic path replacement; in-place mutation is outside the serving contract
because an open descriptor is not a byte snapshot. Docker packaging treats the built static tree as
immutable, and development tooling must publish a replacement file with atomic rename. If navigation fallback is
needed, it separately opens and snapshots `index.html` by the same rule.

The responder preserves Content-Type, Content-Length, Last-Modified, weak ETag, conditional 304/412
behavior, single byte-range 206/416 behavior including date-based `If-Range`; a weak ETag never
satisfies `If-Range`. It also preserves HEAD-without-body behavior and the existing cache/security
headers. SPA fallback is allowed only for a GET/HEAD request that accepts HTML,
is not under `/api` or the configured WebDAV mount, and does not name a missing static asset; otherwise
the responder returns 404 or 405. Static reads consume ordinary request/session and per-chunk file
admissions; they do not get a hidden blocking pool or direct-filesystem exception.

Static preconditions follow the exact RFC order. `If-Match` uses strong comparison, so an explicit
weak current tag cannot match; `*` succeeds when the opened representation exists. If `If-Match` is
absent, `If-Unmodified-Since` may produce 412. `If-None-Match` uses weak comparison and a match produces
304 for these GET/HEAD routes; `If-Modified-Since` is considered only when `If-None-Match` is absent.
Range processing occurs only after all preconditions pass. `If-Range` accepts a matching date or a
strong entity tag, so the responder's weak tag never satisfies its entity-tag form and the full 200
representation is sent instead. Lists, wildcard validators, invalid dates, and precedence are covered
without falling back to string equality.

A `FileOperation` contains a storage-root enum and normalized relative components, never an absolute
path. On Linux, every open/create traversal uses
`openat2` with `RESOLVE_BENEATH | RESOLVE_NO_MAGICLINKS | RESOLVE_NO_SYMLINKS`; mutation uses validated
parent directory handles with `renameat2`, `unlinkat`, `mkdirat`, and `fstatat`. A missing kernel
capability is a startup failure for the supported Docker release rather than an unsafe path fallback.
Parent replacement or symlink insertion therefore cannot redirect an operation outside its root
between validation and mutation.

Large reads and writes use `FILE_IO_CHUNK_BYTES = 1 MiB`. The file worker returns after each bounded
piece so one multi-hour video cannot monopolize a file thread. A single large stream therefore owns
at most one file worker at a time and can place at most one additional 1 MiB operation in the file
queue; all previously queued file operations run before its following chunk.

Every other filesystem operation is bounded too. Copy uses the same 1 MiB read/write pipeline rather
than `std::fs::copy`. Directory enumeration and recursive cleanup process at most
`FILE_IO_ENTRY_BATCH = 256` entries per operation, persist or return their continuation cursor, and
resubmit at the FIFO tail. One rename, metadata/stat, directory sync, or path deletion is one
operation; no closure recursively traverses an unbounded tree.

### SQLite executor

r2d2 manages connections; it does not execute SQL. `SqliteExecutor` provides the requested database
execution threads:

- each SQLite worker receives one owned `DbOperation`;
- it checks out one r2d2 connection using `SQLITE_CONNECTION_TIMEOUT = 5 seconds`;
- it executes the complete bounded query or transaction synchronously;
- it maps rows into an owned DTO;
- it drops the statement, transaction, and connection before replying.

No r2d2 connection, rusqlite row, statement, or transaction leaves a SQLite worker. No caller uses
`pool.get()` directly after this refactor.

Every read/output spec enforces `MAX_DB_ROW_OWNED_BYTES = 256 KiB` and
`MAX_DB_OPERATION_OUTPUT_BYTES = 1 MiB`, with smaller route/operation limits where possible. Schema and
write validation prevent an individual text/blob/aggregate row from exceeding its declared bound.
Paged readers step one row at a time into a fallible charged arena; if the next valid row would exceed
the operation bound, they stop before allocating/copying it and return the last included stable cursor,
so that row appears first on the next page. A row that cannot fit by itself is a schema/data invariant
failure, not an omitted row or oversized reply. Workers never collect all rows and then measure.
The maximum owned DB output appears in each request/durable operation state where a reply may coexist
with its later CPU serialization/stage DTO; it is not also added as a second global per-owner term.
Executor queue/envelope bytes remain separate. Result-record staging uses its smaller explicit
record/batch bounds rather than silently raising this general query output limit.

The pool is fully warmed before publication. Its custom manager is the only non-SQLite-worker code that
may open/validate/replace a connection, and that path is capacity-tokened as described above. Validation
runs only bounded connection pragmas and `SELECT 1`; it never executes a business query or schema
mutation. A checkout never creates a connection inline on the SQLite worker: if no pre-opened connection
is available it waits only within `SQLITE_CONNECTION_TIMEOUT` and returns the typed transient error.

SQLite remains WAL-based. Multiple read operations may run in parallel; write transactions remain
short. `SQLITE_BUSY` and `SQLITE_LOCKED` are returned as typed transient errors. The executor thread
does not retry forever: the durable caller releases ownership and places the job at the queue tail.

Every connection enforces `PRAGMA temp_store = MEMORY` and SQLite internal worker-thread limit zero;
`PRAGMA mmap_size = 0` prevents database cardinality from becoming a process mapping,
`SQLITE_PAGE_CACHE_BYTES = 8 MiB` is applied through negative-KiB `PRAGMA cache_size`, and rusqlite's
prepared-statement cache is exactly `SQLITE_PREPARED_STATEMENTS_PER_CONNECTION = 64` with bounded SQL/
parameter metadata from `OperationSpec`. The statement-cache byte charge is the checked sum of the 64
largest centralized SQL/parameter specs plus source-owned SQLite statement overhead, not `64` times an
unmeasured estimate. `RuntimeSizing` charges page and statement caches for
`sqlite_workers + 1` connections to cover one replacement overlap; the fd formula likewise covers
main/WAL/SHM for that peak. The manager closes/discards a broken connection before beginning another
replacement attempt, so overlap cannot grow past one.
`ATTACH`, ordinary `VACUUM`, and any temp file outside the owned main/WAL/SHM paths are prohibited.
Fresh schema enables incremental auto-vacuum, and maintenance advances it only in declared bounded page
batches under a normal write reservation. Each closed `DbOperation` declares
`max_temp_heap_bytes <= MAX_SQLITE_OPERATION_TEMP_HEAP_BYTES = 8 MiB` from its bounded input/page size;
`RuntimeSizing` charges that maximum for every SQLite worker. Central query-plan tests reject an
unindexed scan/sort or `USE TEMP B-TREE` over unbounded cardinality; unavoidable bounded in-memory
sorting is limited to the declared at-most-256-row DTO. SQLite length/variable/column limits are set to
the closed operation maxima before pool publication. A query that needs more must add an index,
precompute state, or persist a continuation—not spill to `/tmp` or raise a hidden limit.

`SQLITE_OPERATION_TIMEOUT = 30 seconds` bounds connection checkout plus SQL execution.
`SQLITE_BUSY_TIMEOUT = 5 seconds` is never longer than that operation deadline. Each worker installs a
rusqlite progress handler that checks the monotonic deadline and cancellation state, interrupts an
overdue statement, rolls back its transaction, and returns a typed transient timeout. Queries that
cannot reliably fit this bound must page or checkpoint their work into multiple `DbOperation`s. No
route, migration, cleanup, grouping, or reporting query is exempt.

## Error Propagation

Rust has no application exception mechanism. The requested exception-style propagation is implemented
with `Result<T, AppError>` and the `?` operator.

`AppError` has explicit categories:

- `InvalidInput` and `NotFound`;
- `Unauthorized` and `Forbidden`;
- `Network`;
- `Io { operation, path, source }`;
- `DatabaseBusy`;
- `DatabaseTimeout`;
- `Database`;
- `Cpu`;
- `Cancelled`;
- `Internal`.

Executor workers return `ExecutorError`: a structured low-level kind plus operation context and source
error. It contains no retry, cancellation, job-state, HTTP-status, or claim-ownership decision. The
same executor error may have different meaning at different durable stages, so that policy belongs to
the orchestration boundary rather than CPU, file, or SQLite code. HTTP orchestration converts
`ExecutorError` to `AppError`; durable orchestration converts it to `OperationFailure` only after adding
operation kind, durable stage, cancellation reason, and current claim-token state.

Durable action is an exhaustive boundary-owned type, not inferred from display categories:

```text
DurableDisposition::Retry
DurableDisposition::Fail
DurableDisposition::Cancel
DurableDisposition::Superseded
```

`OperationFailure` contains the typed `AppError`, its boundary-selected `DurableDisposition`, and
structured durable context. `Cancel` records a
user-requested terminal cancellation. `Superseded` means the caller no longer owns the claim token and
must release local resources without updating that durable row. Shutdown, client disconnect, timeout,
and explicit user cancellation are distinct typed reasons; the operation boundary maps each reason to
the correct disposition, so shutdown handoff may retry while a user reset cancels.

Central executor conversion tables classify SQLite result codes, OS I/O error kinds,
HTTP/WebSocket protocol failures, child exit/timeout results, and validation errors into
`ExecutorError` without policy. A separate exhaustive durable policy table matches
`(operation_kind, durable_stage, executor_error_kind, cancellation_reason, claim_state)` to one
`DurableDisposition`. It never inspects an error message. Adding an executor error, durable operation,
stage, cancellation reason, or claim state is a non-exhaustive compile error until its boundary policy
is defined.

`ExecutorErrorKind` includes `WorkerPanic`. HTTP maps an operation panic to `AppError::Internal` and
closes any affected stream after releasing its admissions. After verified local cleanup, a durable
business or operation panic is a permanent internal defect and maps to `Fail`, never `Retry`; replaying
the same code and input indefinitely would only create a panic loop. Only explicitly classified
transient database busy/timeout, temporary I/O, and retryable network failures return to the durable
tail. An invariant or executor-loop panic never enters that table because it triggers coordinated
process shutdown and startup recovery instead.

Rules:

1. Executor workers attach low-level operation context and return `ExecutorError`; they do not swallow
   it or choose durable policy.
2. Intermediate functions normally use `?` and do not log the same error repeatedly.
3. The HTTP boundary logs once and converts the error into the appropriate response.
4. A durable-job boundary logs once and exhaustively matches `DurableDisposition`: `Fail` persists a
   terminal failure, `Retry` requeues at the source tail with `available_at`, `Cancel` persists the
   requested cancelled state, and `Superseded` makes no durable state transition from the stale owner.
5. Retry never retains a worker, file handle, r2d2 connection, or executor queue slot.
6. A failed AI result does not block later results. It releases resources and returns to the durable
   tail when transient.

## Rollback and Filesystem Safety

An error cannot magically roll back a network send or filesystem rename. Rollback is therefore
explicit and shared rather than reimplemented for every feature.

### Database-only work

A `DbOperation` uses one rusqlite transaction. Returning `Err` drops and rolls back the transaction
before the r2d2 connection is returned.

### File-only work

- New bytes are written to a unique temporary path.
- The temporary file is synced and validated before publication.
- Publication uses same-filesystem atomic rename.
- After rename, the file worker syncs every affected parent directory: the destination parent and,
  when rename moved the entry between directories, the temporary-file parent. Publication is not
  durably committed until these directory syncs succeed.
- Failure before publication deletes the temporary path.
- Reads open the final path once and stream from that handle.

### Work spanning SQLite and files

All imports, HTTP/WebDAV uploads and mutations, backup finalization, metadata artifacts, LLM result
receipt, and face crops use one generic multi-path journal rather than feature-specific state machines. A single-file operation is a group
with one entry; `MAX_FILE_OPERATION_ENTRIES_PER_GROUP = 256` bounds every prepare/finalize transaction
and in-memory group DTO. A product requiring more independently published paths uses a versioned parent
product and multiple bounded groups exactly like the result-artifact plan; no operation raises this
limit or writes an unbounded entry array. Multi-file results use the same implementation with bounded
entries:

```text
prepared -> publishing -> files_committed -> completed
        \-> rollback_pending -> rolled_back
publishing -> files_committed through forward recovery
publishing -> publication_failed -> publishing after explicit administrator retry
files_committed -> finalize_failed -> completed after explicit administrator retry
completed -> independent cleanup_pending -> cleaned
                                     \-> cleanup_failed -> cleanup_pending after administrator retry
```

The schema has five centralized structures:

- `file_operation_groups`: operation ID, kind, owner/job ID, group state, product target/version,
  durable `cancel_requested` intent, `completion_outcome` (`published` or `discarded` once completed),
  and latest finalization error;
- `file_operation_entries`: group ID, stable sequence, action, Momento storage root, relative source,
  temporary, destination, or tombstone paths, expected size/hash/version evidence, entry state, and
  cleanup state;
- `file_operation_path_claims`: group ID, storage root, canonical path key, `Read` or `Write` mode,
  `Exact` or `Subtree` scope, source/destination role, and expected version.
- `data_dir_space_reservations`: reservation ID, class (`Journal`, `SQLite`, or `Log`), typed owner kind
  and owner ID, optional journal group ID, creation-time filesystem identity for diagnostics,
  `reserved_peak_additional_bytes`, last
  observed `newly_allocated_blocks`, state, and version. It is the common durable mirror for long-lived
  Journal and LLM-result SQLite reservations. Short SQLite/log operation tokens are in memory only and
  become ordinary measured allocation on crash; every durable reservation must retain matching owner
  evidence and cannot outlive that evidence. Linux `st_dev` is enforced only against repeated
  observations during one process lifetime; it may legitimately change across a ZFS pool import,
  filesystem remount, data-volume relocation, or container restart. A restart logs that change and
  reconstructs reservations from their database owner, journal, allocation, and version evidence
  rather than rejecting the same durable data solely because its creation-time `st_dev` changed.
- `file_operation_retry_requests`: administrator-supplied retry request ID, operation ID, expected
  version, exact request hash, stored response state/version, and creation time.

Only storage-root-relative paths are stored. Journal preparation, every bounded entry, every source and
destination path claim, and the product claim occur in one SQLite transaction. Claims remain until the product is
committed or a pre-commit rollback completes. There is no second scheduler path-lock map: this
transactional claim predicate is the sole cross-operation conflict authority. Path normalization is
source-owned and bounded by `MAX_STORAGE_PATH_BYTES = 4096` and
`MAX_STORAGE_PATH_COMPONENTS = 256`, with
`MAX_STORAGE_PATH_COMPONENT_BYTES = 255`; it rejects absolute paths and empty, `.`, `..`, or overlong
components. Every raw component is encoded into one canonical length-delimited binary key so distinct
platform paths cannot alias and an ancestor prefix always ends at a component boundary.
`MAX_STORAGE_PATH_KEY_BYTES = MAX_STORAGE_PATH_BYTES + 4 * MAX_STORAGE_PATH_COMPONENTS` charges the
per-component length delimiters. `OperationSpec` also charges the checked cumulative bytes of all
ancestor keys for a maximum path; normalization and conflict checking never build an unmeasured key
list and reject overflow before SQLite admission.

Durable conflict checking runs in the same `BEGIN IMMEDIATE` prepare transaction. Exact ancestor
checks use the bounded list of canonical ancestor keys; subtree descendant checks use an indexed
half-open key range for the component-boundary prefix. The claims table has covering indexes beginning
with `(storage_root, path_key)` and then mode/scope/group fields. No leading-wildcard `LIKE`, recursive
CTE over unbounded input, or full claim-table scan is permitted. The query rejects claims in the same
storage root when their keys are equal, or when either claim is `Subtree` and its key is an ancestor of
the other; overlapping reads may coexist, but overlap with a write is rejected. SQLite write
serialization prevents concurrent prepares from passing the check together, and explicit version
comparison protects the claimed object across restart. Directory COPY claims its source subtree for
read and its destination subtree for write; directory MOVE/DELETE claims every affected subtree for
write.

Every file-mutating journal continuation also owns one exclusive, generation-checked, move-only
`JournalMutationLease { group_id, group_version, owner_generation }`. The scheduler permits at most one
such lease per group, including normal execution, cancellation handoff, cleanup, and recovery. Every
mutating `FileOperation` carries `MutationAuthorization { group_id, group_version, owner_generation }`.
The orchestration checks it before dispatch; after dequeue, the file worker performs the authoritative
atomic generation comparison immediately before the filesystem syscall. That successful comparison is
the mutation's start linearization point. Cancellation first closes the scheduler's per-group mutation
gate and invalidates its generation through a reserved control acknowledgement, then commits the SQLite
cancellation/version transaction. A queued operation that has not passed the worker comparison returns
`Superseded` without touching a descriptor or path. If cancellation's SQLite transaction fails, the gate
stays closed while the cancellation request retries; it may reopen with a new generation only after a
durable read proves no cancellation intent committed. An operation that already passed the comparison
is the one allowed in-flight mutation and may finish, but cancellation never starts rollback/cleanup
while it remains in flight. After it returns no next chunk or publication is authorized;
the owner closes its logical file token, rechecks durable group/job versions through SQLite, records the
correct rollback or forward-recovery handoff while still holding the lease, and releases it. If the
owner disappears, recovery can acquire the lease only after the scheduler no longer has a live owner;
after process restart there are no surviving in-memory leases, so durable state is authoritative. No
worker waits for another executor while holding this lease: it belongs to the orchestration, covers
only one submitted bounded operation plus its immediate durable state checkpoint, and is released
before any retry returns to the tail.

Entry actions are shared primitives:

- `Publish`: atomically rename a validated temporary file/tree to its destination; overwriting a
  nonempty directory uses supported `renameat2(RENAME_EXCHANGE)` so the old tree becomes the
  journal-owned temporary/tombstone in the same namespace operation and is cleaned only after product
  commit;
- `Move`: atomically rename an existing source to a destination on the same filesystem;
- `Tombstone`: atomically rename an existing source to a unique journal-owned tombstone;
- `Cleanup`: delete an unreferenced temporary, old artifact, or tombstone only after the associated
  product transaction has committed.

The group algorithm is:

1. SQLite prepare records the complete group, all entries, all path claims, and the product claim.
   Any byte-producing group first obtains a provisional File I/O reservation and records that exact
   reservation in this same transaction; only the committed group may activate and spend the token.
2. File and CPU stages create, sync, and validate every required temporary output before the first
   user-visible source/destination mutation. A failure here can enter `rollback_pending` and remove
   temporary data in reverse entry order. `cancel_requested` follows this same rollback path while no
   forward rename has occurred, but only after the current mutation lease has fenced/drained its active
   bounded operation; it is an intent flag, never a parallel journal state.
3. The first successful `Publish`, `Move`, or `Tombstone` rename changes the group to `publishing` and
   is the forward-only commit point for the group. Recovery must finish remaining entries; it never
   reverse-renames an already committed entry.
4. Each committed entry syncs every affected source and destination parent directory before its entry
   state advances. Once every forward entry is durable, the group advances to `files_committed`.
5. One SQLite transaction finalizes the group. Normally it commits all product rows/references, records
   `completion_outcome = published`, and marks the group `completed`. If `cancel_requested` became
   durable after the forward commit point, the transaction instead records
   `completion_outcome = discarded`, creates no product reference, marks the group `completed`, and
   creates the ordinary cleanup work; it never reverse-renames committed entries. Only
   then are old artifacts and tombstones eligible for independent bounded `Cleanup` operations.
   Cleanup processes at most 256 directory entries per operation. Before its continuation checkpoint
   or `cleaned` state commits, it syncs every parent directory changed by unlink/rmdir in that batch.
   `ENOENT` is idempotent but still requires parent sync before completion when the journal has not
   previously recorded durable deletion. Transient cleanup failure retries at the `FileCleanup`
   source tail and never changes the committed product; a permanent path/evidence/permission failure
   records `cleanup_failed`, retains the cleanup evidence, and requires administrator retry without
   blocking unrelated product work.
6. A transient SQLite finalization error retains `files_committed` for retry and never removes or
   reverses published entries. All validation capable of producing a permanent product error occurs
   before the first forward entry.
7. A permanent database finalization error records `finalize_failed`, logs the exact cause, releases
   executor resources, and stops automatic retry. Every path claim and file entry remains intact. An
   authenticated administrator can retry the same group/version idempotently after correcting the
   cause.
8. A transient file error after the first committed entry retains `publishing` and retries forward at
   the `JournalRecovery` tail. A permanent missing-source, conflicting-evidence, containment, or
   permission error records `publication_failed`, preserves every claim and entry, and stops automatic
   retry. An authenticated administrator can retry only the same forward group after correcting the
   cause; rollback is not available.
9. If rollback before the first committed entry fails, the group remains `rollback_pending`; startup
   and `FileCleanup` retry it. Original and rollback errors are both retained and logged.

Recovery inspects only recorded entries and expected evidence. A crash after a filesystem rename but
before directory sync or entry-state persistence is resolved from the recorded source/destination
existence, size, hash, and version; recovery performs missing directory sync and continues forward.
It never guesses an unrelated path or treats rename alone as durable publication.

Database failure classification is centralized and exhaustive. Busy, locked, connection timeout,
deadline interrupt, and an OS `SQLITE_FULL` caused by external post-reservation interference are
transient and retain `files_committed` plus their valid reservation for retry. A requested peak above a
class hard limit is rejected before SQL and is permanent; `SQLITE_FULL` despite an intact reservation
and no external capacity loss is a budget invariant failure that triggers coordinated shutdown rather
than an infinite job retry. Constraint, schema, corruption, and product-invariant violations
are permanent and attempt to record `finalize_failed`. If the database cannot record that transition,
the group remains `files_committed`; recovery retries classification and never discards published
entries. No caller classifies database errors from message strings.

### Journal administration and lifecycle

The journal is recoverable state, not a second audit database. Three administrator-only endpoints use
the repository's typed POST convention:

- `POST /api/v1/admin/file-operations/list` returns cursor-paginated groups filtered by explicit
  states;
- `POST /api/v1/admin/file-operations/get` returns one group with an explicit
  `detailLevel = full | compacted`, current version, classified errors, and cleanup outcome. `full`
  includes ordered entries and active path claims. `compacted` instead includes immutable counts by
  entry action/final state, terminal version/time, final error summary, and cleanup summary; it never
  fabricates empty entry or claim arrays;
- `POST /api/v1/admin/file-operations/retry` requires a unique `retryRequestId`, operation ID, and
  expected version.

Responses expose only `StorageRootId` plus normalized relative paths, never host absolute paths.
Retry is one compare-and-set SQLite transaction. If `retryRequestId` already exists with the identical
operation/version/request hash, it returns the stored response without another transition or wakeup;
reuse with different content is invalid. A new request ID must match the current version and then
transitions `publication_failed -> publishing`, `finalize_failed -> files_committed`, or
`cleanup_failed -> cleanup_pending`, increments the version, and stores the exact response before
commit. A new request with a stale version returns a typed conflict. Only a newly committed transition
wakes the corresponding durable source; the startup durable scan discovers it if the process crashes
before the wakeup. Retry never accepts a new destination, action, owner, or product
target.

`file_operation_retry_requests` is cardinality-bounded. An indexed count permits at most
`MAX_LIVE_RETRY_RECEIPTS_PER_OPERATION = 64` unexpired receipts for one operation; an identical request
still returns its stored response, while a new ID at the limit returns typed 429 without changing the
group. Every receipt expires independently after `JOURNAL_TERMINAL_RETENTION_SECONDS` even when the
group remains failed, and `Maintenance` deletes at most 256 expired receipts per transaction. After
expiry, replay of an old request ID receives the normal expected-version conflict rather than repeating
a transition. Thus idempotency is explicit for the documented seven-day window and an authenticated
caller cannot grow one failed operation's receipt history without bound.

Failed, retryable, and incomplete groups are never garbage-collected. After a completed group's every
cleanup entry is durably `cleaned`, or after a pre-commit group is durably `rolled_back`, the
`Maintenance` source uses one short transaction to recheck the terminal state, delete bulky entries
and released claims, calculate the immutable summary fields, and reduce the group to its compact
terminal summary. The summary remains for source-owned
`JOURNAL_TERMINAL_RETENTION_SECONDS = 604800` (seven days), after which one transaction deletes the
summary/group; retry receipts use that same independent expiry and may already be gone. Operational
history remains in structured logs.
Cleanup/rollback, compaction, and final pruning are independently idempotent, so a crash between them
resumes safely without repeating a product mutation. Retry idempotency is guaranteed for the complete
receipt-retention interval and explicitly expires independently of group compaction.

Existing ready files remain visible while a replacement is uploaded. Conflicting mutations for one
normalized WebDAV path are protected by durable path claims. PUT/PATCH and COPY stream into a new
temporary `Publish` entry. MOVE claims source and destination and uses a `Move` entry; when overwrite
is allowed, an existing destination first becomes a group-owned `Tombstone`. DELETE claims its source
and atomically moves it to a tombstone rather than unlinking it directly. MOVE and DELETE finalize
their WebDAV/database visibility before tombstones are cleaned. Client disconnect before the first
committed entry rolls back temporary data; disconnect after it hands the forward-only group to
recovery.

A recursive directory COPY never creates one journal entry or SQLite row per descendant. Its bounded
group contains one source-subtree read claim, one destination-subtree write claim, and one root
`Publish` entry from a private Journal temporary directory to the destination. The source claim keeps
Momento mutations stable; each enumeration batch additionally rechecks directory/file identity and
metadata so out-of-process mutation becomes a typed rollback instead of a mixed snapshot. A
depth-first cursor uses at most `MAX_STORAGE_PATH_COMPONENTS` durable cursor rows, each with one bounded
component/key, and enumeration/copy advances at most 256 entries or 1 MiB per File I/O operation before
returning to the FIFO tail. Each copied file and completed directory is synced before its cursor
advances; the fully built temporary tree is validated and synced bottom-up before the single root
rename and both-parent sync. Unknown total output extends the same durable Journal reservation before
each next copy chunk. Crash/cancellation resumes the bounded cursor or recursively cleans the private
temporary tree; no partial descendant is visible at the destination. Symlinks/special files or a
source identity change fail under the declared policy rather than escaping the typed roots or silently
changing semantics.

Metadata and face-result groups publish all new versioned artifacts before one database transaction
switches their references. Old thumbnails, previews, or face crops become post-commit cleanup entries;
they are never deleted while the old product references are still active.

Permanent media deletion, trash purge, media regeneration cleanup, metadata reset, per-task and
all-task AI clean/reset, face reset, import-claim recovery, and stale temporary cleanup also use these
same primitives. Referenced originals or artifacts first become `Tombstone` entries in a group whose
product transaction removes or resets their database references; recursive deletion happens only as
post-commit `Cleanup`. Journal-owned temporary data is cleaned through entries belonging to its
existing group. No business module may call `remove_file`, `remove_dir`, or `remove_dir_all` directly,
  ignore a deletion result, or commit database removal before durable tombstone evidence exists. Log
  rotation/pruning uses the file executor's typed `Logs` operations without the business journal;
  SQLite/r2d2-owned database files remain the only application-filesystem executor exception.

## Workload Flow

### HTTP and media streaming

```text
network I/O framing + bounded byte receive
  -> CPU executor parse/authenticate/validate request DTO
  -> SQLite executor when data is needed
  -> CPU executor serialize response or perform requested computation
  -> file I/O open/read when media is needed
  -> network I/O response
```

File download and video streaming do not pass complete media through CPU or SQLite workers.
For a byte-producing HTTP/WebDAV mutation, journal preparation records its disk reservation before body
consumption. A declared body reserves its complete temporary/replacement peak; an undeclared chunked
body extends by one chunk before the network loop reads that chunk. Insufficient budget returns the
operation's typed pre-mutation rejection or, after journal creation, durable rollback/handoff without
consuming unreserved bytes.

### Local, WebDAV, and backup import

```text
file I/O discover/stage
  -> file I/O reserve copy peak + SQLite journal/claim
  -> file I/O read chunks
  -> CPU hash/metadata work
  -> file I/O temporary canonical copy + atomic publication
  -> SQLite finalization + metadata job creation
```

Discovery is incremental and reports actual total media after completion. Exact duplicate content
reuses the canonical media row and still grants ownership to the importing user. Supplemental Google
Photos metadata selection remains deterministic.

Import content deduplication has no process-global hash mutex/map. After bounded file-read/CPU hashing
and before allocating a media ID, one SQLite transaction attempts a
`media_import_hash_claims(content_hash PRIMARY KEY, owner_job_id, claim_token, source, version)` row and
rechecks the partial unique imported-media content-hash index. If another live import owns that hash,
the later import releases every file/CPU/SQLite resource, persists `available_at`, and rejoins its source
tail; it never waits on the other job in memory. The owner alone prepares/finalizes the canonical
original and atomically converts or clears the hash claim with the media/import state. Exact duplicate
bytes reuse the committed media ID, restore/grant access, preserve canonical original bytes, apply the
older source timestamp rule, move a supplied sidecar beside the canonical original, and queue the
required metadata rerun. `DurableClaimRecovery` clears prior-process claim tokens only after journal
recovery and requeues their owner jobs, so a crash cannot allocate two media rows. The unique index is
the final authority; the claim is coordination, not a compatibility fallback.

### Metadata and AI inputs

```text
SQLite claim
  -> file I/O read original
  -> CPU/external media processing
  -> file I/O reserve validated derived-output peak
  -> file I/O publish artifacts
  -> SQLite persist metadata and AI input descriptors
```

Supplemental metadata selection remains a fixed at-most-four candidate sequence: exact Takeout name,
its 51-character Takeout truncation, collision-index name derived from a numbered media filename, then
its truncated collision form, with duplicates removed while preserving that order. File I/O probes one
candidate at a time through the typed Originals descriptor and accepts the first regular non-symlink
match. It never enumerates or loads every similarly named JSON file. The chosen sidecar and exiftool
output each use the bounded metadata-source contract above; supplemental fields remain authoritative
only for the fields they contain.

AI uses original media whenever supported. Momento-owned full-resolution video frames remain the only
derived AI input. UI thumbnails are not AI inputs.

### LLM submission and result handling

```text
non-mutating eligible-job peek
  -> acquire outbound-stream admission
  -> acquire durable-orchestration admission
  -> SQLite claim job/input descriptors
  -> release durable-orchestration admission
  -> file I/O open and verify input
  -> file I/O read verified input chunks
  -> network I/O stream to llm-service
  -> drop logical input token, request file-worker close, and drop byte buffer
  -> reacquire durable-orchestration admission
  -> SQLite persist admitted/rejected/requeued state
  -> release outbound-stream admission

network I/O receive resultStart and acquire inbound stream admission
  -> SQLite read-only terminal/duplicate/correlation probe
  -> for active match only: shared capacity snapshot + Journal result-byte reservation + calculated SQLite-growth reservation
  -> SQLite version recheck + provisional-parent promotion + receive-journal preparation
  -> file I/O open prepared temporary and grant first result frame credit
  -> network/file I/O receive and write one credited frame at a time
  -> file I/O sync exact-size/hash-verified result
  -> journal Publish + SQLite durable inbox file reference
  -> atomically convert admission to urgent receipt credit and acknowledge llm-service
  -> repeat: file I/O read one bounded record
            -> CPU validate/prepare one bounded record
            -> SQLite idempotent staging upsert
  -> SQLite validate the complete staged record set and build durable artifact-plan rows
  -> repeat bounded artifact group: SQLite prepare every entry/claim/reservation for the group
            -> CPU/file I/O create, validate, and journal-publish its versioned artifacts
  -> SQLite validate all groups, atomically publish product references, and set terminal Momento job state
  -> file I/O clean durable inbox bytes and release disk reservation
```

llm-service success still means Momento durably received the result, after which llm-service deletes
its local task. Momento success still requires its later persistence and downstream work.
Result-delivery deferral leaves llm-service's durable result in `callback_pending` without consuming a
delivery attempt; only an invalid/permanently unreceivable result is rejected. Cancellation remains
final across receipt and persistence races, and late terminal results are acknowledged and discarded.
LLM inference wait begins only after Momento has durably stored `submitted`; it retains no outbound,
durable, file-token, CPU, file-worker, or SQLite capacity.

Deduplicate and face-group finalization run after all jobs for a run become terminal, even when some
jobs failed. New face results may join an existing manually merged face group.

Neither finalizer may reconstruct its whole result in memory or hold one transaction for the run.
`DeduplicateFinalization` is a durable versioned state machine:

1. A short transaction freezes the run's successful current clustering-result snapshot and creates an
   invisible finalization generation; failed/missing inference rows contribute only the recorded run
   warning.
2. In stable media-ID order, one SQLite page returns at most 256 source/candidate descriptors within
   the 1 MiB output bound. Bounded CPU operations compare those embeddings and persist at most one best
   near-duplicate edge and one best burst edge per source plus the comparison cursor. No page, source,
   candidate list, or edge set survives as a process collection.
3. Versioned component rows begin with each member labelled by its own media ID. For each cluster kind,
   indexed edge pages of 256 apply deterministic minimum-label propagation in short transactions.
   A durable pass cursor and `changed` count start another pass only when needed; every pass returns to
   the source tail. This may require many bounded passes for a long chain but never a long worker,
   recursive traversal, union-find heap, or attempt limit. Cancellation/version change supersedes the
   invisible generation.
4. Indexed `(generation, kind, label, media_id)` pages write versioned group/member rows and select the
   stable lowest media ID as representative. Near-duplicate and burst groups with identical ordered
   membership retain near-duplicate precedence through a digest/count comparison assembled
   incrementally, never a complete signature vector or JSON document.
5. One final short transaction verifies every cursor/pass/count, atomically switches the active
   duplicate-group generation, completes the run with any partial-inference warning, and schedules the
   prior generation for bounded cleanup. Crash recovery resumes the recorded cursor or observes the
   committed generation; it never deletes the old visible groups before the switch.

`FaceGroupFinalization` uses a separate durable versioned state machine because manual group identity
must survive reruns:

1. It snapshots successful current faces, immutable manual memberships/merge identities, and manual
   anchor versions into an invisible generation. Existing manual group IDs and user-authored membership
   are never deleted or renumbered.
2. It processes faces in stable face-ID order. For one face, SQLite pages at most 256 manual anchors at
   a time; bounded CPU comparisons retain only the best threshold-qualified manual group, breaking equal
   scores by lowest manual group ID. If matched, the generation stages that face as an automatic member
   of the existing manual group.
3. If no manual group matches, it pages at most 256 representatives from automatic groups already
   staged in this generation, retains only the best threshold-qualified group with stable tie-breaking,
   and otherwise creates one new version-owned automatic group using this face as its initial anchor.
   Thus a newly imported face can join a previously manually merged group without loading every anchor
   or group, while processing order and results are deterministic.
4. Representative selection is a paged reduction per affected group: each CPU page returns one best
   `(score, lowest_face_id)` candidate, and SQLite persists the running winner/cursor. It never collects
   every group ID or candidate. Versioned automatic memberships and representatives remain invisible
   until one short transaction verifies all face/group cursors, atomically publishes the generation,
   completes the run with any partial-inference warning, and schedules only the prior automatic
   generation for bounded cleanup. Manual memberships remain outside generation cleanup.

Both state machines use the existing 256-row/1 MiB SQLite and registered CPU-operation bounds, explicit
run/generation claim tokens, `available_at`, cancellation fencing, and ordinary FIFO-tail continuation.
Every staging table/index/WAL peak is added to `SqliteFootprintRegistry`. A transient error releases the
current worker/connection/DTO, records or retains the exact cursor, logs once at the durable boundary,
and requeues; a deterministic malformed embedding fails only that media input and is reflected in the
run warning. No failure prevents successfully inferred media from reaching the published groups.

## Source Organization

```text
src/backend/
  runtime/
    mod.rs
    bootstrap.rs
    config_bootstrap.rs       # sole bounded pre-runtime config read
    scheduler.rs
    job.rs
    panic.rs                 # process/executor panic policy and coordinated shutdown hook
    timezone.rs              # bootstrap-only system timezone snapshot used by cron
    space_budget.rs          # shared Journal/SQLite/Log capacity ledger
  executor/
    mod.rs
    cpu.rs
    process.rs              # closed child specs, descriptor leases, limits, and supervision
    io.rs
    sqlite.rs
  database/
    pool.rs
    operations.rs
  io/
    config_file.rs           # exact config capability and atomic runtime replacement
    file.rs
    handle_registry.rs
    stream.rs
    network.rs              # bounded async DNS and no-idle-pool outbound connector
    log.rs                  # fixed event ring and file-worker Logs sink
    static_files.rs
    journal.rs
  routes/
    middleware/
      request_logging.rs    # bounded body capture/redaction and one boundary event
    admin/
      file_operations.rs
  models/
    admin/
      file_operations.rs
  error.rs
src/common/
  llm.rs                    # one wire/record contract shared by both services
  logging.rs                # pure shared event formatting only; no sink, file I/O, or worker
src/backend_llm/
  logging.rs                # llm-service-owned existing sink/runtime policy
  result_records.py         # one bounded Python record writer and CRC32C implementation
  transport.rs              # one sink arbiter and credited result streams
  scheduler.rs              # durable manifest/records, defer timestamp, recovery
  provider.rs               # bounded provider record stream
```

Business modules depend only on executor handles and owned DTOs. New tests mirror their owner:
`src/backend/** -> tests/backend/**`, `src/common/** -> tests/common/**`, and
`src/backend_llm/** -> tests/backend_llm/**`; Python runtime tests remain beside the llm-service test
hierarchy. No flat catch-all test or duplicate helper implementation is added.

This layout is a move/refactor of existing ownership, not parallel replacement code:

- `src/backend/scheduler/{server,handle,queue}.rs` is dismantled into
  `runtime/scheduler.rs`, the private admission handles, and the three executor queues; delete the old
  scheduler modules only after every caller/test uses the new API.
- `src/backend/utils/process.rs` moves into `executor/process.rs`; extend its one implementation with
  closed child specs/leases/supervision and move `tests/backend/utils/process.rs` to the mirrored new
  test path. No second process helper remains.
- Split `src/backend/logging.rs`: bounded request capture/redaction moves to
  `routes/middleware/request_logging.rs`, the panic hook moves to the runtime failure owner, and the
  only file sink/ring moves to `io/log.rs`. Move existing tests to those mirrored owners, then delete
  the mixed module.
- Existing centralized `database/schema.rs`, `schema.sql`, and `queries.rs` remain the schema/query
  authorities. `database/operations.rs` owns only closed operation DTOs/dispatch and calls centralized
  query constants; it does not duplicate SQL strings.
- Existing route, processor, WebDAV, import, cron, and AI modules are updated in place to call the
  typed facades. A new similarly named module is not added beside an old implementation.
- `src/backend/processor/metadata/reverse_geocoding.rs` remains the single geocoder owner, but its lazy
  `OnceLock`/whole-buffer/`rstar` path is replaced by the bounded embedded-asset decoder and packed
  index. Startup publishes the immutable snapshot into `AppState`; routes and metadata orchestration
  perform lookups only through `CpuExecutor` and never trigger initialization lazily.
- `src/backend/auth/protection.rs` keeps request-source normalization and the public authentication
  policy facade, but removes its process-global bucket mutex/map. Its typed methods call the centralized
  bounded `auth_attempt_buckets` database operations, while `auth/password.rs` receives the immutable
  prebuilt dummy hash and exact validated Argon2 parameters rather than owning a `LazyLock`.
- `src/backend/processor/import/mod.rs` removes `CONTENT_HASH_LOCKS` and its async mutex map. Import
  orchestration calls the centralized `media_import_hash_claims` SQLite operations and uses the ordinary
  durable tail when another owner is active; no replacement in-memory keyed lock is added.
- `src/backend/constants.rs` keeps only value/MIME/task constants. Delete the process-global
  `PATHS`, `init_paths()`, and `paths()` APIs after every caller uses a typed `StorageRootId` operation;
  absolute `PathBuf` storage locations live only inside bootstrap/File I/O capability owners and never
  re-enter business modules through another global accessor.

## Implementation Phases

### Phase 1: Runtime foundation

1. Replace the current scheduler worker-permit model with one scheduler thread and the three executor
   handles.
2. Implement `RuntimeBuilder`: bootstrap and publish ready file workers first, bootstrap and publish
   ready SQLite workers/r2d2 second, start CPU workers third, then construct the scheduler and
   two-thread network runtime. Bootstrap channels are one-shot, private, and closed before public
   executor handles exist.
3. Add executor-owned bounded queues whose raw senders are held only by the scheduler/runtime builder,
   private closed command/output enums, typed facade methods, the exhaustive `OperationSpec` resource
   registry, typed `ExecutorError` replies, boundary-owned exhaustive `OperationFailure` disposition, and ordered
   shutdown. Add the derived ingress-credit capacity so every legitimate sender linearizes inside the
   bounded scheduler channel without an external waiter. Define the seven-member
   `SchedulerControlSource` enum, versioned level-triggered wakeups, and authoritative external state;
   no control payload may exist only in a coalesced command, and scheduler control handling may read
   only in-memory state. SQLite-backed control work is delegated to its closed durable source.
4. Replace the config shape and update all templates and callers without compatibility aliases.
   Remove backup upload concurrency, metadata lease/attempt settings, and durable-worker polling
   intervals; retain only the three worker counts. Add versioned event signals plus exact persisted
   retry deadlines, then add the exact source-owned
   worker maxima, checked `RuntimeSizing` derivation, fallible conversions, and pre-spawn allocation
   validation. Fix runtime-owned thread stack size, calculate every deterministic buffer/channel/DTO
   reservation and the exhaustive process fd peak, enforce `MAX_DERIVED_RUNTIME_BYTES`, and compare
   `required_open_files` with soft `RLIMIT_NOFILE` before spawning.
   Implement the sole bounded pre-runtime config read and its identity/hash handoff into the private
   file bootstrap. Publish the exact `ConfigFileCapability` before runtime config endpoints, move every
   later config read/atomic write through File I/O plus CPU `toml_edit`/full validation, and enforce the
   generation lease, external-edit CAS, temp/rename/sync recovery, same-data-filesystem reservation, and
   separate-filesystem fixed token described above.
5. Keep Axum construction in the application factory and place cloned scheduler/executor handles in
   `AppState`; route modules receive handles rather than pools or filesystem services.
6. Enforce strict FIFO in every executor and the per-stream invariant of at most one executing plus
   one queued chunk. Queue-capacity waits must suspend only the orchestration future and apply network
   backpressure.
7. Add the derived connection/request/inbound-stream/outbound-stream/inbound-durable-stream/file-chunk
   admission counters, global durable orchestration counter, phase-ownership transfer, and
   non-queue-dependent RAII release.
   Test the pre-journal non-waiting boundary and the post-journal durable handoff separately.
   Route both `try_execute` and `execute_durable` through the scheduler arbiter; no AppState or business
   handle may retain a raw executor sender.
8. Implement source-owned HTTP phase deadlines and WebSocket heartbeat expiry at the connection
   boundary. Pause network-idle accounting during internal executor backpressure; a progressing large
   transfer must never be subject to a total-request timeout.
   Configure the fixed HTTP/1.1 read/no-pipeline limit before accepting sockets, charge its complete
   pre-handler connection state, keep server/outbound/WebSocket transports HTTP/1.1-only, reject
   non-identity request content encodings, and remove automatic response-compression middleware.
9. Implement ordered shutdown as one explicit scheduler state machine:
   `running -> quiescing -> securing_mutations -> draining -> stopped`.
   Quiescing closes listener/connection/request admission, stops cron timers and new durable claims,
   and rejects new pre-journal work. Securing mutations cancels pre-journal work and makes every
   journaled group reach `rollback_pending`, `publishing`, `publication_failed`, `files_committed`,
   `finalize_failed`, or `completed` with durable cleanup entries, handing ownership to recovery before
   its request future ends. Every non-journal durable claim, including an active outbound submission,
   cooperatively persists its terminal/retry handoff and releases transport/file state before the
   network runtime stops. Draining closes executor senders only after those handoffs, terminates
   supervised child-process groups and receives already queued replies. It then closes event production,
   asks file workers to drain/sync the bounded log ring and report dropped counters, closes executor
   senders, joins executor threads, and drops r2d2; only the final rate-limited console diagnostic may
   occur after file-worker join.
   `SHUTDOWN_GRACE = 30 seconds` is a source-owned deadline; if an uninterruptible OS syscall exceeds
   it, the process logs both the exact journal group/entry/path-claim evidence and every unresolved
   non-journal durable source, row ID, state, claim token, and correlation attempt, then exits. It relies
   only on journal recovery plus `DurableClaimRecovery` rather than waiting forever or deleting
   uncertain files.
   Implement the closed `SchedulerAdmissionKind` contract at the same time: quiescing rejects only
   `NewClaim`; every `ExistingClaimCompletion` keeps its original claim/credit, and every
   `RecoveryHandoff` remains admissible until resolved or the deadline. Register shutdown handoffs
   promptly after new claims stop but preserve the existing monotonic FIFO order without a bypass
   queue. Atomically reject/remove all not-yet-linearized `NewClaim` waiters at transition, while
   treating already-enqueued executor commands as pre-quiesce admitted work that must complete or hand
   off.
10. Add the closed `DurableSourceId` registry and one-pending-reservation invariant. Replace every
    dynamic worker/job identity and every per-feature queue admission path with that shared registry.
11. Implement explicit file and SQLite bootstrap barriers before recovery, network admission, and
    HTTP startup. Require an existing configured data directory, acquire its lifetime lock without
    creating a lock file, and allow only read-only root/log/database inventory in `RecoveryOnly`.
    After reconstruction reaches `RecoveryReady`, resolve `LogOverQuota` through zero-growth bounded
    pruning first; then budget-create/sync missing typed roots, run mount-ID plus real rename probes, and
    close the generic data-root capability before publishing
    any handle. Verify Static-root non-overlap without mutating Static. Start with console-only logging, derive the shared
    `DataDirSpaceBudget` in `RecoveryOnly`, distinguish a provably fresh database from an existing one,
    publish a fresh schema only through a synced same-directory bootstrap database plus atomic rename
    and parent sync,
    load existing durable reservations through a read-only/no-initialization SQLite bootstrap. When a
    valid crash-left WAL must be recovered before that read is possible, use only the single checked
    recovery-floor token, publish the resulting allocation, and retry the read; reconstruct all
    owner/file/allocation evidence before enabling ordinary SQLite/Journal work or the quota-controlled
    file sink. In the same private file bootstrap, resolve and validate the immutable system IANA
    timezone snapshot from the exact bounded localtime/timezone/zoneinfo evidence; pass it explicitly
    to cron scheduling and remove every `chrono::Local`/libc-localtime/runtime timezone read. No startup
    file append may bypass the ledger.
12. Put `catch_unwind` around each executor operation and each durable business orchestration. Convert
    operation panics to permanent `WorkerPanic` failures only after executor-specific cleanup
    verification, use the normal awaited durable disposition path without retrying deterministic
    panics, and make executor-loop, registry, scheduler, or cleanup-invariant panics trigger coordinated
    fatal shutdown and startup recovery.

### Phase 2: SQLite executor

1. Set r2d2 size from `sqlite_workers`.
   Give it the one-thread source-owned scheduled maintenance pool. Install the budget-aware manager,
   eagerly warm exactly that many validated connections before
   readiness, and route every later replacement through its bounded SQLite token; prohibit lazy
   request-worker creation.
2. Add typed read and transaction operations.
   Register each read's row/output/cardinality/cursor bounds, enforce the global 256 KiB/1 MiB ceilings
   with charged incremental row mapping, and add schema/write validation so a single row always fits.
3. Replace every direct pool checkout with `SqliteExecutor` calls returning owned DTOs.
4. Centralize busy/locked classification and durable tail requeue through the shared exhaustive
   `OperationFailure`/`DurableDisposition` mechanism.
5. Enforce connection, busy, and total operation deadlines with the SQLite progress handler; split
   every potentially large query into paged/checkpointed operations.
   Configure memory-only bounded temp state and zero SQLite internal threads, declare/charge each
   operation's temp heap, prohibit ATTACH/full VACUUM/temp files, and add indexes/continuations until no
   unbounded query plan spills or sorts.
6. Derive `DataDirSpaceBudget` before opening SQLite; enforce shared physical capacity, the WAL
   limit/checkpoint policy, and mandatory per-write `max_growth_bytes` on every
   closed `DbOperation`. Make every writer acquire a SQLite-class reservation through the common ledger
   before its transaction and update allocation before release. Do not impose a fixed SQLite database
   quota or override SQLite's native `max_page_count`.
7. Require every `DbOperation` to carry an explicit `SqliteCapacitySource`. Implement fresh tokens and
   one-time provisional-parent promotion, consuming construction children, and the single rolling
   cleanup child in the same ledger, with cursor/remaining-bound updates in the business transaction,
   rollback return, crash reconstruction, and no double reservation.
8. Move authentication-attempt source/identity buckets into the bounded centralized SQLite schema and
   operations. Enforce the 65,536-row ceiling, fixed digest keys, atomic two-bucket update, 256-row
   expiry pruning, fail-closed saturation, capacity footprint, and maintenance continuation; remove the
   shared in-memory authentication map/mutex after callers and tests use the database owner.

### Phase 3: Filesystem and network I/O

1. Move Axum, WebDAV, streaming, outbound HTTP, and LLM WebSocket work to the network lane.
   Replace blocking/default DNS and idle HTTP pooling with the bounded async resolver, fixed TTL cache,
   one-socket-per-operation connector, and sequential five-hop redirect policy; include their memory/fd
   ownership in `RuntimeSizing`. Load only the two exact bounded system resolver files through the
   private file bootstrap and pass an owned snapshot before starting network workers. Replace
   `dav_server::LocalFs`/`DavHandler` rather than wrapping it: the Momento-owned WebDAV protocol layer
   must use only `FileIoExecutor`, bounded traversal continuations, and incremental multistatus response
   serialization. Remove the `dav-server` dependency after its existing method/precondition/status
   behavior is captured in contract tests.
2. Replace direct `tokio::fs` and `std::fs` calls with `FileIoExecutor` operations.
3. Open the complete fixed `StorageRootId` registry at startup and make every file operation
   descriptor-relative. Add the generation-checked file-handle registry so descriptor close never
   runs outside a file worker. Use the executor-local non-lossy close signal, bounded post-operation
   sweeps, short slot-state locking, exact budget-class capacity formula, and explicit StreamSession,
   CpuPipeline, SourceMaintenance, and FileWorkerHandoff accounting. Retain each class debit until the
   file worker completes physical close; logical token drop never releases registry capacity early.
   Implement `ChildPinned` plus the move-only generation-checked `ChildDescriptorLease` conversion;
   only a file worker may enter `ChildPinned` or restore a returned lease to `InFlight`/`Idle`, while a
   dropped lease may only mark the matching generation `ClosePending`. A CPU worker may use its
   immutable raw number solely in the closed spawn file actions and must return/drop it after the child
   is reaped.
   Include inbound durable result streams in the `StreamSession` capacity term and use Journal-owned,
   synced result files as the durable inbox payload; parsing and deletion reopen them only through the
   bounded source-maintenance class.
   Migrate every `constants::paths()` caller to a typed root-relative facade and then delete
   `PATHS`/`init_paths()`; do not retain an absolute-path compatibility accessor.
   Replace static `ServeDir`
   with the shared read-only `Static` responder. Retain one primary descriptor for each stream session;
   chunk admission operates on it without reopening the path. Use weak metadata ETags, require
   immutable or atomically replaced static assets, and preserve the complete method,
   same-descriptor snapshot, exact weak/strong precondition order, range, cache, MIME, SPA,
   API, and WebDAV semantics defined above. Add bounded chunk streaming; use only the durable
   exact/subtree path-claim predicate for WebDAV mutation conflicts.
4. Add the generic group/entry/path-claim journal, atomic forward publication, pre-commit
   compensation, post-commit cleanup, and startup recovery.
   Add source-derived `DataDirSpaceBudget`, its shared SQLite/Journal data capacity, bounded Log class,
   and recovery floor, plus the common private `SpaceReservationToken` implementation with explicit
   `ProvisionalSpaceToken -> DurableSpaceCheckout` owner commit, durable reservation rows, checked
   additional-peak/new-allocation accounting, one-epoch capacity snapshots, startup reconstruction, and
   the fresh-space admission formula. Require it for every byte-producing group, including declared and chunked uploads, import
   copies, derived/external outputs, deterministic inbound-result `Publish` groups, and face crops.
   Extend unknown-size work before reading/writing each chunk, apply `RLIMIT_FSIZE` to bounded child
   output, and reject any operation that cannot establish a safe peak.
5. Sync all affected parent directories after atomic rename, add permanent `finalize_failed` state
   plus `publication_failed` and `cleanup_failed` states with administrator-only idempotent retry, and
   block conflicting claimed paths while an incomplete published journal exists.
6. Replace whole-file copy, recursive traversal, and recursive deletion with chunked or 256-entry
   continuation operations that return to the shared FIFO tail. Enforce the 256-entry group maximum;
   implement recursive directory COPY as one private temporary tree/root Publish plus the bounded
   depth-first durable cursor, per-entry identity checks, per-file/directory sync, reservation
   extension, crash resume, and bounded rollback cleanup rather than one SQL/journal row per descendant.
7. Express PUT/PATCH/COPY, MOVE with overwrite, DELETE tombstoning, multi-artifact metadata, and
   multi-crop face results through the same entry-action implementation. Remove feature-specific file
   rollback code after all callers and mirrored tests use the group journal.
8. Convert permanent media deletion, trash purge, media regeneration cleanup, metadata reset, AI
   clean/reset, face reset, import recovery, and stale temporary cleanup to Tombstone/Cleanup entries.
   Delete every business-level direct or ignored filesystem deletion path.
9. Add transactional `Read`/`Write` plus `Exact`/`Subtree` path claims and the indexed
   bounded ancestor/descendant conflict predicate.
10. Add administrator-only journal list/detail/retry operations, durable retry request receipts,
    compact terminal summaries, and retention-bounded terminal pruning.
11. Replace any logger thread/direct file appender with the checked fixed-capacity event ring and typed
    `Logs`-root operations drained in bounded batches by file workers. Bound the sink by its derived quota
    and 128 MiB rotations, reserve append/rotation peak through the common ledger, prune oldest closed
    rotations before append, update allocation before releasing the token, expose dropped-by-level
    counters, add the empty-to-nonempty non-lossy coalesced file-worker wake/recheck protocol, and fail
    safely to a rate-limited console health diagnostic without consuming the recovery floor or blocking
    a business worker.
    Remove Momento's `init_synchronous_logging` path from `src/common/logging.rs`. Keep only pure shared
    formatting in common, move llm-service's existing appender/guard into
    `src/backend_llm/logging.rs`, and move `tracing-appender`/sink-only dependencies from common to the
    llm-service manifest. Momento declares only the tracing subscriber support needed for its bounded
    ring layer in `src/backend/Cargo.toml`; it cannot call either a common or llm-service file sink.
    Remove `EnvFilter::try_from_default_env`/`RUST_LOG` from both services; each service owns a fixed
    source `INFO` baseline because log level is not one of the repository's supported config-loader
    environment overrides.

### Phase 4: CPU work

1. Move decode, crop, resize, hashing, parsing, comparison, and Argon2 to `CpuExecutor`.
   This includes every bounded JSON/form/XML control-body parse, authentication cryptographic check,
   and non-streaming response serialization; leave only fixed protocol framing and lightweight
   method/path/header extraction on the network workers.
   Fix password input and Argon2id parameter/workspace/parallelism bounds, validate stored hash
   parameters before verification, eagerly build the immutable dummy hash through a pre-listener CPU
   command, and charge the all-CPU-slots cryptographic peak in `RuntimeSizing`.
   Replace supplemental/exif JSON `read_to_string`/generic `Value` parsing with the 4 MiB encoded,
   1 MiB typed, depth/field/string-bounded metadata parser. Parse sources sequentially, apply the exact
   four-candidate Takeout selection order, exclude binary exiftool fields, and make over-bound input an
   explicit metadata failure.
2. Route ImageMagick, FFmpeg, and exiftool through supervised CPU operations.
3. Split large-media processing into the bounded file-read/CPU/file-write pipeline.
4. Add checked image-header geometry validation, incremental scanline/region decoding, two charged
   16 MiB tiles per active CPU pipeline, and region-only face cropping. Route codecs without a proven
   incremental decoder through the supervised ImageMagick path with its fixed memory/address-space
   limits, map cache disabled, and fixed disk scratch ceiling; never fall back to thumbnails or an
   unbounded full-raster buffer.
5. Apply and verify one-thread limits for every supervised child/native runtime; fail the operation
   when the limit cannot be enforced. Implement the closed per-tool address-space and monotonic
   start/total/termination deadline policies, including the checked duration-derived FFmpeg limit.
6. Replace `Command::output()` and async child-output tasks with one bounded process supervisor that
   drains diagnostic pipes using fixed memory and sends binary output only to pre-opened journal
   descriptors.
7. Enforce the typed child descriptor allowlist, close-on-exec policy, immutable-staging fallback,
   `ChildDescriptorLease` handoff, output sync after lease return, and parent/child descriptor cleanup
   on every spawn, failure, timeout, cancellation, reply-loss, and panic path.
8. Replace reverse geocoding's lazy whole-gzip decode and `rstar` bulk load with the bounded streaming
   asset decoder, fallibly preallocated packed kd-tree/string arena, CPU-executor startup command, and
   immutable AppState snapshot. Validate and charge the embedded/decoded/record/field/runtime/scratch
   maxima before listener publication; remove `rstar` after exact nearest-neighbor golden tests pass.

### Phase 5: Durable workers

1. Convert local/WebDAV/backup import, metadata, LLM submission, LLM cancellation, LLM result
   persistence, deduplicate, face grouping, and maintenance to the common durable
   claim/result/error pattern.
2. Keep manual and cron AI starts on the same durable code path.
   Replace the current per-feature async cron loops and unbounded deduplicate catch-up loop with the
   scheduler-owned seven `CronTaskId` timer states, explicit immutable timezone/config generations,
   one permanent ingress credit per task, 256-occurrence CPU continuations, and one idempotent latest-
   due start through the manual processor operation.
3. Requeue transient failures at the source tail only after all executor resources are released.
4. Preserve independent concurrent AI job types and llm-service-controlled inference ordering.
5. Implement outbound durable phase ownership: acquire outbound admission before durable claim,
   release durable admission immediately after the claim and immutable descriptor snapshot, retain
   only the bounded outbound/session/file state during verification and transfer, drop the logical file
   token, request close, and drop the byte buffer immediately when a peer outcome exists, then reacquire durable admission for the
   local state transition. Release every guard before retrying at the tail.
6. Replace the existing split WebSocket writer/reader tasks, generic binary-message queue, and
   unbounded per-job event channels with the one connection-owned `LlmTransport` state machine,
   round-robin one-frame data arbitration, dependency-ordered per-session markers, reserved bounded
   urgent-control mailbox, bounded control burst, continuous read polling, bounded inbound durable
   stream admission, and independent per-job failure/requeue behavior. Give external urgent producers
   one closed owner credit each, keep loop-originated urgent messages in direct one-item pending slots,
   and prohibit the transport loop from waiting on its own mailbox.
7. Add the closed `DurableClaimRecovery` registry for every durable source. After journal recovery,
   one startup transaction requeues every prior-process nonterminal claim, while preserving external
   wait states and replaying orphaned LLM `submitting` rows with the same durable correlation attempt.
8. Make durable result inbox receipt atomically supersede matching queued/submitting/submitted claims,
   block replay eligibility, and remain idempotent. Make pending cancellation scopes gate submission
   claims. Close the bounded scheduler scope gate and invalidate matching active mutation generations
   before scope commit, retaining that fence across a failed transaction. Materialize exact
   cutoff-bounded job IDs in 256-row transactions, compute the canonical digest through durable
   256-record SHA-256 checkpoints, and stream them through the
   credited Start/4096-ID Chunk/Finished protocol with exact count/hash, atomically publish the
   client-scoped llm-service cancellation set, and delete Momento outbox/items only on matching remote
   acknowledgement. Use canonical fixed-width ordered IDs for bounded lookup, make the published set
   authoritative at every queue/inference/result boundary, convert it crash-safely to exact markers,
   and permit at most one unconverted set per authenticated client.
   Extend the shared Momento/llm-service protocol to manifest-first streamed results with exact size and
   SHA-256 verification, deterministic pre-receive journal/disk reservation, one-credit 64 KiB frame
   flow control, llm-service round-robin result sending, Momento-owned durable inbox files, and
   `resultReceiptDeferred`. Replace monolithic provider/durable/wire results with the shared bounded
   `momento-result-records-v1` representation, its exact 24-byte little-endian header, closed
   kind-specific payload table, pre-allocation length/count validation, CRC32C, and compile-time encoded
   and normalized size proofs; update every local model runtime, provider, queue
   manifest, sender, receiver, and recovery caller directly without a compatibility decoder. Persist
   deferred `nextDeliveryAt` without consuming a delivery attempt; `resultReceiptRejected` is
   permanent/protocol-only. For every Rejected correlated to an active Momento job, first commit the
   bounded terminal-failure transaction; temporary inability to do so is Deferred, never premature
   remote disposal. Atomically convert a committed inbound admission to its urgent receipt
   credit through sink-confirmed `resultReceived`. Enforce the exact total, record-count, minimum-record,
   continuation, and per-task ordering bounds before or during receipt.
   Commit runtime output through synced temporary record/manifest files and an atomic
   `processing -> callback_pending` directory move; recovery delivers a complete pair without rerunning
   inference and requeues only incomplete output.
   Add inbox state/job-version fencing so cancellation wins every receipt/persistence race, late
   terminal/deleted results are acknowledged without recreation, and discarded inbox files are cleaned
   through the shared journal path. Apply the same job-version receive token before every chunk credit,
   Journal Publish, and inbox commit. Add the exclusive generation-checked `JournalMutationLease` so
   cancellation records intent and invalidates ownership but cannot race rollback/cleanup against an
   in-flight bounded write. Carry authorization in every mutating file command, recheck it in the file
   worker immediately before the syscall, and close/invalidate the mutation gate before the SQLite
   cancellation transaction. Cancellation follows `prepared -> rollback_pending` before the first
   forward rename or completes forward with a discarded outcome afterward, acknowledges discard with
   `resultReceived`, and never creates an inbox row.
   On any post-`resultReady` transient failure, permanent invalid result, or cancellation, stop credit,
   close logical resources, and release network/frame admission only after the prepared group is durably
   owned by rollback or forward cleanup; registry and disk debits remain with their durable owners while
   only the final Deferred/Rejected/Received response differs. On Rejected, llm-service atomically moves
   `callback_pending -> failed` only after temporary-write/sync/rename/directory-sync of failure evidence;
   recovery removes temporary-only evidence for redelivery or finishes forward from a complete file,
   and never reruns inference or retries delivery after the durable rejection.
9. Make `LlmResult` use the two-pass result algorithm: first incrementally validate and idempotently
   stage every bounded record without creating files; then build a durable artifact plan, prepare and
   publish complete generic journal groups of at most 256 entries under one result-product version, and
   expose none of them until one token-fenced terminal reference switch proves every group completed.
   Persist record/group progress and continuation ordering. Before `resultReady`, acquire and durably
   bind a schema/task/manifest-derived SQLite growth reservation as well as the Journal byte reservation;
   a failure or cancellation before the switch retains the prior product and schedules paged generic
   cleanup for all invisible new artifacts before releasing unused reserved capacity. Add the durable
   `llm_result_cleanup` owner and bounded 256-row/4 MiB SQLite cleanup stage under the same `LlmResult`
   source; include its WAL peak, shrink the large parent to the one-batch cleanup recovery bound at the
   terminal-outcome transition, and release each reservation only after its corresponding cleanup cursor
   becomes terminal.
10. Replace metadata's media-ID-only/time-lease logic with token-fenced claim guards, the scheduler
   active-claim registry bounded by durable admission plus outbound-transfer admission, cancellation,
   common startup orphan recovery after journal recovery, journal
   authorization, and terminal persistence. Remove live expiry scans and the old attempt-count path;
   update every caller, query, test, fixture, and config template directly without a compatibility
   wrapper.
11. Replace deduplicate's process-local pending-cluster maps/whole-result JSON and face grouping's
    all-faces/all-anchors/long-transaction path with the two durable versioned algorithms above. Add
    bounded candidate/edge/label-propagation/group-publication cursors for deduplicate and bounded
    manual-anchor/automatic-representative/representative-reduction cursors for faces. Preserve manual
    group IDs and memberships, stage new automatic memberships invisibly, publish each complete
    generation once, clean the old automatic generation in bounded batches, and resume exactly after
    crash/cancellation/transient failure.
12. Replace the import `CONTENT_HASH_LOCKS` map with the token-fenced durable content-hash claim table,
    partial unique imported-media hash index, tail retry, and startup orphan recovery. Preserve the
    shared local/WebDAV finalization path and exact-duplicate ownership/timestamp/sidecar/metadata-rerun
    semantics in one transactionally fenced implementation.

### Phase 6: Removal and verification

1. Delete the old scheduler permits, nested capacity model, direct pool access, direct filesystem
   access, and per-feature concurrency code.
   In `src/backend/Cargo.toml`, replace Tokio `full` with the exact runtime/net/time/sync/signal/io
   features used by `RuntimeBuilder`, remove `tower-http`'s `fs` feature with `ServeDir`, remove `glob`
   after typed bounded enumeration replaces it, and delete any now-unused blocking/file helper
   dependency. Add `chrono-tz` for the compiled, explicitly bootstrapped IANA snapshot rather than using
   `chrono::Local`. Keep r2d2 as the declared exception. Move sink-only tracing dependencies according to
   the logging ownership above. Inspect `cargo tree -e features` so a transitive feature does not
   silently re-enable an application filesystem/blocking service. This does not change
   `src/backend_llm`'s Tokio/model-runtime policy.
   Add the Rust `crc32c` dependency to `src/common/Cargo.toml`. Do not add a CRC package independently
   to every Python model venv: all Python runtimes import the one `src/backend_llm/result_records.py`,
   whose fixed 256-entry Castagnoli table and bounded encoder are covered by the same Rust/Python golden
   vectors. Ordinary IEEE CRC32 is never accepted as CRC32C.
   Remove `rstar` after the bounded packed reverse-geocoder index replaces it; retain `flate2`/`csv`
   only for the single bounded streaming embedded-asset initialization path.
2. Update `AGENTS.md` to describe the three execution domains and SQLite executor accurately.
3. Run Rust formatting for `src/common`, `src/backend`, and `src/backend_llm`; run Clippy with all
   targets, release mode, and `-D warnings` for all three manifests; fix every warning at its cause rather than
   adding `allow`, lowering lint levels, or suppressing output. Run all three Rust integration suites and all
   affected Python llm-runtime tests, including shared record golden vectors and crash/recovery cases.
4. Run frontend format-check, lint, Vitest, TypeScript compilation, and Vite production build after
   updating configuration/status/API callers. No generated `dist` output is committed as source.
5. Build both services in release mode with debug symbols disabled. Run the repository-root
   `./build_docker.sh /absolute/path/to/keystore-directory` final integration path when the keystore is available; it
   must rebuild both `momento-api` and `llm-service`, obtain its release APK only through
   `./build_android_client.sh release`, and embed that APK in Momento. Never use a debug Rust build as
   the default because of disk usage.
6. Any Android-specific verification remains exclusively
   `./build_android_client.sh verify` in Docker. Do not invoke host Gradle/ADB/SDK commands or ad hoc
   Android containers. The route/DTO audit must search Android callers; if one changes, run this verify
   step before final packaging. If no Android source/build input changed, the Docker release invoked by
   `build_docker.sh` is the only Android step required for this backend refactor.

The executable verification order uses the repository's actual independent manifests and never
creates a default Rust debug target:

```bash
cargo fmt --manifest-path src/common/Cargo.toml --all -- --check
cargo fmt --manifest-path src/backend/Cargo.toml --all -- --check
cargo fmt --manifest-path src/backend_llm/Cargo.toml --all -- --check

CARGO_TARGET_DIR=build/rust-release cargo clippy --release --manifest-path src/common/Cargo.toml --all-targets -- -D warnings
CARGO_TARGET_DIR=build/rust-release cargo clippy --release --manifest-path src/backend/Cargo.toml --all-targets -- -D warnings
CARGO_TARGET_DIR=build/rust-release cargo clippy --release --manifest-path src/backend_llm/Cargo.toml --all-targets -- -D warnings
CARGO_TARGET_DIR=build/rust-release cargo test --release --manifest-path src/common/Cargo.toml
CARGO_TARGET_DIR=build/rust-release cargo test --release --manifest-path src/backend/Cargo.toml
CARGO_TARGET_DIR=build/rust-release cargo test --release --manifest-path src/backend_llm/Cargo.toml

for test_file in tests/backend_llm/*.py; do
  PYTHONPATH=src/backend_llm python3 "$test_file"
done

pnpm --dir src/frontend run format:check
pnpm --dir src/frontend run lint
pnpm --dir src/frontend run test
pnpm --dir src/frontend run build

# Only when the route/DTO audit changes Android source or Android tests:
./build_android_client.sh verify

# Required final packaging when signing material is available:
./build_docker.sh /absolute/path/to/keystore-directory
```

The Python loop is intentional because the existing llm runtime tests include executable unittest
files that are not all named `test_*.py`; plain unittest/pytest discovery would silently skip them.
`build/rust-release` is disposable build output, not source or `dist` output.

## Required Tests

- Exact configured thread counts and names; no unapproved business or logger thread. Only the scheduler,
  configured network/file/CPU/SQLite workers, and exactly one r2d2-owned maintenance thread exist.
- Source/dependency-audit logging: `src/common/logging.rs` performs formatting only and cannot open a
  path or own an appender guard; Momento has no `tracing_appender`, synchronous file writer, or direct
  rolling dependency/call path; only `src/backend/io/log.rs` can reach its typed Logs sink.
  llm-service's existing sink remains service-owned in `src/backend_llm/logging.rs` and its behavior is
  covered by the moved mirrored tests rather than coupled back into Momento. Setting `RUST_LOG` changes
  neither service; no logging module reads an environment variable, and the fixed INFO threshold is
  tested at DEBUG/INFO/WARN/ERROR.
- Exercise valid/invalid JSON, form, XML, binary, multipart, sensitive, base64, UTF-8/non-UTF-8,
  48 KiB boundary, and 1 MiB control-body boundary. The one CPU parse returns the business DTO/error
  and log capture together; middleware does not reparse, retain an `Arc<Mutex>` body tap, await CPU, or
  delay the response for logging. Streaming media captures only the fixed marker, while the final event
  still contains method/path/status/duration and the permitted redacted body representation.
- Compile-time/source-audit every executor facade: no public raw command constructor/sender, closure,
  trait-object job, arbitrary function, executor handle, pool connection, absolute path, descriptor, or
  unbounded collection can enter a command. Enumerate every command/output pair through
  `OperationSpec`, reject mismatched outputs as an invariant failure, and fail the exhaustive test when
  a new variant lacks its byte/temp/descriptor/cancellation/growth declaration.
- Test every worker minimum and maximum, one below/above each boundary, every derived-count overflow,
  fallible `usize` conversion, and simulated channel/registry allocation failure. Startup must reject
  them before spawning the first worker and must never wrap a capacity or partially construct a runtime.
- Attempt to change each worker field through every runtime configuration endpoint and prove it is
  rejected as startup-only without changing file/live state. Editing TOML and restarting produces the
  exact new topology/status; no running executor observes a later `ConfigManager` worker value and the
  removed `available_workers` key is always unknown.
- Round-trip and live-update the seven `[llm].*_cron` expressions while preserving unrelated comments,
  `[llm].enabled`, and `[face_group]` threshold/weights.
  The default face-group similarity threshold is exactly 0.50. Reject the old representative section,
  per-feature enable flags, timezone field, and every removed concurrency/lease/attempt key without
  silently changing the running snapshot.
- Bound the pre-runtime CLI config immediately below/at/above 1 MiB and test absolute/cwd-relative paths
  immediately below/at/above 4,096 bytes, 256 components, and the 200-byte final filename limit. Reject
  parent components immediately above 255 bytes, empty/dot/dot-dot/repeated/NUL/non-UTF-8 components,
  every intermediate/final symlink or magic link,
  overlong reserved update/init temporary names, missing, non-regular, unreadable, malformed, and
  identity/hash change before file-bootstrap revalidation.
  Exercise nesting/value/key/string/decoded-string/normalized-heap limits, duplicate keys, numeric
  overflow, many tiny values, and allocation failure; lexical preflight rejects before generic TOML
  amplification and the full old/update/new snapshot peak matches `RuntimeSizing`. No
  other main-thread application file read is permitted. For runtime updates, race two administrators
  and an external editor; only the matching config generation/hash wins, unrelated TOML/comments are
  preserved, full validation precedes mutation, and file/live snapshots publish together after sync.
  Crash/fail at temp create/write/sync, pre-rename CAS, rename, and parent sync on both the data and an
  external config filesystem. Pre-rename failure leaves the official/live snapshot unchanged;
  post-rename sync failure is fatal rather than split-brain; restart accepts one complete official file
  and cleans only the exact reserved temp at the permitted recovery phase. Include both config buffers,
  fixed capability fd, metadata peak, and external token in sizing/ownership tests.
- Exercise `--init-config` with missing/existing targets and crash/failure at template temp write/sync,
  rename, and parent sync; it is the sole pre-runtime write, recovers only the exact matching template
  temp, exits without any worker, and never replaces existing config. Exercise stored and environment-
  only password-reset requests: a stored true flag is atomically consumed through the ready config
  capability before the temporary auth override/listener, all unrelated text survives, and failure
  aborts startup without a direct main-thread write.
- Calculate the exact deterministic reservation at the default configuration, at each individual
  worker maximum, and immediately below/above `MAX_DERIVED_RUNTIME_BYTES`. Enumerate bootstrap,
  pre-listener initialization, and running ownership states and take their maximum. Within running,
  add independent owners but take the exhaustive maximum legal state per request, CPU worker, and
  durable owner. Prove request body/parse DTO, DB/response buffers, image/Argon2 scratch, and metadata/
  LLM-result buffers coexist only where their state formula says so; type-state transitions consume the
  old large object before allocating the next. Include fixed thread stacks, all admitted 1 MiB buffers,
  channel envelopes, request/log capture, diagnostics, file handles, and every bounded transport DTO.
  The LLM-result durable member includes one 1 MiB record, 2 MiB normalized DTO, 4 MiB persistence
  batch, and the exact compile-time executor-envelope `size_of` formula; its enclosing maximum permits
  all durable owners to select it, while inbound receive frames remain separately admitted. The CPU
  maximum includes image tiles, exact Argon2 workspace, geocoder initialization scratch when applicable,
  and every other registered operation without summing mutually exclusive CPU commands. Startup rejects
  an over-budget combination before spawning even when
  its individual counts are legal. The error reports the category breakdown and the feasible maximum
  for each one-at-a-time worker adjustment; parser limits are never presented as a guaranteed runnable
  combination. The documented `8/8/4` default itself must pass the 1 GiB limit with nonnegative
  reported headroom after HTTP/1 buffers, request-phase maxima, CPU-operation maxima, durable-operation
  maxima, raw inbound LLM controls, and every other listed category plus the embedded reverse-geocoder
  image/persistent packed index/startup scratch are
  included; otherwise change the documented default or source-owned derivation before
  implementation rather than shipping a startup-invalid template.
- Initialize reverse geocoding with compressed, decoded, record-count, field-length, malformed-gzip,
  malformed-TSV, coordinate, hash, allocation-failure, and one-over-bound fixtures. Instrument peak
  memory to prove there is no complete decoded copy, second bulk-load index, lazy `OnceLock`, or
  `rstar` allocation; initialization is exactly one CPU operation before AppState/listener publication.
  Compare packed-kd-tree nearest results with an exhaustive reference for every shipped record and
  boundary/random coordinates, then saturate CPU admission during normal lookups and prove routes and
  metadata wait/retry through their ordinary bounded ownership without initializing another snapshot.
- Boundary-test 1,024-byte passwords and exact Argon2id `m/t/p` parameters, including stored hashes with
  oversized memory/time/parallelism, malformed fields, and allocation failure. Only exact accepted
  parameters run; every unusable/unknown-user case executes the already built dummy verification with
  indistinguishable policy, no lazy initialization, and no internal parallel worker. Run `cpu_workers`
  simultaneous verifications and measure the peak at or below the all-slot workspace/scratch charge.
- Flood authentication with repeated and unique source/identity pairs below/at/above 65,536 live
  buckets. The atomic SQLite operation never records only one of the pair, never stores raw identity or
  source text, applies lockout consistently across restart, and fails unseen keys closed at capacity.
  Busy/timeout/capacity failures never reach Argon2 or authenticate. Expiry and `Maintenance` prune at
  most 256 rows per transaction, return to the ordinary tail, and eventually restore capacity without
  evicting an unexpired lockout; query-plan and fragmentation tests verify the registered table/index/WAL
  footprint.
- Parse supplemental and exiftool metadata immediately below/at/above the 4 MiB encoded, 1 MiB
  normalized, 4,096-field, depth-16, and 256 KiB string limits, including duplicate fields, binary exif
  values, malformed UTF-8/JSON, huge arrays, and allocation failure. No path constructs a generic
  `Value`, reads an unbounded string, emits an oversized child file, or silently truncates; the one media
  job fails with a bounded reason and releases its buffers. Exercise all exact/truncated/collision-index
  sidecar combinations and disagreeing values: at most four candidates are probed in the documented
  order, the first regular non-symlink match wins, and only present supplemental fields override base
  metadata. The runtime sizing test makes this pair a `per_durable_owner_peak` candidate and verifies
  that every durable slot may select it without also charging another mutually exclusive durable phase.
- Lower soft `RLIMIT_NOFILE` in an isolated test process to immediately below and exactly at the checked
  formula. Startup below it creates no worker/listener; at it, saturate inbound/outbound sockets, the
  complete file registry and log slots, every r2d2 connection, roots/lock, and the maximum supervised-
  child parent descriptor peak without `EMFILE`. Adding a `StorageRootId`, process descriptor,
  Tokio/scheduler platform fd, or connection admission fails exhaustive sizing tests until its fd
  contribution is declared. Repeat at every `BootstrapFdSpec` phase peak: separate lifetime lock and
  temporary root descriptors, resolver snapshot files, fresh/recovery database side files, root/mount
  probes, and child setup. `required_open_files` is the maximum of bootstrap and runtime peaks; neither
  phase can fail with `EMFILE` at the accepted exact limit, and `BootstrapRootFd` closes while
  `DataDirLockFd` remains lock-only through shutdown.
- Delay and fail each private bootstrap command. No public executor handle, durable claim, network
  runtime, AppState, or listener becomes reachable before both file and SQLite bootstraps succeed;
  failure closes descriptors, r2d2 connections, channels, and joins every thread already started.
  Capture startup logs before, during, and after budget creation: they are console-only through
  `RecoveryOnly`, read-only root/log/database inspection, durable-reservation reconstruction, and the
  `RecoveryReady` barrier. A valid crash-left WAL that prevents the read-only probe uses one measured
  `SqliteRecoveryFootprintSpec`: test immediately below/at/above free space and the recovery floor and
  crash every checkpoint/sync boundary; no ordinary class or unknown reservation may be spent. No directory, lock file, stale probe, rotation, or other filesystem entry is
  created/deleted before that barrier. Over-quota existing logs then use only zero-growth bounded
  pruning; root creation/probes use class-bound bootstrap tokens only after ledger health is restored.
  A fresh missing database may use only its exact
  schema bootstrap reservation and must remain absent until the complete private database is
  validated/synced/atomically published. Crash every schema page, temporary sync, rename, parent sync,
  and WAL activation boundary; recovery either removes only the known unpublished bootstrap names or
  reads a complete official schema, never a partial published database. Existing database inspection performs no schema initialization or
  unbounded write; missing schema, recovery beyond the floor, a failed post-recovery read-only probe, or
  inconsistent owner evidence fails startup. Log-sink
  activation failure opens no file sink and fails startup without consuming the recovery floor.
- Test system timezone discovery with valid relative/absolute zoneinfo symlinks, regular localtime plus
  matching/mismatching `/etc/timezone`, missing/oversize/escaping/unknown evidence, maximum IANA name,
  and descriptor/read failures. Only a bootstrap file worker opens these paths; descriptors appear in
  `BootstrapFdSpec` and close before readiness. Cron next-occurrence tests use the explicit immutable
  snapshot across DST gaps/folds and config rescheduling, contain no `chrono::Local`/libc-localtime/TZ
  fallback, and observe a host timezone change only after restart.
- Admission saturation bounds active connections, parsed requests, stream sessions, inbound durable
  streams, file chunks, durable orchestrations, ingress credits, and executor waiters. Excess work is
  rejected before body consumption or mutation, and releasing admission cannot be prevented by a full
  scheduler command channel.
- Exercise HTTP header bytes/count, URI bytes, and buffered control bodies immediately below/at/above
  64 KiB/128/8 KiB/1 MiB, including JSON, form, PROPFIND, and malformed length/chunking. Rejection occurs
  before excess allocation/body consumption. The same middleware audit proves every route is explicitly
  buffered-control or streaming-media; a simulated 20 GiB declared/chunked upload bypasses the buffered
  extractor but remains two-chunk bounded. Saturate every HTTP/1 connection read buffer and attempt
  pipelined requests; at most one parsed request exists per connection and total pre-handler memory
  remains within the startup sizing formula. HTTP/2 prior knowledge, h2c upgrade, and negotiated
  outbound HTTP/2 are unavailable without adding a modeled feature. Compressed request bodies receive 415 without decompression or body
  consumption, and no response-compression task/buffer is created on the network runtime.
- Generate JSON/form/multipart-control/XML at and one above every depth, collection, object-field,
  string, encoded-body, and 1 MiB normalized-DTO bound, including many tiny values, duplicate keys,
  malformed UTF-8, numeric overflow, DTD/entity bombs, and external XML entities. The charged typed
  visitor rejects before excess allocation, never constructs generic `Value`/DOM collections, performs
  no external read, and reports peak normalized heap within its route `OperationSpec`. Runtime sizing
  includes it in the exhaustive per-request phase maximum and permits every active request to occupy
  that phase without also charging its later response phase.
- Exercise WebDAV PROPFIND at depth zero, one, and infinity over empty, 256-entry, 257-entry, deeply
  nested, and very large trees. Each file operation visits at most 256 entries, each XML element streams
  through the charged response chunk, traversal state remains bounded by the normalized path-depth
  cursor, and neither the network nor file lane constructs a whole listing/multistatus body. Disconnect
  at every batch boundary and verify all read-only admissions/tokens close without journal work. The
  replacement preserves the captured WebDAV response properties, status codes, namespaces, and
  precondition behavior without executing `dav_server::LocalFs` or direct filesystem calls.
- Enumerate every route response and test zero/maximum/one-over page cardinality plus encoded size at
  4 MiB. Buffered routes never allocate or serialize beyond the bound; collection cursors return every
  row exactly once across pages; media/static/backup/export responses stream one chunk and never build a
  complete body. Updated web GUI and Android callers consume pagination without duplicate/missing items
  or unbounded all-page accumulation; affected Android tests run only through
  `./build_android_client.sh verify`.
- Populate page queries with maximum strings/blobs, one-over rows, high JSON-escaping amplification,
  and a next row that would cross the 1 MiB owned-output bound. SQLite row mapping never exceeds
  256 KiB per row or 1 MiB per output, stops before copying the crossing row, and the stable cursor
  returns it exactly once on the next page. Oversized stored/input rows fail schema/write validation;
  no worker builds a full result and measures afterward. Every request/durable state that can own a live
  DB output includes it together with its potentially coexisting CPU response/stage DTO; the same owner
  is not charged a second global DB-output term.
- Hold an older durable reservation while executor capacity repeatedly becomes available and flood
  `try_execute` from read-only HTTP requests. Raw executor senders are unreachable outside the
  scheduler/runtime builder. Saturate every request/session/source/control ingress credit and prove
  every legitimate owner can still enqueue exactly one command without waiting outside the channel;
  a second command from one credit is rejected as an invariant violation. Receive order supplies the
  monotonic sequence, `try_execute` rejects while an older waiter exists, and the durable reservation
  takes the next executor slot without starvation.
- Enumerate all seven `SchedulerControlSource` variants. Coalesce thousands of executor completions,
  admission releases, config revisions, cancellations, and shutdown observations while the command
  channel is full; the scheduler must reread authoritative versioned state until caught up and must not
  lose a payload, slot, cancellation, or final shutdown transition. Instrument the scheduler thread and
  prove it performs no SQLite, file, or network operation; cancellation coalescing only wakes
  `LlmCancellation`, which reads the outbox through its own SQLite credit.
- Enumerate all seven `CronTaskId` values with minute/hour/day/month/leap-day expressions, DST
  gap/fold boundaries, startup after years of missed minute occurrences, config changes during every
  CPU page, and crash before/after the final SQLite start. No CPU operation examines more than 256
  occurrences, every continuation returns to the FIFO tail, scheduler/network threads do no calendar
  iteration, stale generations create nothing, and each feature produces at most one idempotent latest-
  due catch-up through the identical manual start path.
- Fill every executor queue while continuously adding durable database rows. No row is claimed before
  global durable-orchestration admission is acquired; active futures and claim tokens never exceed
  `cpu_workers + (io_workers - 2) + sqlite_workers + active_outbound_stream_sessions`. Non-outbound
  claims occupy the first three terms; outbound claims transfer into the final term. Inference wait
  owns neither, and result receipt independently reacquires durable capacity.
- Fill every outbound session with a slow simulated 20 GiB LLM submission. Each transfer releases its
  durable slot immediately after claim/input-descriptor snapshot, so metadata, import, journal recovery,
  result persistence, and unrelated SQLite work continue. On remote admission, rejection, disconnect,
  cancellation, and input verification failure, each transfer eventually reacquires durable capacity,
  drops its logical input token, requests file-worker close, and drops its byte buffer before waiting
  for that capacity, persists exactly
  one token-fenced transition, releases its outbound admission, and retries only from the shared source
  tail. Exercise completion while new eligible jobs are continuously discovered to
  prove the outbound-before-durable acquisition order cannot deadlock.
- Drive simultaneous OCR, tagging, clustering, aesthetics, face, screenshot, and document submissions
  through one WebSocket, including one 20 GiB input. At most one 64 KiB data frame exists per outbound
  session; ready sessions rotate at the tail, and the large input cannot prevent every smaller job from
  reaching `submissionFinished`. Flood data and control traffic independently: Ping/Pong, cancellation,
  result receipt, Close, and admission transitions retain reserved capacity and meet deadlines, while
  the 32-message control burst still guarantees data progress. Delay each final binary sink send and
  prove its `inputFinished` cannot overtake it; delay each `inputFinished` and prove
  `submissionFinished` cannot overtake it. Urgent messages may interleave without violating any
  session dependency. No unbounded channel, per-job writer, reader task, or binary-message backlog
  exists. Disconnect once at every submission state and verify independent token-fenced requeue without
  combining job outcomes.
- Submit zero, one, 1,024, and 1,025 input descriptors; field lengths and checked total bytes immediately
  below/at/above every outbound bound; duplicate/gapped/out-of-range sequences; and a simulated 32 GiB
  job. Both services reject invalid manifests before descriptor allocation or byte receipt, preserve
  valid order, and stream without buffering the complete job or inferring its inputs concurrently.
- Fill every externally owned urgent credit and all connection-local pending slots while the
  `LlmTransport` is both reading and writing. The loop must send its own Pong, Close, connection-error,
  defer, and reject controls directly without enqueueing into or awaiting its mailbox. A second urgent
  item from one request/session/stream/source owner is rejected as an invariant violation; releasing
  any owner cannot leak or duplicate its credit. Heartbeat, cancellation, result receipt, and one data
  frame all make progress, proving the connection cannot self-deadlock.
- Send unfragmented and fragmented WebSocket messages immediately below/at/above each side's library
  message/frame limit, fill the 256 KiB write buffer, and attempt per-message compression. Oversize is
  rejected before protocol JSON/binary allocation, compression is never negotiated, the sink arbiter
  adds no second application message under write backpressure, Momento sizing includes its one library
  buffer/fragment state, and every registered job receives only its independent durable disconnect
  disposition.
- Saturate CPU workers, then send exactly and one above
  `LLM_INBOUND_CONTROL_SLOTS` maximum-size WebSocket JSON controls interleaved with Ping/Pong, Close,
  credited result binary frames, and outbound submission data. One permanent transport ingress credit
  permits only one ordered CPU parse; network workers perform no JSON parse and continue protocol/data
  polling while raw slots remain. Filling the last slot triggers the bounded overload close and each
  registered job's independent durable disconnect disposition, with no hidden frame/task backlog or
  control-specific CPU reserve. Startup sizing charges all raw frames and exact envelopes.
- Saturate SQLite and every durable slot while llm-service sends more results than
  `active_inbound_durable_streams`. Excess `resultStart` envelopes receive
  `resultReceiptDeferred` without retained bytes, remain `callback_pending`, and do not increment the
  llm-service delivery-attempt count. `nextDeliveryAt` is committed to `callback.json` before the result
  becomes selectable again and survives restart/reconnect. Each admitted result owns one 64 KiB frame
  buffer, ingress credit, and StreamSession file-token debit while it writes to Momento-owned Journal
  storage. Ordinary scheduler/SQLite/file saturation waits or defers; it never emits
  `resultReceiptRejected`. Ping/Pong, cancellation, submission progress, and socket reads remain live.
- With Journal and SQLite classes completely reserved, deliver a matching committed duplicate, a late
  terminal/deleted result, an active mismatch, and an active match. The read-only correlation probe
  returns Received for the first two without any reservation; the active mismatch uses only the bounded
  rejection transaction and Deferreds rather than sending Rejected if that terminal write cannot fit.
  The active match alone requests the large result reservations. Restore the small SQLite capacity and
  prove mismatch marks the exact Momento job failed before llm-service moves to failed. Race
  cancellation/terminal transition after probe and before prepare; the prepare transaction recheck
  releases provisional tokens and persists or returns the new terminal outcome.
- Generate maximally sized and fragmented rows for the rejection and result-cleanup schemas at every
  supported SQLite page size. The centralized footprint registry must cover the token-fenced terminal
  update, 4 KiB durable error/truncation metadata, every affected index, 256-row/4 MiB cleanup batch,
  cursor update, and commit/checkpoint WAL peak. One byte/row above each logical bound is rejected
  before SQL; observed allocation above either calculated footprint is an invariant shutdown. An
  unregistered affected table/index makes the exhaustive schema test fail.
- Admit multiple large inbound results together. `resultReady` grants only offset zero; sending a
  second frame without `resultChunkReady`, a frame at the wrong offset, or a frame larger than its
  credit is a protocol rejection. Hold one result's file operation indefinitely and prove llm-service's
  single sink arbiter round-robins the other credited results and urgent control traffic. At most one
  frame per result is resident, and no binary backlog can hide Pong, cancellation, submission control,
  or a late-terminal direct `resultReceived` behind an unbounded result send.
- Golden-test `momento-result-chunk-v1` in both Rust services for the exact 20-byte header and full message
  vectors. Reject bad magic/version/reserved, zero/oversize/non-hex job IDs, zero/oversize payloads,
  checked length overflow, truncation/trailing bytes, wrong stream, and wrong/uncredited offset without
  affecting another active result.
- Stream valid results immediately below and at `MAX_LLM_RESULT_BYTES`, including a result larger than
  1 MiB with many ordered inputs, split text/tag continuations, and face embeddings. Assert the shared
  provider/durable/wire record encoding, exact 24-byte little-endian header/CRC32C vectors shared across
  Rust and Python runtimes, unchanged record bytes, bounded resident memory, exact
  byte/record-count/hash verification, separate Momento and llm-service data roots, synced Momento inbox
  storage, and later journal cleanup. Reject an oversized record/result manifest, malformed framing,
  a record below 24 bytes, more than 1,000,000 records, more than four continuations for one value,
  and text/tag aggregate immediately below/at/above 2 MiB,
  every unchecked/overflowing field length or element count, simulated allocation failure, every
  per-kind encoded/normalized limit, every per-task kind/order violation, size/hash/count mismatch, invalid correlation, and permanent
  durable-receive error. Disconnect after every frame boundary and prove llm-service retries `result-records.bin`
  without rerunning inference or leaving an unowned Momento temporary.
- Crash llm-service during runtime output before/after every record, record-file sync/rename,
  manifest sync/rename, job-directory move, and parent sync. Delivery selects only a complete matching
  manifest/record pair in `callback_pending`. Recovery promotes a complete pair without rerunning the
  model, removes temporary/inconsistent output before requeuing inference, and never interprets a
  partial file as success.
- Crash before receive-journal preparation, after disk reservation, after temporary open, during every
  frame, after sync, after Journal Publish, and before/after inbox commit. No `resultReady` is sent before
  the per-receive group derived from the job/attempt identity and its persisted receive token, Journal
  reservation, schema/task/manifest-derived SQLite reservation,
  product claim, and Publish entry exist. Fill the SQLite class just below/at the calculated footprint;
  insufficient space defers before body receipt, while an admitted result can stage/finalize after
  llm-service deletes its copy even if unrelated SQLite writers consume every unreserved page. Crash
  before/after durable reservation persistence and reconstruct/release exactly one reservation. Generic recovery rolls
  back or finishes the exact group; duplicate delivery either defers behind that recovery, reuses the
  committed inbox receipt, or starts a new per-receive group after a discarded terminal group releases
  its receipt and SQLite reservation. Terminal Journal history remains intact. No result file exists
  without a journal owner.
- For both Journal and SQLite classes, place the same result immediately below/at/above current
  availability and the immutable class hard limit. Current contention/free-space shortage produces
  Deferred and can later succeed; checked overflow or required peak above the class hard limit produces
  one pre-body permanent condition. With the small rejection transaction temporarily blocked it
  Deferreds and remains recoverable; once that transaction commits the Momento failure first, it sends
  Rejected, releases every provisional token, moves llm-service to failed, and is never selected again
  from `callback_pending`.
- Exercise the shared `DataDirSpaceBudget` ledger with declared/chunked HTTP and WebDAV uploads, local/backup import,
  canonical copy, metadata/preview/thumbnail output, bounded child output, inbound LLM result, face
  crops, replacement while old bytes remain, rename-only move/tombstone, rollback, and cleanup. Simulate
  free space immediately below/above each additional peak plus the derived recovery and Log
  protection; concurrent SQLite/Journal reservations; sparse/logical versus allocated
  bytes, external space consumption, real `ENOSPC`, cancellation, permanent failure, normal commit, and
  restart reconstruction. The single-epoch invariant protects the Log remainder and each SQLite or
  Journal reservation's `reserved_peak_additional_bytes - newly_allocated_blocks` without overflow
  or double-counting. Pre-existing originals/tombstones remain ordinary used blocks across rename and
  never consume a reservation. Unknown bodies extend before the next socket read; children
  cannot exceed `RLIMIT_FSIZE`; each newly created directory entry reserves four validated filesystem
  fragments while proven exchange/metadata-neutral rename and pure unlink/rmdir reserve zero; no write accepts a stale/wrong/exhausted
  token. Crash/drop before journal prepare releases the provisional in-memory token without bytes;
  crash after prepare reconstructs the durable token and newly allocated blocks. Deliberately pause
  SQLite/log/Journal writes between reservation, physical allocation, allocation publication, and token
  release while another owner snapshots capacity; every interleaving is safe or conservative, never a
  mixed old-free/new-counter over-admission. Admission never spends another live reservation, the Log
  remainder, or the recovery floor, and group ownership/release remains correct across publication and durable cleanup.
- Force destination, tombstone, temporary, and deep-directory creation at filesystem directory-block
  boundaries with maximum path components/names. Each new entry is admitted only with its four-fragment
  metadata peak, actual block growth is published before release, and concurrent groups cannot spend
  the same metadata reserve. Test proven exchange/metadata-neutral rename and pure unlink/rmdir as the
  only zero-additional cases. An observed metadata allocation above the closed filesystem spec without
  external consumption is an invariant failure, not silently borrowed Journal space.
- Populate millions of durable reservation rows, then instrument every runtime admission: it performs
  one statvfs plus fixed-size aggregate arithmetic and touches no reservation row/collection. Recovery
  pages 256 reservations/entries at a time with admission closed, detects checked-sum/owner mismatches,
  and publishes one epoch only at exact agreement. Inject a ledger error from the log sink and prove the
  lock is released before the console-only diagnostic, with no recursive logger/ledger entry.
- Compile-time ownership tests assert that admission guards, `ProvisionalSpaceToken`,
  `DurableSpaceCheckout`, private `SpaceReservationToken`, `RecoverySpaceToken`, `FileToken`, and
  `JournalMutationLease` are non-`Clone` and expose no public constructor. Move each through its legal
  conversion or durable-owner transfer and inject early return, panic, cancellation, and reply loss.
  A provisional/ephemeral debit releases exactly once; owner preparation consumes it exactly once; a
  dropped durable checkout only clears exclusive in-memory checkout and leaves the ledger row/bytes
  reserved for reacquisition. Only terminal owner plus file/allocation evidence releases that durable
  obligation. No stale generation can close, mutate, spend, reacquire, or release a reused slot.
- Start with log allocation immediately below, at, and above `log_quota` and with external free-space
  deficit. Reconstruction produces the exact health state without unsigned underflow. Over-quota startup
  admits only 256-entry zero-growth closed-log pruning/sync batches, issues no Journal/Log allocation,
  and reaches healthy admission only after remeasurement; an undeletable overage fails visibly.
  External deficit permits cleanup but no new allocation until the invariant is restored.
- Fill SQLite main/WAL/SHM across large database sizes and place WAL immediately below/at/above its
  derived limit; prove the native `max_page_count` remains unchanged, declared transaction growth, bounded checkpoint, blocked-reader retry, restart
  measurement, and post-write invariant behavior. Start all SQLite workers with individually valid
  growth peaks at the same time alongside Journal writes; common reservations prevent their aggregate
  from passing shared physical capacity. For a durable LLM parent, split concurrent boundary-sized children, reject aggregate or
  wrong-generation overdraw, and prove children do not add a second class obligation. Commit and roll
  back each child, crash before/after the business-row/cursor/remaining update, and verify exact parent
  reconstruction plus WAL recovery before the next child. Every `DbOperation` variant must explicitly
  select Fresh, one-time ProvisionalParent, DurableParentConsume, or DurableParentRolling in compile-time
  tests; reject provisional
  reuse and prove initial preparation promotes owner/reservation/cursor atomically. Run many rolling
  cleanup batches with main/WAL allocation retained, crash before/after cursor/allocation publication,
  and prove exactly one child reuses—not consumes or duplicates—the next-batch peak until terminal
  release; reject Rolling for construction and Consume/Fresh for cleanup. Fill the bounded log
  ring and logs across day and 128 MiB rotation boundaries; prove event producers never perform file I/O
  or wait, body capture/redaction respects 48 KiB/64 KiB bounds, drain batches respect 64 events/256 KiB,
  saturation increments exact dropped-by-level counters. With every file worker asleep and no business
  file operation, enqueue one event and race enqueue against the final pre-sleep ring check; the
  coalesced generation wake must cause an existing file worker to drain it without a lost wake, extra
  thread, scheduler command, or producer wait. Repeated events while a wake is pending remain bounded,
  race all file workers for `LogSinkLease`; only one batch appends/rotates in order, losers never wait
  or skip business FIFO work, and the lease is released after the declared batch,
  the two non-borrowable log handle slots cover active append plus simultaneous rotation, and shutdown
  drains/syncs before file-worker join. Prove oldest-closed pruning,
  append/old-new-rotation peak reservation, console-only degradation on sink failure, body redaction
  retention, and preservation of the recovery floor. Verify inherited stdout/stderr flags are never
  modified; accept process-owned `/proc/self/fd` handles only for TTY/pipe/socket and disable regular-
  file/missing/unsafe targets. Fill the accepted nonblocking console pipes, inject
  partial `writev`/`EAGAIN`/closed descriptors, and verify event threads never write them, file workers
  never block or allocate a second colored event, plain file logs contain no ANSI, TTY console segments
  preserve timestamp/severity coloring, and dropped-console counters are exact. Bootstrap/fatal/final
  direct writes use only the bounded nonblocking helper. Race SQLite growth, log
  rotation, journal reservations, and external writes; none may borrow another class or make the shared
  admission snapshot undercount committed capacity.
- Fail each eagerly warmed connection and each runtime replacement before token acquisition, during
  WAL/SHM open/configuration, and before/after allocation publication. No request/SQLite worker lazily
  creates a connection, no replacement becomes visible before validation/release, pool checkout times
  out transiently while below target, and r2d2 maintenance executes no business SQL or unbudgeted file
  growth. For every warmed and replacement connection, assert `mmap_size` remains zero, the negative
  `cache_size` never exceeds the 8 MiB page-cache budget, the prepared-statement cache never exceeds 64
  entries or its checked top-64 SQL/parameter footprint, and SQLite's internal worker-thread limit stays
  zero. Hold every normal connection while replacing one broken connection: memory and descriptors may
  reach, but never exceed, the declared `sqlite_workers + 1` peak, and a second replacement cannot overlap.
- Inspect every centralized query with representative maximum-cardinality data and SQLite query-plan
  assertions. No unbounded path uses a temp B-tree or file; each bounded 256-row sort remains below the
  declared 8 MiB heap. Attempt ATTACH, full VACUUM, temp-file mode, and SQLite worker threads through
  every connection and prove they are unavailable. Incremental vacuum advances a bounded declared page
  batch with normal timeout/reservation and returns to the tail.
- Process every result kind through pass-one record File I/O, CPU validation, and idempotent SQLite staging.
  Assert no complete-result DTO is constructed, record and optional bulk-insert memory stay within the
  1 MiB/2 MiB/4 MiB limits, retries resume/replay by record sequence without duplicate rows, and pass
  one creates no artifact. In pass two, partition zero, one, 256, 257, and many artifact entries into
  complete groups; prove each entire group/reservation/claim is prepared before its first mutation, all
  groups carry one result-product version, partial products remain invisible, and only the complete
  token-fenced terminal transaction switches references. Fail/cancel between every record, group,
  publication, and final switch; the prior product remains visible and every new artifact has durable cleanup.
- Populate result staging/plan rows immediately below/at/above the 256-row and 4 MiB cleanup bounds for
  success, cancellation, supersession, and permanent failure. Each transaction uses one
  DurableParentRolling cleanup child, commits its cleanup cursor without consuming the next-batch peak,
  returns to the tail, and stays within the reserved WAL peak.
  Crash before/after row deletion, cursor commit, inbox cleanup, artifact cleanup, and each reservation
  release; recovery neither repeats visible product mutation nor frees capacity before its durable owner
  is terminal. At entry to cleanup, assert the large result-construction remainder is released and only
  `RESULT_CLEANUP_RECOVERY_MAX_GROWTH_BYTES` remains outstanding. Permanent cleanup failure retains that
  bounded reserve and administrator-visible evidence without occupying a worker or preventing an
  unrelated result/database writer from using the released capacity; administrator retry resumes the
  same cursor.
- For every task and boundary `recordCount`/`byteSize`, verify `ResultSqliteFootprintSpec` against the
  fixed SQLite page size, every staging/final/plan table and index, maximum row payloads, page splits,
  WAL peak, checked overflow, and an intentionally fragmented database. The measured maximum never
  exceeds the reserved footprint, and adding a result table/index/task fails exhaustive compile/tests
  until its footprint contribution is declared.
- Cancel an inbound result before first credit, between every credited frame, during sync, immediately
  before/after Journal Publish, and before inbox commit. The cancellation transaction advances the job
  version and sets `cancel_requested`; no further chunk credit is sent and every stale receive-token
  check fails. Before the first forward rename the group enters `rollback_pending`; afterward it
  completes through `files_committed` with `completion_outcome = discarded` and ordinary cleanup.
  Pause a file operation inside every bounded write/sync/rename boundary: cancellation invalidates the
  mutation generation but rollback/cleanup cannot acquire the `JournalMutationLease` or touch the path
  until that operation returns, the logical token enters close, and the owner records its handoff.
  Also hold mutating commands in the File I/O FIFO: after the gate-close acknowledgement but before
  their syscall, the worker-side generation check returns Superseded and not one byte/rename occurs.
  Race cancellation immediately before/after the worker comparison; exactly the pre-comparison case is
  suppressed, while the post-comparison operation is treated as the single in-flight mutation and its
  observed filesystem result selects rollback versus forward recovery. Fail the cancellation SQLite
  transaction and prove the gate stays closed until durable retry or a verified no-intent reopen.
  Repeat across prepared, publishing, completed-invisible artifact groups and process restart.
  `resultReceived` releases the llm-service task, and no inbox or product row is created.
- Inject both transient and permanent post-`resultReady` receive failures while physical close,
  rollback deletion, and JournalRecovery are
  blocked. Once the existing group durably owns recovery, Momento sends deferred and releases the frame,
  inbound stream, and urgent credit without waiting for cleanup; the permanent case performs the same
  handoff but sends rejected. The registry debit and disk reservation remain bounded and owned until
  cleanup; all result admission slots therefore cannot be pinned by slow rollback.
- Deliver a permanent Rejected result and crash llm-service before receiving Rejected, during/after
  `rejection-failure.tmp` write, after temporary sync, after rename to `failure.json`, after job-directory
  sync, after `callback_pending -> failed`, and before/after both parent syncs. A temporary-only callback
  job deletes/syncs the temporary and redelivers without inference; a complete failure file finishes
  forward exactly once without redelivery; failed with missing/malformed evidence becomes an
  administrator-visible invariant failure. No case retries inference or silently loses the reason.
- After inbox commit, atomically convert the inbound guard to the same owner's urgent receipt credit,
  drop the frame buffer, and retain only the durable inbox file. No competing urgent producer may take
  that credit before sink-confirmed `resultReceived`; disconnect before confirmation releases it and an
  idempotent replay reuses the inbox row rather than creating another one.
- Race a recovered `queued` replay, a live `submitting` transfer, cancellation, and the matching pending
  result in every transaction order. Inbox insertion wins atomically for matching
  queued/submitting/submitted correlation, invalidates the submission token, and blocks claim
  eligibility through `NOT EXISTS llm_result_receipts`; the losing outbound flow releases resources and
  writes no state. Mismatched active attempts are rejected and duplicate matching results reuse one
  receipt. Exercise cancellation before receipt, after receipt but before result claim, during CPU/file
  preparation, and immediately before terminal persistence. Cancellation advances the job version,
  marks any inbox row discarded, prevents stale publication/persistence, and cleans its durable bytes.
  Late results for cancelled/completed/failed/deleted work are acknowledged and discarded without an
  inbox row or recreated job. If result persistence commits first, the later cancellation/reset cleanup
  still determines the final state.
- Create all-task and task-specific cancellation scopes while continuously discovering and claiming
  jobs, including enough 128-byte IDs to exceed many 1 MiB envelopes. A pending scope/cutoff makes old
  matching submission claims impossible in SQL while above-cutoff replacements cannot submit before
  acknowledgement. Materialization and canonical SHA checkpointing each advance at most 256 IDs per
  pass and return to the source tail; transport sends at most
  4096 IDs and one encoded-size-checked control envelope per credit. Crash during database paging,
  start, every chunk, partial llm-service staging, staged-file sync, cancellation-set publication,
  background marker/deletion, conversion-cursor commit, set removal, and acknowledgement persistence.
  Partial remote staging is deleted and replayed from Start; the fixed-width canonical sorted published file uses
  bounded lookup and immediately blocks every listed ID at admission, queue selection, inference start,
  result publication, callback selection, and each delivery credit. Restart at every boundary before
  marker conversion preserves that authority; only a synced marker plus durable cursor may replace set
  membership, and the set is deleted/synced only after every marker/cleanup cursor is durable. The same
  request/scope/cutoff/order/count/hash is reused until one matching acknowledgement transaction deletes
  the Momento outbox/items. Hash/content reuse mismatch is permanent. Above-cutoff replacement work
  survives, and active submission/result delivery stops producing frames at the cancellation fence.
  Pause immediately before/after the scheduler scope-gate acknowledgement and scope transaction while
  starting matching result receives and submissions. The bounded active registry is fully invalidated,
  no new matching mutation can acquire a generation, and a failed scope transaction leaves the gate
  closed until retry or a verified no-intent reopen; no job waits for its later materialization page to
  receive this fence.
  Queue a second cancellation for the same client before marker conversion: it receives retryable busy
  and remains in Momento's outbox; replay of the first request is idempotent and a different client
  proceeds independently. Membership lookup never scans more than one published set for the job's client.
  Golden-test the 129-byte cancellation record and count/SHA-256 across empty, one, prefix-related, and
  128-byte IDs so chunk boundaries and variable string lengths cannot change or alias the hash. Crash
  before/after each CPU page and checkpoint CAS, corrupt every serialized chaining/count/tail field, and
  prove resume hashes each canonical page exactly once logically without a whole-request rescan or
  implementation-private hasher serialization.
- Slow-header, stalled-body, idle keep-alive, stalled-response, and dead-WebSocket tests exhaust each
  admission class, advance its fixed liveness deadline, and prove unrelated clients can connect after
  automatic release. A continuously progressing simulated 20 GiB transfer must not time out.
- Resolve enough hostnames to overflow the fixed DNS cache while every outbound-operation admission is
  saturated. Cache eviction remains bounded, each operation owns at most the declared resolver/service
  sockets, redirects close the prior hop, and disabling idle pooling leaves no socket after guard release.
  Thread/process instrumentation observes no blocking resolver, `spawn_blocking`, helper thread, or
  connector-owned task population; DNS timeout/error propagates through the normal network disposition.
  Make each resolver file missing, symlinked, malformed, and 64 KiB boundary-sized; only the file worker
  opens it, startup either owns a validated bounded snapshot or fails, and no network/main/business
  thread reads `/etc` or reloads it later.
  Fuzz DNS UDP/TCP lengths, truncated fallback, label/name/search counts, compression-pointer cycles/
  hops, CNAME depth, address count, TTL clamps, sequential address failures, and the 30-second total
  deadline. No parse allocates beyond its charged 64 KiB state, no lookup owns two resolver/service
  sockets, and no Happy-Eyeballs/helper task appears.
- Hold a healthy upload and download behind saturated executor queues for longer than their network
  idle limits. Their idle clocks pause during internal backpressure and resume afterward; a peer that
  then stops making network progress still times out and releases admission.
- Fill all stream sessions with slow peers while leaving file workers idle. Ordinary short API requests
  still use the reserved connection/request capacity. A slow upload/download retains one bounded
  session buffer but no file-chunk slot while waiting on its peer; another stream can acquire a chunk
  slot and progress. Request admission is atomically converted and released before streaming begins.
- For inbound upload, media/static download, and Momento-to-llm-service transfer, atomically replace the
  source path between chunks and prove every chunk still uses the one generation-checked
  `StreamSession` token opened for that session. A peer wait retains that one token but no file-chunk
  slot; descriptor-registry use remains bounded by inbound plus outbound session admission, and no
  stream reopens its path to continue.
- Queue saturation before journal creation returns 503 without a temporary file or product mutation;
  saturation after journal creation applies bounded asynchronous backpressure or durable recovery
  handoff. Durable jobs remain in SQLite and do not accumulate channel-send waiters.
- Keep executor queues continuously busy with post-journal HTTP operations while every durable source
  has work. Each source may register only one waiter, all waiters enter the same FIFO, and recovery and
  background work must start without starvation.
- Enumerate every `DurableSourceId`, reject duplicate pending reservations, and prove that thousands of
  media IDs, jobs, retries, cron triggers, and AI task types do not increase the waiter bound. Multiple
  LLM jobs may remain independently in network/inference wait while their shared dispatcher has at
  most one pending reservation per executor.
- A saturated CPU pool does not block video streaming, WebDAV networking, or SQLite reads.
- Saturate CPU workers with media work while sending maximum-size JSON/form/PROPFIND bodies,
  authentication requests, and maximum-size buffered responses. Protocol framing and unrelated socket
  liveness continue on both network workers, while parsing/signature verification/serialization waits
  through bounded CPU admission and never executes on a network thread. CPU completion returns the
  owned bounded DTO/buffer without retaining a worker from another domain.
- Attempt `cpu_workers` simultaneous ImageMagick/FFmpeg/exiftool operations and prove that only
  `max(1, cpu_workers - 1)` are admitted when at least two workers exist, while a control parse and
  response serialization complete on the remaining worker. Inspect the child process tree and prove
  each child remains single-threaded and the Momento-side compute-thread bound is not exceeded.
  Unsupported thread/address-space-limit behavior must fail rather than silently
  oversubscribe. Boundary-test every fixed and duration-derived deadline, invalid/overflowed duration,
  monotonic-clock advance, SIGTERM grace, SIGKILL escalation, process-group reap, and resource release;
  a valid several-hour video receives its calculated window while a wedged child cannot retain a CPU
  worker past its declared ceiling.
- Feed compressed-image headers at, below, and above both geometry limits, including overflowed
  width/height/channel products and small files claiming decompression-bomb dimensions. Allocation
  never precedes checked validation. For supported codecs, peak charged decode memory remains at two
  16 MiB tiles per active CPU pipeline; a full-resolution face crop decodes only intersecting regions.
  For fallback codecs, inspect the child limits and prove ImageMagick cannot exceed 256 MiB pixel
  memory, allocate map cache or more than four GiB of disk scratch, or exceed its 512 MiB address space. Limit failure
  is recorded as a typed media failure with no thumbnail fallback, partial publication, or leaked
  journal reservation.
- Make a supervised child emit diagnostics far beyond every retention limit while another writes a
  simulated multi-gigabyte binary result. Resident memory remains bounded, pipes continue draining
  without deadlock, retained diagnostics are truncated explicitly, binary bytes go only to the
  journal descriptor, and no helper thread or Tokio task appears.
- Inspect `/proc/<child>/fd` for every media tool and prove only stdio diagnostics and explicitly
  declared input/output descriptors are inherited. Cover successful spawn, spawn failure, timeout,
  cancellation, and process-group kill; no listener, SQLite, root-directory, unrelated journal, or
  other-request descriptor may leak. A tool without descriptor input must use immutable journal
  staging or fail.
- Race token drop, queued spawn, spawn failure, child exit, reply-receiver loss, panic, cancellation,
  and shutdown around every `ChildPinned` transition. The file registry never closes/reuses a pinned
  descriptor, the CPU worker never performs file I/O or registry access, the child is reaped before a
  lease can become `ClosePending`, and output sync/validation runs on a file worker before publication.
  Stale handle generations and a child spec requesting undeclared access or duplicate child descriptor
  numbers fail before spawn.
- Run metadata work for longer than the removed lease under maximum SQLite FIFO delay and prove no
  live expiry/reclaim query exists and the active token is never duplicated. Inject timeout,
  cancellation, dropped orchestration, and normal failure; the claim guard completes or requeues before
  unregistering. Restart with a claimed nonterminal row for every `DurableSourceId` plus every
  forward/rollback journal state: journal recovery reaches a stable state first, one
  `DurableClaimRecovery` transaction applies the explicit per-source disposition, clears old tokens,
  and leaves external wait states unchanged. Crash LLM submission before claim, after attempt snapshot,
  during verification, during streaming, after remote admission, and before local transition; recovery
  replays `submitting` with the same `jobId`/client/attempt and never duplicates remote admission. Stale
  tokens cannot prepare a journal, authorize first publication, or persist terminal state. An
  already-forward journal finishes exactly once. Permanent failures stop immediately; repeated
  transient failures release all resources and retry without an attempt cap.
- Saturated file workers do not prevent network timeouts, heartbeat, or LLM result protocol handling.
  A result without file capacity waits only inside its already bounded admitted state or receives
  Deferred through the defined durable handoff; it resumes receipt when file capacity returns rather
  than claiming progress while the required lane has none.
- Cancel streams while tokens are held, drop reply receivers, reuse handle IDs, saturate the file
  queue, and shut down. Instrumented open/sync/close calls must run only on a named file worker; stale
  generations are rejected, cancelled handles close even when scheduler and file queues remain full,
  and no registry lock is held during a blocking syscall. Hold all close operations behind saturated
  file workers and prove each `ClosePending` entry retains its registry slot and budget-class debit;
  no new open spends that credit until the worker consumes descriptor ownership and publishes the
  non-lossy release. Inject success, `EINTR`, and other close errors: close is invoked exactly once,
  generation/slot ownership is invalidated/released, the numeric fd is never retried after possible
  kernel reuse, and required prior sync errors remain independently visible. Saturate every term of the exact capacity
  formula, including every inbound durable result stream, reject one additional open through the
  correct pre/post-journal/deferred path, and prove logical jobs
  waiting for SQLite, llm-service, or timers retain no token. No network or scheduler thread executes a
  filesystem syscall.
- Run multiple metadata/import pipelines from one durable source and fill all `CpuPipeline` token
  capacity; they execute concurrently up to `cpu_workers`. Independently saturate StreamSession,
  SourceMaintenance, and FileWorkerHandoff, reject cross-class borrowing and reclassification, and
  prove file-only maintenance remains one two-token stage per source.
- Import identical bytes concurrently from local, WebDAV, and backup sources with older/newer timestamps
  and zero/one/multiple supplemental sidecars. Exactly one token-fenced hash owner may allocate/finalize
  the canonical media; contenders release resources and rejoin their durable tail without an in-memory
  mutex waiter. The partial unique index prevents duplicate imported rows, every importing user gains
  access, only an older timestamp lowers `created_at`, sidecars remain with the canonical original, and
  metadata is queued exactly as specified. Crash before/after hash claim, media-ID allocation, journal
  publication, duplicate reuse, sidecar move, metadata enqueue, and claim release; journal recovery then
  claim recovery resumes without duplicate bytes/rows or a leaked process-global hash entry.
- Saturated SQLite workers do not consume CPU or file workers.
- Source audit uses a closed allowlist: application filesystem syscalls exist only in File I/O
  implementation modules, the exact `runtime/config_bootstrap.rs` pre-runtime mode, SQLite/r2d2's own
  database/capacity internals, the two exact nonblocking console-handle opens in
  `runtime/bootstrap.rs`, and supervised child setup; `pool.get()`/r2d2 types exist only in
  `database/pool.rs` and `executor/sqlite.rs`. Routes, auth, processors, cron, WebDAV, config runtime
  endpoints, utilities, AppState, scheduler, CPU operations, and network orchestration contain none.
- Source audit also rejects `constants::paths`, `init_paths`, a global absolute-path registry, and any
  business DTO/command containing an absolute `PathBuf`; only bootstrap/File I/O capability internals
  may hold configured absolute roots.
- Backend manifest/source audit rejects Tokio `full`, `tokio::fs`, `spawn_blocking`, `block_in_place`,
  `tower-http/fs`, `ServeDir`, `glob`, Rayon, and any private runtime/pool creation; the resolved backend
  feature tree contains only the explicitly documented Tokio/network features. It also rejects the
  removed `dav-server` dependency and any `LocalFs`/`DavHandler` compatibility wrapper. llm-service is checked
  against its separate scheduling contract rather than this Momento executor restriction.
- File operations accept only `StorageRootId` plus normalized relative components. Attempt absolute
  paths, `..`, symlinks, magic links, and concurrent parent-directory replacement; root-handle
  resolution must reject every escape without an unsafe compatibility fallback.
- Bootstrap a missing set of writable child roots, verify directory sync, close the generic data-root
  capability, and prove that none is created before reservation reconstruction reaches `RecoveryReady`,
  each creation/probe is class-budgeted, and HTTP/durable claims cannot start before the root registry
  and SQLite are ready. A missing configured data directory fails without creating it. Missing optional static content disables fallback; an unsafe existing static root fails
  startup. Static files and SPA fallback must stream exclusively through `FileIoExecutor`.
- Mount or simulate writable child roots on a different device and on a different bind mount with the
  same device ID. Mount-ID validation or the real Journal-to-root rename probe must fail startup before
  admission and clean its probe evidence. Reject Static roots that equal, contain, or are contained by
  data or writable roots, including bind-mount/inode aliases; an unrelated read-only Static root
  succeeds. Starting a second Momento API on the same data directory fails on the process lock.
- Golden-test the replacement static responder for GET, HEAD, MIME, length, ETag/Last-Modified
  conditions, 304/412, valid and invalid single ranges with 206/416, If-Range, cache/security headers,
  HTML-only SPA navigation fallback, missing assets, `/api`, WebDAV, unsupported methods, traversal,
  disconnect, and file-queue saturation.
- Assert that static ETags are weak, weak validators never satisfy `If-Range`, and date-based
  `If-Range` retains correct range behavior. Exhaust `If-Match`, `If-Unmodified-Since`,
  `If-None-Match`, and `If-Modified-Since` precedence with lists, `*`, matching/nonmatching weak and
  strong tags, valid/invalid dates, 304, 412, full 200, and 206 outcomes. Document and test the
  supported immutable/atomic-replace publication path; no implementation path performs an in-place
  static-file update.
- Atomically replace a static path after it is opened. Header validators, preconditions, range offsets,
  body bytes, and Content-Length must all
  come from the same open descriptor/statx snapshot; SPA `index.html` uses its own snapshot and no path
  is reopened after validation.
- Verify every configured storage category maps to one typed root and that Albums, Trash, Backups,
  Journal, and read-only Static cannot fall through to an absolute or generic data-root operation.
- Create canonical claims at the total-path, component-count, 255-byte component, canonical-key, and
  cumulative-ancestor-key bounds; reject one above each bound or checked arithmetic overflow. Use
  query-plan assertions plus a large claim table to prove exact, ancestor, and descendant conflict
  checks use the covering indexes without a full scan or leading-wildcard match, and assert the complete
  ancestor input stays within its registered `OperationSpec` allocation.
- SQL success, rollback, busy/locked classification, connection return, and panic handling. Inject an
  operation panic into every executor and a business panic into every durable stage; local RAII cleanup
  completes, `WorkerPanic` reaches one boundary, the current job becomes permanent `Fail` without
  requeue, and the awaited disposition resolves the claim before unregistering it. Repeated database
  busy/timeout, temporary I/O, and retryable network failures still return to the tail without an
  attempt cap. Separately panic executor loops, the scheduler, registry invariants, and cleanup
  verification; each must enter coordinated fatal shutdown without silently losing a configured worker,
  and restart recovery must resolve the remaining durable evidence.
- Table-test every SQLite code, OS I/O kind, HTTP/WebSocket failure, child exit/timeout, and validation
  error as policy-free `ExecutorError`. Then table-test every durable operation/stage, cancellation
  reason, claim state, and executor-error kind through the boundary `OperationFailure` conversion.
  Assert `Retry`, `Fail`, `Cancel`, and `Superseded` independently;
  superseded owners perform no row transition, user cancellation reaches `cancelled`, and shutdown
  handoff requeues. No message-string classification exists, and a new unclassified variant fails
  compilation or an exhaustive registry test.
- SQLite connection timeout, busy timeout, progress-handler interruption, rollback after deadline,
  paged continuation, and durable FIFO-tail retry; no timed-out statement retains a connection.
- I/O error propagation includes operation and path and is logged once at the boundary.
- Failure before file publication removes the temporary file.
- Failure after the first entry publication but before SQLite finalization completes every remaining
  entry from the generic journal after restart and never rolls back an already committed entry.
- Inject failures before the first rename, between every pair of entries, after every rename, after
  each required parent-directory sync, before each entry-state update, and before the
  `files_committed` group transition. Recovery must establish directory durability and finish the
  exact recorded group without guessing from rename alone.
- A permanent post-publication database finalization error enters `finalize_failed`, preserves and
  blocks every claimed path, and completes only through an administrator-authorized idempotent retry.
- Journal list/detail/retry rejects non-administrators, paginates deterministically, never reveals an
  absolute path, compare-and-sets the expected version, and wakes exactly one recovery source. Reusing
  one retry request ID with identical content returns the stored response without another transition;
  reuse with different content is rejected, and a new ID with a stale version conflicts. Crash after
  commit but before wakeup still recovers. Failed groups survive maintenance; resolved groups compact
  only after a transactional terminal-state recheck. Fill a failed group with 64 live receipts:
  identical replay still succeeds, a 65th new ID returns 429 without transition, and 256-row maintenance
  expires receipts independently after seven days even while failure evidence remains. Resolved summaries
  prune after their own expiry, including crashes around every boundary. Before
  compaction detail returns `detailLevel=full` with ordered entries/claims; afterward it returns
  `detailLevel=compacted` with exact action/state counts and terminal/error/cleanup summaries and omits,
  rather than fabricates, full-only collections.
- After the first committed entry, inject permanent missing-source, conflicting-evidence,
  containment, and permission failures. The group enters `publication_failed`, preserves all entries
  and claims, blocks rollback, and resumes forward only through administrator retry. Transient file
  failures continue retrying at the recovery tail.
- Restart and concurrently attempt group preparation for every claimed source and destination while
  the old group is in each incomplete state. Durable path claims must reject the new mutation even
  without the in-memory keyed lock.
- Concurrently claim a directory and its parent, child, sibling, and identical path using every
  Read/Write and Exact/Subtree combination. Only overlapping reads and non-overlapping claims coexist;
  the `BEGIN IMMEDIATE` conflict check prevents two racing writers from both succeeding.
- Exercise PUT/PATCH/COPY, MOVE with and without overwrite, DELETE, multiple metadata artifacts, and
  multiple face crops. MOVE/DELETE recovery preserves source/destination/tombstone evidence; product
  references switch once only after all forward entries are durable; old artifacts are cleaned only
  after commit; repeated recovery and cleanup are idempotent.
- Exercise permanent media deletion, trash purge, regeneration cleanup, metadata reset, every AI
  clean/reset, face reset, import recovery, and stale temporary cleanup. Each database change has
  durable tombstone/group evidence first, every recursive deletion is bounded, and injected deletion
  failure is never ignored.
- Crash after unlink/rmdir but before parent sync and after parent sync but before the cleanup-state
  transaction. Recovery syncs affected parents before marking cleaned. Permanent cleanup failure
  records `cleanup_failed`, leaves committed product state intact, and supports idempotent
  administrator retry.
- Rollback failure remains durable and does not hide the original error.
- Client disconnect before and after publication follows the documented boundary.
- A simulated 20 GiB stream remains bounded to two chunks and does not monopolize one file worker.
- During that simulated 20 GiB upload, continuously issue short authenticated HTTP requests, media
  reads, LLM submissions, LLM result receipts, CPU work, and SQLite work. When the resource required
  by each operation has capacity, it must be dequeued, start, and make progress without starvation;
  FIFO does not assert that the short operation must finish between two upload chunks. Verify that
  only the socket reader pauses when the file queue is full and that both network event-loop threads
  continue polling unrelated connections.
- Repeat with enough concurrent large uploads to fill the file queue. FIFO order and the two-chunk
  per-stream bound must keep memory and queued work bounded; no stream may insert consecutive chunks
  ahead of an already queued interactive or LLM file operation.
- Copy a simulated 20 GiB file, enumerate a directory larger than one entry batch, and recursively
  clean a large tree while issuing small file operations. Copy remains 1 MiB-bounded, traversal and
  cleanup remain 256-entry-bounded, continuations return to the FIFO tail, and small work progresses.
- COPY a directory with far more than 256 descendants and maximum legal depth. The journal group has
  one root Publish entry and at most the bounded depth cursor—never one row/DTO per descendant—and no
  group can exceed 256 entries. Crash after each enumeration, file sync, directory sync, cursor,
  reservation-extension, root exchange/rename, and parent-sync boundary; recovery resumes or cleans
  only the private tree, and the destination exposes either the complete old tree or complete new tree.
  Concurrent out-of-process source mutation, symlink, special file, depth/path overflow, and one-over
  entry batch produce the typed rollback without a mixed published tree or path escape.
- Concurrent AI task types progress independently; one transient result persistence failure moves to
  the tail and does not block later results.
- Deduplicate and face grouping finalize after partial inference failure. Run each with zero, one, 256,
  257, and millions of successful inputs plus interleaved failures. Deduplicate candidate pages retain
  only their bounded best edges; long/split/cyclic graphs converge through durable 256-edge minimum-label
  passes, near-duplicate precedence is deterministic for identical membership, and no pending-cluster
  map, signature vector, or whole JSON exists. Face grouping pages manual anchors and automatic
  representatives, preserves manual IDs/user memberships, assigns a new matching face to the best
  existing manual group, and reduces representatives without all-group/all-candidate collections.
  Crash, cancel, busy, and transient-fail at every candidate, edge, pass, face, group, representative,
  publication, and cleanup cursor. Only a complete generation becomes visible in one short transaction;
  prior groups remain visible beforehand, every continuation releases resources and returns to the tail,
  and recovery resumes without duplicate membership or a long SQLite transaction.
- Inject shutdown in every group and entry state and while each executor queue is saturated. Graceful
  shutdown rejects `NewClaim` but continues admitting `ExistingClaimCompletion` and `RecoveryHandoff`
  with their original bounded credits. Fill the queues before quiescing and place `NewClaim` both in an
  executor queue and in the scheduler waiter registry. The already-enqueued command is pre-transition
  admitted work and must resolve; every not-yet-linearized waiter is removed with `ShuttingDown`, its
  credit is released, and its row remains unclaimed. Completion/handoff preserves FIFO behind already
  enqueued work and among the remaining admissible waiters, but cannot be blocked by a rejected waiter
  or be rejected merely because new work is closed. It hands off every journaled group, releases every r2d2
  connection, terminates child groups, drains operation queues, closes log production, drains/syncs the
  log ring before file-worker join, joins workers, and reports final diagnostics only to console. A forced
  deadline exit may leave journal group/entry/path-claim evidence and non-journal durable rows with
  source, row ID, state, claim token, and correlation attempt. The shutdown log identifies both sets,
  and journal recovery plus `DurableClaimRecovery` deterministically resolves them.

## Acceptance Criteria

- One scheduler thread routes work but runs no business logic.
- Exactly the configured CPU, total I/O, and SQLite worker counts exist.
- Worker counts are immutable for one process lifetime; runtime configuration cannot create a mixed
  advertised/actual topology, and changing them requires a validated restart.
- Worker counts are inside the hard parser limits and the reported combined runtime budget, and every derived capacity is checked and
  allocated successfully before any worker is spawned; malformed sizing cannot overflow, panic, or
  leave a partial runtime. The checked deterministic reservation includes fixed thread stacks and all
  bounded runtime payloads, including the exact all-durable-slots result-processing multiplier, and
  cannot exceed `MAX_DERIVED_RUNTIME_BYTES`. The same startup report proves the exhaustive maximum open
  descriptors fit soft `RLIMIT_NOFILE`; an insufficient limit fails before any worker or listener exists.
- The exact bounded `config_bootstrap` read and terminal `--init-config` mode are the only pre-runtime
  application-file exceptions needed to discover/create config before worker counts exist. After a
  normal config read and sizing, private one-shot bootstrap channels are the only pre-scheduler
  execution path. Public handles,
  durable recovery, AppState, and HTTP admission exist only after file and SQLite readiness barriers.
  File logging is console-only until the shared capacity budget has loaded every durable reservation,
  verified its owner/file evidence, measured SQLite/log allocation, and reached `RecoveryReady` before
  activating the quota-controlled sink; no startup append bypasses or precedes reconstruction. The
  configured data directory already exists, its descriptor is the allocation-free lifetime lock target,
  and `RecoveryOnly` performs no unreserved filesystem mutation. Missing roots, probe cleanup/renames, and log
  pruning occur only afterward. An over-quota log class is pruned first through bounded zero-growth
  operations; no allocating token is issued until the ledger is healthy. The sole exception is one checked
  recovery-floor token for a valid crash-left SQLite WAL that must be recovered before durable
  reservations can be read; it is bounded, exclusive to bootstrap, and destroyed before ordinary
  admission.
- The startup config snapshot is revalidated by one fixed `ConfigFileCapability`; every normal/startup
  reset/runtime config update thereafter uses file/CPU executors, preserves comments/unrelated fields,
  compare-and-sets external identity, and publishes live state only after atomic replacement and
  parent sync. System timezone is one immutable bootstrap file-worker snapshot used explicitly by all
  cron continuations; no scheduler/runtime `Local` lookup or timezone file I/O exists.
- A proven-fresh database is built and synced under a private source-owned name, then atomically renamed
  and parent-synced; the official path is never a partially initialized schema. Bootstrap crash recovery
  removes only an unpublished known temporary, while ambiguous official-plus-temporary state fails
  visibly instead of guessing.
- One allocation-free lifetime process lock prevents two Momento API processes from owning the same data directory.
- Derived connection, request, inbound/outbound stream-session, inbound-durable-stream, file-chunk,
  durable-orchestration, and ingress-credit
  admission bounds all live futures, buffers, claims, and queue waiters without another configured
  concurrency value. Slow stream sessions do not retain request or file-chunk capacity while waiting
  on their peers.
- Parser/header/URI and buffered-control body sizes are fixed and charged per request; media transfer
  routes are explicitly streaming, so no extractor can buffer a 20 GiB upload or apply the 1 MiB control
  limit to it.
- Every non-streaming API response is cursor/cardinality bounded and at most 4 MiB; all larger byte
  products are streaming responses. No SQL result or serializer can grow an active request outside
  `RuntimeSizing`. WebDAV multistatus output incrementally traverses and serializes bounded batches, so
  even `Depth: infinity` never retains a complete directory tree or XML document.
- The scheduler is the sole executor admission arbiter. Raw queue senders never enter AppState or
  business code. Every legitimate caller owns a pre-reserved ingress credit, so durable work never
  waits outside the bounded command channel and non-waiting HTTP cannot bypass an older FIFO
  reservation. The exact seven scheduler control sources are level-triggered versioned wakeups whose
  authoritative state exists outside the coalesced command, so saturation cannot lose control state.
  The scheduler reads only memory; SQLite-backed cancellation is owned by `LlmCancellation`.
- Network and non-SQL application filesystem I/O, including log append/rotation/pruning, use only the
  I/O executor; CPU-heavy work uses only the CPU executor; all SQL uses only the SQLite executor. Direct
  rate-limited console diagnostics and r2d2/SQLite internal I/O remain the explicitly declared
  infrastructure exceptions, as does file I/O performed inside supervised external media processes
  against paths staged by the I/O executor, plus the exact pre-runtime config bootstrap described
  above. No logger thread exists.
- The embedded GeoNames gzip has compile-time compressed/decompressed/record/field limits and is
  initialized once by a pre-listener CPU operation into the charged packed immutable index. Runtime
  lookup performs no file I/O, decompression, lazy initialization, unbounded allocation, or third-party
  index construction.
- Supplemental and exiftool metadata are separate bounded control sources despite arbitrarily large
  originals: exact candidate selection, 4 MiB encoded input, typed 1 MiB normalized output, structural
  bounds, and sequential parsing prevent a sidecar or embedded metadata blob from escaping
  `RuntimeSizing`.
- Authentication's only mutable cross-request state is the bounded SQLite attempt table; no process
  bucket map/mutex can grow with attacker-controlled identities. Argon2 uses one exact single-threaded
  parameter set, validated stored hashes cannot raise it, its all-CPU-slot memory peak is charged, and
  dummy verification is initialized before requests rather than at first use.
- Concurrent import content identity is serialized only by a token-fenced SQLite hash claim plus the
  partial unique imported-media hash index. A contender retains no worker or in-memory keyed lock while
  waiting; cross-source exact duplicates reuse one canonical media and preserve access, timestamp,
  sidecar, and metadata-rerun semantics across crash recovery.
- DNS is asynchronous on the two network workers with a fixed cache; outbound HTTP has no idle pool and
  bounded sequential redirects. Network admission therefore bounds every resolver/service socket and
  no resolver or connector creates a hidden worker/thread population.
- Application file access is capability-based: business code cannot construct an absolute storage
  path, follow a symlink, or mutate outside a `StorageRootId` directory handle.
- The root registry covers originals, every thumbnail class, previews, imports, albums, trash,
  WebDAV, backups, logs, independent journal storage, and optional read-only static content. The generic
  data-root bootstrap capability is closed before runtime admission, and static serving uses no
  direct filesystem service.
- Every mutable root supports same-filesystem atomic rename with Journal; Static is read-only,
  descriptor-contained, and cannot overlap private storage. Mount-ID validation and actual bootstrap
  rename probes reject cross-mount publication before admission. The replacement static responder
  derives weak validators and range/length decisions from the same open-descriptor `statx` snapshot
  used to stream bytes, accepts only immutable or atomically replaced deployment content, preserves
  the exact strong/weak precondition order plus range/cache/MIME/HEAD behavior, and never SPA-falls
  back for API, WebDAV, missing assets, or non-HTML requests.
- OS file descriptors stay in the bounded, generation-checked File I/O registry. Dropping or
  cancelling orchestration never performs a filesystem close outside a file worker. Close notification
  cannot be lost to queue saturation, registry locks never span blocking I/O, and the exact capacity
  formula with non-interchangeable budget classes bounds every inbound/outbound stream session, CPU
  pipeline, durable maintenance stage, file-worker handoff, and the two fixed log descriptors without serializing one source's CPU
  pipelines. Each stream retains one descriptor across chunks while file-chunk admission remains
  transient. Logical token drop retains its debit through `ClosePending`; only file-worker ownership
  consumption by one non-retried close attempt releases registry capacity, after explicit sync has
  established durability.
- Admission guards, disk tokens, file tokens, and mutation leases are private move-only capabilities;
  they cannot be cloned, fabricated, released twice, or reused after a generation changes.
- Every Journal, SQLite, and log block allocation uses the correct class-bound provisional/ephemeral
  token or durable checkout from the shared private `SpaceReservationToken` implementation in
  `DataDirSpaceBudget`. Declared and incremental writes, copies, replacements,
  derived/child outputs, LLM inbox data, and result artifacts preserve the checked
  additional-peak/new-allocation invariant; newly created directory entries include their declared
  filesystem-metadata peak, while only proven metadata-neutral/exchange/delete actions and pre-existing
  blocks reserve zero. The
  source-derived SQLite and Journal owners share data capacity without spending another live reservation,
  the bounded Log remainder, or the recovery floor. SQLite enforces its WAL limit and declared operation
  growth without a fixed database quota, while bounded log rotation owns only its quota. One ledger epoch covers fresh filesystem capacity, class allocation, and reservation
  transitions; physical I/O occurs outside the lock while its full reservation remains held, so every
  concurrent snapshot is safe or conservative. Durable reservations reconstruct before admission and
  transition only with matching owner evidence, publication, rollback, terminal persistence, or cleanup.
  Every `DbOperation` explicitly uses a fresh SQLite reservation, the one transaction that promotes a
  provisional parent, a consuming construction child, or the single rolling cleanup child; promotion
  and child transfer rather
  than duplicate obligation and advance the parent cursor atomically with business persistence.
- r2d2 manages connections while SQLite executor threads execute SQL.
- All r2d2 connections are warmed before readiness and every replacement is SQLite-budgeted; pool
  maintenance can restore connection count but cannot execute business work or bypass disk accounting.
- SQLite creates no helper worker or temp file: all temporary query memory is declared/charged and
  bounded, large work is indexed/paged, and only bounded incremental vacuum is permitted.
- No worker from one executor waits synchronously for another executor.
- Every executor is strict FIFO. A streaming operation has at most one executing and one queued chunk;
  its next chunk returns to the tail. Queue backpressure suspends the orchestration future rather than
  a network worker, so one 20 GiB transfer cannot monopolize HTTP, file I/O, CPU, SQLite, or LLM work
  while those required executor domains still have capacity.
- Post-journal HTTP, durable jobs, and recovery share the same bounded FIFO reservation path. Each
  closed-enum durable source has at most one waiter per executor, so job cardinality cannot enlarge
  the bound or create starvation.
- A normal durable row is claimed only after acquiring the global orchestration slot derived from all
  three worker counts. A durable outbound row additionally acquires outbound admission before claim,
  transfers out of the durable slot before file verification/network wait, and reacquires durable
  admission only for its local transition. A completed network phase drops its logical file token,
  requests close, and drops its byte buffer before that reacquisition. Active claims/futures cannot grow with database
  cardinality; a slow 20 GiB submission cannot exhaust durable capacity, and inference wait consumes
  no Momento admission or file token.
- One network-runtime `LlmTransport` owns the single WebSocket. It round-robins one bounded frame per
  outbound session, preserves sink-confirmed per-session marker order, reserves bounded urgent-control
  and inbound-result capacity, continuously receives results, and guarantees heartbeat, cancellation,
  and result-receipt progress without allowing control traffic to starve data.
  No AI type, large submission, disconnected job, or unbounded transport channel can block the durable
  state of another job or choose llm-service inference order.
  External urgent producers have exactly one owner credit; loop-local urgent controls never use the
  mailbox the loop drains, so mailbox saturation cannot self-deadlock the connection.
- Deduplicate and face-group downstream work is also durable and bounded after inference. Candidate,
  edge, label, anchor, representative, group, and cleanup scans use versioned 256-row/1 MiB cursors and
  ordinary CPU/SQLite operations; neither finalizer keeps all media/faces/groups in memory or one long
  transaction. Partial inference success still publishes a complete new generation with a warning.
  Duplicate components converge through restartable label passes. Face publication preserves manual
  IDs/memberships and can attach a new face to an existing manually merged group. The old visible
  generation switches only after all new cursors are complete and is then cleaned incrementally.
- Results use a manifest-first bounded stream, not the 1 MiB control-envelope limit. Momento verifies
  exact size/count/hash into its own synced durable inbox file with one credited 64 KiB frame per stream
  and no shared-filesystem assumption. llm-service round-robins only credited result streams, so one
  blocked result cannot hide controls or other results. Every active receive owns a deterministic
  generic journal group, declared-byte Journal reservation, and exhaustive schema/task/manifest-derived
  SQLite growth reservation before `resultReady`; crash recovery can
  roll it back or finish it, and temporarily insufficient safe disk capacity defers instead of rejecting. A result
  without inbound durable admission is deferred without retained bytes or a consumed llm-service
  delivery attempt; its durable `nextDeliveryAt` survives restart, and ordinary saturation is not
  rejection. A committed admission converts without a credit gap into sink-confirmed `resultReceived`
  ownership. Matching receipt atomically supersedes queued/submitting/submitted replay and blocks
  further submission claims; duplicates are idempotent and active mismatched attempts are rejected.
  The SQLite reservation remains tied to the durable correlation until terminal persistence/cleanup,
  so acknowledging receipt never discards the only copy of a result that cannot fit in Momento's database.
  Temporary capacity shortage defers; a checked requirement above the immutable Journal/SQLite hard
  limit becomes a permanent pre-body condition, but the correlated Momento job is first durably failed
  with the bounded rejection transaction. If that small write is temporarily unavailable, delivery
  defers rather than stranding `submitted`; after commit it rejects and cannot loop forever in
  `callback_pending`.
- The old monolithic result representation does not remain. Every model runtime, provider, durable
  queue, transport, and Momento consumer uses bounded `momento-result-records-v1`; logical large values
  use at most four ordered continuations and the complete text/tag aggregate is at most 2 MiB. A larger
  provider value fails explicitly rather than truncating or creating a giant SQLite cell. Its exact endian-stable header, fixed-width/length-prefixed
  payloads, checked pre-allocation decoding, CRC32C, exhaustive per-kind encoded/normalized bounds, and
  exact compile-time executor-envelope formula make both work and memory amplification provable.
  Exact minimum-size, total-count, continuation-count, and task-state limits add stream-level bounds.
  llm-service atomically commits synced records plus manifest
  before `callback_pending`; recovery never reruns a completely persisted result or delivers a partial
  one. Momento first reads, validates, and idempotently stages every bounded record without creating an
  artifact; it then publishes complete journal groups of at most 256 entries under one product version.
  Those groups stay invisible until one complete token-fenced terminal reference switch, and any prior
  failure leaves the old product intact with durable cleanup ownership for all new artifacts. One
  correlation-owned cleanup cursor pages SQLite staging/plan deletion within 256 rows/4 MiB and the
  reserved WAL peak; Journal and SQLite reservations release only after their respective durable cleanup
  evidence is terminal. Entering terminal cleanup transactionally shrinks the SQLite parent to the one-
  batch cleanup recovery bound; even an administrator-visible permanent cleanup failure cannot retain
  the full result-construction reservation or starve unrelated database work.
- Active result receipt is job-version fenced. Cancellation stops frame credits and prevents Publish or
  inbox commit before the forward point, or completes the journal forward with a discarded outcome
  afterward; `cancel_requested` is an intent flag rather than another journal state. It durably hands
  off cleanup and acknowledges intentional discard. Every post-`resultReady` failure closes logical
  resources and durably hands off rollback/forward cleanup before releasing network admission;
  transient, permanent, and cancellation outcomes differ only in Deferred, Rejected, or Received.
  File-registry and disk ownership remain with cleanup until physical work finishes. One exclusive
  `JournalMutationLease` per group prevents cancellation/recovery cleanup from racing an in-flight
  bounded write; cancellation invalidates the generation immediately but state transition waits for
  that operation's durable handoff. Every queued mutating command carries that generation and the file
  worker validates it at the syscall boundary; cancellation closes the gate before its database commit,
  so a command not yet linearized cannot cross the forward commit point. Artifact-group cancellation is inherited from the result-product
  marker and cleaned in bounded pages rather than one unbounded transaction.
- A correlated `resultReceiptRejected` is terminal in llm-service: it commits failure evidence through
  temporary write/sync/rename plus job-directory sync before atomically completing
  `callback_pending -> failed`, with no inference rerun or delivery retry. Temporary-only recovery
  redelivers; complete evidence finishes forward; a disconnect before receipt leaves
  `callback_pending` eligible for idempotent redelivery.
- Cancellation advances the job version and is final across every receipt/result-worker ordering.
  Stale preparation cannot publish or persist; discarded inbox files are durably cleaned, and late
  results for cancelled/completed/failed/deleted work are acknowledged without recreating work.
- Pending all-task or task-specific cancellation scopes gate submission claims in SQLite. Stable
  request IDs, cutoff, paged exact IDs, ordered count/hash, and credited bounded chunks survive
  disconnect/restart and disappear only after the matching acknowledgement transaction. No database
  transaction or control envelope grows with total cancelled-job cardinality; the published llm-service
  canonical sorted set is a bounded on-disk authoritative fence for admission, queued/inference work,
  result publication, callback selection, and active delivery before background marker/deletion
  completes. Startup restores that fence before queue activity; it is removed only after synced exact
  markers and cleanup cursors durably replace every membership. Above-cutoff replacement jobs are not
  cancelled. At most one unconverted published set exists per client, so cancellation history cannot
  turn one membership check into an unbounded scan; a later request waits retryably while another client
  remains independent. Before scope commit, the scheduler closes the bounded scope gate and invalidates
  every matching active mutation generation; new receives/submissions cannot slip into the interval
  before their job row's paged cancellation.
- Every executor operation returns an owned policy-free `ExecutorError`; errors propagate to one HTTP
  or durable-job boundary without suppression or repeated logging. Only that boundary selects durable
  disposition from the exhaustive operation/stage/error/cancellation/claim policy table.
- Operation and business panics are caught at their narrow boundaries, clean operation-local state,
  and permanently fail only their current durable job rather than retrying a deterministic panic.
  Explicit transient errors still release resources and return to the tail. Infrastructure or invariant panics cause
  coordinated fatal shutdown and startup recovery; the process never continues with a missing worker
  or corrupted executor state, and no destructor attempts asynchronous database work.
- Every durable failure carries one exhaustive typed `Retry`, `Fail`, `Cancel`, or `Superseded`
  disposition; no boundary guesses from a display category or error string, and a stale owner cannot
  mutate its durable row.
- Database transactions roll back on error. File writes use temporary paths and atomic publication.
  Atomic rename is followed by affected-parent directory sync before the publication commit point.
  Cross-resource failure uses only the shared group journal and reverse compensation before its first
  committed entry; a partially published group completes forward or remains `publication_failed`,
  while a permanent database finalization error becomes `finalize_failed`. Neither path rolls back a
  committed entry, and obsolete files are cleaned only after product commit. Permanent publication
  and cleanup failures become explicit administrator-retry states rather than infinite loops or
  ignored errors.
- Transient durable failures release execution resources and move to the queue tail. Permanent failures
  retain no worker, handle, connection, buffer, or queue slot; they may retain only their explicitly
  recorded path claims, existing bytes, and bounded recovery reservation. `publication_failed` and
  `finalize_failed` deliberately block conflicting normalized paths, and their accounted disk ownership
  may reduce new disk admission until administrator retry/cleanup, but they cannot monopolize executor
  capacity or an unrelated database result's released construction reservation.
- Path-claim conflict queries are component-boundary correct, bounded by source-owned path limits,
  and index-backed. Administrator journal inspection/retry is typed, authorized, idempotent, and does
  not expose absolute paths. Retry request receipts make identical retries idempotent for the fixed
  receipt-retention interval; at most 64 remain live per operation and bounded maintenance expires them
  even while failed evidence is retained. Fully resolved journal rows compact and later prune.
- Journal detail explicitly reports full or compacted representation; compacted rows preserve exact
  terminal summaries without pretending deleted entries or claims are still available.
- Child processes inherit only their explicit descriptor allowlist. A media tool never gains a
  listener, database connection, storage-root capability, or mutable ordinary-path input.
- Long metadata work is owned by a token-fenced active claim rather than a time lease. Every exit path
  durably resolves its claim; startup recovers journals before requeuing prior-process orphans, stale
  owners cannot publish or persist, and an already committed journal completes forward exactly once.
- Startup `DurableClaimRecovery` covers every closed durable source in one transaction after journal
  reconciliation. It clears all prior-process active claims, preserves true external waits, and replays
  ambiguous LLM admission with the identical durable correlation attempt.
- Shutdown follows the specified state machine and never stops network orchestration before journaled
  requests and non-journal durable claims have reached a durable recovery state. Quiescing rejects only
  not-yet-linearized new claims, removes their waiters, and releases their credits while leaving rows
  unclaimed. Already-enqueued claims are pre-transition admitted work; bounded existing-claim
  completions and recovery handoffs remain admitted in FIFO order without waiting behind removed work.
  A forced deadline
  records both journal evidence and every unresolved durable row for the two startup recovery passes.
- Large media remains streamed and bounded; a several-hour 20 GiB video is not rejected by an
  artificial small-media limit.
- Compressed raster size is bounded independently from encoded byte size: checked geometry precedes
  allocation, native decoders stay within the charged tile window, fallback decoding is confined by
  the declared ImageMagick memory/address-space and fixed four-GiB disk-scratch limits, and AI processing still uses the
  full-resolution original or required original region rather than a thumbnail.
- Existing AI, import, grouping, llm-service separation, logging-body redaction, and Android-in-Docker
  behavior remain intact.

## Non-Goals

- This refactor does not change llm-service model scheduling.
- It does not add database migration code.
- It does not promise an impossible automatic transaction spanning SQLite, the filesystem, and a
  remote network peer. The generic journal and compensation provide deterministic recovery instead.
- It does not retain the old scheduler API or configuration shape.
