# Metadata decoding and recovery

## Decoder contract

Both images build LibRaw 0.22.2 with Adobe DNG SDK 1.7.1 (build 2652,
checksum-pinned). API uses Alpine's JPEG XL library; LLM builds the JPEG XL
sources included in that same SDK for its Ubuntu runtime. ImageMagick's C API
owns a DNG host per decoder instance; `dcraw_emu` explicitly receives `-dngsdk`.
No runtime downloads or shared storage between the services are required.

RAW preview dimensions remain unchanged. The preview feeds the thumbnail, then
the thumbnail feeds the tiny thumbnail. No embedded reduced preview substitutes
for the full-resolution decode. Existing CPU admission, ImageMagick memory
reservation, process address-space limit and mounted-volume disk cache still apply.

ImageMagick diagnostics explicitly reporting an unsupported decoder/RAW format
or compression become `decoder_unsupported`, with the original path and underlying
diagnostic preserved. The metadata job becomes `failed` and has no automatic
retry timer. An administrator may explicitly Generate after updating decoder
support. Timeouts, memory/cache pressure, filesystem and database failures remain
retryable; a failed conversion alone does not prove unsupported decoding.

## Journal scheduling

Background recovery has two ready FIFOs:

- Blocking: publication, file commit, rollback, and cancellation cleanup.
- Ordinary: cleanup with no cancellation request.

Each available slot alternates FIFOs, borrowing the other when empty. Retry delay,
active-group exclusion and `recovery_order` remain authoritative within each FIFO.
Even with one recovery slot, the blocking FIFO is considered within two completed
steps, not after draining the cleanup backlog. Older blocking work still precedes
newer blocking work. Existing running steps are not
preempted. No additional threads or concurrency configuration are introduced.
Startup consistency-critical recovery retains its existing single-slot behavior.

Metadata still cannot overwrite artifacts while its prior rollback owns them.
`waitingForRollbackJobs` counts these queued jobs (a subset, not an extra total),
and both clients show “Waiting for Journal rollback (queued)”. Idle worker logs
include `reason=journal_rollback_pending`. Journal completion wakes metadata.
There are no schema changes or migrations.

## Regression checks

- Rust metadata tests: permanent decoder failure, no automatic claim/delay,
  explicit retry, and transient conversion errors remaining retryable.
- Journal tests: independent ordering behind 1,000 cleanup groups, delayed and
  active exclusion, cancellation cleanup, rolling concurrency and wakeup safety.
- Status API and client tests: waiting count distinct from total queued.
- Each Docker decoder build decodes Adobe's integer-linear, float-linear and
  Bayer JPEG XL DNG samples; API additionally exercises ImageMagick's C API.
- Android checks run only through `./build_android_client.sh verify`.

Decoder build sources: [LibRaw DNG SDK integration](https://github.com/LibRaw/LibRaw/blob/0.22.2/README.DNGSDK.txt),
[Adobe DNG SDK](https://helpx.adobe.com/camera-raw/desktop/dng-and-file-formats/digital-negative.html).
