# Android media cache

Settings → Media cache selects 256 MiB, 512 MiB (default), 1 GiB, or 2 GiB. This is
one shared disk budget for media and small offline browsing snapshots. Shrinking the
limit immediately aborts pending cache writes and evicts the least recently used
completed entries. Reading an entry updates its persistent recency. Android may also
reclaim application cache storage under device storage pressure.

All media, tiny-map, trash, face, and place thumbnails and image/video previews use
the authenticated HTTP client's cache interceptor. Coil's independent disk and memory
caches are disabled so they cannot bypass freshness checks or this disk quota.
Original downloads for sharing, APK downloads, uploads, authentication calls, and
mutating operations are not cached.

Media entries are fresh for five minutes. Subsequent reads validate expired entries
using ETag/If-None-Match or Last-Modified/If-Modified-Since. A 304 renews freshness;
a complete 200 publishes updated bytes. Connection failures and temporary server
errors may fall back to an existing entry. A detected offline connection reads an
existing entry immediately. A returned 401, 403, 404, or 410 invalidates that entry
instead of serving it. Fresh cache hits and offline use do not contact the server;
remote changes become visible on a later online validation, not through push updates.

Downloads stream to the viewer while a bounded cache editor copies the bytes. Only
complete representations are published. Interrupted transfers and partial range
responses are not saved as complete previews. Previously completed video previews
can be served offline as full HTTP 200 representations; Media3 handles seeking within
that local response. Uncached video seeks preserve range requests. Media larger than
the available cache budget still streams online but is not cached. Reservations cover
in-flight writes, and cached entries use immutable data files with atomically replaced
metadata. Restart removes incomplete and orphaned files.

Previously visited timeline, album, face, place, duplicate, trash, and map read queries,
as well as current-user and capability responses, have bounded offline snapshots in
the same cache. They are network-first online and include the complete request body
in their cache identity. Offline browsing therefore covers already fetched pages and
assets, not undiscovered library content. Mutations are never replayed from cache.

Cache identity includes server URL and a persisted random login-session namespace.
Token refresh keeps the namespace; logout or another login makes old entries
inaccessible. Old-session entries remain eligible for normal quota eviction. Cached
headers contain only content type and validators, never authorization tokens.
