#!/bin/sh
set -eu

PUID=${PUID:-1000}
PGID=${PGID:-1000}
UMASK=${UMASK:-022}

if [ -n "${TZ:-}" ]; then
    ln -snf "/usr/share/zoneinfo/$TZ" /etc/localtime
    printf '%s\n' "$TZ" > /etc/timezone
fi

umask "$UMASK"
mkdir -p /config /data/llm/cache/triton /data/llm/queue /data/logs
cp -a /opt/triton-cache/. /data/llm/cache/triton/
chown -R "$PUID:$PGID" /config
chown -R "$PUID:$PGID" /data/llm /data/logs

if [ ! -f /config/config_llm.toml ]; then
    gosu "$PUID:$PGID" env HOME=/data /app/llm-service \
        -c /config/config_llm.toml --init-config
fi
chmod 600 /config/config_llm.toml

exec gosu "$PUID:$PGID" env HOME=/data "$@"
