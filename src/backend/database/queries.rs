pub mod import {
    pub const COUNT_IMPORTED_MEDIA: &str = r#"
    SELECT COUNT(*)
      FROM media
     WHERE import_state = 'imported'
    "#;

    pub const INSERT_IMPORTING_MEDIA: &str = r#"
    INSERT INTO media (
        user_id
      , filename
      , original_filename
      , file_path
      , media_type
      , mime_type
      , file_size
      , content_hash
      , created_at
      , import_state
      , import_source
    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, COALESCE(datetime(?, 'unixepoch'), datetime('now')), 'importing', ?)
    ON CONFLICT DO NOTHING
    "#;

    pub const SELECT_BY_CONTENT_HASH: &str = r#"
    SELECT id
         , file_path
         , import_state
      FROM media
     WHERE content_hash = ?
     LIMIT 1
    "#;

    pub const UPDATE_EARLIER_CREATED_AT: &str = r#"
    UPDATE media
       SET created_at = datetime(?, 'unixepoch')
     WHERE id = ?
       AND created_at > datetime(?, 'unixepoch')
    "#;

    pub const MARK_IMPORTED: &str = r#"
    UPDATE media
       SET filename = ?
         , file_path = ?
         , import_state = 'imported'
         , import_error = NULL
         , imported_at = datetime('now')
     WHERE id = ?
       AND import_state = 'importing'
    "#;

    pub const MARK_FAILED: &str = r#"
    UPDATE media
       SET import_state = 'failed'
         , import_error = ?
         , content_hash = NULL
     WHERE id = ?
       AND import_state = 'importing'
    "#;

    pub const SELECT_INTERRUPTED: &str = r#"
    SELECT id
         , file_path
         , original_filename
         , user_id
      FROM media
     WHERE import_state = 'importing'
     ORDER BY id
    "#;
    pub const FAIL_INTERRUPTED_JOBS: &str = r#"
    UPDATE import_jobs
       SET status = 'failed'
         , completed_at = datetime('now')
         , last_error = 'import interrupted by service restart'
     WHERE status = 'running'
    "#;
    pub const INSERT_JOB: &str = "INSERT INTO import_jobs (source, status) VALUES (?, 'running')";
    pub const SELECT_LATEST_JOB_FOR_SOURCE: &str = "SELECT status, total_files, processed_files, successful_imports, failed_imports, started_at, completed_at, last_error FROM import_jobs WHERE source = ? ORDER BY id DESC LIMIT 1";
    pub const SET_JOB_TOTAL: &str =
        "UPDATE import_jobs SET total_files = ? WHERE id = ? AND status = 'running'";
    pub const SET_WEBDAV_JOB_TOTAL: &str = "UPDATE import_jobs SET total_files = ? WHERE id = ?";
    pub const COMPLETE_JOB: &str = "UPDATE import_jobs SET status = CASE WHEN failed_imports = 0 THEN 'completed' ELSE 'failed' END, completed_at = datetime('now') WHERE id = ? AND status = 'running'";
    pub const UPDATE_JOB_PROGRESS: &str = "UPDATE import_jobs SET processed_files = processed_files + 1, successful_imports = successful_imports + CASE WHEN ? THEN 1 ELSE 0 END, failed_imports = failed_imports + CASE WHEN ? THEN 0 ELSE 1 END, last_error = CASE WHEN ? = '' THEN last_error ELSE ? END WHERE id = ? AND status = 'running'";
}

pub mod backup {
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
      FROM backup_assets
      JOIN backup_upload_sessions ON backup_upload_sessions.asset_id = backup_assets.id
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
      FROM backup_assets
      JOIN backup_upload_sessions ON backup_upload_sessions.asset_id = backup_assets.id
     WHERE backup_assets.user_id = ?
       AND backup_assets.device_id = ?
       AND backup_assets.client_asset_id = ?
    "#;
    pub const COUNT_ACTIVE_UPLOADS: &str = "SELECT COUNT(*) FROM backup_upload_sessions WHERE user_id = ? AND status IN ('uploading', 'writing') AND expires_at > datetime('now')";
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
      FROM backup_assets
      JOIN backup_upload_sessions ON backup_upload_sessions.asset_id = backup_assets.id
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
       AND status IN ('uploading', 'queued')
    "#;
    pub const CANCEL_ASSET: &str = r#"
    UPDATE backup_assets
       SET status = 'cancelled'
         , updated_at = datetime('now')
     WHERE id = ?
       AND status IN ('uploading', 'queued')
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
    pub const COMPLETE_ASSET: &str = "UPDATE backup_assets SET status = 'completed', media_id = ?, error = NULL, completed_at = datetime('now'), updated_at = datetime('now') WHERE id = ? AND status = 'processing'";
    pub const COMPLETE_SESSION: &str = "UPDATE backup_upload_sessions SET status = 'completed', updated_at = datetime('now') WHERE asset_id = ? AND status = 'processing'";
    pub const FAIL_ASSET: &str = "UPDATE backup_assets SET status = 'failed', error = ?, updated_at = datetime('now') WHERE id = ? AND status = 'processing'";
    pub const FAIL_SESSION: &str = "UPDATE backup_upload_sessions SET status = 'failed', updated_at = datetime('now') WHERE asset_id = ? AND status = 'processing'";
    pub const SELECT_PROCESSING_ASSETS: &str = r#"
    SELECT id
         , user_id
         , staged_path
         , content_hash
      FROM backup_assets
     WHERE status = 'processing'
     ORDER BY id
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
    pub const SELECT_RESUMABLE_FILES: &str = r#"
    SELECT backup_assets.id
         , backup_assets.staged_path
         , backup_upload_sessions.uploaded_size
      FROM backup_assets
      JOIN backup_upload_sessions ON backup_upload_sessions.asset_id = backup_assets.id
     WHERE backup_upload_sessions.status = 'uploading'
    "#;
    pub const FAIL_MISSING_STAGED_ASSET: &str = "UPDATE backup_assets SET status = 'failed', error = 'backup staging file is missing', updated_at = datetime('now') WHERE id = ? AND status = 'uploading'";
    pub const FAIL_MISSING_STAGED_SESSION: &str = "UPDATE backup_upload_sessions SET status = 'failed', updated_at = datetime('now') WHERE asset_id = ? AND status = 'uploading'";
    pub const EXPIRE_SESSIONS: &str = "UPDATE backup_upload_sessions SET status = 'expired', updated_at = datetime('now') WHERE status IN ('uploading', 'writing') AND expires_at <= datetime('now')";
    pub const EXPIRE_ASSETS: &str = "UPDATE backup_assets SET status = 'expired', updated_at = datetime('now') WHERE id IN (SELECT asset_id FROM backup_upload_sessions WHERE status = 'expired') AND status = 'uploading'";
}

pub mod webdav_ready {
    pub const UPSERT: &str = r#"
    INSERT INTO webdav_ready_files (user_id, file_path, completed_at)
    VALUES (?, ?, datetime('now'))
    ON CONFLICT(user_id, file_path) DO UPDATE SET
        completed_at = datetime('now')
    "#;
    pub const DELETE: &str = r#"
    DELETE FROM webdav_ready_files
     WHERE user_id = ?
       AND file_path = ?
    "#;
    pub const SELECT_FOR_USER: &str = r#"
    SELECT file_path
      FROM webdav_ready_files
     WHERE user_id = ?
     ORDER BY file_path
    "#;
    pub const EXISTS: &str = r#"
    SELECT EXISTS (
        SELECT 1
          FROM webdav_ready_files
         WHERE user_id = ?
           AND file_path = ?
    )
    "#;
}

pub mod metadata_jobs {
    pub const INSERT_QUEUED: &str = r#"
    INSERT INTO media_metadata_jobs (media_id, status, available_at)
    VALUES (?, 'queued', datetime('now'))
    ON CONFLICT(media_id) DO NOTHING
    "#;

    pub const REQUEST_RERUN: &str = r#"
    INSERT INTO media_metadata_jobs (media_id, status, available_at)
    VALUES (?, 'queued', datetime('now'))
    ON CONFLICT(media_id) DO UPDATE SET
        status = CASE
            WHEN media_metadata_jobs.status = 'processing' THEN 'processing'
            ELSE 'queued'
        END
      , attempts = CASE
            WHEN media_metadata_jobs.status = 'processing' THEN media_metadata_jobs.attempts
            ELSE 0
        END
      , rerun_requested = CASE
            WHEN media_metadata_jobs.status = 'processing' THEN 1
            ELSE 0
        END
      , available_at = datetime('now')
      , completed_at = NULL
      , last_error = NULL
      , updated_at = datetime('now')
    "#;

    pub const QUEUE_INCOMPLETE: &str = r#"
    INSERT INTO media_metadata_jobs (media_id, status, available_at)
    SELECT media.id
         , 'queued'
         , datetime('now')
      FROM media
      LEFT JOIN media_metadata ON media_metadata.media_id = media.id
     WHERE media.import_state = 'imported'
       AND media_metadata.media_id IS NULL
    ON CONFLICT(media_id) DO UPDATE SET
        status = CASE
            WHEN media_metadata_jobs.status = 'failed' THEN 'queued'
            ELSE media_metadata_jobs.status
        END
      , available_at = CASE
            WHEN media_metadata_jobs.status = 'failed' THEN datetime('now')
            ELSE media_metadata_jobs.available_at
        END
      , updated_at = datetime('now')
    "#;

    pub const SELECT_STATUS_COUNTS: &str = r#"
    SELECT status
         , COUNT(*)
      FROM media_metadata_jobs
     GROUP BY status
    "#;

    pub const SELECT_ALL_MEDIA_IDS: &str = "SELECT id FROM media";
    pub const DELETE_TEXT: &str = "DELETE FROM media_text";
    pub const DELETE_TEXT_INPUTS: &str = "DELETE FROM media_text_inputs";
    pub const DELETE_AI_INPUTS: &str = "DELETE FROM media_ai_inputs";
    pub const DELETE_LLM_JOBS: &str = "DELETE FROM llm_jobs";
    pub const DELETE_SIMILARITY_CLUSTERS: &str = "DELETE FROM media_similarity_clusters";
    pub const DELETE_SIMILARITY_BANDS: &str = "DELETE FROM media_similarity_hash_bands";
    pub const DELETE_SIMILARITY_INDEX: &str = "DELETE FROM media_similarity_index";
    pub const DELETE_SIMILARITY_DIRTY: &str = "DELETE FROM media_similarity_dirty";
    pub const DELETE_FACE_GROUPING_RUNS: &str = "DELETE FROM face_grouping_runs";
    pub const DELETE_FACE_GROUPS: &str = "DELETE FROM face_groups";
    pub const DELETE_MEDIA_FACES: &str = "DELETE FROM media_faces";
    pub const DELETE_FACE_DETECTION_RESULTS: &str = "DELETE FROM media_face_detection_results";
    pub const DELETE_AESTHETICS: &str = "DELETE FROM media_aesthetics";
    pub const DELETE_AESTHETIC_INPUTS: &str = "DELETE FROM media_aesthetic_inputs";
    pub const DELETE_SCREENSHOT_CLASSIFICATIONS: &str =
        "DELETE FROM media_screenshot_classifications";
    pub const DELETE_SCREENSHOT_CLASSIFICATION_INPUTS: &str =
        "DELETE FROM media_screenshot_classification_inputs";
    pub const DELETE_DOCUMENT_CLASSIFICATIONS: &str = "DELETE FROM media_document_classifications";
    pub const DELETE_DOCUMENT_CLASSIFICATION_INPUTS: &str =
        "DELETE FROM media_document_classification_inputs";
    pub const DELETE_RTREE: &str = "DELETE FROM media_rtree";
    pub const DELETE_METADATA: &str = "DELETE FROM media_metadata";
    pub const RESET_IMPORTED: &str = "UPDATE media_metadata_jobs SET status = 'queued', rerun_requested = 0, available_at = datetime('now'), claimed_at = NULL, completed_at = NULL, last_error = NULL, updated_at = datetime('now') WHERE media_id IN (SELECT id FROM media WHERE import_state = 'imported')";
    pub const MARK_IMPORTED_DIRTY: &str = "INSERT INTO media_similarity_dirty (media_id, marked_at) SELECT id, datetime('now') FROM media WHERE import_state = 'imported'";
    pub const SELECT_INPUT_PATHS: &str =
        "SELECT storage_root, file_path FROM media_ai_inputs WHERE media_id = ? AND task = ? ORDER BY sequence";
    pub const CLAIM_NEXT_QUEUED: &str = "UPDATE media_metadata_jobs SET status = 'processing', claimed_at = datetime('now'), attempts = attempts + 1, updated_at = datetime('now') WHERE media_id = (SELECT media_id FROM media_metadata_jobs WHERE status = 'queued' AND available_at <= datetime('now') ORDER BY media_id LIMIT 1) AND status = 'queued' RETURNING media_id";
    pub const RECLAIM_EXPIRED: &str = "UPDATE media_metadata_jobs SET status = 'queued', rerun_requested = 0, claimed_at = NULL, available_at = datetime('now'), last_error = 'metadata worker lease expired', updated_at = datetime('now') WHERE status = 'processing' AND claimed_at < datetime('now', ?)";
    pub const MARK_COMPLETED: &str = "UPDATE media_metadata_jobs SET status = CASE WHEN rerun_requested = 1 THEN 'queued' ELSE 'completed' END, attempts = CASE WHEN rerun_requested = 1 THEN 0 ELSE attempts END, rerun_requested = 0, available_at = CASE WHEN rerun_requested = 1 THEN datetime('now') ELSE available_at END, claimed_at = NULL, completed_at = CASE WHEN rerun_requested = 1 THEN NULL ELSE datetime('now') END, last_error = NULL, updated_at = datetime('now') WHERE media_id = ? AND status = 'processing'";
    pub const MARK_FAILED_OR_RETRY: &str = "UPDATE media_metadata_jobs SET status = CASE WHEN rerun_requested = 1 THEN 'queued' WHEN attempts >= ? THEN 'failed' ELSE 'queued' END, attempts = CASE WHEN rerun_requested = 1 THEN 0 ELSE attempts END, rerun_requested = 0, available_at = CASE WHEN rerun_requested = 1 THEN datetime('now') WHEN attempts >= ? THEN available_at ELSE datetime('now', '+30 seconds') END, claimed_at = NULL, last_error = CASE WHEN rerun_requested = 1 THEN NULL ELSE ? END, updated_at = datetime('now') WHERE media_id = ? AND status = 'processing'";
    pub const SELECT_FAILURES: &str = "SELECT last_error FROM media_metadata_jobs WHERE status = 'failed' AND last_error IS NOT NULL ORDER BY updated_at DESC LIMIT 100";
}

