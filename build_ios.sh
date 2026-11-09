#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
MODE="${1:-build}"
if (($#)); then shift; fi
DESTINATION="generic/platform=iOS Simulator"
TEAM=""
DEVICE=""
SERVER=""
AUTOFILL_DOMAIN=""
PHOTOS_FIXTURE=false
while (($#)); do
  case "$1" in
    --destination) DESTINATION="$2"; shift 2 ;;
    --team) TEAM="$2"; shift 2 ;;
    --device) DEVICE="$2"; shift 2 ;;
    --server) SERVER="$2"; shift 2 ;;
    --autofill-domain) AUTOFILL_DOMAIN="$2"; shift 2 ;;
    --photos-fixture) PHOTOS_FIXTURE=true; shift ;;
    *) echo "Unknown argument: $1" >&2; exit 2 ;;
  esac
done
case "$MODE" in build|verify|verify-device|device|project) ;; *) echo "Usage: $0 [build|verify|verify-device|device|project] [--destination VALUE] [--team ID] [--device UDID] [--server ORIGIN] [--autofill-domain HOST]" >&2; exit 2 ;; esac
if [[ "$PHOTOS_FIXTURE" == true && ( "$MODE" != verify || -z "$DEVICE" ) ]]; then
  echo "--photos-fixture requires verify --device DISPOSABLE_SIMULATOR_UDID" >&2; exit 2
fi
mkdir -p "$ROOT/build/tmp" "$ROOT/build/ios" "$ROOT/dist/ios"
mkdir "$ROOT/build/ios.lock" 2>/dev/null || { echo "Another iOS build owns build/ios.lock." >&2; exit 1; }
IOS_TEMP="$(mktemp -d "$ROOT/build/tmp/ios.XXXXXX")"
export TMPDIR="$IOS_TEMP" TEMP="$IOS_TEMP" TMP="$IOS_TEMP"
FIXTURE_PID=""
PHOTOS_SIMULATOR=""
cleanup() {
  if [[ -n "$FIXTURE_PID" ]]; then kill "$FIXTURE_PID" 2>/dev/null || true; wait "$FIXTURE_PID" 2>/dev/null || true; fi
  if [[ -n "$PHOTOS_SIMULATOR" ]]; then
    xcrun simctl shutdown "$PHOTOS_SIMULATOR" >/dev/null 2>&1 || true
    xcrun simctl delete "$PHOTOS_SIMULATOR" >/dev/null 2>&1 || true
  fi
  rm -rf "$IOS_TEMP"
  rmdir "$ROOT/build/ios.lock"
}
trap cleanup EXIT
command -v xcodegen >/dev/null || { echo "XcodeGen is required." >&2; exit 1; }
xcodegen generate --spec "$ROOT/src/ios/project.yml" --project "$ROOT/build/ios"
if [[ "$MODE" == project ]]; then exit 0; fi
ARGS=(-project "$ROOT/build/ios/Momento.xcodeproj" -scheme Momento -derivedDataPath "$ROOT/build/ios/DerivedData" "MOMENTO_DEFAULT_SERVER_ORIGIN=$SERVER")
publish_app() {
  local source_app="$1" destination_app="$2"
  rm -rf "$destination_app"
  ditto "$source_app" "$destination_app"
  codesign --verify --deep --strict "$destination_app"
}
if [[ -n "$AUTOFILL_DOMAIN" ]]; then
  [[ ( "$MODE" == device || "$MODE" == verify-device ) && -n "$TEAM" ]] || { echo "--autofill-domain requires device or verify-device mode and --team" >&2; exit 2; }
  python3 "$ROOT/src/ios/Build/prepare_autofill.py" --domain "$AUTOFILL_DOMAIN" \
    --app-id "$TEAM.io.github.yzard.momento.ios" --entitlements "$ROOT/build/ios/Momento.entitlements" \
    --association "$IOS_TEMP/apple-app-site-association"
  ARGS+=("MOMENTO_APP_ENTITLEMENTS=$ROOT/build/ios/Momento.entitlements")
fi
if [[ "$MODE" == verify-device ]]; then
  [[ -n "$TEAM" && -n "$DEVICE" ]] || { echo "verify-device requires --team and --device" >&2; exit 2; }
  REPORTS="${XDG_STATE_HOME:-$HOME/.local/state}/momento/ios"
  mkdir -p "$REPORTS"
  xcodebuild "${ARGS[@]}" -configuration Debug -destination "id=$DEVICE" -allowProvisioningUpdates \
    DEVELOPMENT_TEAM="$TEAM" MOMENTO_PHYSICAL_READONLY=1 -only-testing:MomentoUITests/PhysicalBrowsingTests \
    -resultBundlePath "$REPORTS/physical-$(date +%Y%m%d-%H%M%S).xcresult" test
  publish_app "$ROOT/build/ios/DerivedData/Build/Products/Debug-iphoneos/Momento.app" "$ROOT/dist/ios/Momento.app"
