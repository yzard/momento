import importlib.util
import os
import sqlite3
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[4]
SPEC = importlib.util.spec_from_file_location(
    "faces_upgrade", ROOT / "src/backend/maintenance/faces/upgrade_20260910.py"
)
upgrade = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(upgrade)


class UpgradeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir=os.environ["TMPDIR"])
        self.addCleanup(self.temp.cleanup)
        self.database = Path(self.temp.name) / "library.db"
        self.backup = Path(self.temp.name) / "backup.db"
        schema = (ROOT / "src/backend/database/schema.sql").read_text()
        schema = schema.replace(upgrade.DDL.strip(), "")
        schema = schema.replace(
            "    face_index INTEGER NOT NULL,\n    frame_timestamp_ms INTEGER,", "    face_index INTEGER NOT NULL,"
        )
        self.db = sqlite3.connect(self.database)
        self.addCleanup(self.db.close)
        self.db.executescript(schema)
        self.db.execute(
            "INSERT INTO media (id,filename,original_filename,file_path,media_type,content_hash) VALUES (1,'a','a','a','image','image-hash'),(2,'b','b','b','video','video-hash')"
        )
        for media in (1, 2):
            self.db.execute(
                "INSERT INTO media_faces (media_id,input_sequence,face_index,x,y,width,height,confidence,face_size_score,frontality_score,visibility_score,feature_clarity_score,embedding,crop_path) VALUES (?,0,0,0.1,0.1,0.2,0.2,1,1,1,1,1,X'00','faces/a.jpg')",
                (media,),
            )
        self.db.execute(
            "INSERT INTO media_ai_inputs (media_id,task,sequence,input_kind,storage_root,file_path,filename,mime_type,byte_size,content_hash,frame_timestamp_ms) VALUES (2,'face_detection',0,'video_frame','previews','frame','frame','image/jpeg',1,'frame-hash',1234)"
        )
        self.db.execute("INSERT INTO face_groups (id,representative_face_id) VALUES (1,1)")
        self.db.execute("INSERT INTO face_group_members (face_group_id,face_id,manual_anchor) VALUES (1,1,1)")
        self.db.commit()

    def test_check_backup_wal_backfill_idempotency_and_trigger(self):
        self.db.execute("PRAGMA journal_mode=WAL")
        self.db.execute("UPDATE media SET filename='committed-in-wal' WHERE id=1")
        self.db.commit()
        upgrade.upgrade(self.database, None, False)
        self.assertNotIn("frame_timestamp_ms", upgrade.columns(self.db, "media_faces"))
        upgrade.upgrade(self.database, self.backup, True)
        with sqlite3.connect(self.backup) as backup:
            self.assertNotIn("frame_timestamp_ms", upgrade.columns(backup, "media_faces"))
            self.assertEqual(backup.execute("SELECT filename FROM media WHERE id=1").fetchone()[0], "committed-in-wal")
            self.assertEqual(backup.execute("SELECT COUNT(*) FROM media_faces").fetchone()[0], 2)
        self.assertEqual(
            self.db.execute("SELECT frame_timestamp_ms FROM media_faces ORDER BY id").fetchall(), [(None,), (1234,)]
        )
        self.assertEqual(self.db.execute("SELECT face_id FROM face_group_members").fetchall(), [(1,)])
        # Re-running does not overwrite the original backup or modify data.
        before = self.backup.read_bytes()
        upgrade.upgrade(self.database, self.backup, True)
        self.assertEqual(before, self.backup.read_bytes())
        self.db.execute(
            "INSERT INTO face_rejections (face_id,content_hash,input_sequence,x,y,width,height,crop_path,rejected_by) VALUES (1,'image-hash',0,0.1,0.1,0.2,0.2,'faces/a.jpg',1)"
        )
        self.db.execute("DELETE FROM media_faces WHERE id=1")
        fields = "media_id,input_sequence,face_index,x,y,width,height,confidence,face_size_score,frontality_score,visibility_score,feature_clarity_score,embedding,crop_path"
        self.assertEqual(
            self.db.execute(
                "INSERT INTO media_faces ("
                + fields
                + ") VALUES (1,0,1,0.105,0.1,0.2,0.2,1,1,1,1,1,X'00','faces/new.jpg')"
            ).rowcount,
            0,
        )
        self.assertEqual(
            self.db.execute(
                "INSERT INTO media_faces ("
                + fields
                + ") VALUES (1,0,2,0.7,0.1,0.2,0.2,1,1,1,1,1,X'00','faces/true.jpg')"
            ).rowcount,
            1,
        )

    def test_missing_video_descriptor_stops_without_change(self):
        self.db.execute("DELETE FROM media_ai_inputs")
        self.db.commit()
        with self.assertRaises(upgrade.UpgradeError):
            upgrade.upgrade(self.database, self.backup, True)
        self.assertFalse(self.backup.exists())
        self.assertNotIn("frame_timestamp_ms", upgrade.columns(self.db, "media_faces"))

    def test_existing_backup_is_not_overwritten(self):
        self.backup.write_bytes(b"keep")
        with self.assertRaises(FileExistsError):
            upgrade.upgrade(self.database, self.backup, True)
        self.assertEqual(self.backup.read_bytes(), b"keep")
        self.assertNotIn("frame_timestamp_ms", upgrade.columns(self.db, "media_faces"))

    def test_incompatible_existing_object_rejected(self):
        self.db.execute("CREATE TABLE face_rejections (face_id INTEGER)")
        self.db.commit()
        with self.assertRaises(upgrade.UpgradeError):
            upgrade.upgrade(self.database, self.backup, True)
        self.assertFalse(self.backup.exists())

    def test_postcheck_failure_rolls_back_and_keeps_backup(self):
        original = upgrade.inspect
        calls = 0

        def inspect(db):
            nonlocal calls
            calls += 1
            if calls == 2:
                raise upgrade.UpgradeError("injected failure")
            return original(db)

        with patch.object(upgrade, "inspect", side_effect=inspect):
            with self.assertRaises(upgrade.UpgradeError):
                upgrade.upgrade(self.database, self.backup, True)
        self.assertTrue(self.backup.exists())
        self.assertNotIn("frame_timestamp_ms", upgrade.columns(self.db, "media_faces"))
        self.assertIsNone(self.db.execute("SELECT name FROM sqlite_master WHERE name='face_rejections'").fetchone())

    def test_repairs_alter_column_layout_and_preserves_sequence(self):
        self.db.execute("ALTER TABLE media_faces ADD COLUMN frame_timestamp_ms INTEGER")
        self.db.execute("UPDATE media_faces SET frame_timestamp_ms=1234 WHERE media_id=2")
        self.db.execute("UPDATE sqlite_sequence SET seq=1000 WHERE name='media_faces'")
        for sql in upgrade.statements():
            self.db.execute(sql)
        self.db.commit()
        upgrade.upgrade(self.database, self.backup, True)
        upgrade.validate_manifest(self.db, False)
        self.assertEqual(
            self.db.execute("SELECT seq FROM sqlite_sequence WHERE name='media_faces'").fetchone()[0], 1000
        )
        self.assertEqual(
            self.db.execute("SELECT id,frame_timestamp_ms FROM media_faces ORDER BY id").fetchall(),
            [(1, None), (2, 1234)],
        )
        self.assertEqual(self.db.execute("SELECT face_id FROM face_group_members").fetchall(), [(1,)])

    def test_fresh_current_schema_needs_no_upgrade(self):
        current = self.database.parent / "current.db"
        with sqlite3.connect(current) as db:
            db.executescript((ROOT / "src/backend/database/schema.sql").read_text())
        upgrade.upgrade(current, self.backup, True)
        self.assertFalse(self.backup.exists())

    def test_foreign_key_violation_stops_before_backup(self):
        self.db.execute("INSERT INTO face_group_members (face_group_id,face_id,manual_anchor) VALUES (1,999,1)")
        self.db.commit()
        with self.assertRaises(upgrade.UpgradeError):
            upgrade.upgrade(self.database, self.backup, True)
        self.assertFalse(self.backup.exists())
        self.assertNotIn("frame_timestamp_ms", upgrade.columns(self.db, "media_faces"))

    def test_missing_database_is_never_created(self):
        missing = self.database.parent / "missing.db"
        with self.assertRaises(FileNotFoundError):
            upgrade.upgrade(missing, self.backup, True)
        self.assertFalse(missing.exists())


if __name__ == "__main__":
    unittest.main()
