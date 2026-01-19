#!/bin/sh

# Set default values if not provided
PUID=${PUID:-1000}
PGID=${PGID:-1000}
UMASK=${UMASK:-022}

echo "Starting with PUID=$PUID, PGID=$PGID, UMASK=$UMASK"

# Set timezone if TZ is provided
if [ -n "$TZ" ]; then
    echo "Setting timezone to $TZ"
    cp /usr/share/zoneinfo/$TZ /etc/localtime 2>/dev/null || true
    echo "$TZ" > /etc/timezone 2>/dev/null || true
fi

# Create group if it doesn't exist
if ! getent group momento > /dev/null 2>&1; then
    addgroup -g "$PGID" momento
fi

# Create user if it doesn't exist
if ! id momento > /dev/null 2>&1; then
    adduser -D -u "$PUID" -G momento -h /app momento
fi

# Set umask
umask "$UMASK"

# Create data directories
mkdir -p /data/originals /data/thumbnails /data/imports

# Change ownership of data directory
chown -R momento:momento /data 2>/dev/null || true

# Create default config if it doesn't exist
if [ ! -f /data/config.yaml ]; then
    su-exec momento:momento python -c "from pathlib import Path; from momento_api.config import save_default_config; save_default_config(Path('/data/config.yaml'))"
fi

echo "Running as user momento ($(id momento))"

# Execute the application as the specified user
exec su-exec momento:momento "$@"
