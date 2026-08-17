#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(dirname "$(realpath "$0")")
COMPOSE_FILE="$ROOT_DIR/docker/docker-compose.yml"
PLAYGROUND_DIR="$ROOT_DIR/playground"

for config_file in "$PLAYGROUND_DIR/config.toml" "$PLAYGROUND_DIR/config_llm.toml"; do
    if [[ ! -f "$config_file" ]]; then
        printf 'Missing playground config: %s\n' "$config_file" >&2
        exit 1
    fi
done

mkdir -p "$PLAYGROUND_DIR/llm" "$PLAYGROUND_DIR/imports" "$PLAYGROUND_DIR/webdav"

export COMPOSE_PROJECT_NAME=momento-playground
export MOMENTO_DATA_DIR="$PLAYGROUND_DIR"
export LLM_DATA_DIR="$PLAYGROUND_DIR/llm"
export LLM_CONFIG_FILE="$PLAYGROUND_DIR/config_llm.toml"
export PUID="$(id -u)"
export PGID="$(id -g)"
export UMASK="${UMASK:-022}"
export TZ="${TZ:-UTC}"

cleanup() {
    docker compose -f "$COMPOSE_FILE" down --remove-orphans
}

trap cleanup EXIT INT TERM

docker compose -f "$COMPOSE_FILE" down --remove-orphans
docker compose -f "$COMPOSE_FILE" build
docker compose -f "$COMPOSE_FILE" up --no-build --remove-orphans --abort-on-container-exit
