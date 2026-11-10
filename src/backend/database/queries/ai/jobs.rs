pub const SELECT_SUBMITTED_PAGE: &str = "SELECT id, media_id, task, attempts FROM llm_jobs WHERE status = 'submitted' AND id > ? AND NOT EXISTS (SELECT 1 FROM llm_result_receipts r WHERE r.job_id = llm_jobs.id AND r.state NOT IN ('discarded', 'cleaned')) ORDER BY id LIMIT 256";
pub const REQUEUE_MISSING_SUBMITTED: &str = "UPDATE llm_jobs SET status = 'queued', state_version = state_version + 1, attempts = attempts - 1, claimed_at = NULL, available_at = datetime('now'), updated_at = datetime('now') WHERE id = ? AND status = 'submitted' AND attempts = ? AND attempts > 0 AND NOT EXISTS (SELECT 1 FROM llm_result_receipts r WHERE r.job_id = llm_jobs.id AND r.state NOT IN ('discarded', 'cleaned'))";
pub fn insert_eligible(task: &str) -> Option<String> {
    let current_ocr = format!(
        "SELECT media_id FROM media_text WHERE model_type = ?1 AND model_version = '{}'",
        momento_common::llm::OCR_MODEL_VERSION
    );
    let completed_media = match task {
        "ocr" => current_ocr.as_str(),
        "image_tagging" => "SELECT media_id FROM media_text WHERE model_type = ?1",
        "image_aesthetics" => "SELECT media_id FROM media_aesthetics",
        "screenshot_detection" => "SELECT media_id FROM media_screenshot_classifications",
        "document_detection" => "SELECT media_id FROM media_document_classifications",
        "face_detection" => "SELECT media_id FROM media_face_detection_results",
        "image_clustering" => {
            "SELECT media_id FROM media_similarity_index WHERE processing_status = 1"
        }
        _ => return None,
    };
    let existing_jobs = if task == "image_clustering" {
        "deduplicate_run_id = ?2"
    } else {
        "status IN ('queued','submitting','submitted')"
    };
    Some(format!(
        r#"
INSERT INTO llm_jobs (id, media_id, task, status, face_grouping_run_id, deduplicate_run_id)
SELECT lower(hex(randomblob(16))), media.id, ?1, 'queued'
 , CASE WHEN ?1 = 'face_detection' THEN ?2 END
 , CASE WHEN ?1 = 'image_clustering' THEN ?2 END
  FROM media
  JOIN media_metadata_jobs ON media_metadata_jobs.media_id = media.id
 WHERE media.import_state = 'imported'
   AND media_metadata_jobs.status = 'completed'
   AND (?1 NOT IN ('screenshot_detection', 'document_detection') OR media.media_type = 'image')
   AND NOT EXISTS (SELECT 1 FROM metadata_clean_operations)
   AND EXISTS (
   SELECT 1 FROM media_ai_inputs
    WHERE media_ai_inputs.media_id = media.id AND task = ?1
   )
   AND media.id NOT IN (
   SELECT media_id FROM llm_jobs
    WHERE task = ?1 AND {existing_jobs}
   )
   AND media.id NOT IN (
   {completed_media}
   )
"#
    ))
}
pub const SELECT_QUEUED: &str = "SELECT id, media_id, task, attempts FROM llm_jobs WHERE status = 'queued' AND available_at <= datetime('now') AND NOT EXISTS (SELECT 1 FROM llm_cancellation_scopes WHERE llm_cancellation_scopes.scope = 'all' OR (llm_cancellation_scopes.scope = 'task' AND llm_cancellation_scopes.task = llm_jobs.task)) ORDER BY created_at LIMIT ?";
pub const NEXT_AVAILABLE_DELAY_SECONDS: &str = r#"
WITH future_work(ready_at) AS (
    SELECT available_at
      FROM llm_jobs
     WHERE status = 'queued'
       AND available_at > datetime('now')
       AND NOT EXISTS (SELECT 1 FROM metadata_clean_operations)
       AND NOT EXISTS (
               SELECT 1
                 FROM llm_cancellation_scopes
                WHERE llm_cancellation_scopes.scope = 'all'
                   OR (llm_cancellation_scopes.scope = 'task'
                       AND llm_cancellation_scopes.task = llm_jobs.task)
           )
    UNION ALL
    SELECT datetime(claimed_at, '+5 minutes')
      FROM llm_jobs
     WHERE status = 'submitting'
       AND claimed_at IS NOT NULL
       AND datetime(claimed_at, '+5 minutes') > datetime('now')
)
SELECT CAST(
           MAX(1, unixepoch(MIN(ready_at)) - unixepoch('now'))
           AS INTEGER
       )
  FROM future_work
