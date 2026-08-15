# AGENTS.md - Momento Codebase Guide

> Guidelines for AI coding agents working in this repository.

## Project Overview

Momento is a self-hosted photo management application with:
- **Backend**: Axum + SQLite (Rust) in `src/backend/`
- **LLM service**: Separate Axum durable-inference queue in `src/backend_llm/`
- **Frontend**: React + TypeScript + Vite + Tailwind in `src/frontend/`

Monorepo managed with pnpm workspaces and Turborepo.

---

## Conventions

This repo follows the shared agent skills. Read them before changing code — they are the
source of truth, and this file only records what is specific to Momento.

| Skill | Governs |
|-------|---------|
| `project-structure` | Where every file goes: `src/`, `tests/` mirroring it 1:1, `docs/`, `playground/`, `build/`, `dist/`, `docker/` |
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
are `ocr`, `image_tagging`, and `image_clustering`; adding another identifier means extending this
same pattern end to end, not creating a direct or type-specific transport path.

### End-to-end ownership

```text
import
  -> metadata job
  -> task-ready inputs in Momento previews + media_ai_inputs descriptors
  -> durable Momento llm_jobs row
  -> manifest-first multipart submission
  -> durable llm-service disk queue
  -> one task runtime performs inference
  -> authenticated callback
  -> transactional Momento result persistence + terminal job state
  -> optional separately scheduled downstream work
```

No stage runs its downstream stage inline. Import only creates metadata work; metadata prepares
inputs; an AI trigger creates inference jobs; the Momento submission worker sends them;
llm-service performs inference; the callback persists results. Type-specific work after inference,
such as deduplication cluster generation, is another scheduled stage.

Momento owns all media preparation. It applies orientation, generates thumbnails or previews,
chooses video frame timestamps, extracts frames, crops/resizes, and records descriptors before an
inference job is eligible. llm-service may decode bytes and perform model-required tensor
transforms, but it never reads Momento paths, generates task inputs, or assumes a shared
filesystem.

The primary implementation points are:

- `src/backend/processor/metadata/generation.rs`: prepare task inputs.
- `src/backend/processor/metadata_worker.rs`: verify required inputs before metadata completes.
- `src/backend/processor/ai/mod.rs`: create/claim jobs, verify bytes, and submit requests.
- `src/backend/routes/internal/llm.rs`: authenticate, validate, and persist callbacks.
- `src/backend_llm/routes.rs`: authenticate and stream multipart admission.
- `src/backend_llm/scheduler.rs`: durable queue, batching, callbacks, retries, and recovery.
- `src/backend_llm/provider.rs`: task registry, local runtime lifecycle, and inference dispatch.

API and source directories mirror each other: `/import/*`, `/metadata/*`, `/ai/*`, and
`/internal/llm/*` have matching backend routes, processors, models, queries, frontend API
callers, and mirrored tests. AI triggers require an administrator. Internal LLM callbacks use
the configured callback key rather than a user JWT.

### Prepared input contract

Momento stores each prepared input below `previews/ai/<media-id>/<task>/` and inserts a
`media_ai_inputs` descriptor containing the task, sequence, input kind, relative file path,
filename, MIME type, byte size, SHA-256 content hash, and optional frame timestamp. Job
eligibility requires imported media, completed metadata, and at least one matching descriptor.

Before each submission, Momento loads descriptors in sequence order, reads the prepared files,
and rechecks their exact byte sizes and SHA-256 hashes. Missing or changed bytes fail the Momento
job; they are never submitted. The request is self-contained because llm-service receives the
raw prepared bytes rather than Momento file paths.

Multi-input jobs preserve every descriptor's `sequence` and optional `frameTimestampMs` through
the queue, provider response, callback, and input-level persistence. Concurrency is across jobs;
inputs within one job are currently inferred sequentially in descriptor order. A new type must
define explicit aggregation and persistence semantics for all inputs rather than silently using
only the first result.

### Submission wire contract

Momento sends:

```text
POST <llm-service>/api/v1/jobs/submit
x-api-key: <configured API key>
Content-Type: multipart/form-data

part 1: manifest                 application/json
part 2+: input-<sequence>        raw prepared bytes
```

