#!/usr/bin/env bash
set -euo pipefail
readonly repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
mkdir -p "$repository_root/build/tmp"
readonly test_root="$(mktemp -d "$repository_root/build/tmp/docker-build.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT
fail() { printf 'Test failure: %s\n' "$1" >&2; exit 1; }
expect_failure() {
    local expected=$1 output
    shift
    if output=$("$@" 2>&1); then fail "expected failure: $*"; fi
    [[ $output == *"$expected"* ]] || fail "expected '$expected', got '$output'"
}
readonly workspace="$test_root/workspace"
readonly keystore_dir="$test_root/keystore"
readonly tools_dir="$test_root/tools"
readonly invocation_log="$test_root/invocations"
mkdir -p "$workspace/src/"{backend,backend_llm,common,frontend} "$keystore_dir" "$tools_dir"
for file in build_docker.sh docker/lib/build_lifecycle.sh docker/lib/validate_oci.py docker/Dockerfile docker/Dockerfile.llm \
    docker/Dockerfile.runtime docker/Dockerfile.llm-runtime docker/Dockerfile.frontend \
    docker/Dockerfile.dockerignore docker/Dockerfile.llm.dockerignore docker/Dockerfile.runtime.dockerignore \
    docker/Dockerfile.llm-runtime.dockerignore docker/Dockerfile.frontend.dockerignore \
    docker/build_raw_decoder.sh docker/libraw-dng-host.patch docker/dng-sdk-portability.patch \
    docker/imagemagick-policy.xml docker/entrypoint.sh docker/entrypoint_llm.sh docker/entrypoint_common.sh \
    tests/docker/build_raw_decoder.sh tests/docker/libraw-dng-host.sh package.json pnpm-lock.yaml pnpm-workspace.yaml; do
    mkdir -p "$workspace/$(dirname "$file")"
    cp "$repository_root/$file" "$workspace/$file"
