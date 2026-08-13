#!/usr/bin/env bash
set -euo pipefail
trap - INT TERM

ROOT_DIR=$(dirname "$(realpath "$0")")
PLAYGROUND_DIR="$ROOT_DIR/playground"
CONFIG_FILE="$PLAYGROUND_DIR/config.toml"
DATA_DIR="$PLAYGROUND_DIR"

if [[ ! -f "$CONFIG_FILE" ]]; then
    printf 'Missing playground config: %s\n' "$CONFIG_FILE" >&2
    exit 1
fi

mkdir -p "$DATA_DIR"

LLM_CONFIG_FILE="$PLAYGROUND_DIR/config_llm.toml"
if [[ ! -f "$LLM_CONFIG_FILE" ]]; then
    printf 'Missing LLM service config: %s\n' "$LLM_CONFIG_FILE" >&2
    exit 1
fi
LOG_DIR="$PLAYGROUND_DIR/logs"
BUILD_DIR="$ROOT_DIR/build"
DIST_DIR="$ROOT_DIR/dist"

BACKEND_BUILD_DIR="$BUILD_DIR/backend"
BACKEND_DIST_DIR="$DIST_DIR/backend"
LLM_BUILD_DIR="$BUILD_DIR/llm"
LLM_DIST_DIR="$DIST_DIR/llm"

FRONTEND_BUILD_DIR="$BUILD_DIR/frontend"
FRONTEND_WORKSPACE_DIR="$FRONTEND_BUILD_DIR/workspace"
FRONTEND_APP_DIR="$FRONTEND_WORKSPACE_DIR/src/frontend"
FRONTEND_DIST_DIR="$DIST_DIR/frontend"

export RUST_BACKTRACE=full
cd "$ROOT_DIR"

rm -rf "$BACKEND_BUILD_DIR" "$FRONTEND_BUILD_DIR" "$LLM_BUILD_DIR" "$BACKEND_DIST_DIR" "$FRONTEND_DIST_DIR" "$LLM_DIST_DIR" "$PLAYGROUND_DIR/output"
mkdir -p "$LOG_DIR" "$PLAYGROUND_DIR/imports" "$PLAYGROUND_DIR/webdav" "$FRONTEND_WORKSPACE_DIR" "$BACKEND_DIST_DIR" "$FRONTEND_DIST_DIR" "$LLM_DIST_DIR"

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

cleanup_llm_containers() {
    local container_id
    local container_ids
    container_ids=$(docker ps -aq --filter 'label=org.momento.llm-service=playground' 2>/dev/null || true)
    while IFS= read -r container_id; do
        if [[ -n "$container_id" ]]; then
            docker rm -f "$container_id" >/dev/null 2>&1 || true
        fi
    done <<< "$container_ids"
}

stop_services() {
    local pid
    for pid in "$BACKEND_PID" "$LLM_PID"; do
        if [[ -n "$pid" ]]; then
            kill -TERM "$pid" 2>/dev/null || true
        fi
    done

    cleanup_llm_containers
    sleep 2

    for pid in "$BACKEND_PID" "$LLM_PID"; do
        if [[ -n "$pid" ]]; then
            kill -KILL "$pid" 2>/dev/null || true
        fi
    done
    cleanup_llm_containers
}

handle_signal() {
    stop_services
    exit 130
}

trap stop_services EXIT
trap handle_signal INT TERM

cleanup_llm_containers
"$LLM_DIST_DIR/llm-service" -c "$LLM_CONFIG_FILE" &
LLM_PID=$!

# storage.data_dir and storage.static_dir in the config are relative to the git root
"$BACKEND_DIST_DIR/momento-api" -c "$CONFIG_FILE" &
BACKEND_PID=$!

while process_alive "$BACKEND_PID" && process_alive "$LLM_PID"; do
    sleep 1
done

if ! process_alive "$LLM_PID"; then
    printf 'LLM service exited during startup. Check %s for details.\n' "$LOG_DIR/llm-service.log" >&2
    exit 1
fi

if ! process_alive "$BACKEND_PID"; then
    printf 'Momento API exited unexpectedly.\n' >&2
    exit 1
fi
