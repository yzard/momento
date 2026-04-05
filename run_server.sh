#!/usr/bin/env bash
set -euo pipefail

export RUST_BACKTRACE=full

pnpm build --force && cd src/api/ && MOMENTO_DATA_DIR=/home/zyin/dev/momento/sample_data cargo run
