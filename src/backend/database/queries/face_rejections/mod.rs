pub const VISIBLE_FACES: &str = r#"
SELECT DISTINCT f.id, f.media_id, f.input_sequence, f.x, f.y, f.width, f.height
FROM media_faces f JOIN face_group_members gm ON gm.face_id=f.id
JOIN media_access ma ON ma.media_id=f.media_id AND ma.user_id=?2 AND ma.deleted_at IS NULL
WHERE gm.face_group_id=?1 AND (gm.automatic_generation_id IS NULL OR gm.automatic_generation_id=(SELECT active_generation_id FROM face_group_generation_state WHERE id=1))
ORDER BY f.id LIMIT 4097"#;
pub const RECORD: &str = "INSERT OR IGNORE INTO face_rejections SELECT f.id,m.content_hash,f.input_sequence,f.frame_timestamp_ms,f.x,f.y,f.width,f.height,f.crop_path,?2,datetime('now') FROM media_faces f JOIN media m ON m.id=f.media_id WHERE f.id=?1";
pub const DELETE_FACE: &str = "DELETE FROM media_faces WHERE id=?";
pub const AFFECTED_GROUPS: &str =
    "SELECT DISTINCT face_group_id FROM face_group_members WHERE face_id=?";
pub const PREVIOUS_OPERATION: &str =
    "SELECT user_id,selection,rejected_count FROM face_rejection_operations WHERE request_id=?";
pub const SAVE_OPERATION: &str = "INSERT INTO face_rejection_operations VALUES (?,?,?,?)";
pub const CLEAN: &str = "DELETE FROM face_rejections";
pub const CLEAN_OPERATIONS: &str = "DELETE FROM face_rejection_operations";

pub const VISIBLE_CROP: &str = "SELECT f.crop_path FROM media_faces f JOIN media_access ma ON ma.media_id=f.media_id WHERE f.id=? AND ma.user_id=? AND ma.deleted_at IS NULL";

pub const ORPHAN_CROP: &str = "SELECT r.crop_path FROM face_rejections r WHERE r.face_id=? AND NOT EXISTS (SELECT 1 FROM media_faces f WHERE f.crop_path=r.crop_path)";
