# Places and aesthetic covers

Read when changing place identity, location metadata, covers, or reverse geocoding.

### Places and aesthetic covers

Places are identified by the exact `location_city`, nullable `location_state`, and
`location_country` tuple. Lists and galleries filter through active `media_access`; list ordering is
visible media count descending, then city, state, and country ascending. Place identifiers are
opaque encodings of the complete tuple. Manual GPS changes immediately recompute or clear the local
reverse-geocoded fields so grouping cannot retain stale location names.

Place covers reuse normal thumbnails; WebGUI and Android crop them to 3:2 at display time.
`image_aesthetics` uses the canonical photo original or the shared full-resolution video frame.
Cover ranking prefers completed
aesthetic inference and combines aesthetic 40%, scenic 25%, simplicity 20%, landscape 10%, and
technical quality 5%, then applies OCR-clutter and dominant-face penalties. Media without an
aesthetic result use a deterministic landscape, capture-date, and media-ID fallback. A user's place
cover is always selected only from media visible to that user. Place cover selection is never
stored or cached as a representative media ID. Every place cover request reruns the ranking
against current metadata, aesthetic results, and active `media_access`, so changed membership is
visible on the next request.

`schema.sql` and the Android Room database define only their current schemas. Keep only the current
Room schema export. Do not add schema migration or compatibility code; breaking schema changes
require a fresh database and data directory.

Reverse geocoding is always local and uses the pinned GeoNames `cities500` asset embedded in the
Momento API binary. It has no enablement, URL, user-agent, timeout, or rate-limit configuration.
Normal metadata generation fills missing location fields immediately; each metadata-worker cycle
does not backfill existing metadata rows. Existing non-empty location fields are preserved.
Updating the dataset requires recording its snapshot date, source checksums, output checksum, and
CC BY 4.0 attribution in the source manifest.
