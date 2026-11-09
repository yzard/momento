# LLM submission and cancellation

Read when changing WebSocket admission, submission retries, correlation, or cancellation.

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

The Momento submission worker uses scheduler durable admission plus the shared typed SQLite, File I/O,
CPU, and network domains. It claims and streams a replacement job as soon as any submission completes, and sleeps only
when no eligible queued job remains. There is no submission-specific concurrency setting; this
application scheduling is independent from every model runtime's inference concurrency.

All AI task types share global `[llm].enabled` gating but retain independent job eligibility and
trigger/run paths. The submission worker sends existing queued jobs; it does not create task jobs.
Submission reads the immutable
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
