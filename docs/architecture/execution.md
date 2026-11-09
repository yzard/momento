# Execution domains

Read when changing scheduling, worker capacity, or CPU, filesystem, and SQLite execution.

Momento API has one dedicated scheduler thread, an asynchronous network runtime configured by
`[thread_pool].network_io_workers` (minimum 2), and three closed executor domains configured by
`[thread_pool].cpu_workers`, `[thread_pool].storage_io_workers`, and
`[thread_pool].sqlite_workers`. Client requests and every background business operation acquire
source-owned capacity from the scheduler, then submit typed bounded operations to the CPU, File I/O,
or SQLite FIFO; no business operation blocks a network thread with CPU, disk, or SQL work. Business
modules do not create their own threads, Tokio tasks, blocking pools, Rayon pools, or semaphore-based
concurrency windows. r2d2 is the deliberate additional connection pool: it owns SQLite connections
and its connection-maintenance thread, while each SQL operation executes only on the configured SQLite
executor worker that checks out that connection. File workers exclusively perform filesystem and log
sink I/O; CPU workers own parsing, encoding, hashing, image work, and supervised child processes.
`storage_io_workers` excludes network threads (minimum 2): one dedicated reader, one dedicated
writer, and remaining workers alternate between ready read/write FIFOs. Log writes, cleanup,
and mixed operations must never execute on the reserved reader. No `io_workers` alias exists.
`sqlite_workers` counts one dedicated writer plus `sqlite_workers - 1` readers (minimum 2).
The scheduler routes SQL through separate bounded read/write FIFOs using the operation's read
specification, never its HTTP method or its name. Reads execute under SQLite `query_only` protection;
transactions that may mutate data, including claim/cleanup operations, must use the writer lane.
The r2d2 connection limit remains `sqlite_workers`, with one additional maintenance thread.

AI finalization uses a shared versioned work signal for deduplicate and face grouping.
The SQLite worker publishes this signal after successful AI start and cancellation operations, including starts that queue no inference jobs.
Workers record its version before draining work and wait for a change from that version afterward.
Both workers observe changes that arrive during draining or before waiting, even if the request caller disconnects.

SQLite capacity publication measures the database, WAL and SHM while holding the budget ledger
lock; callers pass the database path and operation name, never a previously measured byte count.
Each file is charged at the greater of its logical length and allocated physical bytes, including
at reconstruction, so delayed allocation/compression does not create unaccounted logical growth.
Growth errors report the operation, reservation ID, prior baseline, live outstanding growth,
physical/logical/charged bytes and excess bytes. This is not a fixed database-size quota.
Connections disable checkpoint-on-close: pool retirement cannot run an unbudgeted checkpoint.
Writer-owned automatic/explicit checkpoints remain enabled. Connection initialization publishes
its allocation even on failure, before releasing its provisional budget.

OCR start computes its SQLite reservation from prepared input counts and descriptor byte lengths.
The writer calculates this estimate before admission and keeps ownership through insertion.
The estimate includes job rows, input snapshots, indexes, page splits, and WAL growth.
A rejected reservation leaves existing OCR text and queued jobs unchanged.
