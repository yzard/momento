# Face detection and grouping

Read when changing face validation, crops, grouping, representatives, or group ordering.

### Face detection and grouping

The `face_detection` service requires `minimum_face_likelihood` in `(0, 1]` and a positive
`minimum_face_resolution_pixels`. These values belong to the llm-service face service entry and
are passed explicitly to the local runtime. A detection is returned only when its confidence
reaches the likelihood threshold and both detected face-box dimensions reach the configured
source-pixel resolution. Tests that load playground configuration validate the values and
required ranges rather than pinning locally tuned thresholds.

Face results include a normalized `eyeCenter` derived from InsightFace's first two landmarks and a
normalized `frontalityScore` derived from all five landmarks. Frontality accounts for eye-line
roll plus nose and mouth-center horizontal offsets, is constrained to `[0, 1]`, and is persisted
with the face row. The runtime keeps one CUDA BiSeNet ResNet18 ONNX session beside Buffalo L,
parses dynamically batched aligned faces at the model's fixed 512x512 input, and derives
`visibilityScore` from the expected eye, nose, and mouth regions. It derives
`featureClarityScore` from local edge strength inside the visible semantic regions. The old
combined `qualityScore` is replaced by independent `faceSizeScore`, visibility, and feature
clarity values. Momento keeps the 256x256 portrait output size and the existing crop dimensions;
only the crop origin changes so the portrait is centered on `eyeCenter`, subject to image-edge
clamping. Face crops reference the immutable submitted original snapshot. Momento uses ImageMagick
to select its first frame, apply stored orientation, and normalize it to PNG in memory before Rust
decodes and crops it; conversion failures are logged and fail only the corresponding Momento job.

Automatic grouping processes faces in face-ID order and compares each embedding against the fixed
seed embedding that first created each automatic group. A face joins the first automatic seed whose
cosine similarity reaches `face_group.similarity_threshold`; the default is `0.50`, lower values
are more tolerant, and higher values are stricter. Grouping is deliberately greedy: it does not use
the thumbnail representative, compare every automatic member pair, apply transitive closure, or run
a second group-to-group merge pass. A manual merge makes every face selected by that merge a fixed
manual anchor. Later detections compare against every anchor and join the best matching manual group
before automatic grouping. Automatically attached members are reevaluated on each run and never
become anchors, preventing transitive similarity drift. Manual groups and their anchors are never
deleted by automatic regrouping. Changing these semantics requires explicit false-merge analysis
and grouping tests, not just changing the thumbnail representative.

`face_groups.representative_face_id` is a thumbnail choice, not the grouping seed. Select it only
after automatic membership is complete and select it again after a manual merge. Rank each face by
the six weights in `[face_group]`: confidence, face size, center proximity,
frontality, visibility, and feature clarity. The weights must be non-negative and sum to `1`.
Center proximity normalizes squared face-box-center distance from the media center to `[0, 1]`;
higher weighted scores win and face ID ascending is the deterministic tie-breaker. Recompute all
stored representatives at startup so configuration changes apply to existing groups. If the global
representative is not visible to a requesting user, thumbnail lookup applies the same configured
score to that user's accessible members.

The face-group list is sorted in the backend before `LIMIT`/`OFFSET`: distinct visible media count
descending, then face-group ID ascending as the stable tie-breaker. Do not sort paginated face
groups in the frontend, and do not use raw face count in place of distinct accessible media count.

### Administrator false-positive exclusions

`POST /api/v1/faces/reject` requires an administrator and an idempotency `requestId` UUID.
Choose either `groupIds` (up to 200 groups) or `faceGroupId` with exact `faceIds`;
never select by media ID alone. A request excludes at most 4096 detections. Selection is
resolved through the administrator's active `media_access`; an invalid exact selection
rolls back the whole operation. Group selection includes only currently visible members.
Responses contain `rejectedCount`; clients reload affected Faces queries and representatives.
Reusing a request ID with a different user or selection conflicts.

Exclusions are global, independent records keyed by original content hash, normalized region,
and original video frame timestamp (null for images). Persistence suppresses candidates with
IoU at least 0.70 and center shifts no greater than 20% of each rejected box dimension.
Input sequence order is not a frame identity. Other regions and frames remain eligible.
The model can still return the candidate; suppression prevents it from becoming a stored face.
These records survive regrouping, normal re-detection, media-row replacement and service restart.
Only administrator Clean AI Face Data resets them and their request replay records.

The writer transaction records exclusions, deletes exact detections and their memberships,
updates group representatives, advances the manual revision checked by grouping finalization,
and journals orphaned crops for durable cleanup. Suppressed newly generated crops also enter
the existing result cleanup workflow. Originals are never deleted by rejection.
Representative reduction scans bounded pages with the same score and ID tie-breaker.

This changes the current SQLite schema (including frame timestamps on detected faces).
Following the repository's schema contract, deploy with a fresh database/data directory;
there is no automatic migration or deletion of existing data.
For this specific change, an explicitly requested standalone
[manual NAS upgrade](../faces-database-upgrade-20260910.md) preserves the existing database
after verification and backup. It is not invoked by application startup.

### Collection navigation and selection

WebGUI and Android retain Places/Faces list state beneath detail overlays. Returning closes
one layer at a time and restores the list scroll anchor, loaded pages and selection. Web
Escape/Backspace excludes editable inputs; browser history also represents an open media viewer.
A directly opened detail URL closes to its collection list.

Faces selection bars show selected face thumbnails. Detail titles use `Face Group #<groupId>`.
Only administrators see rejection controls. Selecting a media item with one group detection
selects that detection; media with multiple detections requires explicit crop selection.
Mutation success invalidates local Faces metadata and representative caches, including Android
persisted snapshots and in-flight cache publication. Other clients refresh through their
existing on-demand freshness checks; exclusion is authoritative in backend reads immediately.
