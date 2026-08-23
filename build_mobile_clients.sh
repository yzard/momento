#!/usr/bin/env bash
set -euo pipefail

readonly repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly build_dir="$repository_root/build/android"
readonly distribution_dir="$repository_root/dist/mobile/android"
readonly builder_image="momento-android-builder:local"
readonly version_file="$repository_root/src/android/version.txt"

fail() {
    printf 'Error: %s\n' "$1" >&2
    exit 1
}

if (( $# != 1 )); then
    fail "usage: $0 <keystore directory>"
fi

command -v docker >/dev/null 2>&1 || fail "Docker is required to build mobile clients"

[[ -f "$version_file" ]] || fail "Android version file is missing: $version_file"
readonly android_version="$(<"$version_file")"
[[ "$android_version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || fail "src/android/version.txt must contain a semantic version in major.minor.patch format"

readonly keystore_argument="$1"
[[ -d "$keystore_argument" ]] || fail "keystore directory does not exist: $keystore_argument"
readonly keystore_dir="$(realpath "$keystore_argument")"

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

mkdir -p "$build_dir"
rm -rf "$distribution_dir"
mkdir -p "$distribution_dir"

docker build \
    --file "$repository_root/docker/Dockerfile.android" \
    --tag "$builder_image" \
    "$repository_root"

docker run --rm \
    --user "$(id -u):$(id -g)" \
    --volume "$repository_root:/workspace:ro" \
    --volume "$build_dir:/build" \
    --volume "$distribution_dir:/dist" \
    --volume "$keystore_dir:/signing:ro" \
    "$builder_image"

readonly apk_output="$distribution_dir/$keystore_stem-$android_version.apk"
readonly aab_output="$distribution_dir/$keystore_stem-$android_version.aab"
[[ -f "$apk_output" ]] || fail "container did not produce the expected APK"
[[ -f "$aab_output" ]] || fail "container did not produce the expected AAB"

printf 'Signed Android release artifacts:\n%s\n%s\n' "$apk_output" "$aab_output"
