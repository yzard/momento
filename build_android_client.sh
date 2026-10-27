#!/usr/bin/env bash
set -euo pipefail

readonly repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly build_dir="$repository_root/build/android"
readonly distribution_dir="$repository_root/dist/android"
readonly version_file="$repository_root/src/android/version.txt"

usage() {
    local script_name
    script_name=$(basename "$0")
    cat <<EOF
Usage:
  $script_name verify [--no-cache]
  $script_name assemble-debug [--no-cache]
  $script_name instrumented-test [--no-cache]
  $script_name shell [--no-cache]
  $script_name release --keystore-dir PATH [--no-cache]
  $script_name --help

Commands:
  verify             Compile the debug variant, run JVM unit tests, and run
                     Android lint inside the builder container.
  assemble-debug     Build an unsigned debug APK inside the builder container.
  instrumented-test  Start a headless Android emulator and run connected tests
                     inside the emulator container. Requires Linux and /dev/kvm.
  shell              Open an interactive shell in the builder container with
                     Java, Gradle, Android SDK, and ADB available.
  release            Build and verify a signed release APK and AAB. This is the
                     only command that accepts or mounts signing material.

Options:
  --keystore-dir PATH
                     Directory containing exactly one direct .jks file and a
                     password.txt file with exactly one non-empty line.
  --no-cache         Rebuild the selected Docker target without layer cache.
                     This is slower and may download the Android toolchain again.
  -h, --help         Show this help and exit successfully.

Outputs:
  Reports and signing state: build/android/reports/ and build/android/state/
  Debug APK:                dist/android/debug/
  Signed release APK/AAB:   dist/android/

Notes:
  The host needs only Docker. Host Java, Gradle, Android SDK, emulator, and ADB
  are never used. "assemble-debug" is an Android build variant and is unrelated
  to Rust debug symbols. After success, temporary workspaces, Gradle caches,
  emulator data and this invocation's disposable BuildKit cache are removed.
  Reusable toolchain images and unchanged exported Apps are retained.
  Failure retains diagnostics. Final APK/AAB and validation reports are retained.
EOF
}

fail() {
    printf 'Error: %s\n' "$1" >&2
    exit 1
}

usage_error() {
    printf 'Error: %s\n\n' "$1" >&2
    usage >&2
    exit 2
}

