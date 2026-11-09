# Import source replay after interruption

An allocated import now persists its source storage root, complete relative path,
source kind, user, and source/sidecar cleanup policy in `media`. The root plus path
resolves below the configured mounted volume, rather than retaining a temporary
file as the only source reference.

Recovery first rolls back the interrupted Journal publication. Size/hash checks
against a complete source are not applied to an unfinished temporary original or
sidecar: deletion still requires an exact write claim owned by that import and an
application-owned `.importing` temporary namespace. Other files retain their
normal evidence checks. This does not authorize deleting the import source or an
unrelated canonical original.

Once Journal rollback and live content-hash ownership have settled, the media row
becomes `recovering`. Replay uses the normal import implementation: reopen and
hash the source, copy and validate, publish, grant access and enqueue metadata.
The existing media ID is reused where possible. If identical content was already
imported, the normal duplicate path is used and only the uncommitted placeholder
row is retired. Already imported or explicitly failed rows are not reset by a
late recovery attempt.

Transient failures retain the source descriptor, release admission and retry at
the tail. Source absence or temporary I/O failure does not become a business
failure. Explicit validation/unsupported-media rejection does. Recovery uses the
existing scheduler and executor domains, without adding OS threads. A restart
reconstructs outstanding replay work from the database.

The old directory scan itself has no durable per-file cursor. Its aggregate
status becomes `interrupted`, not `failed` or a permanently ownerless `running`.
Allocated media are automatically replayed; files never allocated before the
restart still require another local scan (WebDAV retains its ready-file queue).
The interrupted aggregate counters are historical, not fabricated recovery totals.

## Deployment boundary

This change updates the current schema and intentionally adds no migration.
An existing database with the old schema cannot be used unchanged with this
build. Existing records also lack source descriptors; the code cannot infer
those paths reliably. No production database reset, backfill, or source deletion
is performed by this change. Any production data transition requires a separate
explicit decision and backup.

## Regression coverage

- Owned partial original/sidecar rollback; unowned/out-of-namespace files rejected.
- Sources and canonical originals preserved during partial-file rollback.
- Local/WebDAV/Backup source replay with the original media ID and one metadata job.
- Temporarily missing source returns to processing when it becomes available.
- Explicit unsupported-media rejection is terminal.
- Active content-hash owner and terminal media states cannot be reset by replay.
- Missing source descriptors are preserved and reported, not marked failed.
