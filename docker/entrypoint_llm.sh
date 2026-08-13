#!/bin/sh

PUID=${PUID:-1000}
PGID=${PGID:-1000}
UMASK=${UMASK:-022}

if [ -n "$TZ" ]; then
    ln -snf "/usr/share/zoneinfo/$TZ" /etc/localtime
    printf '%s\n' "$TZ" > /etc/timezone
fi

if ! getent group momento > /dev/null 2>&1; then
    addgroup -g "$PGID" momento
fi

if ! id momento > /dev/null 2>&1; then
    adduser -u "$PUID" -G momento -h /app -D momento
fi

umask "$UMASK"
mkdir -p /data
chown -R momento:momento /data

exec su-exec momento:momento "$@"
