# Momento agent guide

Momento is a self-hosted photo manager: Rust/Axum + SQLite in `src/backend/`,
a separate durable inference service in `src/backend_llm/`, shared Rust infrastructure
in `src/common/`, React/TypeScript in `src/frontend/`, and Android in `src/android/`.
The monorepo uses pnpm workspaces and Turborepo.

## Working scope and completion

Read the affected implementation and only the guidance relevant to the change. A typo or
style-only edit does not require loading architecture docs or the complete skill stack.
Repository contracts below take precedence over generic skill conventions.

Complete the requested behavior, update affected callers, and verify the result before
returning it. For local checks using disposable fixtures with no production access, run
checks, fix failures caused by the change, and rerun affected checks without repeated
approval. Do not turn a passing focused check into an unbounded test loop. Report any
remaining blocker or unverified behavior. This does not authorize publishing or changing
production data; `playground/` can contain user media and is not disposable by default.

New arguments require explicit updates to internal callers; avoid compatibility shims.
Extract shared behavior before adding a near-duplicate implementation. Even a small edit
in a nonconforming directory requires migrating the affected tree to the canonical layout
in the same change. Update imports, callers, mirrored tests, fixtures, build/CI references,
and docs; remove the obsolete layout rather than leaving parallel structures. This migration
is part of the authorized repository workflow and needs no separate approval. Keep its scope
to the affected directory and dependencies, not an unrelated repository-wide rewrite.

Add or update regression tests for changed behavior and meaningful failure paths, using
existing coverage where it already proves the contract. Do not create tests that merely
match comments, documentation wording, or the implementation's structure. Run checks
appropriate to the change; broad suites are warranted for cross-cutting changes or risks
that focused checks do not cover.

## Repository layout and tooling

- New source belongs under its component in `src/`; use the resource hierarchy, not
  existing flat route/query/API modules as templates. Tests mirror the source path under
  `tests/` (for example `src/backend/routes/map.rs` → `tests/backend/routes/map.rs`).
  Move affected tests with source. Documentation belongs in `docs/`.
- Intermediate artifacts belong in root `build/`, deliverables in root `dist/`.
  Runtime playground files belong in `playground/`. The root `docker-compose.yaml`,
  `build_docker.sh`, `run_playground.sh`, and `build_android_client.sh` are intentional
  exceptions to generic layout rules.
- Before host development tools, create `build/tmp/` and set `TMPDIR`, `TEMP`, and `TMP`
  to its absolute path. Host fixtures must stay there, never in system `/tmp`.
  Rust tests use `src/test_support/temporary.rs`, including direct test-binary runs.
  Python fixtures use context managers; shell fixtures use exit traps and explicit paths.
  Clean only the current process's fixtures, including on failure. Rust's retained
  fixtures have exit cleanup; forced termination can leave files behind.
  Production temporary storage remains on the mounted volume (`/data/tmp` or `/data/llm/tmp`).
- **Every Android build/test/debug/dependency/lint operation runs in Docker through
  `./build_android_client.sh`.** Use `verify`, `assemble-debug`, `instrumented-test`,
  `shell`, or `release --keystore-dir PATH`. Never run host Gradle, Java, SDK, emulator,
  or ADB commands, invoke Android Dockerfiles directly, or copy Gradle artifacts yourself.
  Only this script's outputs in `build/android/` and `dist/android/` are valid artifacts.
- Rust uses the release/no-debug workflow and component-specific build directories.
  Frontend uses strict TypeScript, its configured formatter/ESLint, and the shared typed
  `apiClient` in `src/frontend/api/client.ts`. Exact commands: [development](docs/development.md).

## Contracts to preserve

- Momento business operations acquire source-owned scheduler capacity and use bounded,
  typed CPU, File I/O, and SQLite executors. Never block network threads or create private
  threads, Tokio tasks, blocking/Rayon pools, or semaphore concurrency windows in business
  modules. r2d2 is the deliberate additional SQLite connection pool. SQL reads use the
  protected reader lane; potentially mutating operations use the writer lane, regardless
  of HTTP method. Preserve reserved file reader/writer roles. Details: [execution](docs/architecture/execution.md).
