#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(dirname "$(realpath "$0")")
COMPOSE_FILE="$ROOT_DIR/docker-compose.yaml"
PLAYGROUND_DIR="$ROOT_DIR/playground"

if [[ $# -ne 1 ]]; then
    printf 'Usage: %s <keystore directory>\n' "$(basename "$0")" >&2
    exit 2
fi

KEYSTORE_DIR=$1

mkdir -p "$PLAYGROUND_DIR/llm"

export COMPOSE_PROJECT_NAME=momento-playground
export MOMENTO_DATA_DIR="$PLAYGROUND_DIR"
export PUID="$(id -u)"
export PGID="$(id -g)"
export UMASK="${UMASK:-022}"
export TZ="${TZ:-UTC}"

cleanup() {
    docker compose -f "$COMPOSE_FILE" down --remove-orphans || true
}

trap cleanup EXIT INT TERM

docker compose -f "$COMPOSE_FILE" down --remove-orphans
"$ROOT_DIR/build_docker.sh" "$KEYSTORE_DIR"
docker compose -f "$COMPOSE_FILE" up --remove-orphans --abort-on-container-exit
