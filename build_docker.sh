#!/bin/bash
set -euo pipefail

ROOT_DIR=$(dirname "$(realpath "$0")")
LOCAL_NAMESPACE=zhuoyin

usage() {
    local script_name
    script_name=$(basename "$0")
    cat <<EOF
Usage:
  $script_name <keystore-directory>
  $script_name publish <github|docker> <namespace> <keystore-directory>
  $script_name --help

Commands:
  local (default)  Call build_android_client.sh release, then build momento-api
                   and llm-service images and load them into the Docker daemon.
  publish          Build the same Android release and service images, then push
                   both service images. Android development and tests are not run.

Arguments:
  github           Publish to ghcr.io.
  docker           Publish to docker.io.
  namespace        Registry account or organization used in image names.
  keystore-directory
                   Directory containing exactly one Android .jks file and a
                   password.txt file with one non-empty line.

Environment:
  TAG              Image tag. Defaults to src/backend/version.txt.
  SOURCE_REPOSITORY
                   OCI source label. Defaults to https://github.com/yzard/momento.

Behavior:
  build_android_client.sh is the only Android build/test/debug entrypoint. This
  script calls only its release command, verifies that exactly one release APK is
  available to Docker, embeds it as /app/static/momento-android.apk in momento-api,
  and builds the separate llm-service image.
  Both Rust services use Cargo's release profile without debug symbols by default
  to limit disk usage.
EOF
}

if [[ $# -eq 1 && ( $1 == -h || $1 == --help ) ]]; then
    usage
    exit 0
fi

VERSION=$(<"$ROOT_DIR/src/backend/version.txt")

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
            usage >&2
            exit 2
            ;;
    esac
    NAMESPACE=$3
    KEYSTORE_DIR=$4
else
    usage >&2
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

"$ROOT_DIR/build_android_client.sh" release --keystore-dir "$KEYSTORE_DIR"

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
