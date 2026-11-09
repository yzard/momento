# Momento appearance and brand assets

Web, iOS and Android use neutral black, white and gray interface colors. Light mode
uses a white canvas and dark text; dark mode uses a black canvas and light text.
Destructive actions and meaningful status indicators retain their semantic colors.
Selection marks use one circle with a white check. Android floating surfaces have
shared raised elevation, a contrasting gray/white fill and a visible edge.
The shared dark floating fill is deep charcoal (`#1C1C1E`) throughout Android.
Timeline date, Select, menu, period and search controls share a 56 dp height,
including the expanded search field. Viewer date and album/information controls
use the same capsule surface, border, fill and elevation as the circular controls.
iOS retains the native glass material and adaptive black/white accent.
Filled iOS glass buttons use `ProminentGlassButtonStyle`: `InterfaceOnAccent` is
white over the light theme's black accent and black over the dark theme's white
accent. Text and symbols share this foreground; never force white content on a
white filled button.
Enabled iOS switches are the deliberate accent exception: `SystemSwitchToggleStyle`
uses Apple's system green and retains the native gray disabled/off appearance.
Native Settings uses single rows for backup diagnostics (Copy and a share icon),
backup history (Reverify and Clear), media cache and appearance. Choice buttons sit
on the right and display the saved value; their dropdowns mark the current choice.
The Local Cache Limit row has a red Clear action immediately before its chooser;
iOS keeps Clear outside the menu. These rows have no persistent explanatory text. Action failures remain visible on demand.
The device-backup row shows only its current status, upload progress and failure
status. Scheduling hints and background-processing explanations are omitted on both platforms.
Account actions stack on the right, with Change password above Sign out. Server
version appears below the account's server URL. The next row shows only the platform
client name and version (plus Android's update action); update results appear on demand.
iOS choice buttons place their dropdown chevron after the selected value.
Android and Web Places cards show a numeric count at the bottom right. Android regions can
wrap onto two lines, keeping the words in a country name together.
iOS map groups use the representative thumbnail and one count badge in the upper
right, matching Web and Android. Native annotation captions are hidden; single
items have no redundant count badge.

## Shared artwork

`src/branding/resources/momento.png` is the canonical opaque white-background artwork.
The colorful folded aperture is shared by app launchers, login and navigation brand
marks, Web favicons, Apple touch icon and the Web manifest. Android's themed icon
uses an alpha silhouette derived from the same artwork.

Regenerate all committed platform assets from the repository root:

```sh
mkdir -p build/tmp
export TMPDIR="$PWD/build/tmp" TEMP="$PWD/build/tmp" TMP="$PWD/build/tmp"
uv run --cache-dir build/tmp/brand-uv-cache src/branding/tooling/generate.py
```

The master was generated with the built-in image generation tool, then resized into
platform assets with Pillow (no additional image generation for individual platforms).
Final image-generation prompt:

> Use case: logo-brand. Asset type: production app icon for Momento, a private photo and memories library, shared across iOS, Android and web favicon. Create ONE original finished icon, square canvas, pure white opaque background edge to edge. Center a bold simple joyful multicolored photographic aperture / folded photo-ribbon symbol with a clean white negative-space center. Distinctive six interlocking curved geometric ribbons, with coral red, warm orange/yellow, vivid violet, sky blue and fresh mint, balanced saturated colors with subtle smooth tonal transitions. Refined minimal Apple-style brand craft; flat vector-like precision, gently rounded edges, extremely restrained dimensionality. Symbol occupies approximately 60 percent of canvas width, generous perfectly even white margin, clearly recognizable at tiny favicon sizes. No text, no lettering, no watermarks, no mockup, no multiple options, no surrounding frame, no outer rounded-square tile, no cast shadow outside the symbol, no camera body, no flowers or existing corporate logos. White background must remain white even at all corners. Return the actual single usable app-icon artwork.

## Verification

`tests/branding/tooling/test_generate.py` verifies exported files against the master,
opaque white launcher corners, the Android silhouette and rejection of non-square
artwork. Web palette tests verify neutral tokens and readable text contrast in both
themes; theme tests cover browser chrome synchronization. Native tests cover iOS
asset loading/adaptive accent and Android floating-surface contrast.

## Verified delivery on 2026-11-07

| Target | Delivered build | Verification |
| --- | --- | --- |
| iPhone | 1.0.1 (13) | 49 simulator tests and targeted physical original-export/error-path checks; Settings UI regression; PhotoKit MOV retry/hash regression; immediate photo/video display, seek/mute/audio playback tests and signed-in device browsing; cache Clear and aligned backup progress/logs |
| Android / Z Fold 8 | 1.0.12 | Docker compilation, JVM tests, lint, 67 emulator tests; expanded-device Settings/Clear layout and aligned backup status/logs; media 10915 duration/progress/seek verification |
| Web | Neutral themes and shared icon | 263 tests across 75 files, TypeScript, ESLint, formatting and light/dark visual inspection |
| Backend / NAS | `zhuoyin/momento:backup-progress-20261107` and `latest` | App/ingress and build-publication contracts, live health, brand-asset hashes and exact APK bytes |

The API image registry digest is
`sha256:f7d56d5b88223651e806017afe20e3ef45274e94c1daa62355ee7a25786c7565`.
Only `momento-api` was replaced in `/data/docker/6_photo.yaml`. The rollback image
is retained as `zhuoyin/momento:before-backup-progress-20261107`.

Local verification reports and UI captures are retained under
`${XDG_STATE_HOME:-$HOME/.local/state}/momento/branding/20261107/`.
Settings verification reports and captures are under
`${XDG_STATE_HOME:-$HOME/.local/state}/momento/settings/20261107/`.
Account and backup-status follow-up reports are under
`${XDG_STATE_HOME:-$HOME/.local/state}/momento/settings-account/20261107/`.

Backup progress, cache-clear and playback follow-up evidence is retained under
`${XDG_STATE_HOME:-$HOME/.local/state}/momento/native-playback/20261107/`.
