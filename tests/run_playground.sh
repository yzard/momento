#!/usr/bin/env bash
set -euo pipefail

readonly repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly test_root="$(mktemp -d)"
trap 'rm -rf "$test_root"' EXIT

fail() {
    printf 'Test failure: %s\n' "$1" >&2
    exit 1
}

readonly workspace="$test_root/workspace"
readonly keystore_dir="$test_root/keystore"
readonly tools_dir="$test_root/tools"
readonly invocation_log="$test_root/invocations"
mkdir -p "$workspace" "$keystore_dir" "$tools_dir"
cp "$repository_root/run_playground.sh" "$workspace/run_playground.sh"
touch "$workspace/docker-compose.yaml"
chmod +x "$workspace/run_playground.sh"

printf '%s\n' \
    '#!/usr/bin/env bash' \
    'set -euo pipefail' \
    'printf "build:%s\\n" "$*" >> "$MOMENTO_TEST_INVOCATIONS"' \
    > "$workspace/build_docker.sh"
chmod +x "$workspace/build_docker.sh"

printf '%s\n' \
    '#!/usr/bin/env bash' \
    'set -euo pipefail' \
    'printf "docker:%s\\n" "$*" >> "$MOMENTO_TEST_INVOCATIONS"' \
    > "$tools_dir/docker"
chmod +x "$tools_dir/docker"

PATH="$tools_dir:$PATH" \
MOMENTO_TEST_INVOCATIONS="$invocation_log" \
    "$workspace/run_playground.sh" "$keystore_dir"

mapfile -t invocations < "$invocation_log"
[[ "${#invocations[@]}" -eq 4 ]] || fail "expected initial cleanup, build, foreground run, and exit cleanup"
[[ "${invocations[0]}" == "docker:compose -f $workspace/docker-compose.yaml down --remove-orphans" ]] || fail "initial cleanup arguments were incorrect"
[[ "${invocations[1]}" == "build:$keystore_dir" ]] || fail "Docker build did not receive the keystore directory"
[[ "${invocations[2]}" == "docker:compose -f $workspace/docker-compose.yaml up --remove-orphans" ]] || fail "foreground Compose arguments were incorrect"
[[ "${invocations[2]}" != *"--abort-on-container-exit"* ]] || fail "foreground run disables the service restart policy"
[[ "${invocations[3]}" == "docker:compose -f $workspace/docker-compose.yaml down --remove-orphans" ]] || fail "exit cleanup arguments were incorrect"

printf 'run_playground.sh tests passed\n'
