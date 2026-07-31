#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(dirname "$(realpath "$0")")
PLAYGROUND_DIR="$ROOT_DIR/playground"
CONFIG_FILE="$PLAYGROUND_DIR/config.yaml"
DATA_DIR="$PLAYGROUND_DIR/data"

if [[ ! -f "$CONFIG_FILE" ]]; then
    printf 'Missing playground config: %s\n' "$CONFIG_FILE" >&2
    exit 1
fi

if [[ ! -d "$DATA_DIR" ]]; then
    printf 'Missing playground data directory: %s\n' "$DATA_DIR" >&2
    exit 1
fi

cp "$CONFIG_FILE" "$DATA_DIR/config.yaml"

OUTPUT_DIR="$PLAYGROUND_DIR/output"
BACKEND_BUILD_DIR="$OUTPUT_DIR/build/backend"
BACKEND_DIST_DIR="$OUTPUT_DIR/dist/backend"

FRONTEND_BUILD_DIR="$OUTPUT_DIR/build/frontend"
FRONTEND_WORKSPACE_DIR="$FRONTEND_BUILD_DIR/workspace"
FRONTEND_APP_DIR="$FRONTEND_WORKSPACE_DIR/src/frontend"
FRONTEND_DIST_DIR="$OUTPUT_DIR/dist/frontend"

export RUST_BACKTRACE=full

rm -rf "$BACKEND_BUILD_DIR" "$FRONTEND_BUILD_DIR" "$BACKEND_DIST_DIR" "$FRONTEND_DIST_DIR"
mkdir -p "$FRONTEND_WORKSPACE_DIR" "$BACKEND_DIST_DIR" "$FRONTEND_DIST_DIR"

cp "$ROOT_DIR/package.json" "$ROOT_DIR/pnpm-lock.yaml" "$ROOT_DIR/pnpm-workspace.yaml" "$FRONTEND_WORKSPACE_DIR/"
mkdir -p "$FRONTEND_WORKSPACE_DIR/src"
cp -R "$ROOT_DIR/src/frontend" "$FRONTEND_APP_DIR"

pnpm install --dir "$FRONTEND_WORKSPACE_DIR" --frozen-lockfile
pnpm --dir "$FRONTEND_APP_DIR" build
cp -R "$FRONTEND_APP_DIR/dist/." "$FRONTEND_DIST_DIR/"

CARGO_TARGET_DIR="$BACKEND_BUILD_DIR/target" cargo build --release --manifest-path "$ROOT_DIR/src/backend/Cargo.toml"
cp "$BACKEND_BUILD_DIR/target/release/momento-api" "$BACKEND_DIST_DIR/momento-api"

MOMENTO_DATA_DIR="$DATA_DIR" \
MOMENTO_STATIC_DIR="$FRONTEND_DIST_DIR" \
"$BACKEND_DIST_DIR/momento-api"
