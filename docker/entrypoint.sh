#!/bin/sh
set -eu

PUID=${PUID:-1000}
PGID=${PGID:-1000}
UMASK=${UMASK:-022}

echo "Starting with PUID=$PUID, PGID=$PGID, UMASK=$UMASK"

if [ -n "${TZ:-}" ]; then
    echo "Setting timezone to $TZ"
    ln -snf /usr/share/zoneinfo/$TZ /etc/localtime && echo $TZ > /etc/timezone
fi

umask "$UMASK"

mkdir -p \
    /data/albums \
    /data/imports \
    /data/llm \
    /data/logs \
    /data/originals \
    /data/previews \
    /data/thumbnails \
    /data/thumbnails_tiny \
    /data/trash \
    /data/webdav

chown -R "$PUID:$PGID" /data

if [ ! -f /data/config.toml ]; then
    su-exec "$PUID:$PGID" env HOME=/data /app/momento-api -c /data/config.toml --init-config
fi

echo "Running as user $PUID:$PGID"

exec su-exec "$PUID:$PGID" env HOME=/data "$@"
