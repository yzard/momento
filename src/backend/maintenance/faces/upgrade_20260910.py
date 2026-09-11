#!/usr/bin/env python3
"""Standalone, manual Faces schema upgrade. Python 3.8+; standard library only."""

import argparse
import hashlib
import json
import os
import re
import sqlite3
import sys
from pathlib import Path

# Frozen snapshot for this one-time upgrade; intentionally independent of future schema.sql.
DDL = r"""
CREATE TABLE IF NOT EXISTS face_rejections (
    face_id INTEGER PRIMARY KEY,
    content_hash TEXT NOT NULL,
    input_sequence INTEGER NOT NULL,
    frame_timestamp_ms INTEGER,
    x REAL NOT NULL, y REAL NOT NULL, width REAL NOT NULL, height REAL NOT NULL,
    crop_path TEXT NOT NULL,
    rejected_by INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_face_rejections_input ON face_rejections(content_hash, frame_timestamp_ms);
CREATE TABLE IF NOT EXISTS face_rejection_operations (
    request_id TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL,
    selection TEXT NOT NULL,
    rejected_count INTEGER NOT NULL
);
CREATE TRIGGER IF NOT EXISTS suppress_rejected_face BEFORE INSERT ON media_faces
WHEN EXISTS (
    SELECT 1 FROM face_rejections r JOIN media m ON m.id = NEW.media_id
    WHERE r.content_hash = m.content_hash AND r.frame_timestamp_ms IS NEW.frame_timestamp_ms
      AND ABS((NEW.x + NEW.width/2) - (r.x + r.width/2)) <= r.width * 0.2
      AND ABS((NEW.y + NEW.height/2) - (r.y + r.height/2)) <= r.height * 0.2
      AND MAX(0, MIN(NEW.x+NEW.width,r.x+r.width)-MAX(NEW.x,r.x))
          * MAX(0, MIN(NEW.y+NEW.height,r.y+r.height)-MAX(NEW.y,r.y))
          >= 0.7 * (NEW.width*NEW.height + r.width*r.height
          - MAX(0, MIN(NEW.x+NEW.width,r.x+r.width)-MAX(NEW.x,r.x))
          * MAX(0, MIN(NEW.y+NEW.height,r.y+r.height)-MAX(NEW.y,r.y)))
)
BEGIN SELECT RAISE(IGNORE); END;
"""


EXPECTED_MANIFEST_HASH = 'f64b3c11d794d337237c6a671e6e7e715c61316ab5eda0b42ed873977b13d152'
MEDIA_FACES_DDL = "CREATE TABLE media_faces (\n    id INTEGER PRIMARY KEY AUTOINCREMENT,\n    media_id INTEGER NOT NULL,\n    input_sequence INTEGER NOT NULL,\n    face_index INTEGER NOT NULL,\n    frame_timestamp_ms INTEGER,\n    x REAL NOT NULL,\n    y REAL NOT NULL,\n    width REAL NOT NULL,\n    height REAL NOT NULL,\n    confidence REAL NOT NULL CHECK(confidence >= 0.0 AND confidence <= 1.0),\n    face_size_score REAL NOT NULL CHECK(face_size_score >= 0.0 AND face_size_score <= 1.0),\n    frontality_score REAL NOT NULL CHECK(frontality_score >= 0.0 AND frontality_score <= 1.0),\n    visibility_score REAL NOT NULL CHECK(visibility_score >= 0.0 AND visibility_score <= 1.0),\n    feature_clarity_score REAL NOT NULL CHECK(feature_clarity_score >= 0.0 AND feature_clarity_score <= 1.0),\n    embedding BLOB NOT NULL,\n    crop_path TEXT NOT NULL,\n    created_at TEXT NOT NULL DEFAULT (datetime('now')),\n    UNIQUE(media_id, input_sequence, face_index),\n    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE\n)"


class UpgradeError(Exception):
    pass


def statements():
    pending = ""
    for line in DDL.splitlines():
        pending += line + "\n"
        if sqlite3.complete_statement(pending):
            yield pending.strip()
            pending = ""
    if pending.strip():
        raise UpgradeError("Incomplete embedded SQL")


def normalize(sql):
    return re.sub(r"\s+", "", sql.lower().replace("if not exists", "")).rstrip(";")


def columns(db, table):
    # Callers supply fixed identifiers only.
    return {row[1]: row for row in db.execute('PRAGMA table_info("' + table + '")')}


def manifest(db):
    return list(
        db.execute(
            "SELECT type,name,tbl_name,coalesce(sql,'') FROM sqlite_master "
            "WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name"
        )
    )


