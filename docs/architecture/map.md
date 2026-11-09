# Stable per-user map clustering

`POST /api/v1/map/clusters` computes geohash-cell centers, counts, and representative
media from the requesting user's entire active `media_access` set. Viewport bounds
filter the completed centers, never the input members. The existing zoom-to-geohash
precision mapping remains authoritative. A stable dataset and precision therefore
produce identical centers and counts while panning. A marker disappears when its
center leaves the requested bounds, even if some members remain inside those bounds.

Each pooled SQLite connection lazily caches up to four indexes keyed by user ID and
precision. Index construction and SQL aggregation run in the scheduled protected
SQLite reader lane. SQL orders centers into one-degree latitude bands and sorts each
band by longitude. Viewport lookup uses binary range searches on both bands and
longitudes, then applies the exact latitude bounds. Antimeridian viewports use two
longitude ranges. Neither client clusters the returned viewport subset again.

A single read transaction ties cache validation and rebuilding to one database
snapshot. SQLite's connection-local `data_version` detects commits on other
connections; `total_changes()` detects writes through the same connection. Any write
invalidates that connection's cached indexes on its next map query. This deliberately
includes unrelated writes, favoring immediate access correctness over selective
invalidation. No cached index crosses connections or users. Restart, connection
retirement, and least-recently-used eviction discard derived indexes; the next request
rebuilds them. No database schema change or data-directory reset is required.

Index storage is capped at 32 MiB per pooled connection, including conservative
allocation overhead. The existing 4,096-row viewport response bound remains; the
global index is not subject to that response-row limit. Exceeding an index or response
bound returns an explicit error, never a silently incomplete cluster set. Initial
construction can scan the user's complete geotagged library; cache hits avoid that
scan. Frequent database writes may cause rebuilding on subsequent requests.

`POST /api/v1/map/media` with explicit `geohashPrefixes` returns the whole selected
cell membership, filtered through active access, regardless of the supplied viewport.
Without prefixes it remains a viewport media query. Existing bounded media response
limits still apply. Web and Android share these server semantics; their map markers
continue using tiny thumbnails.
