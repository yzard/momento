#!/bin/bash

# Get current date in YYYYMMDD format
TAG=$(date +%Y%m%d)
IMAGE_NAME="momento"

echo "Building Docker image: ${IMAGE_NAME}:${TAG}..."

docker build -t "${IMAGE_NAME}:${TAG}" -t "${IMAGE_NAME}:latest" .

echo "Build complete: ${IMAGE_NAME}:${TAG}"

# Uncomment to push to registry
echo "Pushing Docker images to registry..."
docker push "${IMAGE_NAME}:${TAG}"
docker push "${IMAGE_NAME}:latest"
echo "Push complete: ${IMAGE_NAME}:${TAG} and ${IMAGE_NAME}:latest"
