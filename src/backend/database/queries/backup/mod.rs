pub const UPSERT_DEVICE: &str = r#"
INSERT INTO backup_devices (user_id, device_id, device_name)
VALUES (?, ?, ?)
ON CONFLICT(user_id, device_id) DO UPDATE SET
    device_name = excluded.device_name
  , last_seen_at = datetime('now')
"#;
pub const DEVICE_EXISTS: &str =
    "SELECT EXISTS(SELECT 1 FROM backup_devices WHERE user_id = ? AND device_id = ?)";
pub const SELECT_BY_OPERATION: &str = r#"
SELECT backup_upload_sessions.upload_id
     , backup_assets.status
     , backup_upload_sessions.uploaded_size
     , backup_upload_sessions.expected_size
     , backup_assets.media_id
     , backup_assets.error
     , backup_asset_manifests.content_hash
  FROM backup_assets
  JOIN backup_upload_sessions ON backup_upload_sessions.asset_id = backup_assets.id
  LEFT JOIN backup_asset_manifests ON backup_asset_manifests.asset_id = backup_assets.id
 WHERE backup_assets.user_id = ?
   AND backup_assets.operation_id = ?
"#;
pub const SELECT_BY_CLIENT_ASSET: &str = r#"
SELECT backup_upload_sessions.upload_id
     , backup_assets.status
     , backup_upload_sessions.uploaded_size
     , backup_upload_sessions.expected_size
     , backup_assets.media_id
     , backup_assets.error
     , backup_asset_manifests.content_hash
  FROM backup_assets
  JOIN backup_upload_sessions ON backup_upload_sessions.asset_id = backup_assets.id
  LEFT JOIN backup_asset_manifests ON backup_asset_manifests.asset_id = backup_assets.id
 WHERE backup_assets.user_id = ?
   AND backup_assets.device_id = ?
   AND backup_assets.client_asset_id = ?
"#;
pub const INSERT_ASSET: &str = r#"
INSERT INTO backup_assets (
    user_id
  , device_id
  , client_asset_id
  , operation_id
  , original_filename
  , mime_type
  , byte_size
  , source_modified_at
  , status
  , staged_path
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'uploading', ?)
"#;
pub const INSERT_SESSION: &str = r#"
INSERT INTO backup_upload_sessions (
    upload_id
  , asset_id
  , user_id
  , expected_size
  , status
  , expires_at
) VALUES (?, ?, ?, ?, 'uploading', datetime('now', ?))
"#;
pub const INSERT_MANIFEST: &str = r#"
INSERT INTO backup_asset_manifests (
    asset_id
  , protocol_version
  , content_hash
  , metadata_json
) VALUES (?, ?, ?, ?)
"#;
pub const SELECT_CREATE_CONTRACT: &str = r#"
SELECT backup_assets.id
     , backup_assets.device_id
     , backup_assets.client_asset_id
     , backup_assets.operation_id
     , backup_assets.original_filename
     , backup_assets.mime_type
     , backup_assets.byte_size
     , backup_assets.source_modified_at
     , backup_asset_manifests.protocol_version
     , backup_asset_manifests.content_hash
     , backup_asset_manifests.metadata_json
  FROM backup_assets
  JOIN backup_upload_sessions ON backup_upload_sessions.asset_id = backup_assets.id
  LEFT JOIN backup_asset_manifests ON backup_asset_manifests.asset_id = backup_assets.id
 WHERE backup_upload_sessions.upload_id = ?
   AND backup_upload_sessions.user_id = ?
"#;
pub const SELECT_UPLOAD: &str = r#"
SELECT backup_assets.id
     , backup_upload_sessions.upload_id
     , backup_assets.status
     , backup_upload_sessions.status
     , backup_upload_sessions.uploaded_size
     , backup_upload_sessions.expected_size
     , backup_assets.staged_path
     , backup_assets.media_id
     , backup_assets.error
     , backup_asset_manifests.content_hash
  FROM backup_assets
  JOIN backup_upload_sessions ON backup_upload_sessions.asset_id = backup_assets.id
  LEFT JOIN backup_asset_manifests ON backup_asset_manifests.asset_id = backup_assets.id
 WHERE backup_upload_sessions.upload_id = ?
   AND backup_upload_sessions.user_id = ?
"#;
pub const CLAIM_CHUNK: &str = r#"
UPDATE backup_upload_sessions
   SET status = 'writing'
     , updated_at = datetime('now')
 WHERE upload_id = ?
   AND user_id = ?
   AND status = 'uploading'
   AND uploaded_size = ?
   AND expires_at > datetime('now')
"#;
pub const COMPLETE_CHUNK: &str = r#"
UPDATE backup_upload_sessions
   SET status = 'uploading'
     , uploaded_size = ?
     , expires_at = datetime('now', ?)
     , updated_at = datetime('now')
 WHERE upload_id = ?
   AND user_id = ?
   AND status = 'writing'
   AND uploaded_size = ?
"#;
pub const ABANDON_CHUNK: &str = r#"
UPDATE backup_upload_sessions
   SET status = 'uploading'
     , updated_at = datetime('now')
 WHERE upload_id = ?
   AND user_id = ?
   AND status = 'writing'
"#;
pub const QUEUE_SESSION: &str = r#"
UPDATE backup_upload_sessions
   SET status = 'queued'
     , updated_at = datetime('now')
 WHERE upload_id = ?
   AND user_id = ?
   AND status = 'uploading'
   AND uploaded_size = expected_size
"#;
pub const QUEUE_ASSET: &str = r#"
UPDATE backup_assets
   SET status = 'queued'
     , updated_at = datetime('now')
 WHERE id = ?
   AND status = 'uploading'
