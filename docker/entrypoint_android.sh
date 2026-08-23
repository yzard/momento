#!/usr/bin/env bash
set -euo pipefail

readonly source_root="${MOMENTO_SOURCE_ROOT:?MOMENTO_SOURCE_ROOT is required}"
readonly build_root="${MOMENTO_BUILD_ROOT:?MOMENTO_BUILD_ROOT is required}"
readonly distribution_root="${MOMENTO_DISTRIBUTION_ROOT:?MOMENTO_DISTRIBUTION_ROOT is required}"
readonly signing_root="${MOMENTO_SIGNING_ROOT:?MOMENTO_SIGNING_ROOT is required}"
readonly android_source_dir="$source_root/src/android"
readonly android_test_dir="$source_root/tests/android"
readonly workspace_dir="$build_root/workspace"
readonly android_project_dir="$workspace_dir/src/android"
readonly version_file="$android_source_dir/version.txt"

fail() {
    printf 'Error: %s\n' "$1" >&2
    exit 1
}

[[ -d "$android_source_dir" ]] || fail "Android source directory is missing"
[[ -d "$android_test_dir" ]] || fail "Android test directory is missing"
[[ -d "$signing_root" ]] || fail "signing directory is missing"
[[ -f "$version_file" ]] || fail "Android version file is missing"
readonly android_version="$(<"$version_file")"
[[ "$android_version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || fail "version.txt must contain a semantic version in major.minor.patch format"

shopt -s nullglob
jks_candidates=("$signing_root"/*.jks)
shopt -u nullglob
if (( ${#jks_candidates[@]} != 1 )) || [[ ! -f "${jks_candidates[0]:-}" ]]; then
    fail "signing directory must contain exactly one direct .jks file"
fi

readonly keystore_file="${jks_candidates[0]}"
readonly keystore_basename="$(basename "$keystore_file")"
readonly keystore_stem="${keystore_basename%.jks}"
[[ "$keystore_stem" =~ ^[A-Za-z0-9][A-Za-z0-9_-]*$ ]] || fail "keystore filename contains unsafe characters"

readonly password_file="$signing_root/password.txt"
[[ -f "$password_file" ]] || fail "signing directory must contain password.txt"
mapfile -t password_lines < "$password_file"
if (( ${#password_lines[@]} != 1 )) || [[ -z "${password_lines[0]:-}" ]]; then
    fail "password.txt must contain exactly one non-empty line"
fi
readonly keystore_password="${password_lines[0]}"

keytool_output="$(LC_ALL=C MOMENTO_KEYSTORE_PASSWORD="$keystore_password" keytool \
    -list -v -keystore "$keystore_file" -storepass:env MOMENTO_KEYSTORE_PASSWORD 2>&1)" \
    || fail "unable to open keystore with password.txt"
mapfile -t private_key_aliases < <(printf '%s\n' "$keytool_output" | awk '
    /^Alias name: / { alias = substr($0, 13) }
    /^Entry type: PrivateKeyEntry$/ { print alias }
')
if (( ${#private_key_aliases[@]} != 1 )) || [[ -z "${private_key_aliases[0]:-}" ]]; then
    fail "keystore must contain exactly one PrivateKeyEntry"
fi
readonly key_alias="${private_key_aliases[0]}"
[[ "$key_alias" =~ ^[A-Za-z0-9][A-Za-z0-9_.-]*$ ]] || fail "keystore key alias contains unsafe characters"

rm -rf "$workspace_dir"
mkdir -p "$workspace_dir/src/android" "$workspace_dir/tests" "$distribution_root" "$build_root/home/.android" "$build_root/gradle-user-home"
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
export ORG_GRADLE_PROJECT_momentoReleaseStoreFile="$keystore_file"
export ORG_GRADLE_PROJECT_momentoReleaseStorePassword="$keystore_password"
export ORG_GRADLE_PROJECT_momentoReleaseKeyAlias="$key_alias"
export ORG_GRADLE_PROJECT_momentoReleaseKeyPassword="$keystore_password"

(
    cd "$android_project_dir"
    ./gradlew --no-daemon :app:assembleRelease :app:bundleRelease
)

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
