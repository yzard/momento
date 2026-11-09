# Native iOS client

The iOS client lives in `src/ios/`, with mirrored tests in `tests/ios/`. It uses Swift 6,
SwiftUI, PhotoKit, MapKit, AVKit, Keychain, and native Liquid Glass. The minimum OS is
iOS 26. The client talks only to the existing Momento API; it does not connect directly
to the inference service.

## Build and run

macOS, Xcode with an installed iOS SDK/simulator runtime, and XcodeGen are required.
Use the independent root entrypoint; Android continues to use its Docker entrypoint.

```sh
./build_ios.sh build
./build_ios.sh verify --destination 'platform=iOS Simulator,name=iPhone 17 Pro'
./build_ios.sh device --team YOUR_TEAM_ID --device YOUR_DEVICE_UDID --server https://photos.example.com
./build_ios.sh project
```

`build` creates an ad-hoc signed simulator application in `dist/ios/Momento-simulator.app`.
`device` uses your configured development signing identity, installs the application on
the specified paired device, and launches it. Its output is `dist/ios/Momento.app`.
It does not publish to TestFlight or the App Store. `project` generates an editable Xcode
project at `build/ios/Momento.xcodeproj` without compiling. Sources remain canonical under
`src/ios/`. Build intermediates are removed after a successful outer invocation. Failed
builds retain their intermediates for diagnosis. Temporary host files use process-owned
directories inside `build/tmp/`, cleaned on exit. Verification reports are stored under
`${XDG_STATE_HOME:-$HOME/.local/state}/momento/ios/`.

`--server` only prefills the login field. The server is always editable before login,
including after Sign out. The last successfully used origin is remembered; credentials
and backup records stay isolated from other servers and accounts.

`verify` starts a disposable HTTP fixture on an ephemeral loopback port, runs Swift Testing
and XCUITest, then stops the fixture. It never reads production configuration or media.
The fixture is a contract test double, not a replacement for end-to-end validation against
an actual server. Device installation requires Developer Mode, pairing, and valid signing.

`verify --photos-fixture --device SIMULATOR_TEMPLATE_UDID` creates a separate disposable
simulator with the template's device type and runtime. Allow full Photos access in that
simulator when prompted. It creates synthetic photo and MOV assets, exports a photo through PhotoKit,
uploads originals to the loopback fixture and checks the final byte count and SHA-256. It also
retries a persisted MOV failure without replacing its operation ID or clearing history. The owned
simulator is deleted on exit; the template's library is untouched.

`verify-device --team YOUR_TEAM_ID --device YOUR_DEVICE_UDID` runs the explicitly enabled
read-only UI checks against an already signed-in physical device. It browses available
destinations and photo/video viewers without submitting administrative or media mutations.
Use `device` afterward to install the normal application build.

## Function coverage

| Android area | iOS implementation |
| --- | --- |
| Login, refresh, required password change, logout | Basic initial login; bearer API access; serialized token refresh; Keychain persistence; password sheets |
| Timeline, photos, videos, screenshots, documents | Search, day/week/month/year grouping, date anchor, bidirectional cursor paging, batch selection |
| Viewer | Swipe paging, zoom, filmstrip, native video controls, metadata, original download/share/save, trash, add to album |
| Albums | Create, edit, delete, add/remove media, ordering and cover selection |
| Places and map | Cursor-based places and media; MapKit clusters and cluster media |
| Faces | Groups, merge, group rejection and individual face rejection |
| Deduplicate | Group comparison and selected-item trash operations |
| Trash | Restore, permanent deletion and empty with confirmation |
| Administration | Import, metadata and AI actions/status, per-feature controls, schedules, user management |
| Settings | System/light/dark theme, account, bounded offline media cache, backup diagnostics/history |
| Backup | PhotoKit originals, SHA-256, protocol 2 chunks, durable records, resume, cancellation, server confirmation, background scheduling |
| Check for Updates | Deliberately omitted on iOS |

The viewer uses a Photos-style black canvas: back on the upper left, a two-line
place/date capsule in the center, and a more menu on the right. Its filmstrip sits
above separate share, central action, and trash glass controls. The central actions
are Momento's existing albums, information, and Save to Photos operations; no inert
favorite or editing buttons are shown for unsupported API operations. Android uses
the same arrangement with its existing floating controls and platform share flow.