pub mod ai_jobs {
    pub const INSERT_ELIGIBLE: &str = "INSERT INTO llm_jobs (id, media_id, task, status) SELECT lower(hex(randomblob(16))), media.id, ?, 'queued' FROM media JOIN media_metadata_jobs ON media_metadata_jobs.media_id = media.id WHERE media.import_state = 'imported' AND media_metadata_jobs.status = 'completed' AND EXISTS (SELECT 1 FROM media_ai_inputs WHERE media_ai_inputs.media_id = media.id AND media_ai_inputs.task = ?) AND NOT EXISTS (SELECT 1 FROM media_text WHERE media_text.media_id = media.id AND media_text.model_type = ?) AND NOT EXISTS (SELECT 1 FROM llm_jobs WHERE llm_jobs.media_id = media.id AND llm_jobs.task = ? AND llm_jobs.status IN ('queued','submitting','submitted'))";
    pub const INSERT_FACE_ELIGIBLE: &str = "INSERT INTO llm_jobs (id, media_id, face_grouping_run_id, task, status) SELECT lower(hex(randomblob(16))), media.id, ?, 'face_detection', 'queued' FROM media JOIN media_metadata_jobs ON media_metadata_jobs.media_id = media.id WHERE media.import_state = 'imported' AND media_metadata_jobs.status = 'completed' AND EXISTS (SELECT 1 FROM media_ai_inputs WHERE media_ai_inputs.media_id = media.id AND media_ai_inputs.task = 'face_detection') AND NOT EXISTS (SELECT 1 FROM media_face_detection_results WHERE media_face_detection_results.media_id = media.id) AND NOT EXISTS (SELECT 1 FROM llm_jobs WHERE llm_jobs.media_id = media.id AND llm_jobs.task = 'face_detection' AND llm_jobs.status IN ('queued','submitting','submitted'))";
    pub const INSERT_AESTHETICS_ELIGIBLE: &str = "INSERT INTO llm_jobs (id, media_id, task, status) SELECT lower(hex(randomblob(16))), media.id, 'image_aesthetics', 'queued' FROM media JOIN media_metadata_jobs ON media_metadata_jobs.media_id = media.id WHERE media.import_state = 'imported' AND media_metadata_jobs.status = 'completed' AND EXISTS (SELECT 1 FROM media_ai_inputs WHERE media_ai_inputs.media_id = media.id AND media_ai_inputs.task = 'image_aesthetics') AND NOT EXISTS (SELECT 1 FROM media_aesthetics WHERE media_aesthetics.media_id = media.id) AND NOT EXISTS (SELECT 1 FROM llm_jobs WHERE llm_jobs.media_id = media.id AND llm_jobs.task = 'image_aesthetics' AND llm_jobs.status IN ('queued','submitting','submitted'))";
    pub const INSERT_SCREENSHOT_ELIGIBLE: &str = "INSERT INTO llm_jobs (id, media_id, task, status) SELECT lower(hex(randomblob(16))), media.id, 'screenshot_detection', 'queued' FROM media JOIN media_metadata_jobs ON media_metadata_jobs.media_id = media.id WHERE media.import_state = 'imported' AND media.media_type = 'image' AND media_metadata_jobs.status = 'completed' AND EXISTS (SELECT 1 FROM media_ai_inputs WHERE media_ai_inputs.media_id = media.id AND media_ai_inputs.task = 'screenshot_detection') AND NOT EXISTS (SELECT 1 FROM media_screenshot_classifications WHERE media_screenshot_classifications.media_id = media.id) AND NOT EXISTS (SELECT 1 FROM llm_jobs WHERE llm_jobs.media_id = media.id AND llm_jobs.task = 'screenshot_detection' AND llm_jobs.status IN ('queued','submitting','submitted'))";
    pub const INSERT_DOCUMENT_ELIGIBLE: &str = "INSERT INTO llm_jobs (id, media_id, task, status) SELECT lower(hex(randomblob(16))), media.id, 'document_detection', 'queued' FROM media JOIN media_metadata_jobs ON media_metadata_jobs.media_id = media.id WHERE media.import_state = 'imported' AND media.media_type = 'image' AND media_metadata_jobs.status = 'completed' AND EXISTS (SELECT 1 FROM media_ai_inputs WHERE media_ai_inputs.media_id = media.id AND media_ai_inputs.task = 'document_detection') AND NOT EXISTS (SELECT 1 FROM media_document_classifications WHERE media_document_classifications.media_id = media.id) AND NOT EXISTS (SELECT 1 FROM llm_jobs WHERE llm_jobs.media_id = media.id AND llm_jobs.task = 'document_detection' AND llm_jobs.status IN ('queued','submitting','submitted'))";
    pub const SELECT_QUEUED: &str = "SELECT id, media_id, task, attempts FROM llm_jobs WHERE status = 'queued' AND available_at <= datetime('now') AND NOT EXISTS (SELECT 1 FROM llm_cancellation_scopes WHERE llm_cancellation_scopes.scope = 'all' OR (llm_cancellation_scopes.scope = 'task' AND llm_cancellation_scopes.task = llm_jobs.task)) ORDER BY created_at LIMIT ?";
    pub const CLAIM: &str = "UPDATE llm_jobs SET status = 'submitting', claimed_at = datetime('now'), updated_at = datetime('now') WHERE id = ? AND status = 'queued'";
    pub const MARK_SUBMITTED: &str = "UPDATE llm_jobs SET status = 'submitted', attempts = attempts + 1, submitted_at = datetime('now'), updated_at = datetime('now') WHERE id = ? AND status = 'submitting' AND attempts + 1 = ?";
    pub const REQUEUE_AMBIGUOUS: &str = "UPDATE llm_jobs SET status = 'queued', claimed_at = NULL, available_at = datetime('now'), updated_at = datetime('now') WHERE id = ? AND status = 'submitting'";
    pub const SNAPSHOT_QUEUED_INPUTS: &str = "INSERT OR IGNORE INTO llm_job_inputs (job_id, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash, frame_timestamp_ms) SELECT llm_jobs.id, media_ai_inputs.sequence, media_ai_inputs.input_kind, media_ai_inputs.storage_root, media_ai_inputs.file_path, media_ai_inputs.filename, media_ai_inputs.mime_type, media_ai_inputs.byte_size, media_ai_inputs.content_hash, media_ai_inputs.frame_timestamp_ms FROM llm_jobs JOIN media_ai_inputs ON media_ai_inputs.media_id = llm_jobs.media_id AND media_ai_inputs.task = llm_jobs.task WHERE llm_jobs.status = 'queued'";
    pub const SELECT_INPUTS: &str = "SELECT sequence, storage_root, file_path, filename, mime_type, byte_size, content_hash, input_kind, frame_timestamp_ms FROM llm_job_inputs WHERE job_id = ? ORDER BY sequence";
    pub const RECLAIM_STALE: &str = "UPDATE llm_jobs SET status = 'queued', claimed_at = NULL, updated_at = datetime('now') WHERE status = 'submitting' AND claimed_at < datetime('now', '-5 minutes')";
    pub const RETRY_OR_FAIL: &str = "UPDATE llm_jobs SET status = CASE WHEN attempts + 1 >= 5 THEN 'failed' ELSE 'queued' END, attempts = attempts + 1, available_at = datetime('now', '+30 seconds'), last_error = ?, completed_at = CASE WHEN attempts + 1 >= 5 THEN datetime('now') ELSE NULL END, updated_at = datetime('now') WHERE id = ? AND status = 'submitting'";
    pub const MARK_FAILED: &str = "UPDATE llm_jobs SET status = 'failed', last_error = ?, completed_at = datetime('now'), updated_at = datetime('now') WHERE id = ? AND status = 'submitting'";
    pub const SELECT_LATEST_STATUS_COUNTS: &str = r#"
    WITH ranked_jobs AS (
        SELECT task
             , status
             , ROW_NUMBER() OVER (
                   PARTITION BY media_id, task
                   ORDER BY rowid DESC
               ) AS recency
          FROM llm_jobs
    )
    SELECT task
         , status
         , COUNT(*)
      FROM ranked_jobs
     WHERE recency = 1
     GROUP BY task
            , status
     ORDER BY task
            , status
    "#;
    pub const SELECT_LATEST_FAILURES: &str = r#"
    WITH ranked_jobs AS (
        SELECT task
             , status
             , last_error
             , updated_at
             , ROW_NUMBER() OVER (
                   PARTITION BY media_id, task
                   ORDER BY rowid DESC
               ) AS recency
          FROM llm_jobs
    )
    SELECT task
         , last_error
      FROM ranked_jobs
     WHERE recency = 1
       AND status = 'failed'
       AND last_error IS NOT NULL
     ORDER BY task
            , updated_at DESC
    "#;
    pub const COUNT_ACTIVE_FOR_TASK: &str = "SELECT COUNT(*) FROM llm_jobs WHERE task = ? AND status IN ('queued', 'submitting', 'submitted')";
    pub const COUNT_JOBS_FOR_TASK: &str = "SELECT COUNT(*) FROM llm_jobs WHERE task = ?";
    pub const COUNT_PENDING_CANCELLATION_SCOPE_FOR_TASK: &str = "SELECT COUNT(*) FROM llm_cancellation_scopes WHERE scope = 'all' OR (scope = 'task' AND task = ?)";
    pub const DELETE_TEXT_FOR_TASK: &str = "DELETE FROM media_text WHERE model_type = ?";
    pub const DELETE_TEXT_INPUTS_FOR_TASK: &str =
        "DELETE FROM media_text_inputs WHERE model_type = ?";
    pub const DELETE_AESTHETICS: &str = "DELETE FROM media_aesthetics";
    pub const DELETE_AESTHETIC_INPUTS: &str = "DELETE FROM media_aesthetic_inputs";
    pub const DELETE_SCREENSHOT_CLASSIFICATIONS: &str =
        "DELETE FROM media_screenshot_classifications";
    pub const DELETE_SCREENSHOT_CLASSIFICATION_INPUTS: &str =
        "DELETE FROM media_screenshot_classification_inputs";
    pub const DELETE_DOCUMENT_CLASSIFICATIONS: &str = "DELETE FROM media_document_classifications";
    pub const DELETE_DOCUMENT_CLASSIFICATION_INPUTS: &str =
        "DELETE FROM media_document_classification_inputs";
    pub const DELETE_JOBS_FOR_TASK: &str = "DELETE FROM llm_jobs WHERE task = ?";
    pub const CANCEL_FOR_TASK: &str = "UPDATE llm_jobs SET status = 'cancelled', attempts = attempts + CASE WHEN status = 'submitting' THEN 1 ELSE 0 END, completed_at = datetime('now'), updated_at = datetime('now') WHERE task = ? AND status IN ('queued', 'submitting', 'submitted')";
    pub const CANCEL_ALL: &str = "UPDATE llm_jobs SET status = 'cancelled', attempts = attempts + CASE WHEN status = 'submitting' THEN 1 ELSE 0 END, completed_at = datetime('now'), updated_at = datetime('now') WHERE status IN ('queued', 'submitting', 'submitted')";
    pub const QUEUE_CANCELLATION_SCOPE_FOR_TASK: &str =
        "INSERT OR IGNORE INTO llm_cancellation_scopes (scope, task) VALUES ('task', ?)";
    pub const QUEUE_ALL_CANCELLATION_SCOPE: &str =
        "INSERT OR IGNORE INTO llm_cancellation_scopes (scope, task) VALUES ('all', '')";
    pub const QUEUE_CANCELLATIONS_FOR_TASK: &str = "INSERT OR IGNORE INTO llm_job_cancellations (job_id, task) SELECT id, task FROM llm_jobs WHERE task = ? AND status IN ('queued', 'submitting', 'submitted', 'failed')";
    pub const QUEUE_ALL_CANCELLATIONS: &str = "INSERT OR IGNORE INTO llm_job_cancellations (job_id, task) SELECT id, task FROM llm_jobs WHERE status IN ('queued', 'submitting', 'submitted', 'failed')";
    pub const SELECT_CANCELLATION_SCOPE: &str = "SELECT scope, task FROM llm_cancellation_scopes ORDER BY CASE scope WHEN 'all' THEN 0 ELSE 1 END, created_at, task LIMIT 1";
    pub const SELECT_CANCELLATIONS_FOR_TASK: &str = "SELECT job_id FROM llm_job_cancellations WHERE task = ? ORDER BY created_at, job_id LIMIT ?";
    pub const SELECT_ALL_CANCELLATIONS: &str =
        "SELECT job_id FROM llm_job_cancellations ORDER BY created_at, job_id LIMIT ?";
    pub const DELETE_CANCELLATION: &str = "DELETE FROM llm_job_cancellations WHERE job_id = ?";
    pub const COUNT_CANCELLATIONS_FOR_TASK: &str =
        "SELECT COUNT(*) FROM llm_job_cancellations WHERE task = ?";
    pub const COUNT_ALL_CANCELLATIONS: &str = "SELECT COUNT(*) FROM llm_job_cancellations";
    pub const DELETE_CANCELLATION_SCOPE_FOR_TASK: &str =
        "DELETE FROM llm_cancellation_scopes WHERE scope = 'task' AND task = ?";
    pub const DELETE_ALL_CANCELLATION_SCOPES: &str = "DELETE FROM llm_cancellation_scopes";
}

