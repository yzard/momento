# AGENTS.md - Momento Codebase Guide

> Guidelines for AI coding agents working in this repository.

## Project Overview

Momento is a self-hosted photo management application with:
- **Backend**: Axum + SQLite (Rust) in `src/backend/`
- **Frontend**: React + TypeScript + Vite + Tailwind in `src/frontend/`

Monorepo managed with pnpm workspaces and Turborepo.

---

## Conventions

This repo follows the shared agent skills. Read them before changing code — they are the
source of truth, and this file only records what is specific to Momento.

| Skill | Governs |
|-------|---------|
| `project-structure` | Where every file goes: `src/`, `tests/` mirroring it 1:1, `docs/`, `playground/`, `docker/` |
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

## Configuration

Both binaries (`momento-api`, `llm-service`) require `-c|--config PATH` and read **no**
environment variables. A missing or malformed config is a hard startup failure — it never
falls back to defaults, because that would silently start the server against the wrong
data directory.

Every filesystem location derives from one config value:

```yaml
storage:
  data_dir: /data          # database.sqlite, originals/, thumbnails/, previews/, imports/, webdav/, ...
  static_dir: /app/static  # built frontend served as a fallback
```

`main` calls `constants::init_paths(&config.storage.data_dir)` once after parsing the
config; everything else reads `constants::paths()`. Never hardcode a path under the data
directory and never add a new `std::env::var` call — add a config field instead.

`docker/Dockerfile` and `docker/entrypoint.sh` are the one place environment variables
belong (`PUID`/`PGID`/`UMASK`/`TZ`); they translate into the config file the entrypoint
generates, and `CMD` passes `-c /data/config.yaml`. `RUST_LOG` and `RUST_BACKTRACE` are
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

./run_playground.sh       # Full stack against playground/config.yaml
./build_docker.sh         # Build the Docker image (cds into docker/)
```

`run_playground.sh` and `build_docker.sh` are the only scripts at the git root. Both
resolve paths from their own location, so they work from any working directory.

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
│   ├── processor/          # Media processing, thumbnails, import
│   ├── routes/             # Axum route handlers
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
└── frontend/

docs/                       # All project documentation

docker/
├── Dockerfile
├── Dockerfile.dockerignore # NOT .dockerignore — context is the git root
├── docker-compose.yml
└── entrypoint.sh

playground/                 # End-to-end config, data, and generated output
├── config.yaml             # The one config run_playground.sh starts the stack with
├── data/                   # SQLite database, originals, previews, thumbnails
├── upload/                 # WebDAV upload landing zone
└── output/                 # Generated build and dist artifacts

build_docker.sh             # Builds the Docker image
run_playground.sh           # Runs the full stack against playground/
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
- Resources: `user`, `media`, `album`, `tag`, `share`, `import`, `map`, `timeline`, `trash`
- Operations: `list`, `get`, `create`, `update`, `delete`

**Authentication**:
- Bearer token in `Authorization` header
- Token refresh via `/api/v1/user/refresh`
- Basic auth only for initial login (`/api/v1/user/authenticate`)

**Request/Response**:
- All bodies are JSON
- Serde handles serialization (camelCase for responses)
- Consistent error format: `{"detail": "Error message"}`

---

## Database

- SQLite with r2d2 connection pooling
- Schema in `src/backend/database/schema.sql`
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
