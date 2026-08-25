#!/usr/bin/env bash
set -euo pipefail

readonly repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
readonly script_source="$repository_root/build_android_client.sh"
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
mkdir -p "$workspace/docker" "$workspace/src/android" "$keystore_dir" "$tools_dir"
cp "$script_source" "$workspace/build_android_client.sh"
cp "$repository_root/docker/Dockerfile.android" "$workspace/docker/Dockerfile.android"
cp "$repository_root/src/android/version.txt" "$workspace/src/android/version.txt"
chmod +x "$workspace/build_android_client.sh"

cat > "$tools_dir/docker" <<'DOCKER'
#!/usr/bin/env bash
set -euo pipefail
printf 'docker:%s\n' "$*" >> "$MOMENTO_TEST_INVOCATIONS"

case "$1" in
    build)
        [[ "$*" == *"docker/Dockerfile.android"* ]]
        ;;
    run)
        distribution_dir=
        signing_dir=
        source_dir=
        container_command=${!#}
        while (( $# > 0 )); do
            if [[ "$1" == --volume ]]; then
                mount_value=$2
                case "$mount_value" in
                    *:/dist) distribution_dir=${mount_value%:/dist} ;;
                    *:/signing:ro) signing_dir=${mount_value%:/signing:ro} ;;
                    *:/workspace:ro) source_dir=${mount_value%:/workspace:ro} ;;
                esac
                shift 2
                continue
            fi
            shift
        done
        version=$(<"$source_dir/src/android/version.txt")
        case "$container_command" in
            assemble-debug)
                mkdir -p "$distribution_dir/debug"
                : > "$distribution_dir/debug/momento-android-$version-debug.apk"
                ;;
            release)
                stem=$(basename "$signing_dir"/*.jks .jks)
                : > "$distribution_dir/$stem-$version.apk"
                : > "$distribution_dir/$stem-$version.aab"
                ;;
        esac
        ;;
    *)
        exit 1
        ;;
esac
DOCKER
chmod +x "$tools_dir/docker"

run_script() {
    PATH="$tools_dir:$PATH" \
    MOMENTO_TEST_INVOCATIONS="$invocation_log" \
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
[[ "${verify_invocations[0]}" == *"--target android-builder"*"--no-cache"* ]] || fail "verify did not build the cached builder target correctly"
[[ "${verify_invocations[1]}" == *"momento-android-builder:local verify" ]] || fail "verify did not run the builder command"
[[ "${verify_invocations[1]}" != *":/signing:ro"* ]] || fail "verify mounted signing material"

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
[[ "${release_invocations[0]}" == *"--target android-builder"* ]] || fail "release did not build the builder target"
[[ "${release_invocations[1]}" == *":/signing:ro"*" release" ]] || fail "release did not mount signing material for the release command"

if [[ -c /dev/kvm ]]; then
    : > "$invocation_log"
    run_script instrumented-test
    mapfile -t instrumented_invocations < "$invocation_log"
    [[ "${instrumented_invocations[0]}" == *"--target android-emulator"* ]] || fail "instrumented tests did not build the emulator target"
    [[ "${instrumented_invocations[1]}" == *"--device /dev/kvm"*" instrumented-test" ]] || fail "instrumented tests did not pass KVM into the container"
else
    expect_failure "requires /dev/kvm" run_script instrumented-test
fi

printf 'build_android_client.sh tests passed\n'
