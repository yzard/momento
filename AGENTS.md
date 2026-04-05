# AGENTS.md - Momento Codebase Guide

> Guidelines for AI coding agents working in this repository.

## Project Overview

Momento is a self-hosted photo management application with:
- **Backend**: Axum + SQLite (Rust) in `src/api/`
- **Frontend**: React + TypeScript + Vite + Tailwind in `src/web/`
- **Shared**: TypeScript constants in `packages/shared/`

Monorepo managed with pnpm workspaces and Turborepo.

---

## Build, Lint & Test Commands

### Root (Turborepo)
```bash
pnpm install              # Install all dependencies
pnpm build                # Build all packages
pnpm dev                  # Dev servers (API + web)
pnpm lint                 # Lint all packages
pnpm test                 # Run all tests
```

### Backend (src/api)
```bash
cd src/api

# Build
cargo build                # Debug build
cargo build --release      # Release build

# Run development server
cargo run                  # Starts server on 0.0.0.0:8000

# Linting & formatting
cargo fmt                  # Format code
cargo clippy               # Lint code

# Testing
cargo test                 # Run all tests
cargo test auth            # Run tests matching "auth"
```

### Frontend (src/web)
```bash
cd src/web

pnpm dev                  # Dev server (Vite)
pnpm build                # Production build (tsc + vite)
pnpm lint                 # ESLint
pnpm preview              # Preview production build
```

### Docker
```bash
docker-compose up --build         # Full stack
docker-compose -f docker-compose.dev.yml up   # Dev mode
```

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
- Use `apiClient` from `src/web/src/api/client.ts`
- API methods return typed responses
- URLs relative to baseURL (`/api`)

---

## Project Structure

```
src/
├── api/
│   ├── src/
│   │   ├── auth/           # JWT, password, extractors
│   │   ├── config/         # YAML config loading
│   │   ├── database/       # SQLite pool, schema, queries
│   │   ├── models/         # Request/response DTOs (serde)
│   │   ├── processor/      # Media processing, thumbnails, import
│   │   ├── routes/         # Axum route handlers
│   │   ├── utils/          # Helpers (datetime, geocoding)
│   │   ├── app.rs          # App factory
│   │   ├── constants.rs    # Paths, defaults
│   │   ├── error.rs        # AppError type
│   │   ├── lib.rs          # Library root
│   │   └── main.rs         # Entry point
│   ├── schema.sql          # Database schema
│   └── Cargo.toml          # Rust dependencies
│
├── web/
│   ├── src/
│   │   ├── api/            # API client modules
│   │   ├── components/     # React components
│   │   ├── context/        # React context providers
│   │   ├── hooks/          # Custom hooks
│   │   └── pages/          # Route pages
│   └── package.json
│
packages/
└── shared/
    └── src/constants.ts    # Shared API route constants
```

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
- Schema in `src/api/schema.sql`
- Helper functions in `src/database/mod.rs`:
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