The manifest is camel-case JSON with:

```text
jobId, mediaId, task, attempt, callbackUrl, inputs[]
```

Each input descriptor contains:

```text
sequence, filename, mimeType, byteSize, contentHash, inputKind, frameTimestampMs
```

The `manifest` part must appear before every `input-N` part. A non-empty hexadecimal `jobId`, a
known task, a non-empty callback URL, and at least one input are required. Every declared input
must be present, non-empty, and exactly match its descriptor's byte size and SHA-256 hash. The
current admission contract accepts image MIME types; supporting audio, text, or another payload
requires deliberately extending the shared descriptor and admission abstractions.

Momento job states follow:

```text
queued -> submitting -> submitted -> completed | failed
                  \-> queued       transient network/5xx retry
queued | submitting | submitted -> cancelled
```

Momento retries network errors and llm-service `5xx` responses, but treats other non-`2xx`
responses as permanent submission failures. A successful `2xx` means only that llm-service has
durably admitted the job, not that inference has completed. The callback must return the same
`jobId`, `mediaId`, `task`, and exact submitted `attempt`.

### Durable llm-service queue

Admission streams files into `.tmp/<job-id>/`, validates all descriptors and bytes, syncs the
staged data, and atomically renames the directory into `queuing/<job-id>/`. llm-service stores
the submitted bytes unchanged. Duplicate job IDs in any durable state are acknowledged
idempotently and are not enqueued twice.

```text
.tmp -> queuing -> processing -> deleted after a successful callback
                            \-> callback_pending -> deleted after a successful retry
                                                 \-> failed after retry exhaustion
                  processing -> failed for terminal local queue/processing failure
```

Each job directory contains `manifest.json` and `input-N` files. Inference adds `result.json`;
callback retry state adds `callback.json`; terminal queue failures add `failure.json`. There is
no `completed/` directory and no configurable queue-size limit. A successful callback is the
acknowledgement that permits deletion of all llm-service job data.

Startup recovery removes incomplete `.tmp` admissions, moves interrupted `processing` jobs back
to `queuing`, keeps `callback_pending` jobs that already have `result.json`, and requeues callback
jobs that do not have a durable result. Therefore interrupted inference may run again, while a
completed inference awaiting callback is delivered again without rerunning the model.

### Multiple-request scheduling

Only one model runtime may be active in llm-service. Every model provider is a local managed
subservice; model `base_url` values must use a loopback HTTP address. llm-service itself may run on
a different machine from Momento, but it never delegates inference to a remote model provider.
For each scheduler cycle:

1. A separate callback loop retries due durable `callback_pending` results; it never gates model
   inference and never reruns completed inference.
2. Read valid queued manifests and sort them by job ID.
3. If the currently active task still has queued work, select that task to keep its runtime warm.
4. Otherwise select the task belonging to the first sorted queued job.
5. Claim at most the scheduler's global `dispatch_batch_size`, moving only same-task jobs from
   `queuing` to `processing`.
6. Activate or reuse the selected local runtime and dispatch the homogeneous batch. The scheduler
   and provider do not apply a model concurrency limit.
7. The model subservice alone enforces its configured `max_concurrent_jobs`; vLLM uses
   `--max-num-seqs`, and Python runtimes use their inference semaphore.
8. Finish every claimed job independently, then immediately run another cycle while work remains.

This warm-task preference drains successive batches of one task before switching when that task
continues to have work. Switching task type shuts down the old runtime before starting and
readiness-checking the new one. A runtime is also shut down when no inference job is claimed for
`idle_shutdown_seconds`; callback delivery does not require or keep a model runtime active.

One failed job does not prevent other batch jobs from finishing. Provider or runtime inference
errors become durable failed callback payloads; inference is not retried by llm-service. Callback
delivery uses its own timeout, fixed retry delay, and maximum-attempt policy. Any callback `2xx`
deletes the queue directory; failure moves or keeps it in `callback_pending`, and retry exhaustion
moves it to `failed`.

### Callback contract

