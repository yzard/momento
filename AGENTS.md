# AGENTS.md - Momento Codebase Guide

> Guidelines for AI coding agents working in this repository.

## Project Overview

Momento is a self-hosted photo management application with:
- **Backend**: FastAPI + SQLite (Python 3.12+) in `apps/api/`
- **Frontend**: React + TypeScript + Vite + Tailwind in `apps/web/`
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

### Backend (apps/api)
```bash
cd apps/api

# Install with dev dependencies
uv pip install -e ".[dev]"

# Run development server
uvicorn momento_api.main:create_application --factory --reload --host 0.0.0.0 --port 8000

# Linting & formatting
black momento_api/             # Format code
isort momento_api/             # Sort imports
mypy momento_api/              # Type checking

# Testing
pytest                         # Run all tests
pytest tests/test_auth.py      # Run single test file
pytest tests/test_auth.py::test_login -v   # Run single test
pytest -k "test_login"         # Run tests matching pattern
```

### Frontend (apps/web)
```bash
cd apps/web

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

### Python (Backend)

**Formatting**:
- Line length: 120 characters
- Formatter: `black` with `skip-string-normalization = true`
- Import sorting: `isort` with profile "black"

**Imports** (order enforced by isort):
```python
# 1. Standard library
from typing import Annotated, Optional

# 2. Third-party
from fastapi import APIRouter, Depends, HTTPException, status
from pydantic import BaseModel

# 3. Local
from momento_api.auth.dependencies import CurrentUser, get_current_user
from momento_api.database import fetch_one, fetch_all
```

**Type Hints**:
- Always use type hints for function parameters and return types
- Use `Optional[X]` or `X | None` for nullable types
- Use `Annotated[X, Depends(...)]` for FastAPI dependencies
- Pydantic models for request/response schemas

**Naming Conventions**:
- Files: `snake_case.py`
- Classes: `PascalCase` (e.g., `MediaResponse`, `UserCreateRequest`)
- Functions/variables: `snake_case`
- Constants: `UPPER_SNAKE_CASE`
- Private helpers: prefix with `_` (e.g., `_row_to_media_response`)

**Error Handling**:
```python
# Use HTTPException for API errors
raise HTTPException(
    status_code=status.HTTP_404_NOT_FOUND,
    detail="Media not found"
)

# Custom exceptions in momento_api/exceptions.py for domain errors
raise MediaProcessingError("Failed to generate thumbnail")
```

**Route Patterns**:
- Router prefix defines resource: `APIRouter(prefix="/media", tags=["media"])`
- POST for all mutations and queries (RPC-style API)
- Request bodies use Pydantic models
- Response models specified via `response_model=`

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
- Use `apiClient` from `apps/web/src/api/client.ts`
- API methods return typed responses
- URLs relative to baseURL (`/api`)

---

## Project Structure

```
apps/
├── api/
│   ├── momento_api/
│   │   ├── auth/           # JWT, password, dependencies
│   │   ├── models/         # Pydantic request/response models
│   │   ├── processor/      # Media processing, thumbnails, import
│   │   ├── routes/         # FastAPI routers
│   │   ├── config.py       # YAML config loading
│   │   ├── database.py     # SQLite helpers
│   │   ├── constants.py    # Paths, defaults
│   │   └── main.py         # App factory
│   ├── schema.sql          # Database schema
│   └── pyproject.toml      # Python dependencies
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
- Resources: `user`, `image`, `album`, `tag`, `share`, `import`, `map`, `timeline`
- Operations: `list`, `get`, `create`, `update`, `delete`

**Authentication**:
- Bearer token in `Authorization` header
- Token refresh via `/api/v1/user/refresh`
- Basic auth only for initial login (`/api/v1/user/authenticate`)

**Request/Response**:
- All bodies are JSON
- Pydantic handles validation
- Consistent error format: `{"detail": "Error message"}`

---

## Database

- SQLite with `sqlite3.Row` row factory
- Schema in `apps/api/schema.sql`
- Helper functions in `momento_api/database.py`:
  - `fetch_one()`, `fetch_all()` for queries
  - `execute_query()` for mutations
  - `insert_returning_id()` for inserts

---

## Key Dependencies

**Backend**: FastAPI, Pydantic, python-jose (JWT), bcrypt, Pillow, pillow-heif, ffmpeg-python

**Frontend**: React 18, React Router 7, TanStack Query, Axios, Tailwind CSS, react-leaflet, react-virtuoso
