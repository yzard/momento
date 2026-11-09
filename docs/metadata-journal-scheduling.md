# Metadata and Journal rolling scheduling

## Metadata

The coordinator owns a bounded window of single-job futures, not long-lived
lanes that each drain the entire database queue. Completing one job immediately
refills that slot. An empty claim retires that slot, while the versioned metadata
work notification refills idle slots even if another job is still running.
The outer worker retains empty-queue and retry-deadline handling.

Every job still acquires global scheduler admission and atomically claims its
SQLite row. A coordinator error stops replacement and drains already-started
jobs before returning. No new thread pool, semaphore or concurrency setting is
introduced.

## Journal

Normal background recovery uses a rolling window derived from
`storage_io_workers - 1`. This excludes the reserved storage reader from the
potential writer count; actual reads/writes still use their existing executor
lanes. Each operation acquires global durable admission. The outer recovery
coordinator holds no admission while waiting for its steps. Durable admission
uses a cancellation-safe FIFO turnstile so newly available work cannot overtake
waiting work. HTTP startup does not acquire background admission for logging.

The FIFO query excludes only the bounded set of group IDs already in flight.
Selection order remains `recovery_order, id`; completion order may differ.
There is at most one step per group in the window. Existing path claims,
mutation leases, file identity checks and versioned checkpoints remain enforced.
A completed step frees its slot immediately; it does not wait for an entire
batch. Errors release the step's resources before durable deferral. A fatal
coordinator error drains the current window before returning.

Startup consistency-critical recovery remains serial. SQLite still has exactly
one writer. These changes modify queries and orchestration, not the database
schema, and require no migration.

## Verification

- Metadata: notify new work while one slow job remains; refill idle capacity and
  replace an independently completed job without waiting for the slow jobs.
- Metadata: failure drains active work and stops replacements.
- Journal: observe simultaneous admitted operations, enforce the configured
  window, clean multi-entry groups exactly once, and replay idempotently.
- FIFO: exclude an active head without reordering the remaining queue; preserve
  indexed selection without a temporary ordering table.
- Existing busy-head deferral, rollback, permission and recovery tests remain
  part of the full backend regression suite.

Local tests prove concurrency and correctness, not NAS throughput. After
deployment, compare `recovered_entries` over time and metadata processing counts;
ZFS latency and the shared SQLite writer can still limit throughput.
