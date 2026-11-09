# ImageMagick memory quota

`[media_process].magick_memory_quota_bytes` is a startup-only shared quota for
Momento API's ImageMagick processes, including image inspection and face crops.
The default is 4294967296 bytes (4 GiB); the minimum is 536870912 bytes.
It is not a quota for llm-service or all Momento allocations, nor an RSS/cgroup cap.

Each conversion reserves 512 MiB plus 24 bytes per input pixel for the decoder,
encoder, libraries and bounded heap cache, plus a file-backed mapping allowance
of `min(24 * input_pixels, 1 GiB)`. Inspection reserves 512 MiB with mapping disabled.
The mapping allowance is counted even though mapped pages can be reclaimed. These
are conservative allowances, not measurements of physical RAM. The same allowance
is enforced as the child's address-space limit. Shared reservations never exceed
the configured quota. A request exceeding the whole quota fails explicitly rather
than waiting forever; increase the quota for larger decoder requirements.

Logs distinguish `magick_memory_quota_busy` (INFO: temporary contention, waits
asynchronously) from `magick_memory_quota_exceeded` (ERROR: the request cannot fit
the configured total). Waiting work logs `magick_memory_quota_acquired` when it
resumes, including the wait duration. Quota logs include requested and total bytes;
busy logs also include currently reserved bytes and FIFO/capacity wait reason.
Metadata preflights the largest required conversion and records a non-retryable
failure with media ID, original source path, and byte counts when it cannot fit.
Increase the quota, restart the service, then explicitly generate metadata again.
This failure does not loop through the automatic 30-second transient retry path;
concurrent cancellation and explicit rerun requests still take precedence.

Waiting is asynchronous and FIFO, before taking a CPU execution slot. A queued
caller's cancellation releases its reservation; once dispatched, the executor owns
it until the child exits, even if the caller disconnects. No new threads are added.

ImageMagick's pixel heap cache is limited to 256 MiB. The area limit is passed as a
bare pixel count (for example, `200000000`), never with a `P` suffix that can be
parsed into an effective zero-area limit. File-backed mapping is enabled up to the
per-image allowance, with a matching 1 GiB ceiling in the Docker security policy.
Mappings are included in both shared admission and the child's virtual-address
limit; enabling mmap does not bypass the quota. Caches beyond the heap/mapping
limits use direct disk I/O, up to 32 GiB per process, under the container's existing
`TMPDIR=/data/tmp` mounted data volume. Ensure adequate disk space; this limit is
not a preallocation or a global disk reservation. Decoder-internal allocations
cannot necessarily spill to disk. Original pixel dimensions are preserved.

Square thumbnail generation passes `jpeg:size=2Sx2S` before the input, where `S`
is the configured thumbnail edge (800x800 for a 400px thumbnail). JPEG decoders
can downsample during decoding; other image coders ignore this JPEG-only hint.
Tiny thumbnails use the same path with the existing thumbnail as their source.
Full-resolution previews and face crops never use this hint; originals and AI
input descriptors are unchanged. JPEG thumbnail pixels may differ slightly from
full-resolution decode followed by resize. Admission still conservatively uses
the validated original dimensions, not an assumed decoder downsampling factor.

Metadata replacement now registers old thumbnail, tiny-thumbnail and preview
cleanup in the same SQLite transaction as the replacement. Only already-published
metadata destination claims can be transferred; interrupted publications and
unrelated owners remain protected. Journal performs cleanup asynchronously and
retries failures. A failed transaction retains the old references and claims.
