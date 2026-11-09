# Backup I/O and scheduling

Backup bodies share a bounded reader for resumable chunks and batches. The server
reserves an existing scheduler file-chunk slot before collecting up to 1 MiB of
network frames. It sends that owned buffer through the typed hash and file-write
executors, instead of scheduling one pair of jobs per TCP/HTTP frame. File lengths,
per-file SHA-256 checks, rollback, fsync and durable completion remain unchanged.
No batch-wide media buffer or independent worker pool is introduced.

Authenticated create, prepare, chunk, batch-upload and completion requests hold a
scheduler-owned backup activity guard. During activity and a ten-second idle grace
period, new disk-heavy background claims share one admission lane. This includes
metadata, non-backup imports, AI input submission, finalization and maintenance;
journal recovery also shares that lane when advancing an already-claimed step.
Existing jobs finish normally. FIFO admission within the background lane preserves
progress for recovery instead of letting a metadata worker immediately reclaim it.

Backup import claims, result receipt/persistence, cancellation and file cleanup
remain eligible. Backup import completion (which rereads originals to verify
SHA-256) has its own FIFO lane during backup activity, limited to the smaller of
two and `storage_io_workers - 1`. This avoids turning an uploaded batch into
eighteen competing full-file reads while preserving reserved file worker roles. Recovery handoffs and shutdown draining bypass the background limit.
Throttled work releases the common admission turn, so it cannot block backup work
at the head of the queue. After the final guard is dropped and the idle grace
expires, the ordinary configured scheduler capacity resumes automatically.

The change does not alter worker counts, executor reader/writer reservations,
SQLite durability, journal evidence checks, storage roots, or the original-only
mobile backup contract. Measure upload request time separately from PhotoKit
export/preparation and final durable verification; different media mixes can
change end-to-end throughput even on the same network.