"#;
pub const CLAIM: &str = "UPDATE llm_jobs SET status = 'submitting', state_version = state_version + 1, claimed_at = datetime('now'), updated_at = datetime('now') WHERE id = ? AND status = 'queued' AND NOT EXISTS (SELECT 1 FROM metadata_clean_operations)";
pub const MARK_SUBMITTED: &str = "UPDATE llm_jobs SET status = 'submitted', state_version = state_version + 1, attempts = attempts + 1, submitted_at = datetime('now'), updated_at = datetime('now') WHERE id = ? AND status = 'submitting' AND attempts + 1 = ?";
pub const REQUEUE_AMBIGUOUS: &str = "UPDATE llm_jobs SET status = 'queued', state_version = state_version + 1, claimed_at = NULL, available_at = datetime('now'), updated_at = datetime('now') WHERE id = ? AND status = 'submitting'";
pub const SELECT_MISSING_VIDEO_FRAME_REPAIR_OWNER: &str = r#"
    SELECT j.media_id
      FROM llm_jobs AS j
      JOIN media AS m ON m.id = j.media_id
      JOIN media_metadata_jobs AS metadata ON metadata.media_id = j.media_id
      JOIN llm_job_inputs AS snapshot ON snapshot.job_id = j.id AND snapshot.sequence = ?2
      JOIN media_ai_inputs AS current ON current.media_id = j.media_id
       AND current.task = j.task AND current.sequence = snapshot.sequence
     WHERE j.id = ?1
       AND j.status = 'submitting'
       AND m.import_state = 'imported'
       AND m.media_type = 'video'
       AND m.file_path = ?4
       AND metadata.status IN ('completed', 'queued', 'processing')
       AND snapshot.storage_root = 'previews'
       AND snapshot.input_kind = 'video_frame'
       AND snapshot.frame_timestamp_ms = 0
       AND snapshot.file_path = ?3
       AND current.storage_root = snapshot.storage_root
       AND current.input_kind = snapshot.input_kind
       AND current.file_path = snapshot.file_path
       AND current.byte_size = snapshot.byte_size
       AND current.content_hash = snapshot.content_hash
       AND current.frame_timestamp_ms = snapshot.frame_timestamp_ms
       AND NOT EXISTS (SELECT 1 FROM metadata_clean_operations)
       AND NOT EXISTS (
           SELECT 1 FROM llm_cancellation_scopes AS cancellation
            WHERE cancellation.scope = 'all'
               OR (cancellation.scope = 'task' AND cancellation.task = j.task)
       )