elif [[ "$MODE" == device ]]; then
  [[ -n "$TEAM" && -n "$DEVICE" ]] || { echo "device requires --team and --device" >&2; exit 2; }
  xcodebuild "${ARGS[@]}" -configuration Debug -destination "id=$DEVICE" -allowProvisioningUpdates DEVELOPMENT_TEAM="$TEAM" build
  publish_app "$ROOT/build/ios/DerivedData/Build/Products/Debug-iphoneos/Momento.app" "$ROOT/dist/ios/Momento.app"
  if [[ -n "$AUTOFILL_DOMAIN" ]]; then
    mkdir -p "$ROOT/dist/ios/autofill/$AUTOFILL_DOMAIN"
    cp "$IOS_TEMP/apple-app-site-association" "$ROOT/dist/ios/autofill/$AUTOFILL_DOMAIN/apple-app-site-association"
  fi
  xcrun devicectl device install app --device "$DEVICE" "$ROOT/dist/ios/Momento.app"
  xcrun devicectl device process launch --device "$DEVICE" io.github.yzard.momento.ios
elif [[ "$MODE" == verify ]]; then
  PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s "$ROOT/tests/ios/Build" -v
  [[ "$DESTINATION" != "generic/platform=iOS Simulator" ]] || DESTINATION="platform=iOS Simulator,name=iPhone 17 Pro"
  python3 "$ROOT/tests/ios/UI/fixture_server.py" --port-file "$IOS_TEMP/port" &
  FIXTURE_PID=$!
  for _ in {1..50}; do [[ -s "$IOS_TEMP/port" ]] && break; sleep 0.1; done
  [[ -s "$IOS_TEMP/port" ]] || { echo "UI fixture did not start" >&2; exit 1; }
  REPORTS="${XDG_STATE_HOME:-$HOME/.local/state}/momento/ios"
  mkdir -p "$REPORTS"
  REPORT="$REPORTS/verification-$(date +%Y%m%d-%H%M%S).xcresult"
  if [[ "$PHOTOS_FIXTURE" == true ]]; then
    xcrun simctl list devices --json > "$IOS_TEMP/devices.json"
    python3 - "$DEVICE" "$IOS_TEMP/devices.json" > "$IOS_TEMP/simulator-template" <<'PY'
import json
import sys
from pathlib import Path
devices = json.loads(Path(sys.argv[2]).read_text())["devices"]
for runtime, entries in devices.items():
    for device in entries:
        if device["udid"] == sys.argv[1]:
            print(device["deviceTypeIdentifier"])
            print(runtime)
            raise SystemExit(0)
raise SystemExit("The Photos fixture requires an available simulator as its template")
PY
    PHOTOS_SIMULATOR="$(xcrun simctl create "Momento Photos Fixture $$" "$(sed -n '1p' "$IOS_TEMP/simulator-template")" "$(sed -n '2p' "$IOS_TEMP/simulator-template")")"
    DESTINATION="platform=iOS Simulator,id=$PHOTOS_SIMULATOR"
    xcrun simctl boot "$PHOTOS_SIMULATOR"
    xcrun simctl bootstatus "$PHOTOS_SIMULATOR" -b
    xcodebuild "${ARGS[@]}" -configuration Debug -destination "$DESTINATION" CODE_SIGN_IDENTITY=- \
      "MOMENTO_UI_TEST_ORIGIN=http://127.0.0.1:$(cat "$IOS_TEMP/port")" MOMENTO_PHOTOKIT_FIXTURE=1 build-for-testing
    xcrun simctl install "$PHOTOS_SIMULATOR" "$ROOT/build/ios/DerivedData/Build/Products/Debug-iphonesimulator/Momento.app"
    xcrun simctl launch "$PHOTOS_SIMULATOR" io.github.yzard.momento.ios
    xcrun simctl privacy "$PHOTOS_SIMULATOR" grant photos io.github.yzard.momento.ios
    xcodebuild "${ARGS[@]}" -configuration Debug -destination "$DESTINATION" -resultBundlePath "$REPORT" \
      CODE_SIGN_IDENTITY=- "MOMENTO_UI_TEST_ORIGIN=http://127.0.0.1:$(cat "$IOS_TEMP/port")" MOMENTO_PHOTOKIT_FIXTURE=1 \
      -parallel-testing-enabled NO -only-testing:MomentoTests/PhotoKitBackupTests test-without-building
  else
    xcodebuild "${ARGS[@]}" -configuration Debug -destination "$DESTINATION" -resultBundlePath "$REPORT" \
      CODE_SIGN_IDENTITY=- "MOMENTO_UI_TEST_ORIGIN=http://127.0.0.1:$(cat "$IOS_TEMP/port")" test
  fi
else
  xcodebuild "${ARGS[@]}" -configuration Debug -destination "$DESTINATION" CODE_SIGN_IDENTITY=- build
  publish_app "$ROOT/build/ios/DerivedData/Build/Products/Debug-iphonesimulator/Momento.app" "$ROOT/dist/ios/Momento-simulator.app"
fi
# Only this entrypoint's intermediates are owned here; other components may be building.
rm -rf "$ROOT/build/ios"
