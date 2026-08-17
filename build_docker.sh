#!/bin/bash
set -euo pipefail

TAG=$(date +%Y%m%d)
MOMENTO_IMAGE="zhuoyin/momento"
LLM_IMAGE="zhuoyin/momento-llm-service"

cd "$(dirname "$0")/docker"

docker build -f Dockerfile -t "${MOMENTO_IMAGE}:${TAG}" -t "${MOMENTO_IMAGE}:latest" ..
docker build -f Dockerfile.llm -t "${LLM_IMAGE}:${TAG}" -t "${LLM_IMAGE}:latest" ..

docker push "${MOMENTO_IMAGE}:${TAG}"
docker push "${MOMENTO_IMAGE}:latest"
docker push "${LLM_IMAGE}:${TAG}"
docker push "${LLM_IMAGE}:latest"