"#;
pub const REQUEUE_DEFERRED: &str = "UPDATE llm_jobs SET status = 'queued', state_version = state_version + 1, claimed_at = NULL, available_at = datetime('now', '+' || ? || ' seconds'), last_error = NULL, updated_at = datetime('now') WHERE id = ? AND status = 'submitting'";
pub const SNAPSHOT_QUEUED_INPUTS: &str = "INSERT OR IGNORE INTO llm_job_inputs (job_id, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash, frame_timestamp_ms) SELECT llm_jobs.id, media_ai_inputs.sequence, media_ai_inputs.input_kind, media_ai_inputs.storage_root, media_ai_inputs.file_path, media_ai_inputs.filename, media_ai_inputs.mime_type, media_ai_inputs.byte_size, media_ai_inputs.content_hash, media_ai_inputs.frame_timestamp_ms FROM llm_jobs JOIN media_ai_inputs ON media_ai_inputs.media_id = llm_jobs.media_id AND media_ai_inputs.task = llm_jobs.task WHERE llm_jobs.status = 'queued'";
pub const SELECT_INPUTS: &str = "SELECT sequence, storage_root, file_path, filename, mime_type, byte_size, content_hash, input_kind, frame_timestamp_ms FROM llm_job_inputs WHERE job_id = ? ORDER BY sequence";
pub const RECLAIM_STALE: &str = "UPDATE llm_jobs SET status = 'queued', state_version = state_version + 1, claimed_at = NULL, updated_at = datetime('now') WHERE status = 'submitting' AND (claimed_at IS NULL OR claimed_at <= datetime('now', '-5 minutes'))";
pub const RETRY_OR_FAIL: &str = "UPDATE llm_jobs SET status = CASE WHEN attempts + 1 >= 5 THEN 'failed' ELSE 'queued' END, state_version = state_version + 1, attempts = attempts + 1, available_at = datetime('now', '+30 seconds'), last_error = ?, completed_at = CASE WHEN attempts + 1 >= 5 THEN datetime('now') ELSE NULL END, updated_at = datetime('now') WHERE id = ? AND status = 'submitting'";
pub const MARK_FAILED: &str = "UPDATE llm_jobs SET status = 'failed', state_version = state_version + 1, last_error = ?, completed_at = datetime('now'), updated_at = datetime('now') WHERE id = ? AND status = 'submitting'";
macro_rules! latest_job_rowid {
    ($owner:literal) => {
        concat!(
            "(SELECT MAX(latest.rowid) FROM llm_jobs AS latest WHERE latest.media_id = ",
            $owner,
            ".media_id AND latest.task = ",
            $owner,
            ".task)"
        )
    };
}
pub const SELECT_LATEST_STATUS_COUNTS: &str = concat!(
    r#"
SELECT r.task
     , r.status
     , COUNT(*)
  FROM (SELECT media_id, task FROM llm_jobs GROUP BY media_id, task) AS owners
  JOIN llm_jobs AS r ON r.rowid = "#,
    latest_job_rowid!("owners"),
    r#"
 GROUP BY r.task
        , r.status
 ORDER BY r.task
        , r.status
"#
);
pub const SELECT_LATEST_FAILURES: &str = concat!(
    r#"
SELECT r.task
     , r.last_error
     , r.id
     , r.media_id
     , r.attempts
     , m.original_filename
     , m.file_path
  FROM llm_jobs AS r
  JOIN media AS m ON m.id = r.media_id
 WHERE r.status = 'failed'
   AND r.rowid = "#,
    latest_job_rowid!("r"),
    r#"
   AND last_error IS NOT NULL
 ORDER BY r.task
        , r.updated_at DESC
"#
);
pub const COUNT_ACTIVE_FOR_TASK: &str = "SELECT COUNT(*) FROM llm_jobs WHERE task = ? AND status IN ('queued', 'submitting', 'submitted')";
pub const COUNT_PENDING_RESULT_CLEANUP_FOR_TASK: &str = r#"
    SELECT COUNT(*)
      FROM llm_result_receipts AS r
      JOIN data_dir_space_reservations AS s ON s.id = r.sqlite_reservation_id
     WHERE r.task = ?
       AND s.state = 'active'
"#;
pub const COUNT_JOBS_FOR_TASK: &str = "SELECT COUNT(*) FROM llm_jobs WHERE task = ?";
pub const COUNT_PENDING_CANCELLATION_SCOPE_FOR_TASK: &str = "SELECT COUNT(*) FROM llm_cancellation_scopes WHERE scope = 'all' OR (scope = 'task' AND task = ?)";
pub const DELETE_TEXT_FOR_TASK: &str = "DELETE FROM media_text WHERE model_type = ?";
pub const DELETE_TEXT_INPUTS_FOR_TASK: &str = "DELETE FROM media_text_inputs WHERE model_type = ?";
pub const DELETE_AESTHETICS: &str = "DELETE FROM media_aesthetics";
pub const DELETE_AESTHETIC_INPUTS: &str = "DELETE FROM media_aesthetic_inputs";
pub const DELETE_SCREENSHOT_CLASSIFICATIONS: &str = "DELETE FROM media_screenshot_classifications";
pub const DELETE_SCREENSHOT_CLASSIFICATION_INPUTS: &str =
    "DELETE FROM media_screenshot_classification_inputs";
pub const DELETE_DOCUMENT_CLASSIFICATIONS: &str = "DELETE FROM media_document_classifications";
pub const DELETE_DOCUMENT_CLASSIFICATION_INPUTS: &str =
    "DELETE FROM media_document_classification_inputs";