done
printf '1.0.0\n' > "$workspace/src/backend/version.txt"
printf 'backend source\n' > "$workspace/src/backend/main.rs"
printf 'llm source\n' > "$workspace/src/backend_llm/main.rs"
printf 'frontend source\n' > "$workspace/src/frontend/main.tsx"
cat > "$workspace/build_android_client.sh" <<'ANDROID'
#!/usr/bin/env bash
set -euo pipefail
[[ $# == 3 && $1 == release && $2 == --keystore-dir && -d $3 ]]
mkdir -p "$(dirname "$0")/dist/android"
if [[ ${MOMENTO_TEST_SKIP_ANDROID_APK:-false} != true ]]; then
    printf 'signed-release-apk\n' > "$(dirname "$0")/dist/android/Momento-Release-1.0.0.apk"
fi
printf 'android:%s\n' "$*" >> "$MOMENTO_TEST_INVOCATIONS"
ANDROID
chmod +x "$workspace/build_android_client.sh" "$workspace/build_docker.sh"
cp "$repository_root/tests/docker/fake_build_docker.sh" "$tools_dir/docker"
chmod +x "$tools_dir/docker"
run_script() {
    PATH="$tools_dir:$PATH" MOMENTO_TEST_INVOCATIONS="$invocation_log" \
        MOMENTO_TEST_DOCKER_STATE="$test_root/docker-state" "$workspace/build_docker.sh" "$@"
}
build_count() { local n; n=$(rg -c '^docker:buildx build ' "$invocation_log" || true); printf '%s' "${n:-0}"; }
expect_failure 'Usage:' run_script
help_output=$(run_script --help)
[[ $help_output == *"Cargo's release profile"* && $help_output == *SOURCE_REPOSITORY* ]] || fail "help lost build contract"
MOMENTO_TEST_SKIP_ANDROID_APK=true expect_failure 'exactly one non-empty signed release APK' run_script "$keystore_dir"
: > "$invocation_log"
run_script "$keystore_dir"
[[ $(build_count) == 5 ]] || fail "initial build did not export three reusable artifacts and two services"
[[ $(head -1 "$invocation_log") == android:* ]] || fail "Android release did not run first"
[[ $(<"$invocation_log") == *"--load"* && $(<"$invocation_log") == *"buildx rm --force momento-"* ]] || fail "local export or owned cleanup missing"
[[ $(<"$invocation_log") != *prune* ]] || fail "global cache pruning is forbidden"
[[ -s "$workspace/dist/android/Momento-Release-1.0.0.apk" ]] || fail "cleanup erased final App"
[[ -n $(find "$workspace/dist/docker/llm-runtime" -name inputs -print -quit) ]] || fail "reusable model artifact was erased"
: > "$invocation_log"
run_script "$keystore_dir"
[[ $(build_count) == 0 ]] || fail "unchanged targets rebuilt"
: > "$invocation_log"
TAG=2.0.0 run_script "$keystore_dir"
[[ $(build_count) == 0 ]] || fail "retagging unchanged targets recompiled source"
printf 'backend change\n' >> "$workspace/src/backend/main.rs"
: > "$invocation_log"
run_script "$keystore_dir"
[[ $(build_count) == 1 && $(<"$invocation_log") != *"-f $workspace/docker/Dockerfile.llm "* ]] || fail "backend-only edit rebuilt unrelated target"
printf 'frontend change\n' >> "$workspace/src/frontend/main.tsx"
: > "$invocation_log"
run_script "$keystore_dir"
[[ $(build_count) == 2 ]] || fail "frontend edit did not rebuild only frontend and API"
printf '\n# updated model revision\n' >> "$workspace/docker/Dockerfile.llm-runtime"
: > "$invocation_log"
run_script "$keystore_dir"
[[ $(build_count) == 2 ]] || fail "model revision did not rebuild runtime and LLM only"
printf 'failed source change\n' >> "$workspace/src/backend_llm/main.rs"
MOMENTO_TEST_FAIL_DOCKERFILE=Dockerfile.llm expect_failure 'Diagnostics retained' run_script "$keystore_dir"
: > "$invocation_log"
run_script "$keystore_dir"
[[ $(build_count) == 1 ]] || fail "retry rebuilt valid models or skipped failed source"
: > "$invocation_log"
run_script publish github yzard "$keystore_dir"
[[ $(<"$invocation_log") == *"image push ghcr.io/yzard/momento:1.0.0"* ]] || fail "publishing did not preserve registry contract"
# Simulate a build host with no locally loaded service images. Registry receipts remain valid.
rm -f "$test_root/docker-state/"image-*
# Restore only the remote receipts from the successful publish metadata.
for receipt in "$workspace/build/docker/state/"*.inputs; do
    read -r fingerprint digest < "$receipt"
    for image in ghcr.io/yzard/momento ghcr.io/yzard/momento-llm-service; do
        if [[ $(basename "$receipt" .inputs) == "$(printf '%s\n' "$image:1.0.0" | sha256sum | cut -d ' ' -f1)" ]]; then
            for tag in 1.0.0 latest; do
                printf '%s\n' "$digest" > "$test_root/docker-state/image-$(printf '%s' "$image:$tag" | sha256sum | cut -d ' ' -f1).remote"
            done
        fi
    done
done
: > "$invocation_log"
run_script publish github yzard "$keystore_dir"
[[ $(build_count) == 0 ]] || fail "unchanged published targets rebuilt"
: > "$invocation_log"
run_script "$keystore_dir"
[[ $(build_count) == 2 ]] || fail "missing service images did not rebuild from retained model/runtime artifacts"
printf 'cleanup failure\n' >> "$workspace/src/backend/main.rs"
MOMENTO_TEST_CLEANUP_FAIL=true expect_failure 'did not complete cleanly' run_script "$keystore_dir"
printf 'build_docker.sh tests passed\n'
