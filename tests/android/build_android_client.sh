#!/usr/bin/env bash
set -euo pipefail

readonly repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
readonly script_source="$repository_root/build_android_client.sh"
mkdir -p "$repository_root/build/tmp"
readonly test_root="$(mktemp -d "$repository_root/build/tmp/android-build.XXXXXX")"
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
mkdir -p "$workspace/docker" "$workspace/src/android" "$keystore_dir" "$tools_dir"
cp "$script_source" "$workspace/build_android_client.sh"
cp "$repository_root/docker/Dockerfile.android" "$workspace/docker/Dockerfile.android"
cp "$repository_root/src/android/version.txt" "$workspace/src/android/version.txt"
chmod +x "$workspace/build_android_client.sh"

mkdir -p "$workspace/docker/lib"
cp "$repository_root/docker/lib/build_lifecycle.sh" "$workspace/docker/lib/"
cp "$repository_root/docker/entrypoint_android.sh" "$workspace/docker/"
cp "$repository_root/tests/docker/fake_build_docker.sh" "$tools_dir/docker"
chmod +x "$tools_dir/docker"

run_script() {
    PATH="$tools_dir:$PATH" \
    MOMENTO_TEST_INVOCATIONS="$invocation_log" \
    MOMENTO_TEST_DOCKER_STATE="$test_root/docker-state" \
    "$workspace/build_android_client.sh" "$@"
}

expect_failure "an Android command is required" run_script
expect_failure "unsupported Android command" run_script unknown
expect_failure "valid only for release" run_script verify --keystore-dir "$keystore_dir"

help_output=$(run_script --help)
for help_term in verify assemble-debug instrumented-test shell release --no-cache /dev/kvm; do
    [[ "$help_output" == *"$help_term"* ]] || fail "help did not explain $help_term"
done
[[ "$help_output" == *"Host Java"* ]] || fail "help did not explain the Docker-only toolchain"

printf 'invalid\n' > "$workspace/src/android/version.txt"
expect_failure "major.minor.patch" run_script verify
printf '1.0.0\n' > "$workspace/src/android/version.txt"

: > "$invocation_log"
run_script verify --no-cache
mapfile -t verify_invocations < "$invocation_log"
[[ "$(<"$invocation_log")" == *"--target android-builder"*"--no-cache"* ]] || fail "verify did not build the cached builder target correctly"
[[ "$(<"$invocation_log")" == *"momento-android-toolchain:android-builder-"*" verify"* ]] || fail "verify did not run the builder command"
[[ "$(<"$invocation_log")" != *":/signing:ro"* ]] || fail "verify mounted signing material"

: > "$invocation_log"
run_script assemble-debug
[[ -f "$workspace/dist/android/debug/momento-android-1.0.0-debug.apk" ]] || fail "debug APK was not validated"

expect_failure "release requires --keystore-dir" run_script release
expect_failure "exactly one direct .jks file" run_script release --keystore-dir "$keystore_dir"
: > "$keystore_dir/Momento-Release.jks"
: > "$keystore_dir/second.jks"
printf 'secret\n' > "$keystore_dir/password.txt"
expect_failure "exactly one direct .jks file" run_script release --keystore-dir "$keystore_dir"
rm "$keystore_dir/second.jks"
rm "$keystore_dir/password.txt"
expect_failure "must contain password.txt" run_script release --keystore-dir "$keystore_dir"
: > "$keystore_dir/password.txt"
expect_failure "exactly one non-empty line" run_script release --keystore-dir "$keystore_dir"
printf 'one\ntwo\n' > "$keystore_dir/password.txt"
expect_failure "exactly one non-empty line" run_script release --keystore-dir "$keystore_dir"
printf 'secret\n' > "$keystore_dir/password.txt"

: > "$invocation_log"
run_script release --keystore-dir "$keystore_dir"
[[ -f "$workspace/dist/android/Momento-Release-1.0.0.apk" ]] || fail "release APK name did not include the Android version"
[[ -f "$workspace/dist/android/Momento-Release-1.0.0.aab" ]] || fail "release AAB name did not include the Android version"
mapfile -t release_invocations < "$invocation_log"
[[ "$(<"$invocation_log")" == *"momento-android-toolchain:android-builder-"* ]] || fail "release did not build the builder target"
[[ "$(<"$invocation_log")" == *":/signing:ro"*" release"* ]] || fail "release did not mount signing material for the release command"

if [[ -c /dev/kvm ]]; then
    : > "$invocation_log"
    run_script instrumented-test
    mapfile -t instrumented_invocations < "$invocation_log"
    [[ "$(<"$invocation_log")" == *"--target android-emulator"* ]] || fail "instrumented tests did not build the emulator target"
    [[ "$(<"$invocation_log")" == *"--device /dev/kvm"*" instrumented-test"* ]] || fail "instrumented tests did not pass KVM into the container"
else
    expect_failure "requires /dev/kvm" run_script instrumented-test
fi

# An unchanged release does not invoke Docker or Gradle.
: > "$invocation_log"
run_script release --keystore-dir "$keystore_dir"
[[ "$(<"$invocation_log")" != *"buildx build"* && "$(<"$invocation_log")" != *"docker:run"* ]] || fail "unchanged release rebuilt Android"
[[ -f "$workspace/build/android/state/debug.keystore" ]] || fail "debug signing identity was deleted"
[[ -f "$workspace/build/android/reports/release/reports/test-report.txt" ]] || fail "validation report was deleted"
[[ ! -e "$workspace/build/android/workspace" && ! -e "$workspace/build/android/gradle-user-home" && ! -e "$workspace/build/android/android-avd" ]] || fail "successful build left temporary Android state"
rm "$workspace/dist/android/Momento-Release-1.0.0.apk"
run_script release --keystore-dir "$keystore_dir"
[[ -s "$invocation_log" ]] || fail "missing artifact did not rebuild"
: > "$invocation_log"
printf 'source edit\n' > "$workspace/src/android/example.kt"
run_script release --keystore-dir "$keystore_dir"
[[ "$(<"$invocation_log")" != *"buildx build"* ]] || fail "source-only change rebuilt SDK image"
MOMENTO_TEST_RUN_FAIL=true expect_failure "Diagnostics retained" run_script verify
[[ -d "$workspace/build/android/workspace" ]] || fail "failed build diagnostics were erased"
MOMENTO_TEST_CLEANUP_FAIL=true expect_failure "did not complete cleanly" run_script verify --no-cache

printf 'build_android_client.sh tests passed\n'