def validate_manifest(db, planned):
    entries = {row[1]: row for row in manifest(db)}
    if planned:
        with sqlite3.connect(":memory:") as expected:
            expected.execute(MEDIA_FACES_DDL)
            for sql in statements():
                expected.execute(sql)
            entries.update({row[1]: row for row in manifest(expected)})
    digest = hashlib.sha256(json.dumps(sorted(entries.values(), key=lambda row: (row[0], row[1]))).encode()).hexdigest()
    if digest != EXPECTED_MANIFEST_HASH:
        raise UpgradeError("Full schema differs from the supported release; refusing unexpected changes")


def rebuild_faces(db):
    actual = db.execute("SELECT sql FROM sqlite_master WHERE name='media_faces'").fetchone()[0]
    if actual == MEDIA_FACES_DDL:
        return
    related = db.execute(
        "SELECT sql FROM sqlite_master WHERE tbl_name='media_faces' "
        "AND type IN ('index','trigger') AND sql IS NOT NULL"
    ).fetchall()
    sequence = db.execute("SELECT seq FROM sqlite_sequence WHERE name='media_faces'").fetchone()
    fields = ','.join('"' + name + '"' for name in columns(db, "media_faces"))
    db.execute("CREATE TABLE momento_faces_upgrade_copy_20260910 AS SELECT * FROM media_faces")
    db.execute("DROP TABLE media_faces")
    db.execute(MEDIA_FACES_DDL)
    db.execute(
        "INSERT INTO media_faces (" + fields + ") SELECT " + fields + " FROM momento_faces_upgrade_copy_20260910"
    )
    if db.execute(
        "SELECT " + fields + " FROM momento_faces_upgrade_copy_20260910 EXCEPT SELECT " + fields + " FROM media_faces"
    ).fetchone():
        raise UpgradeError("Face copy verification failed")
    if sequence:
        db.execute("UPDATE sqlite_sequence SET seq=MAX(seq,?) WHERE name='media_faces'", sequence)
    for (sql,) in related:
        db.execute(sql)
    db.execute("DROP TABLE momento_faces_upgrade_copy_20260910")


def inspect(db):
    required = {
        "media": {"id", "media_type", "content_hash"},
        "media_faces": {
            "id",
            "media_id",
            "input_sequence",
            "face_index",
            "x",
            "y",
            "width",
            "height",
            "confidence",
            "face_size_score",
            "frontality_score",
            "visibility_score",
            "feature_clarity_score",
            "embedding",
            "crop_path",
            "created_at",
        },
        "media_ai_inputs": {"media_id", "task", "sequence", "input_kind", "frame_timestamp_ms"},
        "face_group_manual_state": {"id", "revision"},
        "face_group_members": {"face_id", "face_group_id", "manual_anchor", "automatic_generation_id"},
        "file_operation_groups": {"id", "owner_kind", "state"},
    }
    for table, expected in required.items():
        missing = expected - columns(db, table).keys()
        if missing:
            raise UpgradeError("Unsupported baseline: %s is missing %s" % (table, sorted(missing)))
    timestamp = columns(db, "media_faces").get("frame_timestamp_ms")
    if timestamp and (timestamp[2].upper() != "INTEGER" or timestamp[3] or timestamp[4] is not None):
        raise UpgradeError("Existing frame_timestamp_ms has an incompatible definition")
    missing_objects = []
    for sql in statements():
        name = re.search(r"CREATE (?:TABLE|INDEX|TRIGGER) IF NOT EXISTS (\w+)", sql)[1]
        existing = db.execute("SELECT sql FROM sqlite_master WHERE name=?", (name,)).fetchone()
        if existing and normalize(existing[0]) != normalize(sql):
            raise UpgradeError("Existing object has an unexpected definition: " + name)
        if not existing:
            missing_objects.append(sql)
    # Legacy video detections must have an exact source frame descriptor. Never infer by FPS.
    timestamp_filter = "AND f.frame_timestamp_ms IS NULL" if timestamp else ""
    unresolved = db.execute(
        """
        SELECT COUNT(*) FROM media_faces f JOIN media m ON m.id=f.media_id
        WHERE m.media_type='video' """
        + timestamp_filter
        + """
        AND NOT EXISTS (SELECT 1 FROM media_ai_inputs i
            WHERE i.media_id=f.media_id AND i.task='face_detection'
              AND i.sequence=f.input_sequence AND i.input_kind='video_frame'
              AND i.frame_timestamp_ms IS NOT NULL AND i.frame_timestamp_ms >= 0)
    """
    ).fetchone()[0]
    if unresolved:
        raise UpgradeError(
            "%d video face detections lack source frame timestamps; nothing changed. "
            "Recover their face_detection input descriptors before upgrading." % unresolved
        )
    missing_hash = db.execute("""SELECT COUNT(*) FROM media_faces f JOIN media m ON m.id=f.media_id
        WHERE m.content_hash IS NULL OR m.content_hash=''""").fetchone()[0]
    if missing_hash:
        raise UpgradeError("Face media lack original content hashes; nothing changed")
    backfill = db.execute("""SELECT COUNT(*) FROM media_faces f JOIN media m ON m.id=f.media_id
        WHERE m.media_type='video' """ + timestamp_filter).fetchone()[0]
    validate_manifest(db, True)
    return timestamp is None, missing_objects, backfill


