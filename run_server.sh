#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(dirname "$(realpath "$0")")
API_BUILD_DIR="$ROOT_DIR/build/api"
API_DIST_DIR="$ROOT_DIR/dist/api"

WEB_BUILD_DIR="$ROOT_DIR/build/web"
WEB_WORKSPACE_DIR="$WEB_BUILD_DIR/workspace"
WEB_APP_DIR="$WEB_WORKSPACE_DIR/src/web"
WEB_DIST_DIR="$ROOT_DIR/dist/web"

export RUST_BACKTRACE=full

rm -rf "$API_BUILD_DIR" "$WEB_BUILD_DIR" "$API_DIST_DIR" "$WEB_DIST_DIR"
mkdir -p "$WEB_WORKSPACE_DIR" "$API_DIST_DIR" "$WEB_DIST_DIR"

cp "$ROOT_DIR/package.json" "$ROOT_DIR/pnpm-lock.yaml" "$ROOT_DIR/pnpm-workspace.yaml" "$WEB_WORKSPACE_DIR/"
mkdir -p "$WEB_WORKSPACE_DIR/src"
cp -R "$ROOT_DIR/src/web" "$WEB_APP_DIR"

pnpm install --dir "$WEB_WORKSPACE_DIR" --frozen-lockfile
pnpm --dir "$WEB_APP_DIR" build
cp -R "$WEB_APP_DIR/dist/." "$WEB_DIST_DIR/"

CARGO_TARGET_DIR="$API_BUILD_DIR/target" cargo build --release --manifest-path "$ROOT_DIR/src/api/Cargo.toml"
cp "$API_BUILD_DIR/target/release/momento-api" "$API_DIST_DIR/momento-api"

MOMENTO_DATA_DIR="$ROOT_DIR/sample_data" \
MOMENTO_STATIC_DIR="$WEB_DIST_DIR" \
"$API_DIST_DIR/momento-api"
