#!/bin/sh

PUID=${PUID:-1000}
PGID=${PGID:-1000}
UMASK=${UMASK:-022}

echo "Starting with PUID=$PUID, PGID=$PGID, UMASK=$UMASK"

if [ -n "$TZ" ]; then
    echo "Setting timezone to $TZ"
    ln -snf /usr/share/zoneinfo/$TZ /etc/localtime && echo $TZ > /etc/timezone
fi

if ! getent group momento > /dev/null 2>&1; then
    addgroup -g "$PGID" momento
fi

if ! id momento > /dev/null 2>&1; then
    adduser -u "$PUID" -G momento -h /app -D momento
fi

umask "$UMASK"

mkdir -p /data/originals /data/thumbnails /data/imports /data/previews /data/trash /data/albums

chown -R momento:momento /data

if [ ! -f /data/config.yaml ]; then
    su-exec momento:momento /app/momento-api --init-config
fi

echo "Running as user momento ($(id -u momento):$(id -g momento))"

exec su-exec momento:momento "$@"
