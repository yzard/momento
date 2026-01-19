#!/usr/bin/env python3
"""
Momento Development Server Runner

This script starts the Momento server for local development.
It uses the sample_data directory for all data storage.

Usage:
    python run_server.py [--host HOST] [--port PORT] [--reload]

Examples:
    python run_server.py                    # Start with defaults (localhost:8000)
    python run_server.py --reload           # Start with auto-reload for development
    python run_server.py --host 0.0.0.0     # Listen on all interfaces
    python run_server.py --port 3000        # Use different port
"""

import argparse
import os
import sys
from pathlib import Path

# Set up paths
ROOT_DIR = Path(__file__).parent.resolve()
API_DIR = ROOT_DIR / "apps" / "api"
SAMPLE_DATA_DIR = ROOT_DIR / "sample_data"

# Add API directory to Python path
sys.path.insert(0, str(API_DIR))

# Override the data directory to use sample_data
os.environ["MOMENTO_DATA_DIR"] = str(SAMPLE_DATA_DIR)
os.environ["MOMENTO_STATIC_DIR"] = str(ROOT_DIR / "apps" / "web" / "dist")
os.environ["PYTHONPATH"] = str(API_DIR)


def main():
    parser = argparse.ArgumentParser(description="Run Momento development server")
    parser.add_argument(
        "--host", default="127.0.0.1", help="Host to bind to (default: 127.0.0.1)"
    )
    parser.add_argument(
        "--port", type=int, default=8000, help="Port to bind to (default: 8000)"
    )
    parser.add_argument(
        "--reload", action="store_true", help="Enable auto-reload for development"
    )
    args = parser.parse_args()

    # Ensure sample_data directories exist
    for subdir in ["originals", "thumbnails", "imports"]:
        (SAMPLE_DATA_DIR / subdir).mkdir(parents=True, exist_ok=True)

    print(f"Starting Momento server...")
    print(f"  Data directory: {SAMPLE_DATA_DIR}")
    print(f"  Static directory: {os.environ['MOMENTO_STATIC_DIR']}")
    print(f"  Server: http://{args.host}:{args.port}")
    print(f"  API docs: http://{args.host}:{args.port}/docs")
    print(f"  Default login: admin / admin")
    print()

    import uvicorn

    uvicorn.run(
        "momento_api.main:create_application",
        factory=True,
        host=args.host,
        port=args.port,
        reload=args.reload,
        reload_dirs=[str(API_DIR / "momento_api")] if args.reload else None,
    )


if __name__ == "__main__":
    main()