"#;
pub const CANCEL_SESSION: &str = r#"
UPDATE backup_upload_sessions
   SET status = 'cancelled'
     , updated_at = datetime('now')
 WHERE upload_id = ?
   AND user_id = ?
   AND status IN ('uploading', 'queued', 'expired')
"#;
pub const CANCEL_ASSET: &str = r#"
UPDATE backup_assets
   SET status = 'cancelled'
     , updated_at = datetime('now')
 WHERE id = ?
   AND status IN ('uploading', 'queued', 'expired')
"#;
pub const CLAIM_QUEUED: &str = r#"
UPDATE backup_assets
   SET status = 'processing'
     , updated_at = datetime('now')
 WHERE id = (
    SELECT backup_assets.id
      FROM backup_assets
      JOIN backup_upload_sessions ON backup_upload_sessions.asset_id = backup_assets.id
     WHERE backup_assets.status = 'queued'
       AND backup_upload_sessions.status = 'queued'
     ORDER BY backup_assets.id
     LIMIT 1
 )
   AND status = 'queued'
RETURNING id, user_id, staged_path, source_modified_at
"#;
pub const STORE_CONTENT_HASH: &str = r#"
UPDATE backup_assets
   SET content_hash = ?
     , updated_at = datetime('now')
 WHERE id = ?
   AND status = 'processing'
"#;
pub const MARK_SESSION_PROCESSING: &str = "UPDATE backup_upload_sessions SET status = 'processing', updated_at = datetime('now') WHERE asset_id = ? AND status = 'queued'";
pub const SELECT_MANIFEST_FOR_ASSET: &str =
    "SELECT content_hash, metadata_json FROM backup_asset_manifests WHERE asset_id = ?";
pub const SELECT_STAGED_PATH_FOR_ASSET: &str = "SELECT staged_path FROM backup_assets WHERE id = ?";
pub const COMPLETE_ASSET: &str = "UPDATE backup_assets SET status = 'completed', media_id = ?, error = NULL, completed_at = datetime('now'), updated_at = datetime('now') WHERE id = ? AND status = 'processing'";
pub const COMPLETE_SESSION: &str = "UPDATE backup_upload_sessions SET status = 'completed', updated_at = datetime('now') WHERE asset_id = ? AND status = 'processing'";
pub const FAIL_ASSET: &str = "UPDATE backup_assets SET status = 'failed', error = ?, updated_at = datetime('now') WHERE id = ? AND status = 'processing'";
pub const FAIL_SESSION: &str = "UPDATE backup_upload_sessions SET status = 'failed', updated_at = datetime('now') WHERE asset_id = ? AND status = 'processing'";
pub const SELECT_PROCESSING_ASSETS_PAGE: &str = r#"
SELECT id
     , user_id
     , staged_path
     , content_hash
  FROM backup_assets
 WHERE status = 'processing'
   AND id > ?
 ORDER BY id
 LIMIT ?
"#;
pub const SELECT_RECOVERED_MEDIA: &str = r#"
SELECT media.id
  FROM media
  JOIN media_access ON media_access.media_id = media.id
 WHERE media.content_hash = ?
   AND media.import_state = 'imported'
   AND media_access.user_id = ?
   AND media_access.deleted_at IS NULL
 LIMIT 1
"#;
pub const RECOVER_QUEUED_ASSET: &str = "UPDATE backup_assets SET status = 'queued', updated_at = datetime('now') WHERE id = ? AND status = 'processing'";
pub const RECOVER_QUEUED_SESSION: &str = "UPDATE backup_upload_sessions SET status = 'queued', updated_at = datetime('now') WHERE asset_id = ? AND status = 'processing'";
pub const RECOVER_WRITING_SESSIONS: &str = "UPDATE backup_upload_sessions SET status = 'uploading', updated_at = datetime('now') WHERE status = 'writing'";
pub const SELECT_RESUMABLE_FILES_PAGE: &str = r#"
SELECT backup_assets.id
     , backup_assets.staged_path
     , backup_upload_sessions.uploaded_size
  FROM backup_assets
  JOIN backup_upload_sessions ON backup_upload_sessions.asset_id = backup_assets.id
 WHERE backup_upload_sessions.status = 'uploading'
   AND backup_assets.id > ?
 ORDER BY backup_assets.id
 LIMIT ?
"#;
pub const FAIL_MISSING_STAGED_ASSET: &str = "UPDATE backup_assets SET status = 'failed', error = 'backup staging file is missing', updated_at = datetime('now') WHERE id = ? AND status = 'uploading'";
pub const FAIL_MISSING_STAGED_SESSION: &str = "UPDATE backup_upload_sessions SET status = 'failed', updated_at = datetime('now') WHERE asset_id = ? AND status = 'uploading'";
pub const EXPIRE_SESSIONS: &str = "UPDATE backup_upload_sessions SET status = 'expired', updated_at = datetime('now') WHERE status = 'uploading' AND expires_at <= datetime('now')";
pub const EXPIRE_ASSETS: &str = "UPDATE backup_assets SET status = 'expired', updated_at = datetime('now') WHERE id IN (SELECT asset_id FROM backup_upload_sessions WHERE status = 'expired') AND status = 'uploading'";
pub const SELECT_NEXT_EXPIRATION_SECONDS: &str = "SELECT CAST((julianday(MIN(expires_at)) - julianday('now')) * 86400 AS INTEGER) + 1 FROM backup_upload_sessions WHERE status = 'uploading'";

pub const RETIRE_CLIENT_ASSET: &str = "UPDATE backup_assets SET client_asset_id = client_asset_id || '#' || ? WHERE user_id = ? AND device_id = ? AND client_asset_id = ? AND status IN ('cancelled', 'expired', 'failed', 'completed')";
