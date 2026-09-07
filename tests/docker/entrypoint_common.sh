#!/bin/sh
# Executed inside either service image by the mirrored entrypoint test wrappers.
set -eu
if [ "$1" != --inside ]; then
    service=$1
    case "$service" in
        api) image=zhuoyin/momento:latest; entrypoint=entrypoint.sh ;;
        llm) image=zhuoyin/momento-llm-service:latest; entrypoint=entrypoint_llm.sh ;;
        *) exit 2 ;;
    esac
    repository=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
    volume="momento-entrypoint-$service-test-$$"
    docker volume create "$volume" >/dev/null
    trap 'docker volume rm "$volume" >/dev/null' EXIT
    docker run --rm --network none --mount "type=volume,src=$volume,dst=/data,volume-nocopy" \
        --mount "type=bind,src=$repository/docker/$entrypoint,dst=/$entrypoint,readonly" \
        --mount "type=bind,src=$repository/docker/entrypoint_common.sh,dst=/entrypoint_common.sh,readonly" \
        --mount "type=bind,src=$repository/tests/docker/entrypoint_common.sh,dst=/entrypoint-test.sh,readonly" \
        --entrypoint sh "$image" /entrypoint-test.sh --inside "$service"
    exit 0
fi
service=$2
case "$service" in
    api) entrypoint=/entrypoint.sh; config=/data/config.toml; content=/data/originals ;;
    llm) entrypoint=/entrypoint_llm.sh; config=/data/config_llm.toml; content=/data/llm/queue ;;
    *) exit 2 ;;
esac
export PUID=1002 PGID=1003 UMASK=002 TZ=UTC

fail() { printf 'FAIL: %s\n' "$1" >&2; exit 1; }
expect_failure() {
    expected=$1
    shift
    if output=$("$@" 2>&1); then fail "unexpected success: $*"; fi
    case "$output" in *"$expected"*) ;; *) fail "$output" ;; esac
}
start() { sh "$entrypoint" sh -c 'test "$(id -u)" = 1002 && test "$(id -g)" = 1003 && touch /data/runtime-write'; }

# Empty mounted volume: initialize directories/config and drop privileges.
start
test -s "$config" || fail 'missing generated config'
test "$(stat -c %u "$config")" = 1002 || fail 'wrong config ownership'
checksum=$(sha256sum "$config")
chmod 640 "$config"
# An inaccessible nested file must neither be traversed nor silently chowned.
mkdir "$content/private"
chmod 700 "$content/private"
touch "$content/private/root-owned"
before=$(stat -c '%u:%g:%a:%Z' "$content/private/root-owned")
start
test "$(stat -c '%u:%g:%a:%Z' "$content/private/root-owned")" = "$before" || fail 'existing file modified'
test "$(sha256sum "$config")" = "$checksum" || fail 'existing config overwritten'
test "$(stat -c %a "$config")" = 640 || fail 'existing config permissions changed'

# Missing directories are initialized without repairing existing ones.
rmdir /data/logs
start
test "$(stat -c %u /data/logs)" = 1002 || fail 'new directory ownership'
chown 0:0 /data/logs
chmod 700 /data/logs
expect_failure 'path=/data/logs uid=1002 gid=1003' start
test "$(stat -c %u /data/logs)" = 0 || fail 'existing directory silently repaired'
chown 1002:1003 /data/logs
chmod 775 /data/logs

chown 0:0 "$config"
chmod 600 "$config"
expect_failure "path=$config" start
chown 1002:1003 "$config"
expect_failure 'uid=1004' env PUID=1004 PGID=1004 sh "$entrypoint" true
expect_failure 'Invalid PUID/PGID' env PUID=bad sh "$entrypoint" true
expect_failure 'Invalid PUID/PGID' env PGID=1:2 sh "$entrypoint" true
expect_failure 'Invalid UMASK' env UMASK=999 sh "$entrypoint" true

rmdir /data/logs
ln -s "$content" /data/logs
expect_failure 'required directory must not be a symlink' start
unlink /data/logs
start

if [ "$service" = api ]; then
    touch /data/database.sqlite
    chmod 600 /data/database.sqlite
    expect_failure 'path=/data/database.sqlite' start
else
    test "$(stat -c %u /data/llm/cache/triton)" = 1002 || fail 'cache ownership'
    test -z "$(find /data/llm/cache/triton -type f ! -user 1002 -print -quit)" || fail 'root-owned seeded cache'
fi
printf '%s entrypoint regression tests passed\n' "$service"
