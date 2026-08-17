#!/bin/bash
set -euo pipefail

TAG=${TAG:-$(date +%Y%m%d)}
REGISTRY=${REGISTRY:-ghcr.io}
NAMESPACE=${NAMESPACE:-yzard}
SOURCE_REPOSITORY=${SOURCE_REPOSITORY:-https://github.com/yzard/momento}
MOMENTO_IMAGE="${REGISTRY}/${NAMESPACE}/momento"
LLM_IMAGE="${REGISTRY}/${NAMESPACE}/momento-llm-service"

cd "$(dirname "$0")/docker"

docker buildx build --load \
    --build-arg "SOURCE_REPOSITORY=${SOURCE_REPOSITORY}" \
    -f Dockerfile \
    -t "${MOMENTO_IMAGE}:${TAG}" \
    -t "${MOMENTO_IMAGE}:latest" \
    ..
docker buildx build --load \
    --build-arg "SOURCE_REPOSITORY=${SOURCE_REPOSITORY}" \
    -f Dockerfile.llm \
    -t "${LLM_IMAGE}:${TAG}" \
    -t "${LLM_IMAGE}:latest" \
    ..

docker push "${MOMENTO_IMAGE}:${TAG}"
docker push "${MOMENTO_IMAGE}:latest"
docker push "${LLM_IMAGE}:${TAG}"
docker push "${LLM_IMAGE}:latest"
