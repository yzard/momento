import os
import threading
from collections.abc import AsyncGenerator
from contextlib import asynccontextmanager
from pathlib import Path

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel

from momento_api import __version__
from momento_api.auth.password import hash_password
from momento_api.config import Config, load_config
from momento_api.constants import CONFIG_PATH, DATA_DIR, IMPORTS_DIR, ORIGINALS_DIR, PREVIEWS_DIR, THUMBNAILS_DIR
from momento_api.database import close_database, ensure_media_columns, fetch_one, get_connection, init_database
from momento_api.processor.regenerator import run_regeneration
from momento_api.routes.router import api_router


# Path to frontend build artifacts (from env or default for local dev)
STATIC_DIR = Path(os.environ.get("MOMENTO_STATIC_DIR", Path(__file__).parent.parent.parent / "web" / "dist"))


class HealthcheckResponse(BaseModel):
    status: str
    version: str


def _healthcheck_payload() -> HealthcheckResponse:
    return HealthcheckResponse(status="healthy", version=__version__)


def _create_default_admin(config: Config) -> None:
    existing = fetch_one("SELECT id FROM users WHERE role = 'admin' LIMIT 1", ())
    if existing:
        return

    hashed = hash_password(config.admin.password)
    conn = get_connection()
    conn.execute(
        "INSERT INTO users (username, email, hashed_password, role, must_change_password) VALUES (?, ?, ?, 'admin', 1)",
        (config.admin.username, f"{config.admin.username}@localhost", hashed),
    )
    conn.commit()


def _init_directories() -> None:
    for directory in [DATA_DIR, ORIGINALS_DIR, THUMBNAILS_DIR, PREVIEWS_DIR, IMPORTS_DIR]:
        directory.mkdir(parents=True, exist_ok=True)


def _init_db(config: Config) -> None:
    schema_path = Path(__file__).parent.parent / "schema.sql"
    ensure_media_columns()
    init_database(schema_path)
    _create_default_admin(config)


def _start_background_tasks(config: Config) -> None:
    thread = threading.Thread(target=run_regeneration, kwargs={"missing_only": True, "config": config}, daemon=True)
    thread.start()


@asynccontextmanager
async def lifespan(app: FastAPI) -> AsyncGenerator[None, None]:
    config: Config = app.state.config
    _init_directories()
    _init_db(config)
    _start_background_tasks(config)

    yield

    close_database()


def create_application(config: Config | None = None) -> FastAPI:  # pyright: ignore[reportMissingTypeArgument]
    if config is None:
        config = load_config(CONFIG_PATH)
    app = FastAPI(
        title="Momento API",
        description="Self-hosted photo management backend",
        version=__version__,
        lifespan=lifespan,
        docs_url="/docs" if config.server.debug else None,
        redoc_url="/redoc" if config.server.debug else None,
    )

    app.state.config = config

    app.add_middleware(
        CORSMiddleware, allow_origins=["*"], allow_credentials=True, allow_methods=["*"], allow_headers=["*"]
    )

    @app.get("/api/v1/healthcheck", response_model=HealthcheckResponse)
    async def health_check() -> HealthcheckResponse:
        return _healthcheck_payload()

    app.include_router(api_router, prefix="/api/v1")

    # Serve frontend static files if the directory exists
    if STATIC_DIR.exists():
        # Mount assets directory for JS/CSS/images
        assets_dir = STATIC_DIR / "assets"
        if assets_dir.exists():
            app.mount("/assets", StaticFiles(directory=assets_dir), name="assets")

        # Serve index.html for all non-API routes (SPA fallback)
        @app.get("/{full_path:path}")
        async def serve_spa(full_path: str) -> FileResponse:
            # Try to serve static file first
            static_file = STATIC_DIR / full_path
            if static_file.exists() and static_file.is_file():
                return FileResponse(static_file)

            # Fall back to index.html for SPA routing
            index_path = STATIC_DIR / "index.html"
            if index_path.exists():
                return FileResponse(index_path)

            return FileResponse(index_path)

    return app
