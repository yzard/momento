#!/usr/bin/env bash
set -euo pipefail

readonly repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
readonly script_source="$repository_root/build_mobile_clients.sh"
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
mkdir -p "$workspace/docker" "$workspace/src/android" "$keystore_dir" "$tools_dir"
cp "$script_source" "$workspace/build_mobile_clients.sh"
cp "$repository_root/docker/Dockerfile.android" "$workspace/docker/Dockerfile.android"
cp "$repository_root/src/android/version.txt" "$workspace/src/android/version.txt"
chmod +x "$workspace/build_mobile_clients.sh"

cat > "$tools_dir/docker" <<'DOCKER'
#!/usr/bin/env bash
set -euo pipefail
case "$1" in
    build)
        [[ "$*" == *"docker/Dockerfile.android"* ]]
        ;;
    run)
        distribution_dir=
        signing_dir=
        source_dir=
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
        stem=$(basename "$signing_dir"/*.jks .jks)
        version=$(<"$source_dir/src/android/version.txt")
        : > "$distribution_dir/$stem-$version.apk"
        : > "$distribution_dir/$stem-$version.aab"
        ;;
    *)
        exit 1
        ;;
esac
DOCKER
chmod +x "$tools_dir/docker"

run_script() {
    PATH="$tools_dir:$PATH" "$workspace/build_mobile_clients.sh" "$@"
}

expect_failure "usage:" run_script
printf 'invalid\n' > "$workspace/src/android/version.txt"
expect_failure "major.minor.patch" run_script "$keystore_dir"
printf '1.0.0\n' > "$workspace/src/android/version.txt"
expect_failure "exactly one direct .jks file" run_script "$keystore_dir"

: > "$keystore_dir/Momento-Release.jks"
: > "$keystore_dir/second.jks"
printf 'secret\n' > "$keystore_dir/password.txt"
expect_failure "exactly one direct .jks file" run_script "$keystore_dir"
rm "$keystore_dir/second.jks"

rm "$keystore_dir/password.txt"
expect_failure "must contain password.txt" run_script "$keystore_dir"
: > "$keystore_dir/password.txt"
expect_failure "exactly one non-empty line" run_script "$keystore_dir"
printf 'one\ntwo\n' > "$keystore_dir/password.txt"
expect_failure "exactly one non-empty line" run_script "$keystore_dir"
printf 'secret\n' > "$keystore_dir/password.txt"

run_script "$keystore_dir"
[[ -f "$workspace/dist/mobile/android/Momento-Release-1.0.0.apk" ]] || fail "APK name did not include the Android version"
[[ -f "$workspace/dist/mobile/android/Momento-Release-1.0.0.aab" ]] || fail "AAB name did not include the Android version"

printf 'build_mobile_clients.sh tests passed\n'
