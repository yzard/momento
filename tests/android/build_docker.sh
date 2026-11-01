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
for file in build_docker.sh docker/lib/build_lifecycle.sh docker/lib/validate_oci.py docker/lib/docker_archive_oci.py src/backend_llm/ocr/requirements.txt docker/Dockerfile docker/Dockerfile.llm \
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
        MOMENTO_TEST_DOCKER_STATE="$test_root/docker-state" XDG_STATE_HOME="$test_root/state" "$workspace/build_docker.sh" "$@"
}
build_count() { local n; n=$(rg -c '^docker:buildx build ' "$invocation_log" || true); printf '%s' "${n:-0}"; }
expect_failure 'Usage:' run_script
help_output=$(run_script --help)
[[ $help_output == *"Cargo's release profile"* && $help_output == *SOURCE_REPOSITORY* ]] || fail "help lost build contract"
MOMENTO_TEST_SKIP_ANDROID_APK=true expect_failure 'exactly one non-empty signed release APK' run_script "$keystore_dir"
mkdir -p "$workspace/build/old-cache" "$workspace/build/docker/reports"
printf 'old temporary object\n' > "$workspace/build/old-cache/debug.o"
printf 'preserved report\n' > "$workspace/build/docker/reports/legacy.log"
: > "$invocation_log"
run_script "$keystore_dir"
readonly persistent_state="$test_root/state/momento-build/$(printf '%s' "$workspace" | sha256sum | cut -d ' ' -f1)"
[[ -f "$persistent_state/docker/reports/legacy.log" ]] || fail "cleanup erased an existing report"
[[ $(build_count) == 5 ]] || fail "initial build did not export three reusable artifacts and two services"
[[ $(head -1 "$invocation_log") == android:* ]] || fail "Android release did not run first"
[[ $(<"$invocation_log") == *"--load"* && $(<"$invocation_log") == *"buildx rm --force momento-"* ]] || fail "local export or owned cleanup missing"
[[ $(<"$invocation_log") != *prune* ]] || fail "global cache pruning is forbidden"
[[ -s "$workspace/dist/android/Momento-Release-1.0.0.apk" ]] || fail "cleanup erased final App"
[[ ! -e "$workspace/build" && ! -e "$workspace/dist/docker" ]] || fail "successful build retained temporary directories or Docker exports"
[[ "$(<"$invocation_log")" == *"--tag momento-build-llm-runtime:"* ]] || fail "model runtime was not retained as a Docker image"
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
for image in zhuoyin/momento zhuoyin/momento-llm-service ghcr.io/yzard/momento ghcr.io/yzard/momento-llm-service; do
    for tag in 1.0.0 2.0.0 latest; do
        rm -f "$test_root/docker-state/image-$(printf '%s' "$image:$tag" | sha256sum | cut -d ' ' -f1)"
    done
done
: > "$invocation_log"
run_script publish github yzard "$keystore_dir"
[[ $(build_count) == 0 ]] || fail "unchanged published targets rebuilt"
: > "$invocation_log"
run_script "$keystore_dir"
[[ $(build_count) == 2 ]] || fail "missing service images did not rebuild from retained model/runtime artifacts"
# A previously exported model is imported before its disk copy is removed.
legacy="$workspace/dist/docker/llm-runtime/retired"
mkdir -p "$legacy"
PATH="$tools_dir:$PATH" MOMENTO_TEST_INVOCATIONS="$invocation_log" MOMENTO_TEST_DOCKER_STATE="$test_root/docker-state" \
    docker buildx build -f "$workspace/docker/Dockerfile.llm-runtime" --output "type=oci,tar=false,dest=$legacy"
printf 'retired\n' > "$legacy/inputs"
: > "$invocation_log"
run_script "$keystore_dir"
[[ $(build_count) == 1 && ! -e "$workspace/dist/docker" && ! -e "$workspace/build" ]] || fail "legacy model was not migrated and removed"
[[ "$(<"$invocation_log")" == *"--tag momento-build-llm-runtime:retired"* ]] || fail "legacy model was deleted without a retained image"
printf 'cleanup failure\n' >> "$workspace/src/backend/main.rs"
MOMENTO_TEST_CLEANUP_FAIL=true expect_failure 'did not complete cleanly' run_script "$keystore_dir"
[[ -d "$workspace/build" && -s "$workspace/dist/android/Momento-Release-1.0.0.apk" ]] || fail "cleanup failure erased diagnostics or final App"
printf 'build_docker.sh tests passed\n'
