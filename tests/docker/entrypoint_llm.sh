#!/bin/sh
set -eu
exec sh "$(dirname -- "$0")/entrypoint_common.sh" llm