- Import → metadata/input preparation → durable AI jobs → WebSocket submission → local
  inference → durable result receipt → independent persistence → optional downstream work
  are separate scheduled stages. A stage never runs its downstream stage inline.
  Momento owns the immutable original/shared full-resolution frame; UI thumbnails are
  never AI inputs. llm-service receives descriptors and bytes and never reads Momento paths.
- Only one model runtime is active in llm-service. Durable receipt allows queue deletion;
  transient failures keep work retryable. Preserve client/job/attempt/input correlation,
  cancellation races, idempotency, restart recovery, and per-task result validation.
- AI control/status requires an administrator. Normal media, duplicate/face/place browsing
  filters through active `media_access`; never substitute global representatives for
  user-visible selections. LLM transport authenticates client ID + API key, not user JWT.
- Application API paths are `/api/v1/<resource>/<operation>` with POST for normal operations.
  Preserve media/static GET, WebSocket handshakes, and WebDAV methods/streamed bodies.
  Use typed JSON for ordinary application operations, camelCase responses, and
  `AppError`/`{"detail": "Error message"}` for API errors. User auth uses Bearer tokens;
  Basic auth is for initial `/api/v1/user/authenticate`; refresh is `/api/v1/user/refresh`.
- SQL schema and named queries live in `src/backend/database/`, grouped by resource.
  SQLite and Android Room define current schemas only: no migration framework or
  compatibility DDL; keep only the current Room export. Breaking schema changes require
  a fresh database/data directory, not automatic deletion of existing user data.
- Both services require `-c|--config PATH`; malformed/missing config fails startup.
  Preserve typed configuration and `ConfigManager` snapshots. Add config fields rather
  than environment reads in business code. Existing config-loader environment exceptions,
  initialization, and password reset are documented in [configuration](docs/architecture/configuration.md).

## Read when the task crosses these boundaries

The linked documents contain the detailed contracts moved out of this entrypoint. Read the
relevant topic before changing its behavior; do not load the entire table for every edit.

| Task | Reference |
|------|-----------|
| Scheduling, worker capacity, blocking work, SQL/file lanes | [Execution](docs/architecture/execution.md) |
| Local/WebDAV import, metadata jobs, original/frame descriptors | [Media pipeline](docs/architecture/media-pipeline.md) |
| WebSocket admission, submission retries, cancellation/outbox | [LLM transport](docs/architecture/llm-transport.md) |
| Durable queue/capacity/recovery, model activation/concurrency, delivery | [LLM runtime](docs/architecture/llm-runtime.md) |
| Result framing/receipt, validation, transactional persistence, cleanup | [LLM results](docs/architecture/llm-results.md) |
| Adding an inference type | [Adding a type](docs/architecture/llm-results.md#adding-an-inference-type), plus media pipeline, transport, and runtime contracts above |
| Config, hot updates, paths, logging, credentials, container initialization | [Configuration](docs/architecture/configuration.md) |
| Face detection, crops, grouping, representatives, ordering | [Faces](docs/architecture/faces.md) |
| Place identity, covers, location metadata, GeoNames data | [Places](docs/architecture/places.md) |
| Build, test, playground, Docker, Android commands | [Development](docs/development.md) |

## Shared skills

Load a named skill only when its workflow applies. Do not recursively load every skill it
mentions. If a name is duplicated, use `/home/zyin/dev/skills/<name>/SKILL.md` as the canonical
personal copy; repository-specific contracts still win.

- `add-modify-codebase`: changing existing behavior, signatures, or shared implementations.
- `project-structure`: placing or moving source, tests, build outputs, or documentation.
- `general-coding`: explicit inputs, error ownership, and shared coding conventions.
- `naming-conventions`: introducing or renaming concepts across layers.
- `axum-server`: Axum handlers, state, middleware, or service lifecycle.
- `restful-api-design`: defining/changing route contracts and protocol exceptions.
- `sql-coding`: changing schemas, queries, or transactions.
- `docker-build`: changing images, entrypoints, or build/publish behavior.

Maintain this file as a short entrypoint. Keep non-obvious shared constraints here, put
conditional procedures in the linked topic, and update that topic when its contract changes.
Avoid adding code inventories, generic tutorials, or one-off task history.