llm-service posts `result.json` to the manifest's callback URL with
`x-momento-callback-key`. A completed payload contains matching correlation fields,
`status = completed`, model type/version, a top-level result derived from the first input, and
ordered `inputResults` carrying each original sequence and frame timestamp. A failed payload
contains `status = failed` and an error.

Momento validates the callback key and correlation fields inside an immediate SQLite transaction.
Only a matching `submitted` job may transition to `completed` or `failed`. Result persistence and
the terminal state transition commit atomically. Matching callbacks for an already terminal job
are acknowledged idempotently; late callbacks for cancelled jobs are acknowledged without
persisting results.

Persistence is deliberately type-specific. OCR and tagging validate text-like results, store
input-level rows in `media_text_inputs`, and derive ordered media-level text in `media_text`.
Clustering validates its embedding/hash result and updates similarity tables. A generic transport
response does not remove the requirement for explicit validation, storage, clean/reset behavior,
and optional downstream scheduling for each inference type.

### Adding an inference type

Every new inference type must use one exact snake-case task identifier and propagate through all
layers below in the same change. Do not add a second submission endpoint, bypass prepared inputs,
call llm-service inline from metadata/import, let llm-service access Momento storage, or add a
type-specific queue/scheduler.

1. Add metadata preparation and completion verification for task-ready inputs, including stable
   sequence/timestamp rules and durable `media_ai_inputs` descriptors.
2. Extend schema constraints and centralized queries for eligibility, idempotent job creation,
   active-job uniqueness, status, cancellation, reset, clean, retry, and any run relationship.
3. Add administrator trigger/status API behavior and matching frontend API/UI behavior where the
   task is user-controllable.
4. Reuse the shared Momento `llm_jobs` submission worker and manifest-first multipart protocol.
5. Register the task in llm-service `ServiceType`, configuration validation, `ActiveService`,
   provider dispatch, and local runtime activation/readiness/liveness/shutdown.
6. Keep scheduler dispatch batches homogeneous, enforce `max_concurrent_jobs` only inside the
   local model subservice, and preserve ordered per-input correlation in provider responses.
7. Extend the callback DTO only for required result fields, then add strict type-specific result
   validation and transactional persistence in Momento.
8. Define failure, cancellation, restart recovery, duplicate callback, multi-input aggregation,
   clean/reset, and optional downstream-stage semantics explicitly.
9. Add mirrored tests for preparation and eligibility, multipart admission and raw-byte
   preservation, runtime reuse/switching, configured concurrency, result validation, callback
   retries/idempotency, persistence, cancellation, and recovery.

Metadata generation lives in `processor/metadata/`; `processor/regenerator.rs` does not exist.

---

## Configuration

Both binaries (`momento-api`, `llm-service`) require `-c|--config PATH` and read **no**
environment variables. A missing or malformed config is a hard startup failure — it never
falls back to defaults, because that would silently start the server against the wrong
data directory.

Momento filesystem locations derive from `storage.data_dir`:

```yaml
storage:
  data_dir: /data          # database.sqlite, originals/, thumbnails/, previews/, imports/, webdav/, ...
  static_dir: /app/static  # built frontend served as a fallback
```

`main` calls `constants::init_paths(&config.storage.data_dir)` once after parsing the
config; everything else reads `constants::paths()`. Never hardcode a path under the data
directory and never add a new `std::env::var` call — add a config field instead.

llm-service has separate `storage.data_dir` and `storage.queue_dir` settings. Its queue directory
must be durable and must not be a Momento originals, previews, or thumbnail directory. Queue jobs
are accepted only with a non-empty hexadecimal Momento job ID and a manifest that appears before
all `input-N` multipart fields.

`schema.sql` defines the current schema only. Do not add schema migration or compatibility code;
breaking schema changes require a fresh database for development and playground data.

`docker/Dockerfile` and `docker/entrypoint.sh` are the one place environment variables
belong (`PUID`/`PGID`/`UMASK`/`TZ`); they translate into the config file the entrypoint
generates, and `CMD` passes `-c /data/config.toml`. `RUST_LOG` and `RUST_BACKTRACE` are
ecosystem-standard runtime knobs read by `tracing-subscriber`, not application config.

