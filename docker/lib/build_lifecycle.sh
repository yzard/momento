#!/usr/bin/env bash
# Shared lifecycle for repository build entrypoints. Source from Bash with pipefail.

momento_build_state_root() {
    local root=$1
    printf '%s/momento-build/%s\n' "${XDG_STATE_HOME:-$HOME/.local/state}" "$(printf '%s' "$root" | sha256sum | cut -d ' ' -f1)"
}

momento_build_lock() {
    local root=$1
    MOMENTO_BUILD_STATE=$(realpath -m "$(momento_build_state_root "$root")")
    case "$MOMENTO_BUILD_STATE/" in
        "$root/build/"*|"$root/dist/"*) printf 'Persistent build state must be outside build/ and dist/\n' >&2; return 1 ;;
    esac
    local lock_file="$MOMENTO_BUILD_STATE/build.lock"
    mkdir -p "$MOMENTO_BUILD_STATE"
    chmod 700 "$MOMENTO_BUILD_STATE"
    MOMENTO_BUILD_OUTERMOST=true
    if [[ ${MOMENTO_BUILD_LOCK_FD:-} =~ ^[0-9]+$ ]] &&
        [[ $(readlink "/proc/self/fd/$MOMENTO_BUILD_LOCK_FD" 2>/dev/null) == "$lock_file" ]]; then
        MOMENTO_BUILD_OUTERMOST=false
        return
    fi
    exec {MOMENTO_BUILD_LOCK_FD}>"$lock_file"
    flock "$MOMENTO_BUILD_LOCK_FD"
    export MOMENTO_BUILD_LOCK_FD
}

momento_build_begin() {
    local root=$1 component=$2
    [[ ! -L "$root/build" ]] || { printf 'Build directory must not be a symlink\n' >&2; return 1; }
    momento_build_lock "$root"
    MOMENTO_BUILD_ROOT=$root
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
    # Preserve existing reports and signing/receipt state before retiring the old layout.
    local name kind previous
    for name in android frontend docker; do
        for kind in state reports; do
            previous="$root/build/$name/$kind"
            if [[ -d $previous && ! -L $previous ]]; then
                mkdir -p "$MOMENTO_BUILD_STATE/$name/$kind"
                cp -an "$previous/." "$MOMENTO_BUILD_STATE/$name/$kind/"
            fi
        done
    done
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

momento_build_runtime_image() {
    printf 'momento-build-%s:%s\n' "$1" "$2"
}

momento_build_import_oci() {
    local root=$1 name=$2 fingerprint=$3 directory=$4 image
    image=$(momento_build_runtime_image "$name" "$fingerprint")
    [[ -n $MOMENTO_BUILD_BUILDER ]] || momento_build_builder
    printf 'FROM retained-artifact\n' > "$MOMENTO_BUILD_WORK/import.Dockerfile"
    docker buildx build --builder "$MOMENTO_BUILD_BUILDER" --load --provenance=false \
        --build-context "retained-artifact=oci-layout://$directory" \
        --label "io.momento.build.inputs=$fingerprint" --tag "$image" \
        -f "$MOMENTO_BUILD_WORK/import.Dockerfile" "$root" \
        2>&1 | tee "$MOMENTO_BUILD_WORK/import-$name.log"
}

# Retain reusable artifacts in Docker. OCI directories exist only during this invocation.
momento_build_oci() {
    local root=$1 name=$2 dockerfile=$3 fingerprint=$4
    local image destination legacy
    image=$(momento_build_runtime_image "$name" "$fingerprint")
    destination="$MOMENTO_BUILD_WORK/oci/$name/$fingerprint"
    legacy="$root/dist/docker/$name/$fingerprint"
    if [[ $(docker image inspect --format '{{index .Config.Labels "io.momento.build.inputs"}}' "$image" 2>/dev/null || true) != "$fingerprint" ]]; then
        if [[ -e $legacy ]]; then
            momento_build_oci_valid "$root" "$legacy" "$fingerprint" || { printf 'Invalid legacy Docker export: %s\n' "$legacy" >&2; return 1; }
            momento_build_import_oci "$root" "$name" "$fingerprint" "$legacy"
        else
            [[ -n $MOMENTO_BUILD_BUILDER ]] || momento_build_builder
            local platform_arguments=()
            if [[ -n ${MOMENTO_BUILD_PLATFORM:-} ]]; then platform_arguments+=(--platform "$MOMENTO_BUILD_PLATFORM"); fi
            docker buildx build --builder "$MOMENTO_BUILD_BUILDER" --load --provenance=false "${platform_arguments[@]}" \
                --tag "$image" --label "io.momento.build.inputs=$fingerprint" \
                --build-arg "SOURCE_REPOSITORY=${SOURCE_REPOSITORY:-https://github.com/yzard/momento}" \
                -f "$root/docker/$dockerfile" "$root" 2>&1 | tee "$MOMENTO_BUILD_WORK/$name.log"
        fi
    else
        printf 'Reusing %s image %s\n' "$name" "$image"
    fi
    docker image save --output "$MOMENTO_BUILD_WORK/$name.tar" "$image"
    python3 "$root/docker/lib/docker_archive_oci.py" "$MOMENTO_BUILD_WORK/$name.tar" "$destination"
    python3 "$root/docker/lib/validate_oci.py" "$destination"
    rm -- "$MOMENTO_BUILD_WORK/$name.tar"
}

momento_build_migrate_oci() {
    local root=$1 directory name fingerprint image
    [[ ! -L "$root/dist/docker" ]] || { printf 'Docker export directory must not be a symlink\n' >&2; return 1; }
    [[ -d "$root/dist/docker" ]] || return 0
    for name in api-runtime frontend llm-runtime; do
        for directory in "$root/dist/docker/$name/"*; do
            [[ -d $directory ]] || continue
            fingerprint=$(basename "$directory")
            momento_build_oci_valid "$root" "$directory" "$fingerprint" || { printf 'Invalid legacy Docker export: %s\n' "$directory" >&2; return 1; }
            image=$(momento_build_runtime_image "$name" "$fingerprint")
            if [[ $(docker image inspect --format '{{index .Config.Labels "io.momento.build.inputs"}}' "$image" 2>/dev/null || true) != "$fingerprint" ]]; then
                momento_build_import_oci "$root" "$name" "$fingerprint" "$directory"
            fi
        done
    done
    # Delete only validated legacy exports after their Docker images are available.
    for name in api-runtime frontend llm-runtime; do rm -rf -- "$root/dist/docker/$name"; done
    rmdir "$root/dist/docker" 2>/dev/null || { printf 'Docker export directory contains unrecognized files\n' >&2; return 1; }
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
            if [[ $MOMENTO_BUILD_OUTERMOST == true ]]; then
                [[ ! -L "$MOMENTO_BUILD_ROOT/build" ]] && rm -rf -- "$MOMENTO_BUILD_ROOT/build" || status=1
            else
                rm -rf -- "$MOMENTO_BUILD_WORK" || status=1
            fi
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
