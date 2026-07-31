#!/bin/bash
set -euo pipefail

TAG=$(date +%Y%m%d)
IMAGE_NAME="zhuoyin/momento"

cd "$(dirname "$0")/docker"

echo "Building Docker image: ${IMAGE_NAME}:${TAG}..."
docker build -f Dockerfile -t "${IMAGE_NAME}:${TAG}" -t "${IMAGE_NAME}:latest" ..

echo "Build complete: ${IMAGE_NAME}:${TAG}"

echo "Pushing Docker images to registry..."
docker push "${IMAGE_NAME}:${TAG}"
docker push "${IMAGE_NAME}:latest"
echo "Push complete: ${IMAGE_NAME}:${TAG} and ${IMAGE_NAME}:latest"