def check_integrity(db):
    if db.execute("PRAGMA integrity_check").fetchall() != [("ok",)]:
        raise UpgradeError("Database integrity check failed")
    if db.execute("PRAGMA foreign_key_check").fetchone() is not None:
        raise UpgradeError("Database has foreign-key violations")


def upgrade(database, backup, apply):
    database = database.resolve(strict=True)
    uri = database.as_uri()
    db = sqlite3.connect(uri + ("?mode=rw" if apply else "?mode=ro"), uri=True, timeout=5, isolation_level=None)
    try:
        db.execute("PRAGMA foreign_keys=OFF")
        # Acquire the writer reservation before inspecting and backing up. Services must be stopped.
        db.execute("BEGIN IMMEDIATE" if apply else "BEGIN")
        check_integrity(db)
        add_column, additions, backfill = inspect(db)
        table_differs = (
            db.execute("SELECT sql FROM sqlite_master WHERE name='media_faces'").fetchone()[0] != MEDIA_FACES_DDL
        )
        if not add_column and not additions and not backfill and not table_differs:
            print("Already upgraded; no changes or backup needed.")
            db.rollback()
            return
        print(
            "Upgrade: add timestamp column=%s, create %d objects, backfill %d video detections."
            % (add_column, len(additions), backfill)
        )
        if not apply:
            print("Check passed; database unchanged. Stop Momento before using --apply --services-stopped.")
            db.rollback()
            return
        backup = backup.absolute()
        # Never overwrite any existing file, including symlinks. SQLite backup includes committed WAL.
        fd = os.open(str(backup), os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
        os.close(fd)
        try:
            source = sqlite3.connect(uri + "?mode=ro", uri=True, timeout=5)
            target = sqlite3.connect(str(backup))
            try:
                source.backup(target)
                check_integrity(target)
            finally:
                target.close()
                source.close()
            with backup.open("rb") as saved:
                os.fsync(saved.fileno())
        except Exception:
            backup.unlink()
            raise
        print("Verified backup: " + str(backup), flush=True)
        if add_column:
            db.execute("ALTER TABLE media_faces ADD COLUMN frame_timestamp_ms INTEGER")
        db.execute("""UPDATE media_faces SET frame_timestamp_ms=(
            SELECT i.frame_timestamp_ms FROM media_ai_inputs i
            WHERE i.media_id=media_faces.media_id AND i.task='face_detection'
              AND i.sequence=media_faces.input_sequence AND i.input_kind='video_frame')
            WHERE frame_timestamp_ms IS NULL AND media_id IN (SELECT id FROM media WHERE media_type='video')""")
        rebuild_faces(db)
        for sql in additions:
            db.execute(sql)
        validate_manifest(db, False)
        check_integrity(db)
        if any(inspect(db)):
            raise UpgradeError("Post-upgrade verification failed")
        db.commit()
        print("Upgrade complete. Media, face IDs, groups and access records preserved.")
    finally:
        if db.in_transaction:
            db.rollback()
        db.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--database", type=Path, required=True, help="Existing Momento SQLite database")
    parser.add_argument("--apply", action="store_true", help="Apply changes; default is read-only checking")
    parser.add_argument(
        "--services-stopped", action="store_true", help="Confirm all Momento database writers are stopped"
    )
    parser.add_argument("--backup", type=Path, help="New backup file path; required with --apply")
    args = parser.parse_args()
    if args.apply and (not args.services_stopped or args.backup is None):
        parser.error("--apply requires --services-stopped and --backup PATH")
    try:
        upgrade(args.database, args.backup, args.apply)
    except (OSError, sqlite3.Error, UpgradeError) as error:
        print("Upgrade stopped: " + str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
