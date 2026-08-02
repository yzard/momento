#!/usr/bin/env bash
set -euo pipefail
trap - INT TERM

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

LLM_CONFIG_FILE="$PLAYGROUND_DIR/config_llm.yaml"
if [[ ! -f "$LLM_CONFIG_FILE" ]]; then
    printf 'Missing LLM service config: %s\n' "$LLM_CONFIG_FILE" >&2
    exit 1
fi
OUTPUT_DIR="$PLAYGROUND_DIR/output"
BACKEND_BUILD_DIR="$OUTPUT_DIR/build/backend"
BACKEND_DIST_DIR="$OUTPUT_DIR/dist/backend"
LLM_BUILD_DIR="$OUTPUT_DIR/build/llm"
LLM_DIST_DIR="$OUTPUT_DIR/dist/llm"

FRONTEND_BUILD_DIR="$OUTPUT_DIR/build/frontend"
FRONTEND_WORKSPACE_DIR="$FRONTEND_BUILD_DIR/workspace"
FRONTEND_APP_DIR="$FRONTEND_WORKSPACE_DIR/src/frontend"
FRONTEND_DIST_DIR="$OUTPUT_DIR/dist/frontend"

export RUST_BACKTRACE=full

rm -rf "$BACKEND_BUILD_DIR" "$FRONTEND_BUILD_DIR" "$LLM_BUILD_DIR" "$BACKEND_DIST_DIR" "$FRONTEND_DIST_DIR" "$LLM_DIST_DIR"
mkdir -p "$FRONTEND_WORKSPACE_DIR" "$BACKEND_DIST_DIR" "$FRONTEND_DIST_DIR" "$LLM_DIST_DIR"

cp "$ROOT_DIR/package.json" "$ROOT_DIR/pnpm-lock.yaml" "$ROOT_DIR/pnpm-workspace.yaml" "$FRONTEND_WORKSPACE_DIR/"
mkdir -p "$FRONTEND_WORKSPACE_DIR/src"
cp -R "$ROOT_DIR/src/frontend" "$FRONTEND_APP_DIR"

pnpm install --dir "$FRONTEND_WORKSPACE_DIR" --frozen-lockfile
pnpm --dir "$FRONTEND_APP_DIR" build
cp -R "$FRONTEND_APP_DIR/dist/." "$FRONTEND_DIST_DIR/"

CARGO_TARGET_DIR="$BACKEND_BUILD_DIR/target" cargo build --release --manifest-path "$ROOT_DIR/src/backend/Cargo.toml"
cp "$BACKEND_BUILD_DIR/target/release/momento-api" "$BACKEND_DIST_DIR/momento-api"
CARGO_TARGET_DIR="$LLM_BUILD_DIR/target" cargo build --release --manifest-path "$ROOT_DIR/src/backend_llm/Cargo.toml"
cp "$LLM_BUILD_DIR/target/release/llm-service" "$LLM_DIST_DIR/llm-service"

LLM_PID=""
BACKEND_PID=""

process_alive() {
    local pid="$1"
    local state
    state=$(ps -o stat= -p "$pid" 2>/dev/null || true)
    [[ -n "$state" && "$state" != Z* ]]
}

stop_services() {
    local pid
    for pid in "$BACKEND_PID" "$LLM_PID"; do
        if [[ -n "$pid" ]]; then
            kill -TERM "$pid" 2>/dev/null || true
        fi
    done

    sleep 2

    for pid in "$BACKEND_PID" "$LLM_PID"; do
        if [[ -n "$pid" ]]; then
            kill -KILL "$pid" 2>/dev/null || true
        fi
    done
}

handle_signal() {
    stop_services
    exit 130
}

trap stop_services EXIT
trap handle_signal INT TERM

"$LLM_DIST_DIR/llm-service" -c "$LLM_CONFIG_FILE" >"$OUTPUT_DIR/llm-service.log" 2>&1 &
LLM_PID=$!

# storage.data_dir and storage.static_dir in the config are relative to the git root
cd "$ROOT_DIR"
"$BACKEND_DIST_DIR/momento-api" -c "$CONFIG_FILE" &
BACKEND_PID=$!

while process_alive "$BACKEND_PID" && process_alive "$LLM_PID"; do
    sleep 1
done

if ! process_alive "$LLM_PID"; then
    printf 'LLM service exited during startup. Check %s for details.\n' "$OUTPUT_DIR/llm-service.log" >&2
    while IFS= read -r line; do
        printf 'llm-service: %s\n' "$line" >&2
    done < "$OUTPUT_DIR/llm-service.log"
    exit 1
fi

if ! process_alive "$BACKEND_PID"; then
    printf 'Momento API exited unexpectedly.\n' >&2
    exit 1
fi
