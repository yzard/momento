-- Temporary, explicitly triggered backfill; no schema migration or startup work.
INSERT INTO media_metadata_jobs (media_id, status, available_at)
SELECT m.id
     , 'queued'
     , datetime('now')
  FROM media AS m
  JOIN media_metadata AS metadata ON metadata.media_id = m.id
 WHERE m.import_state = 'imported'
   AND m.media_type = 'video'
   AND (lower(m.mime_type) = 'video/quicktime' OR lower(substr(m.file_path, -4)) = '.mov')
   AND (metadata.preview_path IS NULL OR trim(metadata.preview_path) = '')
   AND NOT EXISTS (
       SELECT 1 FROM media_metadata_jobs AS job
        WHERE job.media_id = m.id AND job.status != 'completed'
   )
ON CONFLICT(media_id) DO UPDATE SET
    status = 'queued'
  , attempts = 0
  , available_at = datetime('now')
  , claim_token = NULL
  , claimed_at = NULL
  , completed_at = NULL
  , rerun_requested = 0
  , last_error = NULL
  , updated_at = datetime('now')
WHERE media_metadata_jobs.status = 'completed';
