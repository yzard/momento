#!/usr/bin/env bash
set -euo pipefail

readonly repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$repository_root/docker/lib/build_lifecycle.sh"
momento_build_begin "$repository_root" frontend

readonly source_fingerprint=$(printf '%s\n%s\n%s\n' \
    "$(momento_build_fingerprint "$repository_root" src/frontend src/backend/version.txt package.json pnpm-lock.yaml pnpm-workspace.yaml docker/lib/build_lifecycle.sh)" \
    "$(node --version)" "$(pnpm --version)" | sha256sum | cut -d ' ' -f1)
readonly receipt="$repository_root/build/frontend/state/inputs"
if [[ -f "$receipt" && -s "$repository_root/dist/frontend/index.html" ]]; then
    mapfile -t previous_inputs < "$receipt"
    if [[ ${previous_inputs[0]:-} == "$source_fingerprint" && ${previous_inputs[1]:-} == "$(momento_build_fingerprint "$repository_root" dist/frontend)" ]]; then
        printf 'Reusing unchanged frontend assets.\n'
        momento_build_success
        exit 0
    fi
fi

readonly workspace="$MOMENTO_BUILD_WORK/workspace"
mkdir -p "$workspace/src/frontend" "$workspace/src/backend"
cp "$repository_root/"{package.json,pnpm-lock.yaml,pnpm-workspace.yaml} "$workspace/"
cp "$repository_root/src/backend/version.txt" "$workspace/src/backend/version.txt"
tar -C "$repository_root/src/frontend" \
    --exclude=node_modules --exclude=dist --exclude=dist-node \
    --exclude=.turbo --exclude='*.tsbuildinfo' \
    -cf - . | tar -C "$workspace/src/frontend" -xf -

pnpm --dir "$workspace" install --frozen-lockfile \
    --store-dir "$MOMENTO_BUILD_WORK/store" \
    --config.cache-dir="$MOMENTO_BUILD_WORK/cache" 2>&1 | tee "$MOMENTO_BUILD_WORK/install.log"
(
    cd "$workspace/src/frontend"
    pnpm exec tsc -b
    pnpm exec vite build --outDir "$MOMENTO_BUILD_WORK/export"
) 2>&1 | tee "$MOMENTO_BUILD_WORK/build.log"
[[ -s "$MOMENTO_BUILD_WORK/export/index.html" ]] || { printf 'Frontend export is missing index.html\n' >&2; exit 1; }

mkdir -p "$repository_root/dist" "$repository_root/build/frontend/reports"
if [[ -e "$repository_root/dist/frontend" ]]; then
    mv "$repository_root/dist/frontend" "$MOMENTO_BUILD_WORK/previous-output"
fi
if ! mv "$MOMENTO_BUILD_WORK/export" "$repository_root/dist/frontend"; then
    if [[ -d "$MOMENTO_BUILD_WORK/previous-output" ]]; then
        mv "$MOMENTO_BUILD_WORK/previous-output" "$repository_root/dist/frontend"
    fi
    exit 1
fi
cp "$MOMENTO_BUILD_WORK/"*.log "$repository_root/build/frontend/reports/"
# Remove legacy generated outputs, never the shared development dependency installation.
rm -rf -- "$repository_root/src/frontend/dist" "$repository_root/src/frontend/dist-node" "$repository_root/src/frontend/.turbo"
rm -f -- "$repository_root/src/frontend/"*.tsbuildinfo
printf '%s\n%s\n' "$source_fingerprint" "$(momento_build_fingerprint "$repository_root" dist/frontend)" > "$MOMENTO_BUILD_WORK/artifact.inputs"
momento_build_commit "$MOMENTO_BUILD_WORK/artifact.inputs" "$receipt"
momento_build_success