pub mod faces {
    pub const COUNT_GROUPS: &str = "SELECT COUNT(*) FROM face_groups";
    pub const INSERT_GROUPING_RUN: &str =
        "INSERT INTO face_grouping_runs (status) VALUES ('running')";
    pub const SELECT_ACTIVE_RUN: &str = "SELECT id, status FROM face_grouping_runs WHERE status IN ('running', 'cancelling') ORDER BY id DESC LIMIT 1";
    pub const COUNT_PENDING_JOBS: &str = "SELECT COUNT(*) FROM llm_jobs WHERE face_grouping_run_id = ? AND task = 'face_detection' AND status IN ('queued', 'submitting', 'submitted')";
    pub const COUNT_FAILED_JOBS: &str =
        "SELECT COUNT(*) FROM llm_jobs WHERE face_grouping_run_id = ? AND task = 'face_detection' AND status = 'failed'";
    pub const MARK_RUN: &str = "UPDATE face_grouping_runs SET status = ?, completed_at = datetime('now'), error = ? WHERE id = ? AND status IN ('running', 'cancelling')";
    pub const CANCEL_ACTIVE: &str = "UPDATE llm_jobs SET status = 'cancelled', completed_at = datetime('now'), updated_at = datetime('now') WHERE task = 'face_detection' AND status IN ('queued', 'submitting', 'submitted')";
    pub const SELECT_FACES_FOR_GROUPING: &str = r#"
    SELECT media_faces.id
         , media_faces.embedding
      FROM media_faces
     WHERE NOT EXISTS (
               SELECT 1
                 FROM face_group_members
                WHERE face_group_members.face_id = media_faces.id
                  AND face_group_members.manual_anchor = 1
           )
     ORDER BY media_faces.id
    "#;
    pub const SELECT_MANUAL_GROUP_ANCHORS: &str = r#"
    SELECT face_groups.id
         , media_faces.embedding
      FROM face_groups
      JOIN face_group_members
        ON face_group_members.face_group_id = face_groups.id
      JOIN media_faces
        ON media_faces.id = face_group_members.face_id
     WHERE face_groups.manual_curated = 1
       AND face_group_members.manual_anchor = 1
     ORDER BY face_groups.id
            , media_faces.id
    "#;
    pub const DELETE_AUTOMATIC_GROUPS: &str = "DELETE FROM face_groups WHERE manual_curated = 0";
    pub const DELETE_AUTOMATIC_MANUAL_GROUP_MEMBERS: &str = r#"
    DELETE FROM face_group_members
     WHERE manual_anchor = 0
       AND face_group_id IN (
               SELECT id
                 FROM face_groups
                WHERE manual_curated = 1
           )
    "#;
    pub const INSERT_GROUP: &str = "INSERT INTO face_groups (manual_curated) VALUES (0)";
    pub const INSERT_AUTOMATIC_MEMBER: &str = r#"
    INSERT INTO face_group_members
      ( face_group_id
      , face_id
      , manual_anchor
    ) VALUES (?, ?, 0)
    ON CONFLICT(face_group_id, face_id) DO UPDATE SET
        manual_anchor = 0
    "#;
    pub const INSERT_MANUAL_MEMBER: &str = r#"
    INSERT INTO face_group_members
      ( face_group_id
      , face_id
      , manual_anchor
    ) VALUES (?, ?, 1)
    ON CONFLICT(face_group_id, face_id) DO UPDATE SET
        manual_anchor = 1
    "#;
    pub const INSERT_FACE: &str = r#"
    INSERT INTO media_faces
      ( media_id
      , input_sequence
      , face_index
      , x
      , y
      , width
      , height
      , confidence
      , face_size_score
      , frontality_score
      , visibility_score
      , feature_clarity_score
      , embedding
      , crop_path
    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    "#;
    pub const SELECT_GROUP_REPRESENTATIVE_CANDIDATES: &str = r#"
    SELECT media_faces.id
         , media_faces.crop_path
         , media_faces.x
         , media_faces.y
         , media_faces.width
         , media_faces.height
         , media_faces.confidence
         , media_faces.face_size_score
         , media_faces.frontality_score
         , media_faces.visibility_score
         , media_faces.feature_clarity_score
      FROM face_group_members
      JOIN media_faces ON media_faces.id = face_group_members.face_id
     WHERE face_group_members.face_group_id = ?
     ORDER BY media_faces.id
    "#;
    pub const SELECT_VISIBLE_GROUP_REPRESENTATIVE_CANDIDATES: &str = r#"
    SELECT media_faces.id
         , media_faces.crop_path
         , media_faces.x
         , media_faces.y
         , media_faces.width
         , media_faces.height
         , media_faces.confidence
         , media_faces.face_size_score
         , media_faces.frontality_score
         , media_faces.visibility_score
         , media_faces.feature_clarity_score
      FROM face_group_members
      JOIN media_faces ON media_faces.id = face_group_members.face_id
      JOIN media_access ON media_access.media_id = media_faces.media_id
     WHERE face_group_members.face_group_id = ?
       AND media_access.user_id = ?
       AND media_access.deleted_at IS NULL
     ORDER BY media_faces.id
    "#;
    pub const SELECT_VISIBLE_STORED_REPRESENTATIVE_CROP: &str = r#"
    SELECT media_faces.crop_path
      FROM face_groups
      JOIN media_faces ON media_faces.id = face_groups.representative_face_id
      JOIN media_access ON media_access.media_id = media_faces.media_id
     WHERE face_groups.id = ?
       AND media_access.user_id = ?
       AND media_access.deleted_at IS NULL
    "#;
    pub const UPDATE_GROUP_REPRESENTATIVE_ID: &str =
        "UPDATE face_groups SET representative_face_id = ? WHERE id = ?";
    pub const SELECT_ALL_GROUP_IDS: &str = "SELECT id FROM face_groups ORDER BY id";
    pub const DELETE_MEDIA_FACES: &str = "DELETE FROM media_faces WHERE media_id = ?";
    pub const CANCEL_RECOVERED_CANCELLING_JOBS: &str = "UPDATE llm_jobs SET status = 'cancelled', attempts = attempts + CASE WHEN status = 'submitting' THEN 1 ELSE 0 END, completed_at = datetime('now'), updated_at = datetime('now') WHERE task = 'face_detection' AND status IN ('queued', 'submitting', 'submitted') AND face_grouping_run_id IN (SELECT id FROM face_grouping_runs WHERE status = 'cancelling')";
    pub const QUEUE_RECOVERED_CANCELLATION_SCOPE: &str = "INSERT OR IGNORE INTO llm_cancellation_scopes (scope, task) SELECT 'task', 'face_detection' WHERE EXISTS (SELECT 1 FROM face_grouping_runs WHERE status = 'cancelling')";
    pub const QUEUE_RECOVERED_CANCELLATIONS: &str = "INSERT OR IGNORE INTO llm_job_cancellations (job_id, task) SELECT id, task FROM llm_jobs WHERE task = 'face_detection' AND status IN ('queued', 'submitting', 'submitted') AND face_grouping_run_id IN (SELECT id FROM face_grouping_runs WHERE status = 'cancelling')";
    pub const FINALIZE_RECOVERED_CANCELLING_RUNS: &str = "UPDATE face_grouping_runs SET status = 'cancelled', completed_at = datetime('now'), error = NULL WHERE status = 'cancelling'";
    pub const REQUEST_CANCEL_RUNS: &str =
        "UPDATE face_grouping_runs SET status = 'cancelling' WHERE status = 'running'";
    pub const CLEAN_RUNS: &str = "DELETE FROM face_grouping_runs";
    pub const CLEAN_GROUPS: &str = "DELETE FROM face_groups";
    pub const CLEAN_FACES: &str = "DELETE FROM media_faces";
    pub const CLEAN_RESULTS: &str = "DELETE FROM media_face_detection_results";
    pub const CLEAN_JOBS: &str = "DELETE FROM llm_jobs WHERE task = 'face_detection'";
    pub const SELECT_INPUT_CORRELATION: &str = "SELECT sequence, frame_timestamp_ms FROM llm_job_inputs WHERE job_id = ? ORDER BY sequence";
    pub const SELECT_INPUT_PATH: &str = "SELECT storage_root, file_path, byte_size, content_hash FROM llm_job_inputs WHERE job_id = ? AND sequence = ?";
    pub const SELECT_MEDIA_CROPS: &str = "SELECT crop_path FROM media_faces WHERE media_id = ?";
    pub const UPSERT_RESULT: &str = "INSERT INTO media_face_detection_results (media_id, model_type, model_version) VALUES (?, 'face_detection', ?) ON CONFLICT(media_id) DO UPDATE SET model_type = excluded.model_type, model_version = excluded.model_version, completed_at = datetime('now')";
    pub const LIST_GROUPS: &str = "SELECT fg.id, COUNT(fgm.face_id), COUNT(DISTINCT mf.media_id) AS media_count FROM face_groups AS fg JOIN face_group_members AS fgm ON fgm.face_group_id = fg.id JOIN media_faces AS mf ON mf.id = fgm.face_id JOIN media_access AS ma ON ma.media_id = mf.media_id WHERE ma.user_id = ? AND ma.deleted_at IS NULL GROUP BY fg.id ORDER BY media_count DESC, fg.id ASC LIMIT ? OFFSET ?";
    pub const COUNT_VISIBLE_GROUPS: &str = "SELECT COUNT(*) FROM (SELECT fg.id FROM face_groups AS fg JOIN face_group_members AS fgm ON fgm.face_group_id = fg.id JOIN media_faces AS mf ON mf.id = fgm.face_id JOIN media_access AS ma ON ma.media_id = mf.media_id WHERE ma.user_id = ? AND ma.deleted_at IS NULL GROUP BY fg.id)";
    pub const SELECT_GROUP: &str = "SELECT fg.id, COUNT(fgm.face_id), COUNT(DISTINCT mf.media_id) FROM face_groups AS fg JOIN face_group_members AS fgm ON fgm.face_group_id = fg.id JOIN media_faces AS mf ON mf.id = fgm.face_id JOIN media_access AS ma ON ma.media_id = mf.media_id WHERE fg.id = ? AND ma.user_id = ? AND ma.deleted_at IS NULL GROUP BY fg.id";
    pub const SELECT_GROUP_MEDIA: &str = "SELECT DISTINCT m.id, m.filename, m.original_filename, m.media_type, m.mime_type, mm.width, mm.height, m.file_size, mm.duration_seconds, mm.date_taken, mm.gps_latitude, mm.gps_longitude, mm.camera_make, mm.camera_model, mm.lens_make, mm.lens_model, mm.iso, mm.exposure_time, mm.f_number, mm.focal_length, mm.focal_length_35mm, mm.gps_altitude, mm.location_city, mm.location_state, mm.location_country, mm.video_codec, mm.keywords, m.created_at FROM media AS m JOIN media_faces AS mf ON mf.media_id = m.id JOIN face_group_members AS fgm ON fgm.face_id = mf.id JOIN media_access AS ma ON ma.media_id = m.id LEFT JOIN media_metadata AS mm ON mm.media_id = m.id WHERE fgm.face_group_id = ? AND ma.user_id = ? AND ma.deleted_at IS NULL ORDER BY m.id";
    pub const SELECT_EXISTING_GROUPS: &str = "SELECT id FROM face_groups WHERE id IN (%s)";
    pub const SELECT_MERGE_MEMBERS: &str =
        "SELECT face_id FROM face_group_members WHERE face_group_id IN (%s)";
    pub const UPDATE_MANUAL_GROUP: &str = "UPDATE face_groups SET manual_curated = 1 WHERE id = ?";
    pub const DELETE_GROUP: &str = "DELETE FROM face_groups WHERE id = ?";
    pub const COUNT_GROUP_MEMBERS: &str =
        "SELECT COUNT(*) FROM face_group_members WHERE face_group_id = ?";
    pub const COUNT_GROUP_MEDIA: &str = "SELECT COUNT(DISTINCT media_faces.media_id) FROM face_group_members JOIN media_faces ON media_faces.id = face_group_members.face_id WHERE face_group_members.face_group_id = ?";

    pub fn build_existing_groups_query(count: usize) -> String {
        SELECT_EXISTING_GROUPS.replace("%s", &placeholders(count))
    }
    pub fn build_merge_members_query(count: usize) -> String {
        SELECT_MERGE_MEMBERS.replace("%s", &placeholders(count))
    }
    fn placeholders(count: usize) -> String {
        std::iter::repeat_n("?", count)
            .collect::<Vec<_>>()
            .join(",")
    }
}

pub mod llm_callback {
    pub const SELECT_JOB: &str =
        "SELECT media_id, task, attempts, status FROM llm_jobs WHERE id = ?";
    pub const SELECT_JOB_INPUT_CORRELATION: &str =
        "SELECT sequence, frame_timestamp_ms FROM llm_job_inputs WHERE job_id = ? ORDER BY sequence";
    pub const MARK_COMPLETED: &str = "UPDATE llm_jobs SET status = 'completed', completed_at = datetime('now'), updated_at = datetime('now') WHERE id = ? AND status = 'submitted' AND attempts = ?";
    pub const MARK_FAILED: &str = "UPDATE llm_jobs SET status = 'failed', last_error = ?, completed_at = datetime('now'), updated_at = datetime('now') WHERE id = ? AND status = 'submitted' AND attempts = ?";
    pub const INSERT_RECEIVED_RESULT: &str =
        "INSERT OR IGNORE INTO llm_job_results (job_id, payload) VALUES (?, ?)";
    pub const MARK_UNACKNOWLEDGED_RESULT_SUBMITTED: &str = "UPDATE llm_jobs SET status = 'submitted', attempts = ?, submitted_at = COALESCE(submitted_at, datetime('now')), claimed_at = NULL, updated_at = datetime('now') WHERE id = ? AND status IN ('queued', 'submitting') AND attempts + 1 = ?";
    pub const MARK_RESULT_CORRELATION_FAILED: &str = "UPDATE llm_jobs SET status = 'failed', last_error = ?, completed_at = datetime('now'), updated_at = datetime('now') WHERE id = ? AND status IN ('queued', 'submitting', 'submitted')";
    pub const SELECT_RESULT_CANDIDATES: &str = r#"
        SELECT job_id
             , payload
          FROM llm_job_results
      ORDER BY received_at
             , job_id
         LIMIT ?
    "#;
    pub const DELETE_RESULT: &str = "DELETE FROM llm_job_results WHERE job_id = ?";
    pub const MARK_RECEIVED_RESULT_FAILED: &str = "UPDATE llm_jobs SET status = 'failed', last_error = ?, completed_at = datetime('now'), updated_at = datetime('now') WHERE id = ? AND status = 'submitted'";
    pub const UPSERT_TEXT: &str = "INSERT INTO media_text (media_id, model_type, model_version, string) VALUES (?, ?, ?, ?) ON CONFLICT(media_id, model_type) DO UPDATE SET model_version = excluded.model_version, string = excluded.string, created_at = datetime('now')";
    pub const UPSERT_INPUT_TEXT: &str = "INSERT INTO media_text_inputs (media_id, model_type, sequence, frame_timestamp_ms, model_version, string) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(media_id, model_type, sequence) DO UPDATE SET frame_timestamp_ms = excluded.frame_timestamp_ms, model_version = excluded.model_version, string = excluded.string, created_at = datetime('now')";
    pub const UPSERT_AESTHETICS: &str = "INSERT INTO media_aesthetics (media_id, model_type, model_version, aesthetic_score, scenic_score, simplicity_score, landscape_score, technical_quality_score) VALUES (?, 'image_aesthetics', ?, ?, ?, ?, ?, ?) ON CONFLICT(media_id) DO UPDATE SET model_version = excluded.model_version, aesthetic_score = excluded.aesthetic_score, scenic_score = excluded.scenic_score, simplicity_score = excluded.simplicity_score, landscape_score = excluded.landscape_score, technical_quality_score = excluded.technical_quality_score, completed_at = datetime('now')";
    pub const UPSERT_AESTHETIC_INPUT: &str = "INSERT INTO media_aesthetic_inputs (media_id, sequence, frame_timestamp_ms, model_type, model_version, aesthetic_score, scenic_score, simplicity_score, landscape_score, technical_quality_score) VALUES (?, ?, ?, 'image_aesthetics', ?, ?, ?, ?, ?, ?) ON CONFLICT(media_id, sequence) DO UPDATE SET frame_timestamp_ms = excluded.frame_timestamp_ms, model_version = excluded.model_version, aesthetic_score = excluded.aesthetic_score, scenic_score = excluded.scenic_score, simplicity_score = excluded.simplicity_score, landscape_score = excluded.landscape_score, technical_quality_score = excluded.technical_quality_score, completed_at = datetime('now')";
    pub const UPSERT_SCREENSHOT_CLASSIFICATION: &str = "INSERT INTO media_screenshot_classifications (media_id, model_type, model_version, is_screenshot, confidence) VALUES (?, 'screenshot_detection', ?, ?, ?) ON CONFLICT(media_id) DO UPDATE SET model_version = excluded.model_version, is_screenshot = excluded.is_screenshot, confidence = excluded.confidence, completed_at = datetime('now')";
    pub const UPSERT_SCREENSHOT_CLASSIFICATION_INPUT: &str = "INSERT INTO media_screenshot_classification_inputs (media_id, sequence, frame_timestamp_ms, model_type, model_version, is_screenshot, confidence) VALUES (?, ?, ?, 'screenshot_detection', ?, ?, ?) ON CONFLICT(media_id, sequence) DO UPDATE SET frame_timestamp_ms = excluded.frame_timestamp_ms, model_version = excluded.model_version, is_screenshot = excluded.is_screenshot, confidence = excluded.confidence, completed_at = datetime('now')";
    pub const UPSERT_DOCUMENT_CLASSIFICATION: &str = "INSERT INTO media_document_classifications (media_id, model_type, model_version, is_document, confidence) VALUES (?, 'document_detection', ?, ?, ?) ON CONFLICT(media_id) DO UPDATE SET model_version = excluded.model_version, is_document = excluded.is_document, confidence = excluded.confidence, completed_at = datetime('now')";
    pub const UPSERT_DOCUMENT_CLASSIFICATION_INPUT: &str = "INSERT INTO media_document_classification_inputs (media_id, sequence, frame_timestamp_ms, model_type, model_version, is_document, confidence) VALUES (?, ?, ?, 'document_detection', ?, ?, ?) ON CONFLICT(media_id, sequence) DO UPDATE SET frame_timestamp_ms = excluded.frame_timestamp_ms, model_version = excluded.model_version, is_document = excluded.is_document, confidence = excluded.confidence, completed_at = datetime('now')";
    pub const SELECT_CLUSTER_MEDIA: &str = "SELECT media.content_hash, CAST(strftime('%s', media_metadata.date_taken) AS INTEGER) FROM media LEFT JOIN media_metadata ON media_metadata.media_id = media.id WHERE media.id = ?";
    pub const UPSERT_SIMILARITY_INDEX: &str = "INSERT INTO media_similarity_index (media_id, content_hash, model_version, preprocessing_version, embedding, perceptual_hash, capture_time_seconds, processing_status, processing_error) VALUES (?, ?, ?, 'prepared-input-v1', ?, ?, ?, 1, NULL) ON CONFLICT(media_id) DO UPDATE SET content_hash = excluded.content_hash, model_version = excluded.model_version, preprocessing_version = excluded.preprocessing_version, embedding = excluded.embedding, perceptual_hash = excluded.perceptual_hash, capture_time_seconds = excluded.capture_time_seconds, indexed_at = datetime('now'), processing_status = 1, processing_error = NULL";
    pub const DELETE_HASH_BANDS: &str =
        "DELETE FROM media_similarity_hash_bands WHERE media_id = ?";
    pub const INSERT_HASH_BAND: &str = "INSERT INTO media_similarity_hash_bands (media_id, band_index, band_value) VALUES (?, ?, ?)";
    pub const UPSERT_DIRTY: &str = "INSERT INTO media_similarity_dirty (media_id, marked_at) VALUES (?, datetime('now')) ON CONFLICT(media_id) DO UPDATE SET marked_at = excluded.marked_at";
}

pub mod places {
    const SELECT_CANDIDATES: &str = r#"
    SELECT m.id
         , mm.location_city AS city
         , mm.location_state AS state
         , mm.location_country AS country
         , mm.date_taken
         , mm.thumbnail_path
         , m.file_path
         , CASE WHEN aesthetics.media_id IS NULL THEN 0 ELSE 1 END AS has_aesthetics
         , CASE
               WHEN aesthetics.media_id IS NOT NULL THEN
                   0.40 * aesthetics.aesthetic_score
                 + 0.25 * aesthetics.scenic_score
                 + 0.20 * aesthetics.simplicity_score
                 + 0.10 * aesthetics.landscape_score
                 + 0.05 * aesthetics.technical_quality_score
                 - 0.15 * MIN(CAST(LENGTH(TRIM(COALESCE(ocr.string, ''))) AS REAL) / 200.0, 1.0)
                 - CASE
                       WHEN COALESCE(faces.maximum_area, 0.0) >= 0.18 THEN 0.20
                       WHEN COALESCE(faces.maximum_area, 0.0) >= 0.08 THEN 0.10
                       ELSE 0.0
                   END
               ELSE CASE
                   WHEN mm.width IS NULL OR mm.height IS NULL OR mm.height <= 0 THEN 0.0
                   ELSE MIN(MAX(CAST(mm.width AS REAL) / mm.height - 1.0, 0.0) / 0.5, 1.0)
               END
           END AS cover_score
      FROM media AS m
      JOIN media_access AS access ON access.media_id = m.id
      JOIN media_metadata AS mm ON mm.media_id = m.id
      LEFT JOIN media_aesthetics AS aesthetics ON aesthetics.media_id = m.id
      LEFT JOIN media_text AS ocr
        ON ocr.media_id = m.id
       AND ocr.model_type = 'ocr'
      LEFT JOIN (
          SELECT media_id
               , MAX(width * height) AS maximum_area
            FROM media_faces
        GROUP BY media_id
      ) AS faces ON faces.media_id = m.id
     WHERE access.user_id = ?
       AND access.deleted_at IS NULL
    "#;

