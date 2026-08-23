#!/bin/bash
set -euo pipefail

ROOT_DIR=$(dirname "$(realpath "$0")")
VERSION=$(<"$ROOT_DIR/src/backend/version.txt")
LOCAL_NAMESPACE=zhuoyin

usage() {
    printf 'Usage: %s <keystore directory>\n' "$(basename "$0")" >&2
    printf '       %s publish <github|docker> <namespace> <keystore directory>\n' "$(basename "$0")" >&2
}

PUBLISH=false
REGISTRY=
NAMESPACE=$LOCAL_NAMESPACE
KEYSTORE_DIR=

if [[ $# -eq 1 && $1 != publish ]]; then
    OUTPUT_MODE=--load
    KEYSTORE_DIR=$1
elif [[ $# -eq 4 && $1 == publish ]]; then
    PUBLISH=true
    OUTPUT_MODE=--push
    case "$2" in
        github)
            REGISTRY=ghcr.io
            ;;
        docker)
            REGISTRY=docker.io
            ;;
        *)
            printf 'Unsupported registry: %s\n' "$2" >&2
            usage
            exit 2
            ;;
    esac
    NAMESPACE=$3
    KEYSTORE_DIR=$4
else
    usage
    exit 2
fi

if [[ ! "$NAMESPACE" =~ ^[a-z0-9]+([._-][a-z0-9]+)*$ ]]; then
    printf 'Invalid registry namespace: %s\n' "$NAMESPACE" >&2
    exit 2
fi

if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]]; then
    printf 'Invalid version in src/backend/version.txt: %s\n' "$VERSION" >&2
    exit 1
fi

"$ROOT_DIR/build_mobile_clients.sh" "$KEYSTORE_DIR"

TAG=${TAG:-$VERSION}
SOURCE_REPOSITORY=${SOURCE_REPOSITORY:-https://github.com/yzard/momento}
if [[ -n "$REGISTRY" ]]; then
    IMAGE_PREFIX="${REGISTRY}/${NAMESPACE}"
else
    IMAGE_PREFIX=$NAMESPACE
fi

build_image() {
    local dockerfile=$1
    local image=$2

    docker buildx build "$OUTPUT_MODE" \
        --build-arg "SOURCE_REPOSITORY=${SOURCE_REPOSITORY}" \
        -f "$ROOT_DIR/docker/$dockerfile" \
        -t "${image}:${TAG}" \
        -t "${image}:latest" \
        "$ROOT_DIR"
}

build_image Dockerfile "${IMAGE_PREFIX}/momento"
build_image Dockerfile.llm "${IMAGE_PREFIX}/momento-llm-service"

if [[ "$PUBLISH" == true ]]; then
    printf 'Published Momento %s images to %s\n' "$TAG" "$IMAGE_PREFIX"
else
    printf 'Built Momento %s images locally under %s\n' "$TAG" "$IMAGE_PREFIX"
fi
