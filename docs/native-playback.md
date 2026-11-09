# Native media playback

The iOS viewer owns one `VideoPlayback` for the selected item. Its lifetime is
independent of the neighboring pages created by SwiftUI's pager. Loading a video
shows its cached thumbnail until `AVPlayerLayer` has decoded a frame. A zero-time
seek primes that frame while paused. Photo zoom surfaces use the actual page
viewport rather than an inferred scroll-container size.

The iOS playback row contains play/pause, position, elapsed/total time and a
mute/unmute speaker. It uses `AVPlayerLayer` rather than `VideoPlayer`, so AVKit's
subtitle and text-analysis menus are absent. Starting playback activates the
`.playback` / `.moviePlayback` audio session, including when the phone's silent
switch is enabled. Pausing, leaving the viewer or backgrounding the app releases
the viewer's audio session. Speaker mute is retained across video selections.

Android uses Media3 1.11.1 with fragmented-MP4 MFRA seeking enabled. Existing
server previews have an empty initial `moov`, fragments and an end-of-file `mfra`
index, without a global `sidx`. Older Media3 versions treated these as unseekable
and reported an unknown duration. The client must read the existing index; server
preview regeneration is unnecessary. An MFRA index alone does not supply the
container duration: `MetadataDurationTimeline` fills missing window/period times
from the server's parsed media duration. A duration supplied by the container takes
precedence. The wrapper preserves the source's seekability and sample stream.
This makes progress available before a large preview has downloaded to its end.

Cached video range requests return bounded 206 responses with Content-Range.
The file channel seeks directly to the requested offset, including the tail index;
it does not make the player read and discard the beginning of a cached video.
Invalid ranges return 416; mismatched If-Range validators return the full entity.

Media3 1.11 requires Kotlin 2.2, so the
Compose compiler plugin, KSP, Room and serialization dependencies are updated
with it. This does not change the application's database schema.

`tests/common/resources/media/fragmented-preview.mp4` is a synthetic ten-second
320×240 test pattern with AAC test tone, two-second GOPs and the server's
`+frag_keyframe+empty_moov+default_base_moof` flags. It contains no user media.
Both platforms use this fixture to verify duration, advancing time and seeking.
iOS UI coverage additionally verifies initial photo/video visibility and the
speaker button through the authenticated range loader. Android tests construct
the production player's media-source factory, rather than a separate test player.

## Backup settings

On both native clients, one button displays **Back up now** when idle and
**Cancel** only while a backup run is active. Failed history by itself does not
make Cancel appear. Cancellation targets the active run and disables duplicate
requests while the worker stops. The cache-size chooser is labelled
**Local Cache Limit**; existing saved limits remain valid.

The Local Cache Limit row places a red **Clear** action immediately before the
right-side limit chooser on both platforms. iOS does not duplicate Clear inside
the menu. Clearing removes downloaded media/cache metadata and aborts pending
cache writes on Android, while preserving the selected limit and backup history.

The backup status shows the original filename followed by acknowledged/total
bytes, with MB as the smallest unit and GB for values of at least one billion
bytes. Both clients round to at most two decimal places. The media progress bar
keeps a separate completed/total media count. Upload failures remain below it in
red, starting with the original filename and the server or transfer failure reason.
Progress and logs fill the available Settings width, including expanded layouts,
while retaining the standard row gutters (20 points on iOS, 16 dp on Android).
They are outside the header row so the action button and icon cannot narrow them.