Visited destinations retain their view identity, data, scroll position, search and selection.
Editors open above their parent page. Successful edits patch the owning page; returning to
a destination does not itself trigger a request. Navigation uses the same left drawer,
collapsible Timeline/Utility/Admin groups and pinned Settings/Logout as Android, with native
glass on iOS. The same content remains mounted while the window changes size. Layout uses
one 44-point glass-control height for the menu, search, period selector, date picker button
and selection button. Native toolbar backgrounds are hidden behind these explicit glass
surfaces so toolbar defaults do not shrink or double-wrap them. Layout uses
window dimensions rather than device model names and supports iPad,
landscape and resizable windows. Build with the iOS 27.1 SDK or newer for native iPhone Duo
display adaptation. The scene manifest, asymmetric safe areas and native bars follow
[Apple's Duo preparation guidance](https://developer.apple.com/iphone-duo/prepare/).

All main iOS destinations hide the navigation bar material and top scroll-edge veil,
so scrolling content continues behind their headers. Selection actions occupy the same
bottom row as Library, align to the trailing edge, and replace the ordinary controls to
its right. Long action rows scroll horizontally without shrinking touch targets. Timeline,
Faces, duplicates and Trash use this rule; Android uses the equivalent shared shell dock.
Selection marks in Web, iOS and Android are a single outlined circle with a white check
when selected. The Web change is limited to those marks and preserves its page layouts.

The 2026-11-07 selection follow-up passed the iOS regression
`verification-20261107-091538.xcresult`, including two-person selection, Merge enablement,
clearing selection, and explicit same-row/trailing-edge geometry assertions. Android
passed its 44 Docker emulator checks plus JVM tests and lint; the emulator checks cover
trailing alignment, replacing Timeline's normal dock, and accessing actions at large text
sizes by scrolling. Both signed apps were installed without clearing user data. Real
Faces pages were inspected for circle appearance, selection clearing, dock placement and
transparent scrolled headers. Web's 19 affected tests, TypeScript check, ESLint and asset
build passed; no production Web deployment was performed by this follow-up.

## Admin and Settings layout parity

The four iOS administrator pages use the Android information hierarchy: WebDAV and
local-import panels; a two-column metadata metrics grid with processing controls and
failure log; AI overview, job-count table and inline five-field schedule controls; and
a horizontally scrollable user table with username, email, role, status and actions.
Role changes remain confirmed, current-account protections remain active, and changing
one user patches the returned row without reloading the table. Polling keeps unfinished
cron edits intact. Submitting a cron field dismisses its keyboard before table scrolling.
AI count columns align right and cron fields align center, matching Android's tables.

Settings uses the Android section order and flat icon-led rows with separators, centered
at a maximum width of 840 points on expanded screens. Change password and Sign out stack
on the account row's right, with server version below the server URL. A separate row shows
the iOS client name and version. Cache size and appearance choices open above the
retained Settings page. iOS omits Android's app-update action and retains its native
background-backup scheduling and diagnostics sharing. The camera metadata filter remains
available. The Photos settings link appears only after access has been denied; starting
backup requests access when it has not yet been requested. iOS remembers a denial, so a
later backup attempt cannot show the initial system prompt again.

The selected destination uses session-scoped scene restoration so system window recreation
on Duo does not silently return Settings to Timeline. The expanded media information panel
has its own title and glass Done button; it no longer inherits the viewer's hidden navigation
bar. Compact information continues to use a native sheet.

The 2026-11-07 layout follow-up passed the focused library/Admin regression and API
checks in `admin-layout-final-20261107-103106.xcresult` (12 tests, no failures), covering
inline activation, five-field cron editing/saving, metadata/import panels, password
validation, sign-out and subsequent server editing. The Settings-specific case passed
in `verification-20261107-102506.xcresult`, including cache/theme restoration and denying
the actual Photos prompt before checking the conditional Settings link. That broader
run's sole failure was an input-replacement assumption in the cron test; the final
focused run uses Select All before replacement. The earlier core and login checks
passed in that same broader run.

The real iPhone passed all 16 destinations, native video play/pause, photo information,
and Settings/Admin rotation checks in `physical-20261107-102031.xcresult`. A normal signed
application was installed afterward, preserving `webcredentials:photos.zhuoyin.info`,
and the separate UI-test runner was removed. Actual Duo Open/Closed transitions and
expanded Settings, Import, Metadata, AI and User Management layouts were inspected through
Device Hub; cache/appearance popovers and the create-user overlay were also checked.
Open-pose XCTest attempts returned inactive-display screenshots and stale process
snapshots, so those attempts are not recorded as passing expanded-layout automation.
Private logs and screenshots remain in the project XDG state directory.

## Backup and platform differences

Photos access is requested when the user starts backup. Limited access means only the
authorized subset can be scanned. Original photo, video, alternate photo and paired Live
Photo video resources are separate upload records. Resource IDs, original modification
time, dimensions, creation time and available location are retained alongside the original
bytes. No UI preview or recompressed image is used as the backup input. Restoring Live
Photo pairing in Apple's Photos is not provided by the existing server's independent-media
storage contract; the original resource relationship is retained in backup metadata.

iOS does not expose Android's DCIM folder. The camera-only option therefore identifies
camera metadata in original image/QuickTime resources. It excludes files without that
metadata, including some edited/imported files. Disable it to include every supported
resource. iCloud originals may need downloading before upload. PhotoKit applies the
system's iCloud policy; the app checks allowed network access before preparation and
between upload chunks, and disables cellular access on its chunk requests when requested.

Durable snapshots use an opaque `.original` extension. QuickTime camera metadata reads
therefore pass the record's MIME type through `AVURLAssetOverrideMIMETypeKey`; inferring
format from that snapshot extension produces AVFoundation `-11828` (Cannot Open) for
valid MOV originals, including Live Photo paired videos. Metadata inspection does not
rename, transcode or modify the snapshot. Invalid media still raises an error; it is not
silently classified as a non-camera resource. Tests use synthetic camera/non-camera
MOV fixtures and a failed-record retry with server-side hash verification.

PhotoKit internal export failures receive at most two attempts against the same original resource.
Each attempt starts with a clean staging file; cancellation and empty/partial exports
never publish a snapshot. Read/download errors identify the Photos stage and retain
native error codes and any underlying cause. Network errors retain their transport type
so the controller can pause the run. An original that Photos cannot provide remains
failed and retryable, even when an edited full-resolution version is still available.
The edited version is not substituted or uploaded as the missing original. Camera-filter
exclusion clears the previous error, and diagnostics omit obsolete errors on older
excluded records.

The opt-in `PhotoLibraryDeviceTests` test reads one explicitly selected, already-authorized
image without uploading it or modifying Photos. Set `MOMENTO_PHYSICAL_READONLY=1`,
`MOMENTO_PHOTO_DIAGNOSTIC_ASSET` to its local identifier and
`MOMENTO_PHOTO_DIAGNOSTIC_EXPECTATION` to `readable` or `unavailable`. Normal test runs
skip this device-only diagnostic. It compares the resource, original editing-input and
original image-data APIs and checks listed edited-resource availability without uploading it;
results contain byte counts/hashes or native error details.

Each record and protected snapshot is stored in an account/server-specific Application
Support directory, excluded from iCloud backup. Record writes are atomic. The original
and its server-confirmed hash must agree before a record is marked completed. Server
`queued`/`processing` states remain pending. Cancellation intent is persisted and wins
over a late chunk acknowledgement; server completion can still win a cancellation race.
Clearing history is unavailable while any record is active and never deletes server media.

Daily backup uses `BGProcessingTask`; iOS chooses when to launch it and can expire it.
This cannot provide Android foreground-service execution guarantees. Foreground retry
or the next background pass reconciles server offsets and continues durable work.
Turning on daily backup expresses an earliest start, not an exact wall-clock alarm.

Release builds accept HTTPS origins only. Debug builds also permit HTTP origins for local
testing, subject to iOS App Transport Security. Authenticated requests reject cross-origin
redirects. The app does not disable TLS certificate validation or embed server credentials.

## Password manager domain matching

The native login fields declare username/password content types. Manually choosing a saved
password through the keyboard works without a website association. Automatic suggestions
for a website require Apple's mutual association between the signed app and that website.
An arbitrary server entered at runtime cannot add a signed associated-domain entitlement.

For a deployment-specific app, add `--autofill-domain photos.example.com` to the `device`
command (with `--team`). The build generates the exact `webcredentials` entitlement and
exports `dist/ios/autofill/photos.example.com/apple-app-site-association`. Serve that JSON
at `https://photos.example.com/.well-known/apple-app-site-association` with HTTP 200,
`Content-Type: application/json`, valid HTTPS, and no redirects. Merge its `webcredentials`
entry with an existing association file rather than replacing other app associations.
The Apple signing team must support the Associated Domains capability. Installation and
Apple's association cache must complete before automatic suggestions can be verified.
The build never deploys this file or locks the server field. Other server origins remain
editable; their credentials can still be selected manually in the password manager.

## Verification record — 2026-11-06

- Compared all 16 Android destinations on the USB-connected foldable with the iOS client.
  The comparison drove the drawer, grid/card layouts, retention badges, duplicate-selection
  protection, administration guards and viewer changes.
- Swift Testing and both simulator UI scenarios passed. The UI scenarios exercise login,
  rotation, Keychain restoration, photo selection, non-overlapping selection controls,
  retained page data, album creation and the editable server field after logout.
- The separate disposable PhotoKit test passed: an actual original resource was exported,
  uploaded in multiple chunks and confirmed against the server fixture's byte count and hash.
  An initial crash in the test's Photos creation callback was fixed by explicitly making
  that background callback Sendable. No subsequent Momento crash report was observed.
- The signed-in iPhone passed all 16 destination checks against the real server, photo
  information, native video play/pause and return navigation. Screenshots were inspected.
  Settled landscape was additionally checked through Device Hub and portrait restored.
- Actual Duo simulator Closed → Open → Closed transitions retained selected media and
  reflowed the grid. The selection bar and navigation dock remained separate in both poses.
- The date, Select, menu, period and search controls passed 44-point height assertions.
  Their shared glass appearance was also inspected in Duo Closed and Open poses without
  overlap or clipping (`verification-20261106-195013.xcresult`).

The follow-up parity audit added upload-ID correlation to every backup state response;
invalid responses leave the durable record unchanged. It also corrected the production
save-to-Photos callback's actor isolation and made Timeline Done clear selection, matching
Android. The isolated PhotoKit regression passed after these changes
(`verification-20261106-201019.xcresult`): original chunk upload and hash verification,
durable reopen without duplicate upload, and the viewer's save-to-Photos method followed
by byte-for-byte original export. These tests used only a disposable simulator library.
The iOS 26.4 full regression also passed (`verification-20261106-203250.xcresult`),
including selection clearing, album creation, retained navigation data, and first-open
duplicate comparison and map galleries. Older XCTest could not resolve a grid hit point;
the test sends a touch to the visible cell center and still asserts the resulting selection.
The duplicate and map first-open failures were independently reproduced and rechecked in
the populated Duo fixture. Non-Timeline navigation now reserves bottom scroll space so
the floating Library control cannot permanently obscure the final actions.

The populated-fixture checks also exposed a user editor that reset its Active switch.
Editor state now initializes once from the selected user; disabling a fixture user sends
`isActive: false` and updates the list. Cron fields retain their labels when populated.
Scheduled backup work now shares the controller's cancellable task lifecycle: logout
waits for it to stop, and a stopped controller cannot restart or cancel a new account's
schedule. The paused-request regression passed in
`backup-lifecycle-final-20261106.xcresult`; its source is mirrored under
`tests/ios/Feature/Backup/BackupControllerTests.swift`.

The follow-up native Duo run submitted 26 operations to a disposable in-memory
fixture: album creation/editing/media membership/cover/order/deletion; user creation,
activation and deletion; face merge/rejection; media trash, restore, permanent delete
and empty; import; metadata and AI start/cancel/clean; and schedule changes. Request
payloads and resulting client UI were inspected. Theme and cache-limit settings were
restored after checking them. These checks do not demonstrate production backend
execution or persistence.

The Photos-style viewer was checked in Closed and Open poses. A clipping regression
with the information panel open was fixed by measuring the window proposal rather
than the expanded content. Open → Closed → Open retains the selected photo and moves
information between a sheet and the side panel. The Android counterpart passed 257
JVM tests and lint in Docker, was signed with the existing key and installed over the
USB device's app without clearing its data. Real-server photo information, overflow
menu, share chooser, trash confirmation and video play/pause were checked. Android
device-state overrides exercised its inner/outer display layouts and retained the
same media item; the override was reset afterward. No production item was deleted or
shared. Detailed evidence is in the private XDG reports, not the source repository.

Final Duo regression `verification-20261106-213641.xcresult` passed: 32 tests,
44 executions including parameterized cases, no failures. The two deliberately
separate PhotoKit and physical-device scenarios are skipped in that general suite.
It includes viewer control geometry and rotation, the actual user-toggle touch path,
password confirmation validation and successful fixture password change, logout, and
editing the server afterward. Drawer tests explicitly establish their expanded groups
because the app intentionally remembers the previous scene's navigation state.

The latest signed build passed the real iPhone 16 Pro Max check on iOS 27.3
(`physical-20261106-214035.xcresult`): all 16 destinations, photo information,
native video play/pause, return navigation and landscape. The normal development
application was then installed and launched, with no test plug-ins in the exported
app. Its verified signature retains `webcredentials:photos.zhuoyin.info`.

Reports and private screenshots stay outside the repository under the project XDG state
directory. Successful reports are `verification-20261106-183806.xcresult` (PhotoKit),
`verification-20261106-185015.xcresult` (simulator suite), and
`physical-20261106-190823.xcresult` (real-server browsing).

Production uploads, deletion, account changes and inference jobs were not submitted by
these checks. Background execution timing remains controlled by iOS; it was not verified
by waiting for a daily background launch. Website AutoFill association was published on
2026-11-06. The signed app entitlement matches the public AASA endpoint, which returns
HTTP 200 with `application/json`. After Apple's CDN refreshed and the signed application
was reinstalled, the user confirmed that opening Bitwarden through the login keyboard
automatically filtered to the correct `photos.zhuoyin.info` account. Device Hub screen
sharing suppressed the software keyboard system-wide; stopping sharing restored it.

The association is packaged from
`src/backend/resources/webcredentials/apple-app-site-association` into the API image.
Only its exact public path is permitted; other hidden files remain inaccessible. GET,
HEAD, JSON content type and missing-file behavior are covered by the backend static-file
integration test. The 2026-11-06 deployment used NAS `6_photo.yaml` (the actual Momento
service), targeted `pull`/`up --no-deps`, and retained
`zhuoyin/momento:before-autofill-20261106` for rollback. No global Docker prune was run.


## Bulk original backup

The server advertises optional `backup.batch` version 1 alongside backup protocol 2.
Its default `maxBytes` is 50,000,000, configured with `[backup].max_batch_bytes`.
iOS and Android group original file sizes up to this limit, with at most 256 media
and the advertised JSON manifest limit. A larger original is sent alone through
resumable chunks, up to 8 MiB per request or the smaller server chunk limit.
Existing partial uploads retain their durable byte offsets.

Small-file batches prepare upload IDs in one JSON request, persist all accepted IDs,
and stream one length-framed binary request. The body contains a four-byte big-endian
manifest length, a version-1 JSON manifest, then the exact original bytes in manifest
order. The server validates each file's full SHA-256, records each file independently,
and queues the existing durable verification/import stage. A failed or interrupted
file does not invalidate earlier acknowledged files. Status polling accepts multiple
upload IDs. Clients validate response keys, sizes and hashes before accepting completion.
Old servers without the capability continue to use the shipped single-file protocol.

Snapshots and source metadata remain immutable. The iOS batch staging file is protected,
removed after the request, and cleaned after restart; edited Photos resources are never
substituted for unavailable originals. Progress continues to show the original filename,
MB/GB byte progress, verified media count, and per-original failure details.


### Unedited PhotoKit resource identity

Original discovery and export use the asset identifier, original resource type and filename,
with the uniform type identifier recorded for newly discovered resources. Asset modification
dates and resource array positions are metadata, not proof that original bytes changed.
Exports resolve the matching `.photo`, `.video`, `.alternatePhoto` or `.pairedVideo` resource;
rendered/edited resources are never candidates. Missing or ambiguous originals remain failures.
Protected snapshots and server acknowledgements retain the size/SHA-256 checks.

History created by earlier timestamp-based discovery retains its operation and upload IDs.
Redundant unprepared local discovery records are excluded with `duplicateOriginalOf` metadata;
verified, hashed and in-flight operations are preserved. Disabling the camera filter does not
requeue these duplicate tracking entries. The disposable PhotoKit test covers metadata changes,
resource reordering, byte-for-byte original export, and avoiding a second discovery record.

An explicit device-only audit accepts a JSON list of asset identifiers in
`MOMENTO_BACKUP_DIAGNOSTIC_ASSETS` with `MOMENTO_PHYSICAL_READONLY=1`. It reads selected originals
through the resource API and, on failure, original editing-input/image-data APIs. It never
uploads diagnostic files, modifies Photos, or reads edited resource bytes. Reports live in
the project XDG state directory, not source or final client artifacts.