    const SELECT_PAGE: &str = r#"
    SELECT city
         , state
         , country
         , COUNT(*) AS media_count
      FROM candidates
  GROUP BY city
         , state
         , country
     ORDER BY media_count DESC
            , city ASC
            , CASE WHEN state IS NULL THEN 0 ELSE 1 END ASC
            , state ASC
            , country ASC
     LIMIT ?
    OFFSET ?
    "#;

    const SELECT_SUMMARY: &str = r#"
    SELECT city
         , state
         , country
         , COUNT(*) AS media_count
      FROM candidates
    HAVING COUNT(*) > 0
    "#;

    const SELECT_COVER: &str = r#"
    SELECT thumbnail_path
         , file_path
      FROM candidates
     ORDER BY has_aesthetics DESC
            , cover_score DESC
            , COALESCE(date_taken, '') DESC
            , id ASC
     LIMIT 1
    "#;

    pub fn select_page_query() -> String {
        format!(
            "WITH candidates AS ({SELECT_CANDIDATES} AND mm.location_city IS NOT NULL AND TRIM(mm.location_city) != '' AND mm.location_country IS NOT NULL AND TRIM(mm.location_country) != '') {SELECT_PAGE}"
        )
    }

    pub fn select_summary_query() -> String {
        format!(
            "WITH candidates AS ({SELECT_CANDIDATES} AND mm.location_city = ? AND mm.location_state IS ? AND mm.location_country = ?) {SELECT_SUMMARY}"
        )
    }

    pub fn select_cover_query() -> String {
        format!(
            "WITH candidates AS ({SELECT_CANDIDATES} AND mm.location_city = ? AND mm.location_state IS ? AND mm.location_country = ?) {SELECT_COVER}"
        )
    }

