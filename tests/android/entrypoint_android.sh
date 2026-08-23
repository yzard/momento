#!/usr/bin/env bash
set -euo pipefail

readonly repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
readonly test_root="$(mktemp -d)"
trap 'rm -rf "$test_root"' EXIT

fail() {
    printf 'Test failure: %s\n' "$1" >&2
    exit 1
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
[[ -z "${ORG_GRADLE_PROJECT_momentoReleaseAppName:-}" ]]
mkdir -p app/build/outputs/apk/release app/build/outputs/bundle/release
: > app/build/outputs/apk/release/app-release.apk
: > app/build/outputs/bundle/release/app-release.aab
GRADLE
chmod +x "$test_root/source/src/android/gradlew"
printf '1.0.0\n' > "$test_root/source/src/android/version.txt"
: > "$test_root/signing/Momento-Release.jks"
printf 'secret\n' > "$test_root/signing/password.txt"

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
chmod +x "$test_root/tools/keytool" "$test_root/tools/apksigner" "$test_root/tools/jarsigner"

run_entrypoint() {
    PATH="$test_root/tools:$PATH" \
    MOMENTO_SOURCE_ROOT="$test_root/source" \
    MOMENTO_BUILD_ROOT="$test_root/build" \
    MOMENTO_DISTRIBUTION_ROOT="$test_root/dist" \
    MOMENTO_SIGNING_ROOT="$test_root/signing" \
    "$repository_root/docker/entrypoint_android.sh"
}

if FAKE_KEYTOOL_MODE=two run_entrypoint >/dev/null 2>&1; then
    fail "multiple private-key aliases should fail"
fi

run_entrypoint
[[ -f "$test_root/dist/Momento-Release-1.0.0.apk" ]] || fail "entrypoint did not copy the versioned APK"
[[ -f "$test_root/dist/Momento-Release-1.0.0.aab" ]] || fail "entrypoint did not copy the versioned AAB"

printf 'entrypoint_android.sh tests passed\n'
