#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
Usage:
  build-momento-android verify
  build-momento-android assemble-debug
  build-momento-android instrumented-test
  build-momento-android shell
  build-momento-android release
  build-momento-android --help

Commands:
  verify             Assemble the debug application, run JVM unit tests, and lint.
  assemble-debug     Assemble and export the debug APK.
  instrumented-test  Start the bundled headless emulator and run connected tests.
  shell              Open Bash in the staged Android project.
  release            Assemble, sign, verify, and export the release APK and AAB.

This entrypoint is invoked by build_android_client.sh. Signing environment and
/signing are required only for release.
EOF
}

fail() {
    printf 'Error: %s\n' "$1" >&2
    exit 1
}

if (( $# == 1 )) && [[ $1 == -h || $1 == --help ]]; then
    usage
    exit 0
fi
if (( $# != 1 )); then
    usage >&2
    exit 2
fi

readonly command_name="$1"
case "$command_name" in
    verify | assemble-debug | instrumented-test | shell | release)
        ;;
    *)
        printf 'Error: unsupported Android container command: %s\n\n' "$command_name" >&2
        usage >&2
        exit 2
        ;;
esac

readonly source_root="${MOMENTO_SOURCE_ROOT:?MOMENTO_SOURCE_ROOT is required}"
readonly build_root="${MOMENTO_BUILD_ROOT:?MOMENTO_BUILD_ROOT is required}"
readonly distribution_root="${MOMENTO_DISTRIBUTION_ROOT:?MOMENTO_DISTRIBUTION_ROOT is required}"
readonly android_source_dir="$source_root/src/android"
readonly android_test_dir="$source_root/tests/android"
readonly workspace_dir="$build_root/workspace"
readonly android_project_dir="$workspace_dir/src/android"
readonly version_file="$android_source_dir/version.txt"

[[ -d "$android_source_dir" ]] || fail "Android source directory is missing"
[[ -d "$android_test_dir" ]] || fail "Android test directory is missing"
[[ -f "$version_file" ]] || fail "Android version file is missing"
readonly android_version="$(<"$version_file")"
[[ "$android_version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || fail "version.txt must contain a semantic version in major.minor.patch format"

rm -rf "$workspace_dir"
mkdir -p \
    "$workspace_dir/src/android" \
    "$workspace_dir/tests" \
    "$distribution_root" \
    "$build_root/home/.android" \
    "$build_root/gradle-user-home"
tar -C "$android_source_dir" \
    --exclude=.gradle \
    --exclude=app/build \
    --exclude=local.properties \
    -cf - . | tar -C "$android_project_dir" -xf -
cp -a "$android_test_dir" "$workspace_dir/tests/android"
chmod +x "$android_project_dir/gradlew"

export HOME="$build_root/home"
export GRADLE_USER_HOME="$build_root/gradle-user-home"
export GRADLE_OPTS="-Duser.home=$build_root/home"
export JAVA_TOOL_OPTIONS="-Duser.home=$build_root/home"

run_gradle() {
    (
        cd "$android_project_dir"
        ./gradlew --no-daemon "$@"
    )
}

configure_release_signing() {
    readonly signing_root="${MOMENTO_SIGNING_ROOT:?MOMENTO_SIGNING_ROOT is required for release}"
    [[ -d "$signing_root" ]] || fail "signing directory is missing"

    shopt -s nullglob
    local jks_candidates=("$signing_root"/*.jks)
    shopt -u nullglob
    if (( ${#jks_candidates[@]} != 1 )) || [[ ! -f "${jks_candidates[0]:-}" ]]; then
        fail "signing directory must contain exactly one direct .jks file"
    fi

    keystore_file="${jks_candidates[0]}"
    keystore_basename="$(basename "$keystore_file")"
    keystore_stem="${keystore_basename%.jks}"
    [[ "$keystore_stem" =~ ^[A-Za-z0-9][A-Za-z0-9_-]*$ ]] || fail "keystore filename contains unsafe characters"

    local password_file="$signing_root/password.txt"
    [[ -f "$password_file" ]] || fail "signing directory must contain password.txt"
    local password_lines
    mapfile -t password_lines < "$password_file"
    if (( ${#password_lines[@]} != 1 )) || [[ -z "${password_lines[0]:-}" ]]; then
        fail "password.txt must contain exactly one non-empty line"
    fi
    keystore_password="${password_lines[0]}"

    local keytool_output
    keytool_output="$(LC_ALL=C MOMENTO_KEYSTORE_PASSWORD="$keystore_password" keytool \
        -list -v -keystore "$keystore_file" -storepass:env MOMENTO_KEYSTORE_PASSWORD 2>&1)" \
        || fail "unable to open keystore with password.txt"
    local private_key_aliases
    mapfile -t private_key_aliases < <(printf '%s\n' "$keytool_output" | awk '
        /^Alias name: / { alias = substr($0, 13) }
        /^Entry type: PrivateKeyEntry$/ { print alias }
    ')
    if (( ${#private_key_aliases[@]} != 1 )) || [[ -z "${private_key_aliases[0]:-}" ]]; then
        fail "keystore must contain exactly one PrivateKeyEntry"
    fi
    key_alias="${private_key_aliases[0]}"
    [[ "$key_alias" =~ ^[A-Za-z0-9][A-Za-z0-9_.-]*$ ]] || fail "keystore key alias contains unsafe characters"

    export ORG_GRADLE_PROJECT_momentoReleaseStoreFile="$keystore_file"
    export ORG_GRADLE_PROJECT_momentoReleaseStorePassword="$keystore_password"
    export ORG_GRADLE_PROJECT_momentoReleaseKeyAlias="$key_alias"
    export ORG_GRADLE_PROJECT_momentoReleaseKeyPassword="$keystore_password"
}

export_debug_apk() {
    local apk_source="$android_project_dir/app/build/outputs/apk/debug/app-debug.apk"
    [[ -f "$apk_source" ]] || fail "Gradle did not produce a debug APK"

    local debug_distribution="$distribution_root/debug"
    mkdir -p "$debug_distribution"
    rm -f "$debug_distribution"/*.apk
    cp "$apk_source" "$debug_distribution/momento-android-$android_version-debug.apk"
}

run_instrumented_tests() {
    export ANDROID_AVD_HOME="$build_root/android-avd"
    mkdir -p "$ANDROID_AVD_HOME"
    if [[ ! -f "$ANDROID_AVD_HOME/momento-test.avd/config.ini" ]]; then
        printf 'no\n' | avdmanager create avd \
            --force \
            --name momento-test \
            --package 'system-images;android-35;google_apis;x86_64' \
            --device pixel_6
    fi

    local emulator_log="$build_root/emulator.log"
    emulator \
        -avd momento-test \
        -no-window \
        -no-audio \
        -no-boot-anim \
        -no-metrics \
        -no-snapshot \
        -wipe-data \
        -gpu swiftshader_indirect \
        -accel on \
        >"$emulator_log" 2>&1 &
    emulator_pid=$!
    trap stop_emulator EXIT INT TERM

    sleep 1
    if ! kill -0 "$emulator_pid" 2>/dev/null; then
        wait "$emulator_pid" 2>/dev/null || true
        fail "Android emulator exited during startup; see $emulator_log"
    fi

    if ! timeout 60 adb wait-for-device; then
        fail "Android emulator did not expose ADB within 60 seconds; see $emulator_log"
    fi

    local boot_attempt
    for ((boot_attempt = 1; boot_attempt <= 90; boot_attempt++)); do
        if [[ "$(adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" == 1 ]]; then
            adb shell settings put global window_animation_scale 0
            adb shell settings put global transition_animation_scale 0
            adb shell settings put global animator_duration_scale 0
            run_gradle :app:connectedDebugAndroidTest
            return
        fi
        sleep 2
    done

    fail "Android emulator did not finish booting within 180 seconds; see $emulator_log"
}

stop_emulator() {
    if [[ -n "${emulator_pid:-}" ]]; then
        adb emu kill >/dev/null 2>&1 || true
        kill "$emulator_pid" 2>/dev/null || true
        wait "$emulator_pid" 2>/dev/null || true
    fi
}

case "$command_name" in
    verify)
        run_gradle :app:assembleDebug :app:testDebugUnitTest :app:lintDebug
        ;;
    assemble-debug)
        run_gradle :app:assembleDebug
        export_debug_apk
        ;;
    instrumented-test)
        run_instrumented_tests
        ;;
    shell)
        cd "$android_project_dir"
        exec bash
        ;;
    release)
        configure_release_signing
        run_gradle :app:assembleRelease :app:bundleRelease

        readonly apk_source="$android_project_dir/app/build/outputs/apk/release/app-release.apk"
        readonly aab_source="$android_project_dir/app/build/outputs/bundle/release/app-release.aab"
        [[ -f "$apk_source" ]] || fail "Gradle did not produce a release APK"
        [[ -f "$aab_source" ]] || fail "Gradle did not produce a release AAB"

        readonly apk_output="$distribution_root/$keystore_stem-$android_version.apk"
        readonly aab_output="$distribution_root/$keystore_stem-$android_version.aab"
        rm -f "$distribution_root"/*.apk "$distribution_root"/*.aab
        cp "$apk_source" "$apk_output"
        cp "$aab_source" "$aab_output"
        apksigner verify --verbose "$apk_output" >/dev/null
        jarsigner -verify "$aab_output" >/dev/null
        ;;
esac