    pub const SELECT_MEDIA_PAGE: &str = r#"
    SELECT m.id
         , m.filename
         , m.original_filename
         , m.media_type
         , m.mime_type
         , mm.width
         , mm.height
         , m.file_size
         , mm.duration_seconds
         , mm.date_taken
         , mm.gps_latitude
         , mm.gps_longitude
         , mm.camera_make
         , mm.camera_model
         , mm.lens_make
         , mm.lens_model
         , mm.iso
         , mm.exposure_time
         , mm.f_number
         , mm.focal_length
         , mm.focal_length_35mm
         , mm.gps_altitude
         , mm.location_city
         , mm.location_state
         , mm.location_country
         , mm.video_codec
         , mm.keywords
         , m.created_at
      FROM media AS m
      JOIN media_access AS access ON access.media_id = m.id
      JOIN media_metadata AS mm ON mm.media_id = m.id
     WHERE access.user_id = ?
       AND access.deleted_at IS NULL
       AND mm.location_city = ?
       AND mm.location_state IS ?
       AND mm.location_country = ?
     ORDER BY COALESCE(mm.date_taken, '') DESC
            , m.id DESC
     LIMIT ?
    OFFSET ?
    "#;
}

pub mod media {
    pub const INSERT_RTREE: &str =
        "INSERT INTO media_rtree (media_id, min_lat, max_lat, min_lon, max_lon) VALUES (?, ?, ?, ?, ?)";
    pub const DELETE_RTREE: &str = "DELETE FROM media_rtree WHERE media_id = ?";
    pub const UPSERT_EDITABLE_METADATA: &str = "INSERT INTO media_metadata (media_id, date_taken, gps_latitude, gps_longitude) VALUES (?, ?, ?, ?) ON CONFLICT(media_id) DO UPDATE SET date_taken = COALESCE(excluded.date_taken, media_metadata.date_taken), gps_latitude = COALESCE(excluded.gps_latitude, media_metadata.gps_latitude), gps_longitude = COALESCE(excluded.gps_longitude, media_metadata.gps_longitude)";
    pub const UPSERT_GEOHASH: &str = "INSERT INTO media_metadata (media_id, geohash) VALUES (?, ?) ON CONFLICT(media_id) DO UPDATE SET geohash = excluded.geohash";
    pub const UPDATE_LOCATION: &str = "UPDATE media_metadata SET geohash = ?, location_city = ?, location_state = ?, location_country = ? WHERE media_id = ?";
    pub const INSERT: &str = r#"
    INSERT INTO media (
        user_id
      , filename
      , original_filename
      , file_path
      , media_type
      , mime_type
      , file_size
      , content_hash
    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
    ON CONFLICT DO NOTHING
    "#;

    pub const SELECT_BY_ID: &str = r#"
    SELECT m.id
         , m.filename
         , m.original_filename
         , m.media_type
         , m.mime_type
         , mm.width
         , mm.height
         , m.file_size
         , mm.duration_seconds
         , mm.date_taken
         , mm.gps_latitude
         , mm.gps_longitude
         , mm.camera_make
         , mm.camera_model
         , mm.lens_make
         , mm.lens_model
         , mm.iso
         , mm.exposure_time
         , mm.f_number
         , mm.focal_length
         , mm.focal_length_35mm
         , mm.gps_altitude
         , mm.location_city
         , mm.location_state
         , mm.location_country
         , mm.video_codec
         , mm.keywords
         , m.created_at
      FROM media AS m
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE m.id = ?
    "#;

    pub const SELECT_BY_ID_AND_USER: &str = r#"
    SELECT m.id
         , m.filename
         , m.original_filename
         , m.media_type
         , m.mime_type
         , mm.width
         , mm.height
         , m.file_size
         , mm.duration_seconds
         , mm.date_taken
         , mm.gps_latitude
         , mm.gps_longitude
         , mm.camera_make
         , mm.camera_model
         , mm.lens_make
         , mm.lens_model
         , mm.iso
         , mm.exposure_time
         , mm.f_number
         , mm.focal_length
         , mm.focal_length_35mm
         , mm.gps_altitude
         , mm.location_city
         , mm.location_state
         , mm.location_country
         , mm.video_codec
         , mm.keywords
         , m.created_at
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE m.id = ?
       AND ma.user_id = ?
       AND ma.deleted_at IS NULL
    "#;

    pub const CHECK_EXISTS: &str = r#"
    SELECT m.id
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
     WHERE m.id = ?
       AND ma.user_id = ?
       AND ma.deleted_at IS NULL
    "#;

    pub const UPDATE_DELETED_AT: &str = r#"
    UPDATE media_access
       SET deleted_at = ?
     WHERE media_id = ?
       AND user_id = ?
       AND deleted_at IS NULL
    "#;

    pub const SELECT_FILE_INFO: &str = r#"
    SELECT m.file_path
         , m.mime_type
         , m.original_filename
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
     WHERE m.id = ?
       AND ma.user_id = ?
       AND ma.deleted_at IS NULL
    "#;

    pub const SELECT_BINARY_MEDIA_INFO: &str = r#"
    SELECT m.file_path
         , m.mime_type
         , m.original_filename
         , m.media_type
         , mm.thumbnail_path
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE m.id = ?
       AND ma.user_id = ?
       AND ma.deleted_at IS NULL
    "#;

    pub const SELECT_DELETED_BINARY_MEDIA_INFO: &str = r#"
    SELECT m.file_path
         , m.mime_type
         , m.original_filename
         , m.media_type
         , mm.thumbnail_path
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE m.id = ?
       AND ma.user_id = ?
       AND ma.deleted_at IS NOT NULL
    "#;

    pub const SELECT_FOR_MAP: &str = r#"
    SELECT m.id
         , m.filename
         , m.original_filename
         , m.media_type
         , m.mime_type
         , mm.width
         , mm.height
         , m.file_size
         , mm.duration_seconds
         , mm.date_taken
         , mm.gps_latitude
         , mm.gps_longitude
         , mm.camera_make
         , mm.camera_model
         , mm.lens_make
         , mm.lens_model
         , mm.iso
         , mm.exposure_time
         , mm.f_number
         , mm.focal_length
         , mm.focal_length_35mm
         , mm.gps_altitude
         , mm.location_city
         , mm.location_state
         , mm.location_country
         , mm.video_codec
         , mm.keywords
         , m.created_at
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE ma.user_id = ?
       AND ma.deleted_at IS NULL
       AND mm.gps_latitude IS NOT NULL
       AND mm.gps_longitude IS NOT NULL
    "#;

    const SELECT_THUMBNAIL_BATCH: &str = r#"
    SELECT m.id
         , mm.thumbnail_path
         , m.file_path
         , m.media_type
         , ma.user_id
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE ma.user_id = ?
       AND ma.deleted_at IS NULL
       AND m.id IN (%s)
    "#;

    const SELECT_PREVIEW_BATCH: &str = r#"
    SELECT m.id
         , m.file_path
         , m.media_type
         , m.mime_type
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
     WHERE ma.user_id = ?
       AND ma.deleted_at IS NULL
       AND m.id IN (%s)
    "#;

    pub fn build_thumbnail_batch_query(count: usize) -> String {
        SELECT_THUMBNAIL_BATCH.replace("%s", &placeholders(count))
    }

    pub fn build_preview_batch_query(count: usize) -> String {
        SELECT_PREVIEW_BATCH.replace("%s", &placeholders(count))
    }

    pub const UPDATE_CONTENT_HASH: &str = r#"
    UPDATE media
       SET content_hash = ?
     WHERE id = ?
    "#;

    pub const SELECT_WITHOUT_HASH: &str = r#"
    SELECT id, file_path
      FROM media
     WHERE content_hash IS NULL
    "#;

    pub fn build_select_by_ids(count: usize) -> String {
        let placeholders = placeholders(count);

        format!(
            r#"
            SELECT m.id
                 , m.filename
                 , m.original_filename
                 , m.media_type
                 , m.mime_type
                 , mm.width
                 , mm.height
                 , m.file_size
                 , mm.duration_seconds
                 , mm.date_taken
                 , mm.gps_latitude
                 , mm.gps_longitude
                 , mm.camera_make
                 , mm.camera_model
                 , mm.lens_make
                 , mm.lens_model
                 , mm.iso
                 , mm.exposure_time
                 , mm.f_number
                 , mm.focal_length
                 , mm.focal_length_35mm
                 , mm.gps_altitude
                 , mm.location_city
                 , mm.location_state
                 , mm.location_country
                 , mm.video_codec
                 , mm.keywords
                 , m.created_at
              FROM media AS m
              JOIN media_access AS ma ON m.id = ma.media_id
              LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
             WHERE ma.user_id = ?
               AND ma.deleted_at IS NULL
               AND m.id IN ({placeholders})
            "#,
            placeholders = placeholders
        )
    }

    fn placeholders(count: usize) -> String {
        std::iter::repeat_n("?", count)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub mod timeline {
    pub const SELECT_MONTH_MARKERS: &str = r#"
     SELECT substr(mm.date_taken, 1, 7)
          , MAX(mm.date_taken)
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
      JOIN media_metadata AS mm ON m.id = mm.media_id
       WHERE ma.user_id = ?
        AND ma.deleted_at IS NULL
        AND mm.date_taken IS NOT NULL
        AND (
             ? = ''
          OR m.id IN (
                 SELECT media_text.media_id
                   FROM media_text
                   WHERE media_text.string LIKE ? ESCAPE '\'
             )
        )
        AND (? = '' OR m.media_type = ?)
        AND (
             ? = ''
          OR (? = 'screenshot' AND EXISTS (
                 SELECT 1 FROM media_screenshot_classifications
                  WHERE media_screenshot_classifications.media_id = m.id
                    AND media_screenshot_classifications.is_screenshot = 1
             ))
          OR (? = 'document' AND EXISTS (
                 SELECT 1 FROM media_document_classifications
                  WHERE media_document_classifications.media_id = m.id
                    AND media_document_classifications.is_document = 1
             ))
        )
     GROUP BY substr(mm.date_taken, 1, 7)
     ORDER BY substr(mm.date_taken, 1, 7) DESC
     "#;

    pub const SELECT_WINDOW: &str = r#"
    SELECT m.id
         , m.filename
         , m.original_filename
         , m.media_type
         , m.mime_type
         , mm.width
         , mm.height
         , m.file_size
         , mm.duration_seconds
         , mm.date_taken
         , mm.gps_latitude
         , mm.gps_longitude
         , mm.camera_make
         , mm.camera_model
         , mm.lens_make
         , mm.lens_model
         , mm.iso
         , mm.exposure_time
         , mm.f_number
         , mm.focal_length
         , mm.focal_length_35mm
         , mm.gps_altitude
         , mm.location_city
         , mm.location_state
         , mm.location_country
         , mm.video_codec
         , mm.keywords
         , m.created_at
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE ma.user_id = ?
       AND ma.deleted_at IS NULL
       AND mm.date_taken >= ?
        AND mm.date_taken <= ?
       AND (
            ? = ''
         OR m.id IN (
                SELECT media_text.media_id
                  FROM media_text
                  WHERE media_text.string LIKE ? ESCAPE '\'
            )
        )
         AND (? = '' OR m.media_type = ?)
         AND (
              ? = ''
           OR (? = 'screenshot' AND EXISTS (
                  SELECT 1 FROM media_screenshot_classifications
                   WHERE media_screenshot_classifications.media_id = m.id
                     AND media_screenshot_classifications.is_screenshot = 1
              ))
           OR (? = 'document' AND EXISTS (
                  SELECT 1 FROM media_document_classifications
                   WHERE media_document_classifications.media_id = m.id
                     AND media_document_classifications.is_document = 1
              ))
         )
         AND mm.date_taken <= ?
      ORDER BY mm.date_taken DESC, m.id DESC
     LIMIT ?
    "#;

    pub const SELECT_PAGINATED_WINDOW: &str = r#"
    SELECT m.id
         , m.filename
         , m.original_filename
         , m.media_type
         , m.mime_type
         , mm.width
         , mm.height
         , m.file_size
         , mm.duration_seconds
         , mm.date_taken
         , mm.gps_latitude
         , mm.gps_longitude
         , mm.camera_make
         , mm.camera_model
         , mm.lens_make
         , mm.lens_model
         , mm.iso
         , mm.exposure_time
         , mm.f_number
         , mm.focal_length
         , mm.focal_length_35mm
         , mm.gps_altitude
         , mm.location_city
         , mm.location_state
         , mm.location_country
         , mm.video_codec
         , mm.keywords
         , m.created_at
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE ma.user_id = ?
       AND ma.deleted_at IS NULL
       AND mm.date_taken >= ?
       AND mm.date_taken <= ?
       AND (
            ? = ''
         OR m.id IN (
                SELECT media_text.media_id
                  FROM media_text
                  WHERE media_text.string LIKE ? ESCAPE '\'
            )
       )
       AND (? = '' OR m.media_type = ?)
       AND (
            ? = ''
         OR (? = 'screenshot' AND EXISTS (
                SELECT 1 FROM media_screenshot_classifications
                 WHERE media_screenshot_classifications.media_id = m.id
                   AND media_screenshot_classifications.is_screenshot = 1
            ))
         OR (? = 'document' AND EXISTS (
                SELECT 1 FROM media_document_classifications
                 WHERE media_document_classifications.media_id = m.id
                   AND media_document_classifications.is_document = 1
            ))
       )
       AND (mm.date_taken < ? OR (mm.date_taken = ? AND m.id < ?))
     ORDER BY mm.date_taken DESC, m.id DESC
     LIMIT ?
    "#;

    pub const SELECT_PAGINATED_WINDOW_ASC: &str = r#"
    SELECT m.id
         , m.filename
         , m.original_filename
         , m.media_type
         , m.mime_type
         , mm.width
         , mm.height
         , m.file_size
         , mm.duration_seconds
         , mm.date_taken
         , mm.gps_latitude
         , mm.gps_longitude
         , mm.camera_make
         , mm.camera_model
         , mm.lens_make
         , mm.lens_model
         , mm.iso
         , mm.exposure_time
         , mm.f_number
         , mm.focal_length
         , mm.focal_length_35mm
         , mm.gps_altitude
         , mm.location_city
         , mm.location_state
         , mm.location_country
         , mm.video_codec
         , mm.keywords
         , m.created_at
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE ma.user_id = ?
       AND ma.deleted_at IS NULL
       AND mm.date_taken >= ?
       AND mm.date_taken <= ?
       AND (
            ? = ''
         OR m.id IN (
                SELECT media_text.media_id
                  FROM media_text
                  WHERE media_text.string LIKE ? ESCAPE '\'
           )
       )
       AND (? = '' OR m.media_type = ?)
       AND (
            ? = ''
         OR (? = 'screenshot' AND EXISTS (
                SELECT 1 FROM media_screenshot_classifications
                 WHERE media_screenshot_classifications.media_id = m.id
                   AND media_screenshot_classifications.is_screenshot = 1
            ))
         OR (? = 'document' AND EXISTS (
                SELECT 1 FROM media_document_classifications
                 WHERE media_document_classifications.media_id = m.id
                   AND media_document_classifications.is_document = 1
            ))
       )
       AND (mm.date_taken > ? OR (mm.date_taken = ? AND m.id > ?))
     ORDER BY mm.date_taken ASC, m.id ASC
     LIMIT ?
    "#;
}

pub mod media_text {
    pub const DELETE_BY_MEDIA_ID_AND_MODEL_TYPE: &str = r#"
    DELETE FROM media_text
     WHERE media_id = ?
       AND model_type = ?
    "#;

    pub const INSERT: &str = r#"
    INSERT INTO media_text (
        media_id
      , model_type
      , model_version
      , string
    ) VALUES (?, ?, ?, ?)
    "#;

    pub const DELETE_BY_MEDIA_ID: &str = r#"
    DELETE FROM media_text
     WHERE media_id = ?
    "#;

    pub const SELECT_MISSING_FOR_MODEL_TYPE: &str = r#"
    SELECT m.id
         , m.file_path
      FROM media AS m
     WHERE m.media_type = 'image'
       AND NOT EXISTS (
            SELECT 1
              FROM media_text
              WHERE media_text.media_id = m.id
                AND media_text.model_type = ?
       )
     ORDER BY m.id
    "#;
}

pub mod metadata {
    pub const SELECT_IMPORTED_MEDIA: &str =
        "SELECT file_path, media_type, content_hash, original_filename, mime_type, file_size FROM media WHERE id = ? AND import_state = 'imported'";
    pub const DELETE_RTREE_FOR_MEDIA: &str = "DELETE FROM media_rtree WHERE media_id = ?";
    pub const INSERT_RTREE: &str = "INSERT INTO media_rtree (media_id, min_lat, max_lat, min_lon, max_lon) VALUES (?, ?, ?, ?, ?)";
    pub const UPSERT_GEOHASH: &str = "INSERT INTO media_metadata (media_id, geohash) VALUES (?, ?) ON CONFLICT(media_id) DO UPDATE SET geohash = excluded.geohash";
    pub const DELETE_AI_INPUTS_FOR_TASK: &str =
        "DELETE FROM media_ai_inputs WHERE media_id = ? AND task = ?";
    pub const INSERT_AI_INPUT: &str = "INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash, frame_timestamp_ms) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";
    pub const SELECT_THUMBNAILS: &str = r#"
    SELECT m.id
         , mm.thumbnail_path
      FROM media AS m
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
    "#;

    pub const CLEAR_METADATA: &str = r#"
    DELETE FROM media_metadata
     WHERE media_id = ?
    "#;

    pub const SELECT_MISSING_METADATA: &str = r#"
    SELECT m.id
         , -1 as user_id
         , m.file_path
         , mm.thumbnail_path
         , m.media_type
         , mm.width
         , mm.height
         , mm.duration_seconds
         , mm.date_taken
         , mm.gps_latitude
         , mm.gps_longitude
         , mm.gps_altitude
         , mm.camera_make
         , mm.camera_model
         , mm.lens_make
         , mm.lens_model
         , mm.iso
         , mm.exposure_time
         , mm.f_number
         , mm.focal_length
         , mm.focal_length_35mm
         , mm.location_city
         , mm.location_state
         , mm.location_country
         , mm.video_codec
         , mm.keywords
      FROM media AS m
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
      WHERE mm.media_id IS NULL
         OR mm.thumbnail_path IS NULL
         OR mm.width IS NULL
         OR mm.height IS NULL
         OR (mm.gps_latitude = 0 AND mm.gps_longitude = 0)
      ORDER BY m.id
    "#;

    pub const UPDATE_METADATA: &str = r#"
    INSERT INTO media_metadata (
        media_id
      , width
      , height
      , date_taken
      , gps_latitude
      , gps_longitude
      , gps_altitude
      , camera_make
      , camera_model
      , lens_make
      , lens_model
      , iso
      , exposure_time
      , f_number
      , focal_length
      , focal_length_35mm
      , location_city
      , location_state
      , location_country
      , video_codec
      , keywords
      , duration_seconds
    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    ON CONFLICT(media_id) DO UPDATE SET
        width = excluded.width
      , height = excluded.height
      , date_taken = excluded.date_taken
      , gps_latitude = excluded.gps_latitude
      , gps_longitude = excluded.gps_longitude
      , gps_altitude = excluded.gps_altitude
      , camera_make = excluded.camera_make
      , camera_model = excluded.camera_model
      , lens_make = excluded.lens_make
      , lens_model = excluded.lens_model
      , iso = excluded.iso
      , exposure_time = excluded.exposure_time
      , f_number = excluded.f_number
      , focal_length = excluded.focal_length
      , focal_length_35mm = excluded.focal_length_35mm
      , location_city = excluded.location_city
      , location_state = excluded.location_state
      , location_country = excluded.location_country
      , video_codec = excluded.video_codec
      , keywords = excluded.keywords
      , duration_seconds = excluded.duration_seconds
    "#;

    pub const UPDATE_THUMBNAIL: &str = r#"
    INSERT INTO media_metadata (thumbnail_path, media_id)
    VALUES (?, ?)
    ON CONFLICT(media_id) DO UPDATE SET
        thumbnail_path = excluded.thumbnail_path
    "#;
}

pub mod albums {
    pub const UPDATE_NAME: &str = "UPDATE albums SET name = ? WHERE id = ?";
    pub const UPDATE_DESCRIPTION: &str = "UPDATE albums SET description = ? WHERE id = ?";
    pub const UPDATE_COVER_MEDIA_ID: &str = "UPDATE albums SET cover_media_id = ? WHERE id = ?";
    pub const UPDATE_NAME_DESCRIPTION: &str =
        "UPDATE albums SET name = ?, description = ? WHERE id = ?";
    pub const UPDATE_NAME_COVER_MEDIA_ID: &str =
        "UPDATE albums SET name = ?, cover_media_id = ? WHERE id = ?";
    pub const UPDATE_DESCRIPTION_COVER_MEDIA_ID: &str =
        "UPDATE albums SET description = ?, cover_media_id = ? WHERE id = ?";
    pub const UPDATE_NAME_DESCRIPTION_COVER_MEDIA_ID: &str =
        "UPDATE albums SET name = ?, description = ?, cover_media_id = ? WHERE id = ?";
    pub const INSERT: &str = r#"
    INSERT INTO albums (
        user_id
      , name
      , description
    ) VALUES (?, ?, ?)
    "#;

    pub const SELECT_BY_ID: &str = r#"
    SELECT a.id
         , a.name
         , a.description
         , a.cover_media_id
         , 0 as media_count
         , a.created_at
      FROM albums AS a
     WHERE a.id = ?
    "#;

    const SELECT_WITH_THUMBNAILS: &str = r#"
    WITH ranked_thumbnails AS (
        SELECT am.album_id
             , am.media_id
             , ROW_NUMBER() OVER (
                   PARTITION BY am.album_id
                   ORDER BY COALESCE(aesthetics.aesthetic_score, 0.0) DESC
                          , am.position
                          , am.media_id
               ) AS thumbnail_rank
          FROM album_media AS am
          LEFT JOIN media_aesthetics AS aesthetics ON aesthetics.media_id = am.media_id
    )
    SELECT a.id
         , a.name
         , a.description
         , a.cover_media_id
         , (SELECT COUNT(*) FROM album_media WHERE album_id = a.id) AS media_count
         , a.created_at
         , MAX(CASE WHEN ranked.thumbnail_rank = 1 THEN ranked.media_id END) AS thumbnail_media_id_1
         , MAX(CASE WHEN ranked.thumbnail_rank = 2 THEN ranked.media_id END) AS thumbnail_media_id_2
         , MAX(CASE WHEN ranked.thumbnail_rank = 3 THEN ranked.media_id END) AS thumbnail_media_id_3
         , MAX(CASE WHEN ranked.thumbnail_rank = 4 THEN ranked.media_id END) AS thumbnail_media_id_4
      FROM albums AS a
      %access_join%
      LEFT JOIN ranked_thumbnails AS ranked
        ON ranked.album_id = a.id
       AND ranked.thumbnail_rank <= 4
     WHERE %filter%
     GROUP BY a.id
     %order_by%
    "#;

    fn select_with_thumbnails_query(access_join: &str, filter: &str, order_by: &str) -> String {
        SELECT_WITH_THUMBNAILS
            .replace("%access_join%", access_join)
            .replace("%filter%", filter)
            .replace("%order_by%", order_by)
    }

    pub fn select_all_for_user_query() -> String {
        select_with_thumbnails_query(
            "JOIN album_access AS aa ON a.id = aa.album_id",
            "aa.user_id = ?",
            "ORDER BY a.created_at DESC",
        )
    }

    pub const CHECK_OWNERSHIP: &str = r#"
    SELECT a.id
      FROM albums AS a
      JOIN album_access AS aa ON a.id = aa.album_id
     WHERE a.id = ?
       AND aa.user_id = ?
       AND aa.access_level = 2
    "#;

    pub const DELETE: &str = r#"
    DELETE FROM albums
     WHERE id = ?
    "#;

    const ADD_MEDIA_BATCH: &str = r#"
    WITH requested(media_id, requested_position) AS (
        VALUES %s
    ), accessible AS (
        SELECT requested.media_id
             , requested.requested_position
             , ROW_NUMBER() OVER (ORDER BY requested.requested_position) - 1 AS position_offset
          FROM requested
          JOIN media_access ON media_access.media_id = requested.media_id
         WHERE media_access.user_id = ?
           AND media_access.deleted_at IS NULL
    )
    INSERT OR IGNORE INTO album_media (
        album_id
      , media_id
      , position
    )
    SELECT ?
         , accessible.media_id
         , COALESCE((SELECT MAX(position) FROM album_media WHERE album_id = ?), -1)
           + 1
           + accessible.position_offset
      FROM accessible
     ORDER BY accessible.requested_position
    "#;

    pub fn build_add_media_batch_query(count: usize) -> String {
        let values = std::iter::repeat_n("(?, ?)", count)
            .collect::<Vec<_>>()
            .join(", ");
        ADD_MEDIA_BATCH.replace("%s", &values)
    }

    pub const REMOVE_MEDIA: &str = r#"
    DELETE FROM album_media
     WHERE album_id = ?
       AND media_id = ?
    "#;

    pub const UPDATE_POSITION: &str = r#"
    UPDATE album_media
       SET position = ?
     WHERE album_id = ?
       AND media_id = ?
    "#;

    pub const SELECT_MEDIA_IDS: &str = r#"
    SELECT media_id
      FROM album_media
     WHERE album_id = ?
     ORDER BY position
            , media_id
    "#;

    pub const SELECT_MEDIA: &str = r#"
    SELECT m.id
         , m.filename
         , m.original_filename
         , m.media_type
         , m.mime_type
         , mm.width
         , mm.height
         , m.file_size
         , mm.duration_seconds
         , mm.date_taken
         , mm.gps_latitude
         , mm.gps_longitude
         , mm.camera_make
         , mm.camera_model
         , mm.lens_make
         , mm.lens_model
         , mm.iso
         , mm.exposure_time
         , mm.f_number
         , mm.focal_length
         , mm.focal_length_35mm
         , mm.gps_altitude
         , mm.location_city
         , mm.location_state
         , mm.location_country
         , mm.video_codec
         , mm.keywords
         , m.created_at
      FROM media AS m
      JOIN album_media AS am ON m.id = am.media_id
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE am.album_id = ?
     ORDER BY am.position
            , m.id
    "#;

    pub const DELETE_ACCESS: &str = r#"
    DELETE FROM album_access
     WHERE album_id = ?
       AND user_id = ?
    "#;

    pub const CHECK_ACCESS_COUNT: &str = r#"
    SELECT COUNT(*) FROM album_access WHERE album_id = ?
    "#;

    pub fn select_with_count_query() -> String {
        select_with_thumbnails_query("", "a.id = ?", "")
    }
}

pub mod map {
    pub const LONGITUDE_CLAUSE_STANDARD: &str = "mm.gps_longitude BETWEEN ? AND ?";
    pub const LONGITUDE_CLAUSE_ANTIMERIDIAN: &str =
        "(mm.gps_longitude >= ? OR mm.gps_longitude <= ?)";

    pub fn build_clusters_query(precision: usize, longitude_clause: &str) -> String {
        format!(
            r#"
            WITH clustered AS (
                SELECT SUBSTR(mm.geohash, 1, {precision}) AS cell
                     , COUNT(*) AS count
                     , AVG(mm.gps_latitude) AS center_lat
                     , AVG(mm.gps_longitude) AS center_lon
                     , MAX(COALESCE(mm.date_taken, m.created_at) || '_' || m.id) AS latest
                  FROM media AS m
                  JOIN media_access AS ma ON m.id = ma.media_id
                  JOIN media_metadata AS mm ON m.id = mm.media_id
                 WHERE ma.user_id = ?
                   AND ma.deleted_at IS NULL
                   AND mm.gps_latitude BETWEEN ? AND ?
                   AND {longitude_clause}
                   AND mm.gps_latitude <> 0
                   AND mm.gps_longitude <> 0
                   AND mm.geohash IS NOT NULL
                  GROUP BY cell
            )
            SELECT c.cell
                 , c.count
                 , c.center_lat
                 , c.center_lon
                 , CAST(SUBSTR(c.latest, INSTR(c.latest, '_') + 1) AS INTEGER) AS representative_id
              FROM clustered AS c
            "#,
            precision = precision,
            longitude_clause = longitude_clause
        )
    }

    pub fn build_media_query(geohash_count: usize, longitude_clause: &str) -> String {
        let geohash_clause = if geohash_count > 0 {
            let conditions = (0..geohash_count)
                .map(|_| "mm.geohash LIKE ?")
                .collect::<Vec<_>>()
                .join(" OR ");
            format!("\n               AND ({})", conditions)
        } else {
            String::new()
        };

        format!(
            r#"
            SELECT m.id
                 , m.filename
                 , m.original_filename
                 , m.media_type
                 , m.mime_type
                 , mm.width
                 , mm.height
                 , m.file_size
                 , mm.duration_seconds
                 , mm.date_taken
                 , mm.gps_latitude
                 , mm.gps_longitude
                 , mm.camera_make
                 , mm.camera_model
                 , mm.lens_make
                 , mm.lens_model
                 , mm.iso
                 , mm.exposure_time
                 , mm.f_number
                 , mm.focal_length
                 , mm.focal_length_35mm
                 , mm.gps_altitude
                 , mm.location_city
                 , mm.location_state
                 , mm.location_country
                 , mm.video_codec
                 , mm.keywords
                 , m.content_hash
                 , m.created_at
              FROM media AS m
              JOIN media_access AS ma ON m.id = ma.media_id
              JOIN media_metadata AS mm ON m.id = mm.media_id
             WHERE ma.user_id = ?
               AND ma.deleted_at IS NULL
                AND mm.gps_latitude BETWEEN ? AND ?
                AND {longitude_clause}
                AND mm.gps_latitude <> 0
                AND mm.gps_longitude <> 0
                AND mm.gps_latitude IS NOT NULL
               AND mm.gps_longitude IS NOT NULL
               AND mm.geohash IS NOT NULL{geohash_clause}
             ORDER BY COALESCE(mm.date_taken, m.created_at) DESC
                    , m.id DESC
            "#,
            longitude_clause = longitude_clause,
            geohash_clause = geohash_clause
        )
    }
}

pub mod users {
    pub const UPDATE_ROLE: &str = "UPDATE users SET role = ? WHERE id = ?";
    pub const UPDATE_ACTIVE: &str = "UPDATE users SET is_active = ? WHERE id = ?";
    pub const UPDATE_ROLE_ACTIVE: &str = "UPDATE users SET role = ?, is_active = ? WHERE id = ?";
    pub const SELECT_ID_BY_CREDENTIALS: &str = r#"
    SELECT id
      FROM users
     WHERE username = ?
        OR email = ?
    "#;

    pub const INSERT: &str = r#"
    INSERT INTO users (
        username
      , email
      , hashed_password
      , role
      , must_change_password
    ) VALUES (?, ?, ?, ?, 0)
    "#;

    pub const SELECT_BY_ID: &str = r#"
    SELECT id
         , username
         , email
         , role
         , must_change_password
         , is_active
         , created_at
      FROM users
     WHERE id = ?
    "#;

    pub const SELECT_ALL: &str = r#"
    SELECT id
         , username
         , email
         , role
         , must_change_password
         , is_active
         , created_at
      FROM users
     ORDER BY created_at DESC
    "#;

    pub const CHECK_EXISTS: &str = r#"
    SELECT id
      FROM users
     WHERE id = ?
    "#;

    pub const DELETE: &str = r#"
    DELETE FROM users
     WHERE id = ?
    "#;

    pub const CHECK_ADMIN: &str = r#"
    SELECT id
      FROM users
     WHERE role = 'admin'
     LIMIT 1
    "#;

    pub const CHECK_ADMIN_BY_ID: &str = r#"
    SELECT id
      FROM users
     WHERE id = ?
       AND role = 'admin'
    "#;

    pub const INSERT_ADMIN: &str = r#"
    INSERT INTO users (
        username
      , email
      , hashed_password
      , role
      , must_change_password
    ) VALUES (?, ?, ?, 'admin', 1)
    "#;
}

pub mod auth {
    pub const SELECT_USER_BY_USERNAME: &str = r#"
    SELECT id
         , username
         , email
         , role
         , hashed_password
         , is_active
      FROM users
     WHERE username = ?
    "#;

    pub const SELECT_USER_BY_ID: &str = r#"
    SELECT id
         , username
         , email
         , role
         , hashed_password
         , is_active
      FROM users
     WHERE id = ?
    "#;

    pub const UPDATE_PASSWORD: &str = r#"
    UPDATE users
       SET hashed_password = ?
     WHERE id = ?
    "#;

    pub const UPDATE_PASSWORD_AND_RESET_FLAG_IF_UNCHANGED: &str = r#"
    UPDATE users
       SET hashed_password = ?
         , must_change_password = 0
     WHERE id = ?
       AND hashed_password = ?
    "#;

    pub const INSERT_REFRESH_TOKEN: &str = r#"
    INSERT INTO refresh_tokens (
        token_hash
      , user_id
      , expires_at
    ) VALUES (?, ?, ?)
    "#;

    pub const VALIDATE_REFRESH_TOKEN: &str = r#"
    SELECT rt.id
         , rt.user_id
         , rt.expires_at
         , rt.revoked
         , u.username
         , u.role
         , u.is_active
      FROM refresh_tokens AS rt
      JOIN users AS u ON rt.user_id = u.id
     WHERE rt.token_hash = ?
       AND rt.revoked = 0
       AND datetime(rt.expires_at) > datetime(?)
    "#;

    pub const REVOKE_REFRESH_TOKEN: &str = r#"
    UPDATE refresh_tokens
       SET revoked = 1
     WHERE id = ?
       AND revoked = 0
       AND datetime(expires_at) > datetime(?)
    "#;

    pub const REVOKE_REFRESH_TOKEN_BY_HASH: &str = r#"
    UPDATE refresh_tokens
       SET revoked = 1
     WHERE token_hash = ?
    "#;

    pub const REVOKE_ALL_USER_TOKENS: &str = r#"
    UPDATE refresh_tokens
       SET revoked = 1
     WHERE user_id = ?
    "#;

    pub const DELETE_REVOKED_TOKEN: &str = r#"
    DELETE FROM refresh_tokens
     WHERE revoked = 1
       AND id = ?
    "#;

    pub const SELECT_PASSWORD_HASH: &str = r#"
    SELECT hashed_password
      FROM users
     WHERE id = ?
    "#;

    pub const SELECT_USER_FOR_TOKEN: &str = r#"
    SELECT id
         , username
         , email
         , role
         , must_change_password
         , is_active
      FROM users
     WHERE id = ?
    "#;
}

pub mod share {
    pub const CHECK_MEDIA_OWNERSHIP: &str = r#"
    SELECT m.id
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
     WHERE m.id = ?
       AND ma.user_id = ?
       AND ma.deleted_at IS NULL
       AND ma.access_level = 2
    "#;

    pub const CHECK_ALBUM_OWNERSHIP: &str = r#"
    SELECT a.id
      FROM albums AS a
      JOIN album_access AS aa ON a.id = aa.album_id
     WHERE a.id = ?
       AND aa.user_id = ?
       AND aa.access_level = 2
    "#;

    pub const INSERT: &str = r#"
    INSERT INTO share_links (
        user_id
      , media_id
      , album_id
      , token
      , password_hash
      , expires_at
    ) VALUES (?, ?, ?, ?, ?, ?)
    "#;

    pub const SELECT_BY_ID: &str = r#"
    SELECT id
         , token
         , media_id
         , album_id
         , password_hash
         , expires_at
         , view_count
         , created_at
      FROM share_links
     WHERE id = ?
    "#;

    pub const SELECT_ALL_FOR_USER: &str = r#"
    SELECT id
         , token
         , media_id
         , album_id
         , password_hash
         , expires_at
         , view_count
         , created_at
      FROM share_links
     WHERE user_id = ?
     ORDER BY created_at DESC
    "#;

    pub const CHECK_OWNERSHIP: &str = r#"
    SELECT id
      FROM share_links
     WHERE id = ?
       AND user_id = ?
    "#;

    pub const DELETE: &str = r#"
    DELETE FROM share_links
     WHERE id = ?
    "#;

    pub const SELECT_BY_TOKEN: &str = r#"
    SELECT id
         , media_id
         , album_id
         , password_hash
         , expires_at
      FROM share_links
     WHERE token = ?
    "#;

    pub const INCREMENT_VIEW_COUNT: &str = r#"
    UPDATE share_links
       SET view_count = view_count + 1
     WHERE id = ?
    "#;
}

pub mod public {
    pub const SELECT_ALBUM_BASIC: &str = r#"
    SELECT id
         , name
         , description
      FROM albums
     WHERE id = ?
    "#;

    pub const SELECT_ALBUM_MEDIA: &str = r#"
    SELECT m.id
         , m.filename
         , m.original_filename
         , m.media_type
         , m.mime_type
         , mm.width
         , mm.height
         , m.file_size
         , mm.duration_seconds
         , mm.date_taken
         , mm.gps_latitude
         , mm.gps_longitude
         , mm.camera_make
         , mm.camera_model
         , mm.lens_make
         , mm.lens_model
         , mm.iso
         , mm.exposure_time
         , mm.f_number
         , mm.focal_length
         , mm.focal_length_35mm
         , mm.gps_altitude
         , mm.location_city
         , mm.location_state
         , mm.location_country
         , mm.video_codec
         , mm.keywords
         , m.created_at
      FROM media AS m
      JOIN album_media AS am ON m.id = am.media_id
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE am.album_id = ?
     ORDER BY am.position
    "#;

    pub const CHECK_ALBUM_MEDIA: &str = r#"
    SELECT 1
      FROM album_media
     WHERE album_id = ?
       AND media_id = ?
    "#;

    pub const SELECT_MEDIA_FILE_INFO: &str = r#"
    SELECT file_path
         , mime_type
         , original_filename
      FROM media
     WHERE id = ?
    "#;

    pub const SELECT_MEDIA_THUMBNAIL: &str = r#"
    SELECT thumbnail_path
      FROM media_metadata
     WHERE media_id = ?
    "#;
}

pub mod trash {
    const SELECT_THUMBNAIL_BATCH: &str = r#"
    SELECT m.id
         , mm.thumbnail_path
         , m.file_path
         , m.media_type
         , ma.user_id
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE ma.user_id = ?
       AND ma.deleted_at IS NOT NULL
       AND m.id IN (%s)
    "#;

    pub fn build_thumbnail_batch_query(count: usize) -> String {
        let placeholders = std::iter::repeat_n("?", count)
            .collect::<Vec<_>>()
            .join(", ");
        SELECT_THUMBNAIL_BATCH.replace("%s", &placeholders)
    }

    pub const DELETE_EMPTY_FACE_GROUPS: &str = "DELETE FROM face_groups WHERE NOT EXISTS (SELECT 1 FROM face_group_members WHERE face_group_members.face_group_id = face_groups.id)";
    pub const SELECT_DELETED: &str = r#"
    SELECT m.id
         , m.filename
         , m.original_filename
         , m.media_type
         , m.mime_type
         , mm.width
         , mm.height
         , m.file_size
         , mm.duration_seconds
         , mm.date_taken
         , ma.deleted_at
         , m.created_at
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE ma.user_id = ?
       AND ma.deleted_at IS NOT NULL
     ORDER BY ma.deleted_at DESC
    "#;

    pub const RESTORE_MEDIA: &str = r#"
    UPDATE media_access
       SET deleted_at = NULL
     WHERE media_id IN ({})
       AND user_id = ?
       AND deleted_at IS NOT NULL
    "#;

    pub const SELECT_FOR_DELETE: &str = r#"
    SELECT m.id
         , m.file_path
         , mm.thumbnail_path
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE m.id IN ({})
       AND ma.user_id = ?
       AND ma.deleted_at IS NOT NULL
    "#;

    pub const DELETE_PERMANENTLY: &str = r#"
    DELETE FROM media
     WHERE id = ?
    "#;

    pub const DELETE_ACCESS: &str = r#"
    DELETE FROM media_access
     WHERE media_id = ?
       AND user_id = ?
       AND deleted_at IS NOT NULL
    "#;

    pub const CHECK_ACCESS_COUNT: &str = r#"
    SELECT COUNT(*) FROM media_access WHERE media_id = ?
    "#;

    pub const SELECT_ALL_DELETED: &str = r#"
    SELECT m.id
         , m.file_path
         , mm.thumbnail_path
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE ma.user_id = ?
       AND ma.deleted_at IS NOT NULL
    "#;

    pub const SELECT_OLD_DELETED: &str = r#"
    SELECT m.id
         , m.file_path
         , mm.thumbnail_path
         , ma.user_id
      FROM media_access AS ma
      JOIN media AS m ON ma.media_id = m.id
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE ma.deleted_at IS NOT NULL
       AND ma.deleted_at < ?
    "#;
}

pub mod access {
    pub const INSERT_MEDIA_ACCESS: &str = r#"
    INSERT OR IGNORE INTO media_access (media_id, user_id, access_level, deleted_at)
    VALUES (?, ?, ?, NULL)
    "#;

    pub const UPSERT_SHARED_MEDIA_ACCESS: &str = r#"
    INSERT INTO media_access (media_id, user_id, access_level, deleted_at)
    VALUES (?, ?, ?, NULL)
    ON CONFLICT (media_id, user_id) DO UPDATE SET
        access_level = excluded.access_level
      , deleted_at = NULL
    "#;

    pub const RESTORE_MEDIA_ACCESS: &str = r#"
    UPDATE media_access
       SET deleted_at = NULL
     WHERE media_id = ?
       AND user_id = ?
    "#;

    pub const INSERT_ALBUM_ACCESS: &str = r#"
    INSERT OR IGNORE INTO album_access (album_id, user_id, access_level)
    VALUES (?, ?, ?)
    "#;

    pub const UPSERT_SHARED_ALBUM_ACCESS: &str = r#"
    INSERT INTO album_access (album_id, user_id, access_level)
    VALUES (?, ?, ?)
    ON CONFLICT (album_id, user_id) DO UPDATE SET
        access_level = excluded.access_level
    "#;

    pub const CHECK_MEDIA_ACCESS: &str = r#"
    SELECT access_level
      FROM media_access
     WHERE media_id = ?
       AND user_id = ?
       AND deleted_at IS NULL
    "#;

    pub const REMOVE_MEDIA_ACCESS: &str = r#"
    DELETE FROM media_access WHERE media_id = ? AND user_id = ?
    "#;

    pub const COUNT_MEDIA_ACCESS: &str = r#"
    SELECT COUNT(*) FROM media_access WHERE media_id = ?
    "#;

    pub const DELETE_MEDIA_PERMANENTLY: &str = r#"
    DELETE FROM media WHERE id = ?
    "#;
}

pub mod deduplicate {
    pub const COUNT_ENSEMBLED_MEDIA: &str =
        "SELECT COUNT(*) FROM media_similarity_index WHERE processing_status = 1";
    pub const COUNT_INDEXES_FROM_OTHER_MODEL: &str =
        "SELECT COUNT(*) FROM media_similarity_index WHERE processing_status = 1 AND model_version != ?";
    pub const DELETE_HASH_BANDS_FROM_OTHER_MODEL: &str = "DELETE FROM media_similarity_hash_bands WHERE media_id IN (SELECT media_id FROM media_similarity_index WHERE processing_status = 1 AND model_version != ?)";
    pub const DELETE_INDEXES_FROM_OTHER_MODEL: &str =
        "DELETE FROM media_similarity_index WHERE processing_status = 1 AND model_version != ?";
    pub const RECOVER_SUBMITTING_JOBS: &str = "UPDATE llm_jobs SET status = 'queued', claimed_at = NULL, updated_at = datetime('now') WHERE task = 'image_clustering' AND status = 'submitting'";
    pub const CANCEL_SUBMITTED_JOBS: &str = "UPDATE llm_jobs SET status = 'cancelled', completed_at = datetime('now'), updated_at = datetime('now') WHERE task = 'image_clustering' AND status = 'submitted'";
    pub const FAIL_INTERRUPTED_RUNS: &str = "UPDATE media_similarity_runs SET status = 'failed', completed_at = datetime('now'), error = 'deduplicate inference was interrupted during restart' WHERE status = 'running' AND EXISTS (SELECT 1 FROM llm_jobs WHERE llm_jobs.deduplicate_run_id = media_similarity_runs.id AND llm_jobs.status = 'cancelled')";
    pub const CREATE_CLUSTERING_JOBS: &str = "INSERT INTO llm_jobs (id, media_id, deduplicate_run_id, task, status) SELECT lower(hex(randomblob(16))), media.id, ?, 'image_clustering', 'queued' FROM media JOIN media_metadata_jobs ON media_metadata_jobs.media_id = media.id WHERE media.import_state = 'imported' AND media_metadata_jobs.status = 'completed' AND EXISTS (SELECT 1 FROM media_ai_inputs WHERE media_ai_inputs.media_id = media.id AND media_ai_inputs.task = 'image_clustering') AND NOT EXISTS (SELECT 1 FROM media_similarity_index WHERE media_similarity_index.media_id = media.id AND media_similarity_index.processing_status = 1) AND NOT EXISTS (SELECT 1 FROM llm_jobs WHERE llm_jobs.deduplicate_run_id = ? AND llm_jobs.media_id = media.id AND llm_jobs.task = 'image_clustering')";
    pub const REQUEUE_MISSING_INPUT_JOBS: &str = "UPDATE llm_jobs SET status = 'queued', last_error = NULL, claimed_at = NULL, completed_at = NULL, available_at = datetime('now'), updated_at = datetime('now') WHERE deduplicate_run_id = ? AND task = 'image_clustering' AND status = 'failed' AND last_error = 'missing prepared AI inputs' AND EXISTS (SELECT 1 FROM media_ai_inputs WHERE media_ai_inputs.media_id = llm_jobs.media_id AND media_ai_inputs.task = 'image_clustering')";
    pub const SELECT_ACTIVE_RUNS: &str =
        "SELECT id, status FROM media_similarity_runs WHERE status IN ('running', 'cancelling')";
    pub const CANCEL_UNSUBMITTED_JOBS: &str = "UPDATE llm_jobs SET status = 'cancelled', completed_at = datetime('now'), updated_at = datetime('now') WHERE deduplicate_run_id = ? AND status IN ('queued', 'submitting')";
    pub const MARK_RUN_CANCELLED: &str = "UPDATE media_similarity_runs SET status = 'cancelled', completed_at = datetime('now') WHERE id = ?";
    pub const COUNT_PENDING_JOBS: &str = "SELECT COUNT(*) FROM llm_jobs WHERE deduplicate_run_id = ? AND status IN ('queued', 'submitting', 'submitted')";
    pub const COUNT_FAILED_JOBS: &str =
        "SELECT COUNT(*) FROM llm_jobs WHERE deduplicate_run_id = ? AND status = 'failed'";
    pub const INTERRUPT_RUNNING: &str = r#"
    UPDATE media_similarity_runs
       SET status = 'interrupted'
         , completed_at = datetime('now')
         , error = 'momento-api restarted while the scan was running'
     WHERE status IN ('running', 'cancelling')
    "#;

    pub const INSERT_RUN: &str = r#"
    INSERT INTO media_similarity_runs (
        trigger
      , status
      , scheduled_for
    ) VALUES (?, 'running', ?)
    "#;

    pub const SELECT_LATEST_RUN: &str = r#"
    SELECT id
         , trigger
         , status
         , scheduled_for
         , started_at
         , completed_at
         , indexed_media
         , processed_media
         , candidate_comparisons
         , clusters_created
         , error
      FROM media_similarity_runs
     ORDER BY id DESC
     LIMIT 1
    "#;

    pub const SELECT_LAST_SCHEDULED_FOR: &str = r#"
    SELECT COALESCE(scheduled_for, started_at)
      FROM media_similarity_runs
     WHERE status = 'completed'
     ORDER BY id DESC
     LIMIT 1
    "#;

    pub const REQUEST_CANCEL: &str = r#"
    UPDATE media_similarity_runs
       SET status = 'cancelling'
     WHERE id = ?
       AND status = 'running'
    "#;

    pub const SELECT_RUN_STATUS: &str = r#"
    SELECT status
      FROM media_similarity_runs
     WHERE id = ?
    "#;

    pub const LOCK_RUN_FOR_REPLACEMENT: &str = r#"
    UPDATE media_similarity_runs
       SET status = status
     WHERE id = ?
       AND status = 'running'
    "#;

    pub const COMPLETE_RUN: &str = r#"
    UPDATE media_similarity_runs
       SET status = ?
         , completed_at = datetime('now')
         , error = ?
     WHERE id = ?
    "#;

    pub const UPDATE_RUN_PROGRESS: &str = r#"
    UPDATE media_similarity_runs
       SET indexed_media = indexed_media + ?
         , processed_media = processed_media + ?
         , candidate_comparisons = candidate_comparisons + ?
         , clusters_created = clusters_created + ?
     WHERE id = ?
    "#;

    pub const SELECT_INDEX_PAGE: &str = r#"
    SELECT media.id
         , media.file_path
         , media.media_type
         , media.content_hash
         , media_metadata.date_taken
      FROM media
       LEFT JOIN media_metadata ON media_metadata.media_id = media.id
       LEFT JOIN media_similarity_index ON media_similarity_index.media_id = media.id
     WHERE media.id > ?
       AND media.content_hash IS NOT NULL
       AND media_similarity_index.media_id IS NULL
     ORDER BY media.id
     LIMIT ?
    "#;

    pub const UPSERT_FAILED_INDEX: &str = r#"
    INSERT INTO media_similarity_index (
        media_id
      , content_hash
      , model_version
      , preprocessing_version
      , embedding
      , perceptual_hash
      , indexed_at
      , processing_status
      , processing_error
    ) VALUES (?, ?, 'unavailable', 'unavailable', X'', -1, datetime('now'), -1, ?)
    ON CONFLICT(media_id) DO UPDATE SET
        content_hash = excluded.content_hash
      , model_version = excluded.model_version
      , preprocessing_version = excluded.preprocessing_version
      , embedding = excluded.embedding
      , perceptual_hash = excluded.perceptual_hash
      , capture_time_seconds = NULL
      , indexed_at = excluded.indexed_at
      , processing_status = excluded.processing_status
      , processing_error = excluded.processing_error
    "#;

    pub const UPSERT_INDEX: &str = r#"
    INSERT INTO media_similarity_index (
        media_id
      , content_hash
      , model_version
      , preprocessing_version
      , embedding
      , perceptual_hash
      , capture_time_seconds
      , indexed_at
      , processing_status
      , processing_error
    ) VALUES (?, ?, ?, ?, ?, ?, ?, datetime('now'), 1, NULL)
    ON CONFLICT(media_id) DO UPDATE SET
        content_hash = excluded.content_hash
      , model_version = excluded.model_version
      , preprocessing_version = excluded.preprocessing_version
      , embedding = excluded.embedding
      , perceptual_hash = excluded.perceptual_hash
      , capture_time_seconds = excluded.capture_time_seconds
      , indexed_at = excluded.indexed_at
      , processing_status = 1
      , processing_error = NULL
    "#;

    pub const DELETE_BANDS: &str = r#"
    DELETE FROM media_similarity_hash_bands
     WHERE media_id = ?
    "#;

    pub const INSERT_BAND: &str = r#"
    INSERT INTO media_similarity_hash_bands (
        media_id
      , band_index
      , band_value
    ) VALUES (?, ?, ?)
    "#;

    pub const MARK_DIRTY: &str = r#"
    INSERT INTO media_similarity_dirty (media_id, marked_at)
    VALUES (?, datetime('now'))
    ON CONFLICT(media_id) DO UPDATE SET marked_at = excluded.marked_at
    "#;

    pub const SELECT_DIRTY_PAGE: &str = r#"
    SELECT dirty.media_id
         , similarity_index.embedding
         , similarity_index.perceptual_hash
         , similarity_index.capture_time_seconds
         , media_metadata.date_taken
      FROM media_similarity_dirty AS dirty
      JOIN media_similarity_index AS similarity_index ON similarity_index.media_id = dirty.media_id
      LEFT JOIN media_metadata ON media_metadata.media_id = dirty.media_id
     WHERE dirty.media_id > ?
       AND similarity_index.processing_status = 1
     ORDER BY dirty.media_id
     LIMIT ?
    "#;

    pub const COUNT_DIRTY: &str = r#"
    SELECT COUNT(*)
      FROM media_similarity_dirty
    "#;

    pub const SELECT_CURRENT_INDEX_PAGE: &str = r#"
    SELECT media_id
         , embedding
         , perceptual_hash
         , capture_time_seconds
     FROM media_similarity_index
     WHERE media_id > ?
       AND processing_status = 1
     ORDER BY media_id
     LIMIT ?
    "#;

    pub const SELECT_CURRENT_INDEX_BY_MEDIA_ID: &str = r#"
    SELECT media_id
         , embedding
         , perceptual_hash
         , capture_time_seconds
      FROM media_similarity_index
     WHERE media_id = ?
       AND processing_status = 1
    "#;

    pub const UPDATE_CAPTURE_TIME: &str = r#"
    UPDATE media_similarity_index
       SET capture_time_seconds = ?
     WHERE media_id = ?
    "#;

    pub const SELECT_BAND_CANDIDATES: &str = r#"
    SELECT DISTINCT candidate_index.media_id
         , candidate_index.embedding
         , candidate_index.perceptual_hash
         , candidate_index.capture_time_seconds
      FROM media_similarity_hash_bands AS source_band
      JOIN media_similarity_hash_bands AS candidate_band
        ON candidate_band.band_index = source_band.band_index
       AND candidate_band.band_value = source_band.band_value
      JOIN media_similarity_index AS candidate_index ON candidate_index.media_id = candidate_band.media_id
     WHERE source_band.media_id = ?
       AND candidate_index.media_id < ?
       AND candidate_index.media_id > ?
       AND candidate_index.processing_status = 1
     ORDER BY candidate_index.media_id
     LIMIT ?
    "#;

    pub const SELECT_TIME_CANDIDATES: &str = r#"
    SELECT media_id
         , embedding
         , perceptual_hash
         , capture_time_seconds
      FROM media_similarity_index
     WHERE capture_time_seconds BETWEEN ? AND ?
       AND media_id < ?
       AND media_id > ?
       AND processing_status = 1
     ORDER BY media_id
     LIMIT ?
    "#;

    pub const INSERT_CLUSTER: &str = r#"
    INSERT INTO media_similarity_clusters (
        kind
      , representative_media_id
    ) VALUES (?, ?)
    "#;

    pub const INSERT_CLUSTER_MEMBER: &str = r#"
    INSERT OR IGNORE INTO media_similarity_cluster_members (
        cluster_id
      , media_id
      , cosine_similarity
      , perceptual_hash_distance
    ) VALUES (?, ?, ?, ?)
    "#;

    pub const INSERT_CLUSTERS_FROM_JSON: &str = r#"
    INSERT INTO media_similarity_clusters (
        id
      , kind
      , representative_media_id
    )
    SELECT json_extract(cluster.value, '$.id')
         , json_extract(cluster.value, '$.kind')
         , json_extract(cluster.value, '$.representativeMediaId')
      FROM json_each(?) AS cluster
     ORDER BY json_extract(cluster.value, '$.id')
    "#;

    pub const INSERT_CLUSTER_MEMBERS_FROM_JSON: &str = r#"
    INSERT INTO media_similarity_cluster_members (
        cluster_id
      , media_id
      , cosine_similarity
      , perceptual_hash_distance
    )
    SELECT json_extract(cluster.value, '$.id')
         , json_extract(member.value, '$.mediaId')
         , json_extract(member.value, '$.cosineSimilarity')
         , json_extract(member.value, '$.perceptualHashDistance')
      FROM json_each(?) AS cluster
      JOIN json_each(json_extract(cluster.value, '$.members')) AS member
     ORDER BY json_extract(cluster.value, '$.id')
            , json_extract(member.value, '$.mediaId')
    "#;

    pub const SELECT_VISIBLE_CLUSTER_PAGE: &str = r#"
    WITH ordered_visible_members AS (
        SELECT members.cluster_id
             , members.media_id
          FROM media_similarity_cluster_members AS members
          JOIN media_access ON media_access.media_id = members.media_id
         WHERE media_access.user_id = ?
           AND media_access.deleted_at IS NULL
         GROUP BY members.cluster_id
                , members.media_id
         ORDER BY members.cluster_id
                , members.media_id
    ), visible_clusters AS (
        SELECT cluster_id
             , group_concat(media_id, ',') AS media_ids
          FROM ordered_visible_members
         GROUP BY cluster_id
        HAVING COUNT(*) >= 2
    ), canonical_clusters AS (
        SELECT MIN(cluster_id) AS cluster_id
             , media_ids
          FROM visible_clusters
         GROUP BY media_ids
    ), cluster_page AS (
        SELECT cluster_id
          FROM canonical_clusters
         WHERE cluster_id > ?
         ORDER BY cluster_id
         LIMIT ?
    ), totals AS (
        SELECT COUNT(*) AS total_groups
          FROM canonical_clusters
    ), media_totals AS (
        SELECT COUNT(DISTINCT members.media_id) AS total_media
          FROM ordered_visible_members AS members
          JOIN canonical_clusters AS clusters ON clusters.cluster_id = members.cluster_id
    )
    SELECT cluster_page.cluster_id
         , totals.total_groups
         , media_totals.total_media
      FROM totals
      CROSS JOIN media_totals
      LEFT JOIN cluster_page ON TRUE
     ORDER BY cluster_page.cluster_id
    "#;

    const SELECT_VISIBLE_CLUSTER_MEDIA_BATCH: &str = r#"
    SELECT media.id
         , media.filename
         , media.original_filename
         , media.media_type
         , media.mime_type
         , media_metadata.width
         , media_metadata.height
         , media.file_size
         , media_metadata.duration_seconds
         , media_metadata.date_taken
         , media_metadata.gps_latitude
         , media_metadata.gps_longitude
         , media_metadata.camera_make
         , media_metadata.camera_model
         , media_metadata.lens_make
         , media_metadata.lens_model
         , media_metadata.iso
         , media_metadata.exposure_time
         , media_metadata.f_number
         , media_metadata.focal_length
         , media_metadata.focal_length_35mm
         , media_metadata.gps_altitude
         , media_metadata.location_city
         , media_metadata.location_state
         , media_metadata.location_country
         , media_metadata.video_codec
         , media_metadata.keywords
         , media.created_at
         , members.cluster_id
       FROM media_similarity_cluster_members AS members
      JOIN media ON media.id = members.media_id
      JOIN media_access ON media_access.media_id = media.id
      LEFT JOIN media_metadata ON media_metadata.media_id = media.id
     WHERE members.cluster_id IN (%s)
       AND media_access.user_id = ?
       AND media_access.deleted_at IS NULL
     ORDER BY members.cluster_id
            , media.id
    "#;

    pub fn build_visible_cluster_media_query(cluster_count: usize) -> String {
        let placeholders = std::iter::repeat_n("?", cluster_count)
            .collect::<Vec<_>>()
            .join(",");
        SELECT_VISIBLE_CLUSTER_MEDIA_BATCH.replace("%s", &placeholders)
    }

    pub const CLEAN_CLUSTERS: &str = "DELETE FROM media_similarity_clusters";
    pub const CLEAN_BANDS: &str = "DELETE FROM media_similarity_hash_bands";
    pub const CLEAN_INDEX: &str = "DELETE FROM media_similarity_index";
    pub const CLEAN_DIRTY: &str = "DELETE FROM media_similarity_dirty";
    pub const CLEAN_RUNS: &str = "DELETE FROM media_similarity_runs";
    pub const LOCK_RUNS: &str = r#"
    UPDATE media_similarity_runs
       SET status = status
     WHERE status IN ('running', 'cancelling')
    "#;
    pub const COUNT_ACTIVE_RUNS: &str = r#"
    SELECT COUNT(*)
      FROM media_similarity_runs
     WHERE status IN ('running', 'cancelling')
    "#;
    pub const MARK_ALL_DIRTY: &str = r#"
    INSERT OR IGNORE INTO media_similarity_dirty (media_id)
    SELECT id
      FROM media
    "#;
}
