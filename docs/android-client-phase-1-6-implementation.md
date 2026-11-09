# Android Client Phase 1–6 Implementation Record

Implementation date: 2026-08-27

This record accompanies `android-client-code-audit.md` and describes the code changes completed from its six-phase repair plan.

## Phase 1 — Unified page contract

- Added `MomentoPageScaffold` as the sole standard owner of title, subtitle, back action, safe insets, content padding, and optional bottom chrome.
- Migrated Timeline, Trash, Albums, Places, Faces, Deduplicate, Map, Settings, Admin, and Album/Place/Face details.
- Removed the shell title exclusion list and duplicate page headers.
- Added compact/expanded layout tests plus light/dark and large-font Compose coverage.

## Phase 2 — Shared media presentation

- Added `MomentoAsyncImage` with one authenticated loader, consistent loading/failure rendering, crop behavior, and container-level accessibility semantics.
- Added `AuthenticatedImageSource`; media thumbnails, album collages, place covers, face thumbnails, and osmdroid marker bitmaps now share it. Map keeps only the required Bitmap-to-Drawable presentation adapter.
- Added `MomentoSelectionMark` and `MomentoSelectionDock`; migrated Timeline, Trash, Faces, and Deduplicate selection chrome and reused the existing shared dock in Album detail.
- Added `MomentoCollectionDetail`; Album, Place, and Face detail now share title/back/inset/state/grid/empty/error rendering and vary only through explicit parameters.

## Phase 3 — Request and paging state

- Added typed `RequestResult` / `RequestError` classification.
- Replaced the unused media pager with generic keyed `CursorPagingState`, using linear-time page deduplication.
- Migrated Places, Place detail, Faces, and Deduplicate to the shared request/paging primitives.
- Replaced Timeline's nullable groups plus independent cursor/loading/error flags with one immutable `TimelinePagingState`. Its specialized two-direction cursors now use explicit `TimelineDirection` transitions, preserve content on refresh failure, and share typed request errors.
- Removed duplicated append helpers and their obsolete tests.
- Capability discovery now has explicit Loading, Available, and Failed states. Generated collections and Backup no longer fail open; a failed request exposes Retry.

## Phase 4 — Navigation and viewer

- Replaced parallel destination/viewer flags with an ordered typed route stack covering collections, Album/Place/Face detail, and Viewer.
- Removed page-owned detail back handlers and `contentRevision` forced reconstruction. Viewer mutations publish an explicit `LibraryChange` event consumed by visible collections.
- Viewer always runs inside one dark chrome context.
- Restored the thumbnail filmstrip in portrait and landscape, removed its container background, and added independent left/right edge fades.
- Media information uses shared content in a portrait bottom sheet or landscape right panel.
- Video progress uses a thin round track and dot thumb while preserving a large touch target and committing seek only on release.
- Expanded the tested Viewer controller to own Chrome visibility, active Sheet, bounded Filmstrip/Pager navigation, and Seek drag state. Seek cancel restores the player position; release emits one commit; open Info state restores across recreation and adapts from bottom to right when orientation changes.

## Phase 5 — Workflow boundaries

- Admin polls only the selected section; Users, Import/Metadata, and AI sections are separate source modules. Independent AI controls now have independent busy state.
- APK download, package validation, cache cleanup, and installer intent creation moved into `AndroidUpdateCoordinator`. Unknown-source permission is requested only after a valid newer APK has been found and staged.
- Backup was split into dedicated Scanner, Snapshot, Transfer, Cancellation, Scheduler/history, Network, and Permission modules. Existing WorkManager names and durable database behavior were preserved.
- Settings is now an 80-line page composer. Account, Android Update, Backup, and Appearance each own their state, dialogs, launchers, and side effects in separate modules.
- Core Retrofit resources were split into Authentication, Library, Administration, and Backup APIs. Repository dependencies now expose narrow Administration, Account, Update, Authenticated Media, and Backup resource interfaces.
- Core DTOs were split into timeline/media, albums, collections, admin/AI, and capability/backup resource files while preserving one wire model per API field.

## Phase 6 — Verification matrix

Automated coverage now includes:

- Page scaffold compact/expanded calculations, light/dark rendering, large font, back semantics, content, and bottom controls.
- Admin tab and rail selection in phone and landscape/tablet layouts.
- Shared image accessibility when loading fails.
- Cursor paging reset, failure, exhaustion, and keyed deduplication.
- Typed request error classification.
- Viewer filmstrip edge fades, orientation presenter, progress bounds, chrome interaction reset, and drag/sheet auto-hide inhibition.
- Viewer navigation bounds, seek drag/cancel/single-release commit, and open-sheet recreation/orientation adaptation.
- Backup network and permission transitions plus the existing durable database and history tests.
- A complete page QA contract matrix covering 12 major pages across Compact Portrait/Landscape, Expanded Portrait/Landscape, Light/Dark/System themes, normal/large fonts, and Loading/Empty/Error/Content/Refreshing/Selection/Working states.
- Device-side state UI tests for loading, empty, error/retry, System theme, selection/admin semantics, Room backup behavior, and large-font page layout.

Required final gates:

- `./build_android_client.sh verify`
- `./build_android_client.sh instrumented-test`
- `git diff --check`

Final result: all six implementation phases and their automated gates are complete. OEM package-installer behavior, hardware video decoding, and tile-provider performance remain release-device integration checks rather than code-phase work.
