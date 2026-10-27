#!/usr/bin/env bash
# State-backed Docker substitute for build orchestration tests, never a production tool.
set -euo pipefail
printf 'docker:%s\n' "$*" >> "$MOMENTO_TEST_INVOCATIONS"
state=${MOMENTO_TEST_DOCKER_STATE:?}
mkdir -p "$state"
image_path() { printf '%s/image-%s' "$state" "$(printf '%s' "$1" | sha256sum | cut -d ' ' -f1)"; }
if [[ $1 == version ]]; then
    printf 'linux/amd64\n'
elif [[ $1 == buildx && $2 == create ]]; then
    printf 'test-builder\n'
elif [[ $1 == buildx && $2 == rm ]]; then
    [[ ${MOMENTO_TEST_CLEANUP_FAIL:-false} != true ]]
elif [[ $1 == buildx && $2 == build ]]; then
    output= metadata= dockerfile= fingerprint=
    tags=()
    shift 2
    while (( $# > 0 )); do
        case $1 in
            -f|--file) dockerfile=$2; shift 2 ;;
            -t|--tag) tags+=("$2"); shift 2 ;;
            --label) fingerprint=${2#io.momento.build.inputs=}; shift 2 ;;
            --output) output=$2; shift 2 ;;
            --metadata-file) metadata=$2; shift 2 ;;
            *) shift ;;
        esac
    done
    if [[ ${MOMENTO_TEST_FAIL_DOCKERFILE:-} == "$(basename "$dockerfile")" ]]; then exit 7; fi
    if [[ -n $output ]]; then
        destination=${output##*dest=}
        mkdir -p "$destination/blobs/sha256"
        python3 - "$destination" <<'PYOCI'
import hashlib, json, sys
from pathlib import Path
root = Path(sys.argv[1])
def blob(data):
    digest = hashlib.sha256(data).hexdigest()
    (root / "blobs" / "sha256" / digest).write_bytes(data)
    return {"digest": "sha256:" + digest, "size": len(data)}
layer = blob(b"retained-model-layer")
layer["mediaType"] = "application/vnd.oci.image.layer.v1.tar"
config = blob(b"{}")
config["mediaType"] = "application/vnd.oci.image.config.v1+json"
manifest = blob(json.dumps({"schemaVersion": 2, "config": config, "layers": [layer]}).encode())
manifest["mediaType"] = "application/vnd.oci.image.manifest.v1+json"
(root / "index.json").write_text(json.dumps({"schemaVersion": 2, "manifests": [manifest]}))
PYOCI
    fi
    for tag in "${tags[@]}"; do printf '%s\n' "$fingerprint" > "$(image_path "$tag")"; done
    if [[ -n $metadata ]]; then
        printf '{"containerimage.digest": "sha256:%s"}\n' "$fingerprint" > "$metadata"
        for tag in "${tags[@]}"; do printf 'sha256:%s\n' "$fingerprint" > "$(image_path "$tag").remote"; done
    fi
elif [[ $1 == image && $2 == inspect ]]; then
    path=$(image_path "${!#}")
    [[ -f $path ]] || exit 1
    cat "$path"
elif [[ $1 == image && $2 == tag ]]; then
    if [[ $3 != "$4" ]]; then cp "$(image_path "$3")" "$(image_path "$4")"; fi
elif [[ $1 == image && $2 == push ]]; then
    printf 'sha256:%s\n' "$(<"$(image_path "$3")")" > "$(image_path "$3").remote"
elif [[ $1 == image && $2 == rm ]]; then
    rm -f "$(image_path "$3")"
elif [[ $1 == buildx && $2 == imagetools && $3 == inspect ]]; then
    cat "$(image_path "${!#}").remote"
elif [[ $1 == buildx && $2 == imagetools && $3 == create ]]; then
    :
elif [[ $1 == run ]]; then
    distribution= signing= source= build=
    container_command=${!#}
    while (( $# > 0 )); do
        if [[ $1 == --volume ]]; then
            case $2 in
                *:/dist) distribution=${2%:/dist} ;;
                *:/signing:ro) signing=${2%:/signing:ro} ;;
                *:/workspace:ro) source=${2%:/workspace:ro} ;;
                *:/build) build=${2%:/build} ;;
            esac
            shift 2
        else shift; fi
    done
    mkdir -p "$build/workspace/src/android/app/build/reports" "$build/gradle-user-home" "$build/home/.android" "$build/android-avd"
    printf 'report\n' > "$build/workspace/src/android/app/build/reports/test-report.txt"
    printf 'debug-signing-identity\n' > "$build/home/.android/debug.keystore"
    if [[ ${MOMENTO_TEST_RUN_FAIL:-false} == true ]]; then exit 9; fi
    version=$(<"$source/src/android/version.txt")
    case $container_command in
        assemble-debug)
            mkdir -p "$distribution/debug"
            printf 'debug-apk\n' > "$distribution/debug/momento-android-$version-debug.apk" ;;
        release)
            stem=$(basename "$signing"/*.jks .jks)
            printf 'signed-apk\n' > "$distribution/$stem-$version.apk"
            printf 'signed-aab\n' > "$distribution/$stem-$version.aab" ;;
    esac
else
    printf 'Unsupported fake Docker command\n' >&2
    exit 1
fi
