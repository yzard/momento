#!/usr/bin/env bash
# Real Docker integration for cache deletion with retained, reusable OCI layers.
set -euo pipefail
readonly repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
mkdir -p "$repository_root/build/tmp"
readonly fixture="$(mktemp -d "$repository_root/build/tmp/build-lifecycle-real.XXXXXX")"
export XDG_STATE_HOME="$fixture/state"
readonly model_fingerprint="$(printf '%s' "$fixture" | sha256sum | cut -d ' ' -f1)"
export MOMENTO_TEST_MODEL_FINGERPRINT="$model_fingerprint"
readonly image="momento-build-cleanup-test:$(basename "$fixture" | tr '[:upper:].' '[:lower:]-')"
cleanup() {
    docker image rm "$image" "momento-build-model:$model_fingerprint" >/dev/null 2>&1 || true
    rm -rf "$fixture"
}
trap cleanup EXIT
mkdir -p "$fixture/docker/lib"
cp "$repository_root/docker/lib/"{build_lifecycle.sh,validate_oci.py,docker_archive_oci.py} "$fixture/docker/lib/"
cat > "$fixture/lock-child.sh" <<'CHILD'
#!/usr/bin/env bash
set -euo pipefail
source "$1/docker/lib/build_lifecycle.sh"
momento_build_begin "$1" child
momento_build_success
CHILD
cat > "$fixture/lock-parent.sh" <<'PARENT'
#!/usr/bin/env bash
set -euo pipefail
source "$1/docker/lib/build_lifecycle.sh"
momento_build_begin "$1" parent
printf 'parent-owned temporary file\n' > "$1/build/parent-marker"
timeout 5 bash "$1/lock-child.sh" "$1"
[[ -f "$1/build/parent-marker" ]]
momento_build_success
PARENT
# The Android child must inherit the parent's lock instead of deadlocking on itself.
timeout 10 bash "$fixture/lock-parent.sh" "$fixture"
printf 'shared model weights\n' > "$fixture/weights"
printf 'source version one\n' > "$fixture/source"
cat > "$fixture/docker/Dockerfile.model" <<'MODEL'
FROM scratch
COPY weights /weights
MODEL
cat > "$fixture/docker/Dockerfile.service" <<'SERVICE'
FROM retained-model
COPY source /source
CMD ["/unused"]
SERVICE
cat > "$fixture/run.sh" <<'RUN'
#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")" && pwd)
source "$root/docker/lib/build_lifecycle.sh"
momento_build_begin "$root" docker
momento_build_oci "$root" model Dockerfile.model "$MOMENTO_TEST_MODEL_FINGERPRINT"
[[ -n $MOMENTO_BUILD_BUILDER ]] || momento_build_builder
docker buildx build --builder "$MOMENTO_BUILD_BUILDER" --load --provenance=false \
    --build-context "retained-model=oci-layout://$MOMENTO_BUILD_WORK/oci/model/$MOMENTO_TEST_MODEL_FINGERPRINT" \
    -f "$root/docker/Dockerfile.service" -t "$MOMENTO_TEST_REAL_IMAGE" "$root"
momento_build_success
RUN
MOMENTO_TEST_REAL_IMAGE="$image" bash "$fixture/run.sh"
first_layers=$(docker image inspect --format '{{json .RootFS.Layers}}' "$image")
docker image inspect "momento-build-model:$model_fingerprint" >/dev/null
[[ ! -e "$fixture/build" && ! -e "$fixture/dist/docker" ]]
# Removing the model build definition makes any accidental rebuild fail.
rm "$fixture/docker/Dockerfile.model"
printf 'source version two\n' > "$fixture/source"
MOMENTO_TEST_REAL_IMAGE="$image" bash "$fixture/run.sh"
second_layers=$(docker image inspect --format '{{json .RootFS.Layers}}' "$image")
python3 - "$first_layers" "$second_layers" <<'PY'
import json, sys
first, second = map(json.loads, sys.argv[1:])
assert first[0] == second[0], "shared model layer changed"
assert first[-1] != second[-1], "source change did not rebuild source layer"
PY
[[ ! -e "$fixture/build" && ! -e "$fixture/dist/docker" ]]
printf 'Real Docker cleanup and retained-layer reuse passed\n'
