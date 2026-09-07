#!/bin/sh

startup_error() {
    printf 'Startup permission error: path=%s uid=%s gid=%s reason=%s. Fix the mounted path permissions explicitly; existing data is not recursively modified.\n' \
        "$1" "$PUID" "$PGID" "$2" >&2
    exit 1
}

run_as_service() {
    "$privilege_drop" "$PUID:$PGID" env HOME=/data "$@"
}

initialize_identity() {
    privilege_drop=$1
    PUID=${PUID:-1000}
    PGID=${PGID:-1000}
    UMASK=${UMASK:-022}
    for identity in "$PUID" "$PGID"; do
        case "$identity" in
            ''|*[!0-9]*) printf 'Invalid PUID/PGID: expected numeric IDs\n' >&2; exit 1 ;;
        esac
    done
    case "$UMASK" in
        [0-7][0-7][0-7]|0[0-7][0-7][0-7]) ;;
        *) printf 'Invalid UMASK: expected three octal digits\n' >&2; exit 1 ;;
    esac
    printf 'Starting with PUID=%s, PGID=%s, UMASK=%s\n' "$PUID" "$PGID" "$UMASK"
    if [ -n "${TZ:-}" ]; then
        printf 'Setting timezone to %s\n' "$TZ"
        ln -snf "/usr/share/zoneinfo/$TZ" /etc/localtime
        printf '%s\n' "$TZ" > /etc/timezone
    fi
    umask "$UMASK"
}

prepare_data_directories() {
    # A new bind mount / named volume can be an empty root-owned directory.
    # Initialize only that empty root, never scan or repair an existing data tree.
    if [ -d /data ] && [ ! -L /data ]; then
        data_empty=true
        for entry in /data/* /data/.[!.]* /data/..?*; do
            if [ -e "$entry" ] || [ -L "$entry" ]; then
                data_empty=false
                break
            fi
        done
        if [ "$data_empty" = true ]; then
            chown "$PUID:$PGID" /data || startup_error /data 'cannot initialize empty volume ownership'
        fi
    fi
    for directory in /data "$@"; do
        if [ -L "$directory" ]; then
            startup_error "$directory" 'required directory must not be a symlink'
        fi
        if [ ! -e "$directory" ]; then
            mkdir "$directory" || startup_error "$directory" 'cannot create directory'
            chown "$PUID:$PGID" "$directory" || startup_error "$directory" 'cannot initialize new directory ownership'
        fi
        run_as_service sh -c 'test -d "$1" && test -r "$1" && test -w "$1" && test -x "$1"' sh "$directory" \
            || startup_error "$directory" 'runtime user requires directory read/write/search access'
    done
    printf 'Startup directory permission checks completed (no recursive ownership changes)\n'
}

check_existing_file() {
    if [ -e "$1" ] || [ -L "$1" ]; then
        run_as_service sh -c 'test -f "$1" && test -r "$1"' sh "$1" \
            || startup_error "$1" 'runtime user cannot read required file'
        if [ "$2" = writable ]; then
            run_as_service test -w "$1" \
                || startup_error "$1" 'runtime user cannot write required file'
        fi
    fi
}
