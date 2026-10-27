#!/usr/bin/env bash
# Shared lifecycle for repository build entrypoints. Source from Bash with pipefail.

momento_build_lock() {
    local root=$1
    local lock_file="$root/build/.locks/build.lock"
    mkdir -p "$(dirname "$lock_file")"
    if [[ ${MOMENTO_BUILD_LOCK_FD:-} =~ ^[0-9]+$ ]] &&
        [[ $(readlink "/proc/self/fd/$MOMENTO_BUILD_LOCK_FD" 2>/dev/null) == "$lock_file" ]]; then
        return
    fi
    exec {MOMENTO_BUILD_LOCK_FD}>"$lock_file"
    flock "$MOMENTO_BUILD_LOCK_FD"
    export MOMENTO_BUILD_LOCK_FD
}

momento_build_begin() {
    local root=$1 component=$2
    momento_build_lock "$root"
    mkdir -p "$root/build/$component"
    MOMENTO_BUILD_WORK=$(mktemp -d "$root/build/$component/run.XXXXXX")
    mkdir -p "$MOMENTO_BUILD_WORK/tmp"
    export TMPDIR="$MOMENTO_BUILD_WORK/tmp" TEMP="$MOMENTO_BUILD_WORK/tmp" TMP="$MOMENTO_BUILD_WORK/tmp"
    MOMENTO_BUILD_COMPLETE=false
    MOMENTO_BUILD_BUILDER=
    MOMENTO_BUILD_IMAGES=()
    MOMENTO_BUILD_CLEANUP_HOOK=
    MOMENTO_BUILD_COMMIT_SOURCES=()
    MOMENTO_BUILD_COMMIT_DESTINATIONS=()
    trap 'momento_build_exit "$?"' EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM
}

momento_build_builder() {
    MOMENTO_BUILD_BUILDER="momento-$(basename "$MOMENTO_BUILD_WORK" | tr '[:upper:].' '[:lower:]-')-$$"
    printf '%s\n' "$MOMENTO_BUILD_BUILDER" > "$MOMENTO_BUILD_WORK/builder-name"
    docker buildx create --name "$MOMENTO_BUILD_BUILDER" --driver docker-container >/dev/null
}

# Paths are explicit target inputs. Generated and dependency trees never affect source keys.
momento_build_fingerprint() {
    local root=$1
    shift
    (
        cd "$root"
        find "$@" \
            \( -type d \( -name .git -o -name node_modules -o -name target -o -name build -o -name dist -o -name dist-node -o -name .gradle -o -name .turbo -o -name __pycache__ \) -prune \) -o \
            \( -type f ! -name '*.tsbuildinfo' ! -name '*.pyc' -print0 \) |
            LC_ALL=C sort -z |
            while IFS= read -r -d '' file; do
                printf '%s\0%s\0' "$file" "$(stat -c %a "$file")"
                sha256sum "$file"
            done
    ) | sha256sum | cut -d ' ' -f1
}

momento_build_commit() {
    MOMENTO_BUILD_COMMIT_SOURCES+=("$1")
    MOMENTO_BUILD_COMMIT_DESTINATIONS+=("$2")
}

momento_build_oci_valid() {
    local root=$1 directory=$2 fingerprint=$3
    [[ -s "$directory/index.json" && -d "$directory/blobs" && -f "$directory/inputs" ]] &&
        [[ $(<"$directory/inputs") == "$fingerprint" ]] &&
        python3 "$root/docker/lib/validate_oci.py" "$directory" >/dev/null 2>&1
}

momento_build_oci() {
    local root=$1 name=$2 dockerfile=$3 fingerprint=$4
    local destination="$root/dist/docker/$name/$fingerprint"
    if momento_build_oci_valid "$root" "$destination" "$fingerprint"; then
        printf 'Reusing %s artifact %s\n' "$name" "$fingerprint"
        return
    fi
    [[ -n $MOMENTO_BUILD_BUILDER ]] || momento_build_builder
    local staging="$MOMENTO_BUILD_WORK/$name"
    local platform_arguments=()
    if [[ -n ${MOMENTO_BUILD_PLATFORM:-} ]]; then platform_arguments+=(--platform "$MOMENTO_BUILD_PLATFORM"); fi
    docker buildx build --builder "$MOMENTO_BUILD_BUILDER" --provenance=false "${platform_arguments[@]}" \
        --output "type=oci,tar=false,dest=$staging" \
        --build-arg "SOURCE_REPOSITORY=${SOURCE_REPOSITORY:-https://github.com/yzard/momento}" \
        -f "$root/docker/$dockerfile" "$root" 2>&1 | tee "$MOMENTO_BUILD_WORK/$name.log"
    python3 "$root/docker/lib/validate_oci.py" "$staging" || return 1
    mkdir -p "$(dirname "$destination")"
    # An incomplete export for this exact fingerprint is disposable, never another version.
    rm -rf -- "$destination"
    mv "$staging" "$destination"
    printf '%s\n' "$fingerprint" > "$destination/inputs"
}

momento_build_success() {
    MOMENTO_BUILD_COMPLETE=true
}

momento_build_exit() {
    local status=$1
    trap - EXIT INT TERM
    if [[ $status == 0 && $MOMENTO_BUILD_COMPLETE == true ]]; then
        if [[ -n $MOMENTO_BUILD_CLEANUP_HOOK ]]; then
            "$MOMENTO_BUILD_CLEANUP_HOOK" || status=1
        fi
        if [[ $status == 0 && -n $MOMENTO_BUILD_BUILDER ]]; then
            docker buildx rm --force "$MOMENTO_BUILD_BUILDER" || status=1
        fi
        if [[ $status == 0 ]]; then
            local image
            for image in "${MOMENTO_BUILD_IMAGES[@]}"; do
                docker image rm "$image" >/dev/null || status=1
            done
        fi
        if [[ $status == 0 ]]; then
            local index
            for ((index = 0; index < ${#MOMENTO_BUILD_COMMIT_SOURCES[@]}; index++)); do
                mkdir -p "$(dirname "${MOMENTO_BUILD_COMMIT_DESTINATIONS[index]}")" || { status=1; break; }
                mv "${MOMENTO_BUILD_COMMIT_SOURCES[index]}" "${MOMENTO_BUILD_COMMIT_DESTINATIONS[index]}" || { status=1; break; }
            done
        fi
        if [[ $status == 0 ]]; then
            rm -rf -- "$MOMENTO_BUILD_WORK" || status=1
        fi
    fi
    if [[ $status != 0 ]]; then
        printf 'Build did not complete cleanly. Diagnostics retained in %s\n' "$MOMENTO_BUILD_WORK" >&2
        if [[ -n $MOMENTO_BUILD_BUILDER ]]; then
            printf 'BuildKit diagnostics and cache retained in builder %s\n' "$MOMENTO_BUILD_BUILDER" >&2
        fi
    fi
    exit "$status"
}
