#!/bin/bash
set -euo pipefail

ROOT_DIR=$(dirname "$(realpath "$0")")
VERSION=$(<"$ROOT_DIR/src/backend/version.txt")

if [[ $# -ne 2 ]]; then
    printf 'Usage: %s <github|docker> <namespace>\n' "$(basename "$0")" >&2
    exit 2
fi

case "$1" in
    github)
        REGISTRY=ghcr.io
        ;;
    docker)
        REGISTRY=docker.io
        ;;
    *)
        printf 'Unsupported registry: %s\n' "$1" >&2
        printf 'Usage: %s <github|docker> <namespace>\n' "$(basename "$0")" >&2
        exit 2
        ;;
esac

NAMESPACE=$2
if [[ ! "$NAMESPACE" =~ ^[a-z0-9]+([._-][a-z0-9]+)*$ ]]; then
    printf 'Invalid registry namespace: %s\n' "$NAMESPACE" >&2
    exit 2
fi

if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]]; then
    printf 'Invalid version in src/backend/version.txt: %s\n' "$VERSION" >&2
    exit 1
fi

TAG=${TAG:-$VERSION}
SOURCE_REPOSITORY=${SOURCE_REPOSITORY:-https://github.com/yzard/momento}
MOMENTO_IMAGE="${REGISTRY}/${NAMESPACE}/momento"
LLM_IMAGE="${REGISTRY}/${NAMESPACE}/momento-llm-service"

cd "$ROOT_DIR/docker"

docker buildx build --push \
    --build-arg "SOURCE_REPOSITORY=${SOURCE_REPOSITORY}" \
    -f Dockerfile \
    -t "${MOMENTO_IMAGE}:${TAG}" \
    -t "${MOMENTO_IMAGE}:latest" \
    ..
docker buildx build --push \
    --build-arg "SOURCE_REPOSITORY=${SOURCE_REPOSITORY}" \
    -f Dockerfile.llm \
    -t "${LLM_IMAGE}:${TAG}" \
    -t "${LLM_IMAGE}:latest" \
    ..