if (( $# == 1 )) && [[ $1 == -h || $1 == --help ]]; then
    usage
    exit 0
fi
if (( $# == 0 )); then
    usage_error "an Android command is required"
fi

readonly command_name="$1"
shift

keystore_argument=
no_cache=false
while (( $# > 0 )); do
    case "$1" in
        --keystore-dir)
            (( $# >= 2 )) || usage_error "--keystore-dir requires a path"
            [[ -z "$keystore_argument" ]] || usage_error "--keystore-dir may be passed only once"
            keystore_argument=$2
            shift 2
            ;;
        --no-cache)
            [[ "$no_cache" == false ]] || usage_error "--no-cache may be passed only once"
            no_cache=true
            shift
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            usage_error "unsupported option: $1"
            ;;
    esac
done

case "$command_name" in
    verify | assemble-debug | instrumented-test | shell)
        [[ -z "$keystore_argument" ]] || usage_error "--keystore-dir is valid only for release"
        ;;
    release)
        [[ -n "$keystore_argument" ]] || usage_error "release requires --keystore-dir PATH"
        ;;
    *)
        usage_error "unsupported Android command: $command_name"
        ;;
esac

command -v docker >/dev/null 2>&1 || fail "Docker is required for every Android command"
[[ -f "$version_file" ]] || fail "Android version file is missing: $version_file"
readonly android_version="$(<"$version_file")"
[[ "$android_version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || fail "src/android/version.txt must contain a semantic version in major.minor.patch format"

readonly docker_target=$(
    if [[ "$command_name" == instrumented-test ]]; then
        printf 'android-emulator'
    else
        printf 'android-builder'
    fi
)

if [[ "$command_name" == instrumented-test ]]; then
    [[ "$(uname -s)" == Linux ]] || fail "instrumented-test requires a Linux Docker host with KVM"
    [[ -c /dev/kvm ]] || fail "instrumented-test requires /dev/kvm; enable KVM virtualization on the Docker host"
fi

keystore_dir=
if [[ "$command_name" == release ]]; then
    [[ -d "$keystore_argument" ]] || fail "keystore directory does not exist: $keystore_argument"
    keystore_dir="$(realpath "$keystore_argument")"

    shopt -s nullglob
    jks_candidates=("$keystore_dir"/*.jks)
    shopt -u nullglob
    if (( ${#jks_candidates[@]} != 1 )) || [[ ! -f "${jks_candidates[0]:-}" ]]; then
        fail "keystore directory must contain exactly one direct .jks file"
    fi

    readonly keystore_basename="$(basename "${jks_candidates[0]}")"
    readonly keystore_stem="${keystore_basename%.jks}"
    [[ "$keystore_stem" =~ ^[A-Za-z0-9][A-Za-z0-9_-]*$ ]] || fail "keystore filename must use only letters, numbers, hyphens, and underscores before .jks"

    readonly password_file="$keystore_dir/password.txt"
    [[ -f "$password_file" ]] || fail "keystore directory must contain password.txt"
    mapfile -t password_lines < "$password_file"
    if (( ${#password_lines[@]} != 1 )) || [[ -z "${password_lines[0]:-}" ]]; then
        fail "password.txt must contain exactly one non-empty line"
    fi
    unset password_lines
fi

source "$repository_root/docker/lib/build_lifecycle.sh"
momento_build_begin "$repository_root" android
mkdir -p "$distribution_dir" "$build_dir/state"

cleanup_android_build() {
    local reports_dir="$build_dir/reports/$command_name"
    rm -rf -- "$reports_dir" || return 1
    mkdir -p "$reports_dir" "$build_dir/state" || return 1
    if [[ -f "$build_dir/home/.android/debug.keystore" ]]; then
        cp "$build_dir/home/.android/debug.keystore" "$build_dir/state/debug.keystore" || return 1
    fi
    local path
    for path in reports test-results outputs/androidTest-results; do
        if [[ -d "$build_dir/workspace/src/android/app/build/$path" ]]; then
            mkdir -p "$reports_dir/$(dirname "$path")" || return 1
            cp -a "$build_dir/workspace/src/android/app/build/$path" "$reports_dir/$path" || return 1
        fi
    done
    if [[ -f "$build_dir/emulator.log" ]]; then
        cp "$build_dir/emulator.log" "$reports_dir/emulator.log" || return 1
    fi
    cp "$MOMENTO_BUILD_WORK/"*.log "$reports_dir/" || return 1
    rm -rf -- "$build_dir/workspace" "$build_dir/gradle-user-home" "$build_dir/home" "$build_dir/android-avd" || return 1
    rm -f -- "$build_dir/emulator.log" || return 1
    if [[ $command_name == release ]]; then
        rm -rf -- "$distribution_dir/debug" || return 1
    fi
}
readonly toolchain_fingerprint=$(printf '%s\n%s\n%s\n' \
    "$(momento_build_fingerprint "$repository_root" docker/Dockerfile.android docker/entrypoint_android.sh)" "$docker_target" "$(docker version --format '{{.Server.Os}}/{{.Server.Arch}}')" | sha256sum | cut -d ' ' -f1)
readonly invocation_image="momento-android-toolchain:$docker_target-$toolchain_fingerprint"
readonly source_fingerprint=$(printf '%s\n%s\n%s\n' \
    "$(momento_build_fingerprint "$repository_root" src/android build_android_client.sh docker/lib/build_lifecycle.sh)" \
    "$toolchain_fingerprint" \
    "$(if [[ $command_name == release ]]; then momento_build_fingerprint "$keystore_dir" "$keystore_basename" password.txt; fi)" | sha256sum | cut -d ' ' -f1)
readonly receipt="$build_dir/state/$command_name.inputs"
if [[ $no_cache == false && -f "$receipt" && ( $command_name == release || $command_name == assemble-debug ) ]]; then
    mapfile -t previous_inputs < "$receipt"
    if [[ ${previous_inputs[0]:-} == "$source_fingerprint" ]]; then
        if [[ $command_name == release && -s "$distribution_dir/$keystore_stem-$android_version.apk" && -s "$distribution_dir/$keystore_stem-$android_version.aab" ]] &&
            [[ ${previous_inputs[1]:-} == "$(sha256sum "$distribution_dir/$keystore_stem-$android_version.apk" | cut -d ' ' -f1)" && ${previous_inputs[2]:-} == "$(sha256sum "$distribution_dir/$keystore_stem-$android_version.aab" | cut -d ' ' -f1)" ]]; then
            printf 'Reusing unchanged signed Android release artifacts.\n'
            momento_build_success
            exit 0
        elif [[ $command_name == assemble-debug && -s "$distribution_dir/debug/momento-android-$android_version-debug.apk" ]] &&
            [[ ${previous_inputs[1]:-} == "$(sha256sum "$distribution_dir/debug/momento-android-$android_version-debug.apk" | cut -d ' ' -f1)" ]]; then
            printf 'Reusing unchanged Android debug APK.\n'
            momento_build_success
            exit 0
        fi
    fi
fi
mkdir -p "$distribution_dir" "$build_dir/home/.android" "$build_dir/state"
if [[ ! -f "$build_dir/home/.android/debug.keystore" && -f "$build_dir/state/debug.keystore" ]]; then
    cp "$build_dir/state/debug.keystore" "$build_dir/home/.android/debug.keystore"
fi

MOMENTO_BUILD_CLEANUP_HOOK=cleanup_android_build
if [[ $no_cache == true ]] || [[ $(docker image inspect --format '{{index .Config.Labels "io.momento.build.inputs"}}' "$invocation_image" 2>/dev/null || true) != "$toolchain_fingerprint" ]]; then
    momento_build_builder
    docker_build_arguments=(
        buildx build --load --builder "$MOMENTO_BUILD_BUILDER"
        --file "$repository_root/docker/Dockerfile.android"
        --target "$docker_target" --tag "$invocation_image"
        --label "io.momento.build.inputs=$toolchain_fingerprint"
    )
    if [[ $no_cache == true ]]; then docker_build_arguments+=(--no-cache); fi
    docker "${docker_build_arguments[@]}" "$repository_root" 2>&1 | tee "$MOMENTO_BUILD_WORK/toolchain.log"
else
    printf 'Reusing Android toolchain image %s\n' "$invocation_image" | tee "$MOMENTO_BUILD_WORK/toolchain.log"
fi

docker_run_arguments=(
    run
    --rm
    --user "$(id -u):$(id -g)"
    --volume "$repository_root:/workspace:ro"
    --volume "$build_dir:/build"
    --volume "$distribution_dir:/dist"
)

if [[ "$command_name" == instrumented-test ]]; then
    docker_run_arguments+=(
        --device /dev/kvm
        --group-add "$(stat -c '%g' /dev/kvm)"
    )
fi
if [[ "$command_name" == shell ]]; then
    docker_run_arguments+=(--interactive --tty)
fi
if [[ "$command_name" == release ]]; then
    docker_run_arguments+=(--volume "$keystore_dir:/signing:ro")
fi

if [[ $command_name == shell ]]; then
    docker "${docker_run_arguments[@]}" "$invocation_image" "$command_name"
else
    docker "${docker_run_arguments[@]}" "$invocation_image" "$command_name" 2>&1 | tee "$MOMENTO_BUILD_WORK/android.log"
fi

case "$command_name" in
    assemble-debug)
        readonly debug_apk="$distribution_dir/debug/momento-android-$android_version-debug.apk"
        [[ -f "$debug_apk" ]] || fail "container did not produce the expected debug APK: $debug_apk"
        printf 'Android debug APK:\n%s\n' "$debug_apk"
        ;;
    release)
        readonly apk_output="$distribution_dir/$keystore_stem-$android_version.apk"
        readonly aab_output="$distribution_dir/$keystore_stem-$android_version.aab"
        [[ -f "$apk_output" ]] || fail "container did not produce the expected release APK: $apk_output"
        [[ -f "$aab_output" ]] || fail "container did not produce the expected release AAB: $aab_output"
        printf 'Signed Android release artifacts:\n%s\n%s\n' "$apk_output" "$aab_output"
        ;;
    verify)
        printf 'Android compile, JVM tests, and lint passed in Docker.\n'
        ;;
    instrumented-test)
        printf 'Android instrumented tests passed in the Docker emulator.\n'
        ;;
    shell)
        ;;
esac

if [[ $command_name == release ]]; then
    printf '%s\n%s\n%s\n' "$source_fingerprint" "$(sha256sum "$apk_output" | cut -d ' ' -f1)" "$(sha256sum "$aab_output" | cut -d ' ' -f1)" > "$MOMENTO_BUILD_WORK/artifact.inputs"
    momento_build_commit "$MOMENTO_BUILD_WORK/artifact.inputs" "$receipt"
elif [[ $command_name == assemble-debug ]]; then
    printf '%s\n%s\n' "$source_fingerprint" "$(sha256sum "$debug_apk" | cut -d ' ' -f1)" > "$MOMENTO_BUILD_WORK/artifact.inputs"
    momento_build_commit "$MOMENTO_BUILD_WORK/artifact.inputs" "$receipt"
fi
momento_build_success