pub const DELETE_JOBS_FOR_TASK: &str = "DELETE FROM llm_jobs WHERE task = ?";
pub const CANCEL_FOR_TASK: &str = "UPDATE llm_jobs SET status = 'cancelled', state_version = state_version + 1, attempts = attempts + CASE WHEN status = 'submitting' THEN 1 ELSE 0 END, completed_at = datetime('now'), updated_at = datetime('now') WHERE task = ? AND status IN ('queued', 'submitting', 'submitted')";
pub const CANCEL_ALL: &str = "UPDATE llm_jobs SET status = 'cancelled', state_version = state_version + 1, attempts = attempts + CASE WHEN status = 'submitting' THEN 1 ELSE 0 END, completed_at = datetime('now'), updated_at = datetime('now') WHERE status IN ('queued', 'submitting', 'submitted')";
pub const CANCEL_RESULT_RECEIPTS_FOR_TASK: &str = "UPDATE llm_result_receipts SET cancel_requested = 1, state = 'discarded', claim_token = NULL, updated_at = datetime('now') WHERE task = ? AND state IN ('receiving', 'received', 'processing')";
pub const CANCEL_ALL_RESULT_RECEIPTS: &str = "UPDATE llm_result_receipts SET cancel_requested = 1, state = 'discarded', claim_token = NULL, updated_at = datetime('now') WHERE state IN ('receiving', 'received', 'processing')";
pub const QUEUE_CANCELLATION_SCOPE_FOR_TASK: &str =
    "INSERT OR IGNORE INTO llm_cancellation_scopes (scope, task) VALUES ('task', ?)";
pub const QUEUE_ALL_CANCELLATION_SCOPE: &str =
    "INSERT OR IGNORE INTO llm_cancellation_scopes (scope, task) VALUES ('all', '')";
pub const QUEUE_CANCELLATIONS_FOR_TASK: &str = "INSERT OR IGNORE INTO llm_job_cancellations (job_id, task) SELECT id, task FROM llm_jobs WHERE task = ? AND status IN ('queued', 'submitting', 'submitted', 'failed')";
pub const QUEUE_ALL_CANCELLATIONS: &str = "INSERT OR IGNORE INTO llm_job_cancellations (job_id, task) SELECT id, task FROM llm_jobs WHERE status IN ('queued', 'submitting', 'submitted', 'failed')";
pub const SELECT_CANCELLATION_SCOPE: &str = "SELECT scope, task FROM llm_cancellation_scopes ORDER BY CASE scope WHEN 'all' THEN 0 ELSE 1 END, created_at, task LIMIT 1";
pub const SELECT_CANCELLATIONS_FOR_TASK: &str =
    "SELECT job_id FROM llm_job_cancellations WHERE task = ? ORDER BY created_at, job_id LIMIT ?";
pub const SELECT_ALL_CANCELLATIONS: &str =
    "SELECT job_id FROM llm_job_cancellations ORDER BY created_at, job_id LIMIT ?";
pub const DELETE_CANCELLATION: &str = "DELETE FROM llm_job_cancellations WHERE job_id = ?";
pub const COUNT_CANCELLATIONS_FOR_TASK: &str =
    "SELECT COUNT(*) FROM llm_job_cancellations WHERE task = ?";
pub const COUNT_ALL_CANCELLATIONS: &str = "SELECT COUNT(*) FROM llm_job_cancellations";
pub const DELETE_CANCELLATION_SCOPE_FOR_TASK: &str =
    "DELETE FROM llm_cancellation_scopes WHERE scope = 'task' AND task = ?";
pub const DELETE_ALL_CANCELLATION_SCOPES: &str = "DELETE FROM llm_cancellation_scopes";

// Bound all potential OCR jobs and any queued inputs that the shared snapshot copies.
pub const SELECT_OCR_START_FOOTPRINT: &str = r#"
WITH inputs AS (
    SELECT i.*
      FROM media_ai_inputs AS i
     WHERE i.task = 'ocr'
        OR EXISTS (
            SELECT 1 FROM llm_jobs AS j
             WHERE j.media_id = i.media_id AND j.task = i.task AND j.status = 'queued'
        )
)
SELECT (SELECT COUNT(DISTINCT media_id) FROM inputs WHERE task = 'ocr')
     , COUNT(*)
     , COALESCE(SUM(LENGTH(CAST(file_path AS BLOB))
                 + LENGTH(CAST(filename AS BLOB))
                 + LENGTH(CAST(mime_type AS BLOB))
                 + LENGTH(CAST(content_hash AS BLOB))), 0)
  FROM inputs
"#;
