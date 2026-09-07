#!/bin/sh
set -eu

. /entrypoint_common.sh
initialize_identity su-exec
prepare_data_directories \
    /data/albums \
    /data/imports \
    /data/logs \
    /data/originals \
    /data/previews \
    /data/tmp \
    /data/thumbnails \
    /data/thumbnails_tiny \
    /data/trash \
    /data/webdav \
    /data/backups \
    /data/journal

check_existing_file /data/config.toml readable
check_existing_file /data/database.sqlite writable
check_existing_file /data/database.sqlite-wal writable
check_existing_file /data/database.sqlite-shm writable

if [ ! -f /data/config.toml ]; then
    run_as_service /app/momento-api -c /data/config.toml --init-config
fi

echo "Running as user $PUID:$PGID"

exec su-exec "$PUID:$PGID" env HOME=/data "$@"
