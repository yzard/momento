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

mkdir -p \
    "$test_root/source/src/android/app" \
    "$test_root/source/tests/android" \
    "$test_root/build" \
    "$test_root/dist" \
    "$test_root/signing" \
    "$test_root/tools"

cat > "$test_root/source/src/android/gradlew" <<'GRADLE'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$MOMENTO_TEST_GRADLE_LOG"
if [[ "$*" != *":app:assembleRelease"* ]]; then
    [[ -z "${ORG_GRADLE_PROJECT_momentoReleaseStoreFile:-}" ]]
fi
if [[ "$*" == *":app:assembleDebug"* ]]; then
    mkdir -p app/build/outputs/apk/debug
    : > app/build/outputs/apk/debug/app-debug.apk
fi
if [[ "$*" == *":app:assembleRelease"* ]]; then
    [[ -n "${ORG_GRADLE_PROJECT_momentoReleaseStoreFile:-}" ]]
    mkdir -p app/build/outputs/apk/release app/build/outputs/bundle/release
    : > app/build/outputs/apk/release/app-release.apk
    : > app/build/outputs/bundle/release/app-release.aab
fi
GRADLE
chmod +x "$test_root/source/src/android/gradlew"
printf '1.0.0\n' > "$test_root/source/src/android/version.txt"
: > "$test_root/signing/Momento-Release.jks"
printf 'secret\n' > "$test_root/signing/password.txt"
: > "$test_root/gradle-invocations"

cat > "$test_root/tools/keytool" <<'KEYTOOL'
#!/usr/bin/env bash
if [[ "${FAKE_KEYTOOL_MODE:-single}" == two ]]; then
    printf 'Alias name: first\nEntry type: PrivateKeyEntry\nAlias name: second\nEntry type: PrivateKeyEntry\n'
else
    printf 'Alias name: release\nEntry type: PrivateKeyEntry\n'
fi
KEYTOOL
cat > "$test_root/tools/apksigner" <<'APKSIGNER'
#!/usr/bin/env bash
exit 0
APKSIGNER
cat > "$test_root/tools/jarsigner" <<'JARSIGNER'
#!/usr/bin/env bash
exit 0
JARSIGNER
cat > "$test_root/tools/avdmanager" <<'AVDMANAGER'
#!/usr/bin/env bash
set -euo pipefail
mkdir -p "$ANDROID_AVD_HOME/momento-test.avd"
: > "$ANDROID_AVD_HOME/momento-test.avd/config.ini"
AVDMANAGER
cat > "$test_root/tools/adb" <<'ADB'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$*" == *"getprop sys.boot_completed"* ]]; then
    printf '1\n'
fi
ADB
cat > "$test_root/tools/emulator" <<'EMULATOR'
#!/usr/bin/env bash
while true; do
    sleep 60
done
EMULATOR
chmod +x "$test_root/tools"/*

run_entrypoint() {
    PATH="$test_root/tools:$PATH" \
    MOMENTO_SOURCE_ROOT="$test_root/source" \
    MOMENTO_BUILD_ROOT="$test_root/build" \
    MOMENTO_DISTRIBUTION_ROOT="$test_root/dist" \
    MOMENTO_SIGNING_ROOT="$test_root/signing" \
    MOMENTO_TEST_GRADLE_LOG="$test_root/gradle-invocations" \
    /bin/bash "$repository_root/docker/entrypoint_android.sh" "$@"
}

help_output=$(/bin/bash "$repository_root/docker/entrypoint_android.sh" --help)
[[ "$help_output" == *"Signing environment"* ]] || fail "entrypoint help did not explain release signing"
expect_failure "unsupported Android container command" run_entrypoint unknown

: > "$test_root/gradle-invocations"
run_entrypoint verify
verify_tasks=$(<"$test_root/gradle-invocations")
[[ "$verify_tasks" == *":app:assembleDebug"* ]] || fail "verify did not assemble debug"
[[ "$verify_tasks" == *":app:testDebugUnitTest"* ]] || fail "verify did not run JVM tests"
[[ "$verify_tasks" == *":app:lintDebug"* ]] || fail "verify did not run lint"

: > "$test_root/gradle-invocations"
run_entrypoint assemble-debug
[[ -f "$test_root/dist/debug/momento-android-1.0.0-debug.apk" ]] || fail "entrypoint did not export the debug APK"

: > "$test_root/gradle-invocations"
run_entrypoint instrumented-test
instrumented_tasks=$(<"$test_root/gradle-invocations")
[[ "$instrumented_tasks" == *":app:connectedDebugAndroidTest"* ]] || fail "entrypoint did not run connected tests"

run_entrypoint shell </dev/null
[[ -d "$test_root/build/workspace/src/android" ]] || fail "shell did not stage the Android workspace"

if FAKE_KEYTOOL_MODE=two run_entrypoint release >/dev/null 2>&1; then
    fail "multiple private-key aliases should fail"
fi

run_entrypoint release
[[ -f "$test_root/dist/Momento-Release-1.0.0.apk" ]] || fail "entrypoint did not copy the versioned APK"
[[ -f "$test_root/dist/Momento-Release-1.0.0.aab" ]] || fail "entrypoint did not copy the versioned AAB"

printf 'entrypoint_android.sh tests passed\n'
