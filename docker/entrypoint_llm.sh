#!/bin/sh
set -eu

. /entrypoint_common.sh
initialize_identity gosu
prepare_data_directories /data/llm /data/llm/cache /data/llm/cache/triton \
    /data/llm/queue /data/llm/tmp /data/logs
check_existing_file /data/config_llm.toml readable
run_as_service cp -R -n /opt/triton-cache/. /data/llm/cache/triton/ \
    || startup_error /data/llm/cache/triton 'cannot seed bundled Triton cache as runtime user'

if [ ! -f /data/config_llm.toml ]; then
    run_as_service /app/llm-service \
        -c /data/config_llm.toml --init-config
    run_as_service chmod 600 /data/config_llm.toml
fi

echo "Running as user $PUID:$PGID"
exec gosu "$PUID:$PGID" env HOME=/data "$@"