---

## Build, Lint & Test Commands

### Root
```bash
pnpm install              # Install all dependencies
pnpm build                # Build all packages
pnpm dev                  # Dev servers (backend + frontend)
pnpm lint                 # Lint all packages
pnpm test                 # Run all tests

./run_playground.sh       # Full stack against playground/config.toml
./build_docker.sh         # Build the Docker image (cds into docker/)
```

`run_playground.sh` and `build_docker.sh` are the only scripts at the git root. Both
resolve paths from their own location, so they work from any working directory.

### Where build output goes

`run_playground.sh` writes nothing into `src/` or `playground/`. Intermediate artifacts
go to `build/`, final artifacts to `dist/`, both at the git root, one subdirectory per
component:

| Component | Intermediate | Final |
|-----------|--------------|-------|
| `momento-api` | `build/backend/target/` (`CARGO_TARGET_DIR`) | `dist/backend/momento-api` |
| `llm-service` | `build/llm/target/` (`CARGO_TARGET_DIR`) | `dist/llm/llm-service` |
| frontend | `build/frontend/workspace/` (staged pnpm workspace) | `dist/frontend/` |

The script starts both binaries from `dist/`, never from `build/` and never from
`src/backend/target/release/`, and `storage.static_dir` in `playground/config.toml` is
`dist/frontend`. It removes each component's `build/` and `dist/` subdirectory before
building, so a run can never pick up a stale binary. Both trees are gitignored.

`playground/logs/` holds the append-only logs from the *running* stack. Build artifacts
do not belong there.

### Backend (src/backend)
```bash
cd src/backend

# Build
cargo build                # Debug build
cargo build --release      # Release build

# Run development server
cargo run                  # Starts server on 0.0.0.0:8000

# Linting & formatting
cargo fmt                  # Format code
cargo clippy               # Lint code

# Testing (integration tests live in tests/backend/, not #[cfg(test)] modules)
cargo test                 # Run all tests
cargo test auth            # Run tests matching "auth"
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

All Docker files live in `docker/`; `build_docker.sh` at the git root drives the build
with the git root as the build context.

```bash
./build_docker.sh                                        # Build image
docker compose -f docker/docker-compose.yml up --build   # Full stack
```

The ignore file is `docker/Dockerfile.dockerignore`, not `.dockerignore` — Docker
resolves ignore files against the build context (the git root), so that name is what
BuildKit actually honors from inside `docker/`.

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
│   │   └── internal/llm/   # Callback-key authenticated LLM callback
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
│
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
└── frontend/

docs/                       # All project documentation

docker/
├── Dockerfile
├── Dockerfile.dockerignore # NOT .dockerignore — context is the git root
├── docker-compose.yml
└── entrypoint.sh

playground/                 # End-to-end config and data
├── config.toml             # The one config run_playground.sh starts the stack with
├── config_llm.toml         # Config for the LLM service
├── database.sqlite         # SQLite database
├── originals/              # Original media files
├── previews/               # Generated previews
├── thumbnails/             # Generated thumbnails
├── imports/                # Import staging area
├── webdav/                 # WebDAV processing area
└── logs/                   # Runtime logs from momento-api and llm-service

build/                      # Intermediate build artifacts, never run from
├── backend/                # CARGO_TARGET_DIR for momento-api
├── llm/                    # CARGO_TARGET_DIR for llm-service
└── frontend/               # Staged pnpm workspace and Vite intermediates

dist/                       # What run_playground.sh actually executes and serves
├── backend/momento-api
├── llm/llm-service
└── frontend/               # Built frontend — storage.static_dir points here

build_docker.sh             # Builds the Docker image
run_playground.sh           # Builds into build/ and dist/, runs against playground/
```

### The `tests/` mirror

`tests/` is an exact structural mirror of `src/`. A source file's test location is
derived mechanically — swap the leading `src/` for `tests/` and keep every intermediate
directory:

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
- `/api/v1/internal/llm/callback` is the exception: it uses `x-momento-callback-key`, never a JWT.

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
