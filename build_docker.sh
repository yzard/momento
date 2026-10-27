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
  RUNTIME_REFRESH
                   Explicit key for refreshing unchanged runtime/base image tags.
  SOURCE_REPOSITORY
                   OCI source label. Defaults to https://github.com/yzard/momento.

Behavior:
  build_android_client.sh is the only Android build/test/debug entrypoint. This
  script calls only its release command, verifies that exactly one release APK is
  available to Docker, embeds it as /app/static/momento-android.apk in momento-api,
  and builds the separate llm-service image.
  Both Rust services use Cargo's release profile without debug symbols by default
  to limit disk usage. Success removes this invocation's BuildKit cache and
  temporary workspace. Final images, reusable runtime/model artifacts, APK/AAB,
  and failure diagnostics are retained. Unchanged targets are reused.
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

source "$ROOT_DIR/docker/lib/build_lifecycle.sh"
momento_build_begin "$ROOT_DIR" docker

"$ROOT_DIR/build_android_client.sh" release --keystore-dir "$KEYSTORE_DIR"

release_apk() {
    shopt -s nullglob
    local apk_candidates=("$ROOT_DIR"/dist/android/*.apk)
    shopt -u nullglob
    if (( ${#apk_candidates[@]} != 1 )) || [[ ! -s "${apk_candidates[0]:-}" ]]; then
        printf 'Expected exactly one non-empty signed release APK in dist/android\n' >&2
        exit 1
    fi
    printf '%s\n' "${apk_candidates[0]}"
}

ANDROID_RELEASE_APK=$(release_apk)
printf 'Embedding signed Android release: %s\n' "$(basename "$ANDROID_RELEASE_APK")"

TAG=${TAG:-$VERSION}
SOURCE_REPOSITORY=${SOURCE_REPOSITORY:-https://github.com/yzard/momento}
if [[ -n "$REGISTRY" ]]; then
    IMAGE_PREFIX="${REGISTRY}/${NAMESPACE}"
else
    IMAGE_PREFIX=$NAMESPACE
fi

archive_build_reports() {
    local logs=("$MOMENTO_BUILD_WORK/"*.log)
    if [[ -f ${logs[0]} ]]; then
        mkdir -p "$ROOT_DIR/build/docker/reports" || return 1
        cp "${logs[@]}" "$ROOT_DIR/build/docker/reports/" || return 1
    fi
}
MOMENTO_BUILD_CLEANUP_HOOK=archive_build_reports

# Runtime/model artifacts are immutable OCI images. Disposable BuildKit state is separate.
RUNTIME_REFRESH=${RUNTIME_REFRESH:-0}
MOMENTO_BUILD_PLATFORM=$(docker version --format '{{.Server.Os}}/{{.Server.Arch}}')
key() { printf '%s\n' "$@" | sha256sum | cut -d ' ' -f1; }
common_inputs=$(momento_build_fingerprint "$ROOT_DIR" build_docker.sh docker/lib/build_lifecycle.sh docker/lib/validate_oci.py)
decoder_inputs=$(momento_build_fingerprint "$ROOT_DIR" docker/build_raw_decoder.sh docker/libraw-dng-host.patch docker/dng-sdk-portability.patch tests/docker/build_raw_decoder.sh)
api_runtime_inputs=$(key "$MOMENTO_BUILD_PLATFORM" "$decoder_inputs" "$RUNTIME_REFRESH" "$SOURCE_REPOSITORY" "$(momento_build_fingerprint "$ROOT_DIR" docker/Dockerfile.runtime docker/Dockerfile.runtime.dockerignore docker/imagemagick-policy.xml tests/docker/libraw-dng-host.sh)")
llm_runtime_inputs=$(key "$MOMENTO_BUILD_PLATFORM" "$decoder_inputs" "$RUNTIME_REFRESH" "$SOURCE_REPOSITORY" "$(momento_build_fingerprint "$ROOT_DIR" docker/Dockerfile.llm-runtime docker/Dockerfile.llm-runtime.dockerignore)")
frontend_inputs=$(key "$MOMENTO_BUILD_PLATFORM" "$(momento_build_fingerprint "$ROOT_DIR" src/frontend src/backend/version.txt package.json pnpm-lock.yaml pnpm-workspace.yaml docker/Dockerfile.frontend docker/Dockerfile.frontend.dockerignore docker/lib/build_lifecycle.sh)")
api_inputs=$(key "$common_inputs" "$SOURCE_REPOSITORY" "$api_runtime_inputs" "$frontend_inputs" "$(sha256sum "$ANDROID_RELEASE_APK" | cut -d ' ' -f1)" "$(momento_build_fingerprint "$ROOT_DIR" src/backend src/common docker/Dockerfile docker/Dockerfile.dockerignore docker/entrypoint.sh docker/entrypoint_common.sh)")
llm_inputs=$(key "$common_inputs" "$SOURCE_REPOSITORY" "$llm_runtime_inputs" "$(momento_build_fingerprint "$ROOT_DIR" src/backend_llm src/common src/backend/version.txt docker/Dockerfile.llm docker/Dockerfile.llm.dockerignore docker/entrypoint_llm.sh docker/entrypoint_common.sh)")

service_receipt() {
    printf '%s/build/docker/state/%s.inputs\n' "$ROOT_DIR" "$(key "$1:$TAG")"
}

service_reused() {
    local image=$1 fingerprint=$2
    local receipt
    receipt=$(service_receipt "$image")
    local candidate
    for candidate in "$image:$TAG" "$image:latest" "${LOCAL_NAMESPACE}/${image##*/}:$TAG" "${LOCAL_NAMESPACE}/${image##*/}:latest"; do
        if [[ $(docker image inspect --format '{{index .Config.Labels "io.momento.build.inputs"}}' "$candidate" 2>/dev/null || true) == "$fingerprint" ]]; then
            docker image tag "$candidate" "$image:$TAG" || exit 1
            docker image tag "$candidate" "$image:latest" || exit 1
            if [[ $PUBLISH == true ]]; then
                docker image push "$image:$TAG" || exit 1
                docker image push "$image:latest" || exit 1
                local digest
                digest=$(docker buildx imagetools inspect --format '{{.Manifest.Digest}}' "$image:$TAG") || exit 1
                [[ -n $digest ]] || exit 1
                mkdir -p "$(dirname "$receipt")" || exit 1
                printf '%s %s\n' "$fingerprint" "$digest" > "$receipt" || exit 1
            fi
            printf 'Reusing unchanged image %s:%s\n' "$image" "$TAG"
            return 0
        fi
    done
    if [[ $PUBLISH == true && -f $receipt ]]; then
        local previous_fingerprint previous_digest actual_digest latest_digest
        read -r previous_fingerprint previous_digest < "$receipt"
        if [[ $previous_fingerprint == "$fingerprint" ]]; then
            actual_digest=$(docker buildx imagetools inspect --format '{{.Manifest.Digest}}' "$image:$TAG" 2>/dev/null || true)
            if [[ -n $actual_digest && $actual_digest == "$previous_digest" ]]; then
                latest_digest=$(docker buildx imagetools inspect --format '{{.Manifest.Digest}}' "$image:latest" 2>/dev/null || true)
                if [[ $latest_digest != "$actual_digest" ]]; then
                    docker buildx imagetools create -t "$image:latest" "$image@$actual_digest" || exit 1
                fi
                printf 'Reusing unchanged published image %s:%s\n' "$image" "$TAG"
                return 0
            fi
        fi
    fi
    return 1
}

build_image() {
    local dockerfile=$1 image=$2 fingerprint=$3
    shift 3
    [[ -n $MOMENTO_BUILD_BUILDER ]] || momento_build_builder
    local metadata="$MOMENTO_BUILD_WORK/$dockerfile.metadata.json"
    docker buildx build --builder "$MOMENTO_BUILD_BUILDER" --platform "$MOMENTO_BUILD_PLATFORM" "$OUTPUT_MODE" \
        --build-arg "SOURCE_REPOSITORY=${SOURCE_REPOSITORY}" \
        --label "io.momento.build.inputs=$fingerprint" \
        --metadata-file "$metadata" \
        -f "$ROOT_DIR/docker/$dockerfile" \
        -t "${image}:${TAG}" -t "${image}:latest" "$@" "$ROOT_DIR" \
        2>&1 | tee "$MOMENTO_BUILD_WORK/$dockerfile.log"
    if [[ $PUBLISH == true ]]; then
        local digest
        digest=$(sed -n 's/.*"containerimage.digest": "\([^"]*\)".*/\1/p' "$metadata")
        [[ -n $digest ]] || { printf 'Published image digest is missing\n' >&2; return 1; }
        local receipt="$MOMENTO_BUILD_WORK/$dockerfile.inputs"
        printf '%s %s\n' "$fingerprint" "$digest" > "$receipt"
        mkdir -p "$ROOT_DIR/build/docker/state"
        mv "$receipt" "$(service_receipt "$image")"
    fi
}

if ! service_reused "${IMAGE_PREFIX}/momento" "$api_inputs"; then
    momento_build_oci "$ROOT_DIR" api-runtime Dockerfile.runtime "$api_runtime_inputs"
    momento_build_oci "$ROOT_DIR" frontend Dockerfile.frontend "$frontend_inputs"
    build_image Dockerfile "${IMAGE_PREFIX}/momento" "$api_inputs" \
        --build-context "momento-api-runtime=oci-layout://$ROOT_DIR/dist/docker/api-runtime/$api_runtime_inputs" \
        --build-context "momento-frontend=oci-layout://$ROOT_DIR/dist/docker/frontend/$frontend_inputs"
fi
if ! service_reused "${IMAGE_PREFIX}/momento-llm-service" "$llm_inputs"; then
    momento_build_oci "$ROOT_DIR" llm-runtime Dockerfile.llm-runtime "$llm_runtime_inputs"
    build_image Dockerfile.llm "${IMAGE_PREFIX}/momento-llm-service" "$llm_inputs" \
        --build-context "momento-llm-runtime=oci-layout://$ROOT_DIR/dist/docker/llm-runtime/$llm_runtime_inputs"
fi

if [[ "$PUBLISH" == true ]]; then
    printf 'Published Momento %s images to %s\n' "$TAG" "$IMAGE_PREFIX"
else
    printf 'Built Momento %s images locally under %s\n' "$TAG" "$IMAGE_PREFIX"
fi
momento_build_success
