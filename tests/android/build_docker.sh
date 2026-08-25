#!/usr/bin/env bash
set -euo pipefail

readonly repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
readonly test_root="$(mktemp -d)"
trap 'rm -rf "$test_root"' EXIT

fail() {
    printf 'Test failure: %s\n' "$1" >&2
    exit 1
}

expect_failure() {
    local expected_message="$1"
    shift
    local output
    if output="$("$@" 2>&1)"; then
        fail "expected command to fail: $*"
    fi
    [[ "$output" == *"$expected_message"* ]] || fail "expected '$expected_message', got '$output'"
}

readonly workspace="$test_root/workspace"
readonly keystore_dir="$test_root/keystore"
readonly tools_dir="$test_root/tools"
readonly invocation_log="$test_root/invocations"
mkdir -p "$workspace/docker" "$workspace/src/backend" "$keystore_dir" "$tools_dir"
cp "$repository_root/build_docker.sh" "$workspace/build_docker.sh"
cp "$repository_root/docker/Dockerfile" "$workspace/docker/Dockerfile"
cp "$repository_root/docker/Dockerfile.llm" "$workspace/docker/Dockerfile.llm"
printf '1.0.0\n' > "$workspace/src/backend/version.txt"
chmod +x "$workspace/build_docker.sh"

cat > "$workspace/build_android_client.sh" <<'ANDROID'
#!/usr/bin/env bash
set -euo pipefail
[[ "$#" -eq 3 ]]
[[ "$1" == release ]]
[[ "$2" == --keystore-dir ]]
[[ -d "$3" ]]
mkdir -p "$(dirname "$0")/dist/mobile/android"
: > "$(dirname "$0")/dist/mobile/android/Momento-Release-1.0.0.apk"
printf 'android:%s\n' "$*" >> "$MOMENTO_TEST_INVOCATIONS"
ANDROID
chmod +x "$workspace/build_android_client.sh"

cat > "$tools_dir/docker" <<'DOCKER'
#!/usr/bin/env bash
set -euo pipefail
[[ "$1" == buildx && "$2" == build ]]
if [[ "$*" == *"docker/Dockerfile "* ]]; then
    [[ -f "${MOMENTO_TEST_WORKSPACE}/dist/mobile/android/Momento-Release-1.0.0.apk" ]]
fi
printf 'docker:%s\n' "$*" >> "$MOMENTO_TEST_INVOCATIONS"
DOCKER
chmod +x "$tools_dir/docker"

run_script() {
    PATH="$tools_dir:$PATH" \
    MOMENTO_TEST_INVOCATIONS="$invocation_log" \
    MOMENTO_TEST_WORKSPACE="$workspace" \
    "$workspace/build_docker.sh" "$@"
}

expect_failure "Usage:" run_script
help_output=$(run_script --help)
[[ "$help_output" == *"Cargo's release profile"* ]] || fail "help did not explain Rust release behavior"
[[ "$help_output" == *"SOURCE_REPOSITORY"* ]] || fail "help did not explain environment options"
run_script "$keystore_dir"
mapfile -t local_invocations < "$invocation_log"
[[ "${local_invocations[0]}" == "android:release --keystore-dir $keystore_dir" ]] || fail "Android release did not run first with the keystore directory"
[[ "${#local_invocations[@]}" -eq 3 ]] || fail "local build did not build both service images"
[[ "${local_invocations[1]}" == *"--load"*"zhuoyin/momento:1.0.0"* ]] || fail "local Momento image arguments were incorrect"
[[ "${local_invocations[2]}" == *"zhuoyin/momento-llm-service:1.0.0"* ]] || fail "local LLM image arguments were incorrect"

: > "$invocation_log"
run_script publish github yzard "$keystore_dir"
mapfile -t publish_invocations < "$invocation_log"
[[ "${publish_invocations[0]}" == "android:release --keystore-dir $keystore_dir" ]] || fail "publish did not run the Android release first"
[[ "${publish_invocations[1]}" == *"--push"*"ghcr.io/yzard/momento:1.0.0"* ]] || fail "published Momento image arguments were incorrect"
[[ "${publish_invocations[2]}" == *"ghcr.io/yzard/momento-llm-service:1.0.0"* ]] || fail "published LLM image arguments were incorrect"

printf 'build_docker.sh tests passed\n'
