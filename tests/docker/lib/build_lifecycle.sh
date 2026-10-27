#!/usr/bin/env bash
# Real Docker integration for cache deletion with retained, reusable OCI layers.
set -euo pipefail
readonly repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
mkdir -p "$repository_root/build/tmp"
readonly fixture="$(mktemp -d "$repository_root/build/tmp/build-lifecycle-real.XXXXXX")"
readonly image="momento-build-cleanup-test:$(basename "$fixture" | tr '[:upper:].' '[:lower:]-')"
cleanup() {
    docker image rm "$image" >/dev/null 2>&1 || true
    rm -rf "$fixture"
}
trap cleanup EXIT
mkdir -p "$fixture/docker/lib"
cp "$repository_root/docker/lib/"{build_lifecycle.sh,validate_oci.py} "$fixture/docker/lib/"
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
timeout 5 bash "$1/lock-child.sh" "$1"
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
momento_build_oci "$root" model Dockerfile.model stable-model-input
[[ -n $MOMENTO_BUILD_BUILDER ]] || momento_build_builder
docker buildx build --builder "$MOMENTO_BUILD_BUILDER" --load --provenance=false \
    --build-context "retained-model=oci-layout://$root/dist/docker/model/stable-model-input" \
    -f "$root/docker/Dockerfile.service" -t "$MOMENTO_TEST_REAL_IMAGE" "$root"
momento_build_success
RUN
MOMENTO_TEST_REAL_IMAGE="$image" bash "$fixture/run.sh"
first_layers=$(docker image inspect --format '{{json .RootFS.Layers}}' "$image")
[[ -f "$fixture/dist/docker/model/stable-model-input/inputs" ]]
[[ -z $(find "$fixture/build/docker" -maxdepth 1 -name 'run.*' -print -quit) ]]
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
[[ -z $(find "$fixture/build/docker" -maxdepth 1 -name 'run.*' -print -quit) ]]
printf 'Real Docker cleanup and retained-layer reuse passed\n'
