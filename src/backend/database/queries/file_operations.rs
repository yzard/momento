pub const REQUEUE_DISCARDED_METADATA: &str = r#"
    UPDATE media_metadata_jobs
       SET status = 'queued', claim_token = NULL, claimed_at = NULL,
           available_at = datetime('now'), completed_at = NULL,
           updated_at = datetime('now')
     WHERE media_id = (SELECT CAST(owner_id AS INTEGER) FROM file_operation_groups WHERE id = ?1)
       AND EXISTS (
           SELECT 1 FROM file_operation_groups AS g
            WHERE g.id = ?1 AND g.kind = 'metadata_artifacts'
              AND g.owner_kind = 'metadata_generation'
              AND g.owner_id = CAST(media_metadata_jobs.media_id AS TEXT)
              AND g.cancel_requested = 1 AND g.state IN ('cleaned', 'rolled_back')
              AND g.product_version > COALESCE((
                  SELECT artifact_version FROM media_metadata WHERE media_id = media_metadata_jobs.media_id
              ), 0)
              AND (media_metadata_jobs.status = 'failed'
                   OR (media_metadata_jobs.status = 'processing' AND media_metadata_jobs.claim_token = g.claim_token))
     )
"#;
// Only detached, unpublished derived products are disposable. Originals and
// general filesystem mutations retain their evidence-checked recovery path.
pub const BEGIN_DERIVED_PRODUCT_DISCARD: &str = r#"
    UPDATE file_operation_groups
       SET state = 'cleanup_pending', completion_outcome = 'discarded',
           version = version + 1, updated_at = datetime('now'),
           finalization_error_kind = COALESCE(finalization_error_kind, 'InterruptedProduct'),
           finalization_error = COALESCE(finalization_error, 'Interrupted product discarded for regeneration')
     WHERE id = ? AND cancel_requested = 1 AND product_target IS NULL
       AND state IN ('publishing', 'publication_failed', 'files_committed', 'finalize_failed')
       AND ((kind = 'metadata_artifacts' AND owner_kind = 'metadata_generation')
            OR (kind IN ('llm_result_artifacts', 'llm_result_receive') AND owner_kind = 'llm_result')
            OR (kind = 'video_ai_frame' AND owner_kind = 'generated_artifact'))
       AND entry_count > 0
       AND NOT EXISTS (
           SELECT 1 FROM file_operation_entries
            WHERE group_id = file_operation_groups.id
              AND (action != 'publish' OR storage_root NOT IN ('thumbnails', 'tiny_thumbnails', 'previews', 'journal'))
       )
"#;
pub const IS_DERIVED_PRODUCT_DISCARD: &str = r#"
    SELECT 1 FROM file_operation_groups
     WHERE id = ? AND cancel_requested = 1 AND product_target IS NULL
       AND completion_outcome = 'discarded' AND state = 'cleanup_pending'
       AND ((kind = 'metadata_artifacts' AND owner_kind = 'metadata_generation')
            OR (kind IN ('llm_result_artifacts', 'llm_result_receive') AND owner_kind = 'llm_result')
            OR (kind = 'video_ai_frame' AND owner_kind = 'generated_artifact'))
       AND NOT EXISTS (
           SELECT 1 FROM file_operation_entries
            WHERE group_id = file_operation_groups.id
              AND (action != 'publish' OR storage_root NOT IN ('thumbnails', 'tiny_thumbnails', 'previews', 'journal'))
       )
"#;
pub const SELECT_DERIVED_PRODUCT_DISCARD_ENTRIES: &str = r#"
    SELECT e.sequence, 'cleanup', e.storage_root, e.temporary_path, NULL,
           CASE WHEN EXISTS (
               SELECT 1 FROM file_product_references AS reference
                WHERE reference.storage_root = e.storage_root
                  AND reference.file_path = e.destination_path
           ) THEN NULL ELSE e.destination_path END,
           NULL, NULL, NULL, NULL
      FROM file_operation_entries AS e
      JOIN file_operation_groups AS g ON g.id = e.group_id
     WHERE e.group_id = ? AND e.cleanup_state = 'pending'
     ORDER BY e.sequence LIMIT ?
"#;
pub const INSERT_GROUP: &str = "INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, claim_token, state, product_target, product_version, entry_count, recovery_order) VALUES (?, ?, ?, ?, ?, 'prepared', ?, ?, ?, (SELECT COALESCE(MAX(recovery_order), 0) + 1 FROM file_operation_groups))";
pub const INSERT_COMMITTED_CLEANUP_GROUP: &str = "INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, claim_token, state, product_target, product_version, entry_count, completion_outcome, recovery_order) VALUES (?, ?, ?, ?, ?, 'cleanup_pending', ?, ?, ?, 'published', (SELECT COALESCE(MAX(recovery_order), 0) + 1 FROM file_operation_groups))";
pub const INSERT_ENTRY: &str = "INSERT INTO file_operation_entries (group_id, sequence, action, storage_root, source_path, temporary_path, destination_path, tombstone_path, expected_size, expected_sha256, expected_version) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";
pub const INSERT_DIRECTORY_COPY: &str = "INSERT INTO directory_copy_constructions (group_id, storage_root, source_root, temporary_root, expected_file_bytes, expected_entry_count, expected_fingerprint) VALUES (?, ?, ?, ?, ?, ?, ?)";
pub const INSERT_DIRECTORY_COPY_ROOT_CURSOR: &str = "INSERT INTO directory_copy_cursors (group_id, depth, source_path, temporary_path, resume_offset) VALUES (?, 0, ?, ?, 0)";
pub const SELECT_DIRECTORY_COPY: &str = "SELECT c.group_id, c.storage_root, c.expected_file_bytes, c.expected_entry_count, c.expected_fingerprint, c.copied_file_bytes, c.copied_entry_count, c.copied_fingerprint, c.state, g.entry_count FROM directory_copy_constructions AS c JOIN file_operation_groups AS g ON g.id = c.group_id WHERE g.state = 'prepared' AND (? IS NULL OR c.group_id = ?) ORDER BY c.group_id LIMIT 1";
pub const SELECT_DIRECTORY_COPY_CURSORS: &str = "SELECT depth, source_path, temporary_path, resume_offset FROM directory_copy_cursors WHERE group_id = ? ORDER BY depth";
pub const SELECT_DIRECTORY_COPY_CURSOR: &str = "SELECT source_path, temporary_path, resume_offset FROM directory_copy_cursors WHERE group_id = ? AND depth = ?";
pub const ADVANCE_DIRECTORY_COPY_CURSOR: &str = "UPDATE directory_copy_cursors SET resume_offset = ? WHERE group_id = ? AND depth = ? AND resume_offset = ?";
pub const INSERT_DIRECTORY_COPY_CURSOR: &str = "INSERT INTO directory_copy_cursors (group_id, depth, source_path, temporary_path, resume_offset) VALUES (?, ?, ?, ?, 0)";
pub const UPDATE_DIRECTORY_COPY_MEASUREMENT: &str = "UPDATE directory_copy_constructions SET copied_file_bytes = copied_file_bytes + ?, copied_entry_count = copied_entry_count + 1, copied_fingerprint = ?, updated_at = datetime('now') WHERE group_id = ? AND state = 'building' AND copied_file_bytes <= expected_file_bytes - ? AND copied_entry_count < expected_entry_count";
pub const DELETE_DIRECTORY_COPY_CURSOR: &str =
    "DELETE FROM directory_copy_cursors WHERE group_id = ? AND depth = ?";
pub const COMPLETE_DIRECTORY_COPY: &str = "UPDATE directory_copy_constructions SET state = 'complete', updated_at = datetime('now') WHERE group_id = ? AND state = 'building' AND copied_file_bytes = expected_file_bytes AND copied_entry_count = expected_entry_count AND copied_fingerprint = expected_fingerprint AND NOT EXISTS (SELECT 1 FROM directory_copy_cursors WHERE group_id = directory_copy_constructions.group_id)";
pub const INSERT_PATH_CLAIM: &str = "INSERT INTO file_operation_path_claims (group_id, sequence, storage_root, relative_path, path_key, mode, scope, role, expected_version) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)";
pub const INSERT_JOURNAL_RESERVATION: &str = "INSERT INTO data_dir_space_reservations (id, class, owner_kind, owner_id, journal_group_id, filesystem_id, reserved_peak_additional_bytes, state) VALUES (?, 'journal', ?, ?, ?, ?, ?, 'active')";
pub const INSERT_SQLITE_RESULT_RESERVATION: &str = "INSERT INTO data_dir_space_reservations (id, class, owner_kind, owner_id, filesystem_id, reserved_peak_additional_bytes, state, version) VALUES (?, 'sqlite', 'llm_result', ?, ?, ?, 'active', ?)";
pub const FIND_EQUAL_CLAIM_CONFLICT: &str = "SELECT 1 FROM file_operation_path_claims WHERE storage_root = ? AND path_key = ? AND (? = 'write' OR mode = 'write') LIMIT 1";
pub const FIND_SUBTREE_ANCESTOR_CONFLICT: &str = "SELECT 1 FROM file_operation_path_claims WHERE storage_root = ? AND path_key = ? AND scope = 'subtree' AND (? = 'write' OR mode = 'write') LIMIT 1";
pub const FIND_SUBTREE_DESCENDANT_CONFLICT: &str = "SELECT 1 FROM file_operation_path_claims WHERE storage_root = ? AND path_key >= ? AND path_key < ? AND (? = 'write' OR mode = 'write') LIMIT 1";
pub const BEGIN_PUBLICATION: &str = "UPDATE file_operation_groups SET state = 'publishing', version = version + 1, recovery_order = (SELECT COALESCE(MAX(recovery_order), 0) + 1 FROM file_operation_groups), updated_at = datetime('now') WHERE id = ? AND version = ? AND state = 'prepared' AND (claim_token IS NULL OR EXISTS (SELECT 1 FROM media_metadata_jobs WHERE claim_token = file_operation_groups.claim_token AND status = 'processing') OR EXISTS (SELECT 1 FROM llm_result_receipts WHERE claim_token = file_operation_groups.claim_token AND state = 'processing') OR EXISTS (SELECT 1 FROM import_content_hash_claims WHERE claim_token = file_operation_groups.claim_token))";
pub const VERIFY_PUBLICATION: &str =
    "SELECT 1 FROM file_operation_groups WHERE id = ? AND version = ? AND state = 'publishing' AND (claim_token IS NULL OR (kind = 'import_sidecar_publication' AND owner_kind = 'import') OR EXISTS (SELECT 1 FROM media_metadata_jobs WHERE claim_token = file_operation_groups.claim_token AND status = 'processing') OR EXISTS (SELECT 1 FROM llm_result_receipts WHERE claim_token = file_operation_groups.claim_token AND state = 'processing') OR EXISTS (SELECT 1 FROM import_content_hash_claims WHERE claim_token = file_operation_groups.claim_token))";
pub const SELECT_PENDING_PUBLICATION_ENTRIES: &str = "SELECT sequence, action, storage_root, source_path, temporary_path, destination_path, tombstone_path, expected_size, expected_sha256, expected_version FROM file_operation_entries WHERE group_id = ? AND action IN ('publish', 'move', 'tombstone') AND state = 'prepared' ORDER BY sequence LIMIT ?";
pub const COMMIT_ENTRY: &str = "UPDATE file_operation_entries SET state = 'committed', last_error_kind = NULL, last_error = NULL WHERE group_id = ? AND sequence = ? AND action IN ('publish', 'move', 'tombstone') AND state = 'prepared'";
pub const COUNT_UNCOMMITTED_ENTRIES: &str = "SELECT COUNT(*) FROM file_operation_entries WHERE group_id = ? AND action IN ('publish', 'move', 'tombstone') AND state != 'committed'";
pub const CHECKPOINT_PUBLICATION: &str = "UPDATE file_operation_groups SET state = ?, version = version + 1, updated_at = datetime('now') WHERE id = ? AND version = ? AND state = 'publishing' AND (claim_token IS NULL OR (kind = 'import_sidecar_publication' AND owner_kind = 'import') OR EXISTS (SELECT 1 FROM media_metadata_jobs WHERE claim_token = file_operation_groups.claim_token AND status = 'processing') OR EXISTS (SELECT 1 FROM llm_result_receipts WHERE claim_token = file_operation_groups.claim_token AND state = 'processing') OR EXISTS (SELECT 1 FROM import_content_hash_claims WHERE claim_token = file_operation_groups.claim_token))";
pub const COMPLETE_PUBLICATION: &str = "UPDATE file_operation_groups SET state = CASE WHEN EXISTS (SELECT 1 FROM file_operation_entries WHERE group_id = file_operation_groups.id AND cleanup_state = 'pending' AND (action = 'cleanup' OR (action = 'publish' AND state = 'committed'))) THEN 'cleanup_pending' ELSE 'completed' END, completion_outcome = CASE WHEN cancel_requested = 1 THEN 'discarded' ELSE 'published' END, version = version + 1, updated_at = datetime('now'), terminal_at = CASE WHEN EXISTS (SELECT 1 FROM file_operation_entries WHERE group_id = file_operation_groups.id AND cleanup_state = 'pending' AND (action = 'cleanup' OR (action = 'publish' AND state = 'committed'))) THEN NULL ELSE datetime('now') END WHERE id = ? AND version = ? AND state = 'files_committed' AND product_target IS NULL AND (claim_token IS NULL OR (kind = 'import_sidecar_publication' AND owner_kind = 'import') OR EXISTS (SELECT 1 FROM media_metadata_jobs WHERE claim_token = file_operation_groups.claim_token AND status = 'processing') OR EXISTS (SELECT 1 FROM llm_result_receipts WHERE claim_token = file_operation_groups.claim_token AND state = 'processing'))";
pub const VERIFY_OPERATION_CLAIM_OWNER: &str = "SELECT 1 WHERE EXISTS (SELECT 1 FROM media_metadata_jobs WHERE claim_token = ?1 AND status = 'processing') OR EXISTS (SELECT 1 FROM llm_result_receipts WHERE claim_token = ?1 AND state = 'processing') OR EXISTS (SELECT 1 FROM import_content_hash_claims WHERE claim_token = ?1)";
pub const VERIFY_CLEANUP: &str = "SELECT 1 FROM file_operation_groups WHERE id = ? AND version = ? AND state = 'cleanup_pending'";
pub const SELECT_PENDING_CLEANUP_ENTRIES: &str = "SELECT e.sequence, 'cleanup', e.storage_root, CASE WHEN e.action = 'publish' AND g.completion_outcome = 'discarded' THEN e.destination_path WHEN e.action = 'publish' THEN e.temporary_path ELSE e.source_path END, NULL, NULL, NULL, CASE WHEN e.action = 'publish' AND g.completion_outcome = 'discarded' THEN e.expected_size WHEN e.action = 'cleanup' THEN e.expected_size ELSE NULL END, CASE WHEN e.action = 'publish' AND g.completion_outcome = 'discarded' THEN e.expected_sha256 WHEN e.action = 'cleanup' THEN e.expected_sha256 ELSE NULL END, CASE WHEN e.action = 'publish' AND g.completion_outcome = 'discarded' THEN e.expected_version WHEN e.action = 'cleanup' THEN e.expected_version ELSE NULL END FROM file_operation_entries AS e JOIN file_operation_groups AS g ON g.id = e.group_id WHERE e.group_id = ? AND e.cleanup_state = 'pending' AND (e.action = 'cleanup' OR (e.action = 'publish' AND e.state = 'committed')) ORDER BY e.sequence LIMIT ?";
pub const CLEAN_ENTRY: &str = "UPDATE file_operation_entries SET cleanup_state = 'cleaned', last_error_kind = NULL, last_error = NULL WHERE group_id = ? AND sequence = ? AND cleanup_state = 'pending' AND action IN ('cleanup', 'publish')";
pub const COUNT_UNCLEANED_ENTRIES: &str = "SELECT COUNT(*) FROM file_operation_entries WHERE group_id = ? AND cleanup_state != 'cleaned' AND action IN ('cleanup', 'publish')";
pub const CHECKPOINT_CLEANUP: &str = "UPDATE file_operation_groups SET state = ?, version = version + 1, updated_at = datetime('now'), terminal_at = CASE WHEN ? = 'cleaned' THEN datetime('now') ELSE NULL END WHERE id = ? AND version = ? AND state = 'cleanup_pending'";
pub const RECORD_PUBLICATION_FAILURE_GROUP: &str = "UPDATE file_operation_groups SET state = 'publication_failed', version = version + 1, finalization_error_kind = ?, finalization_error = ?, updated_at = datetime('now') WHERE id = ? AND version = ? AND state = 'publishing'";
pub const RECORD_PUBLICATION_FAILURE_ENTRY: &str = "UPDATE file_operation_entries SET last_error_kind = ?, last_error = ? WHERE group_id = ? AND sequence = ? AND action IN ('publish', 'move', 'tombstone') AND state = 'prepared'";
pub const RECORD_CLEANUP_FAILURE_GROUP: &str = "UPDATE file_operation_groups SET state = 'cleanup_failed', version = version + 1, finalization_error_kind = ?, finalization_error = ?, updated_at = datetime('now') WHERE id = ? AND version = ? AND state = 'cleanup_pending'";
pub const RECORD_CLEANUP_FAILURE_ENTRY: &str = "UPDATE file_operation_entries SET cleanup_state = 'failed', last_error_kind = ?, last_error = ? WHERE group_id = ? AND sequence = ? AND cleanup_state = 'pending' AND (action = 'cleanup' OR (action = 'publish' AND state = 'committed'))";
pub const RELEASE_GROUP_CLAIMS: &str = "DELETE FROM file_operation_path_claims WHERE group_id = ?";
pub const RELEASE_GROUP_RESERVATION: &str = "UPDATE data_dir_space_reservations SET state = 'released', version = version + 1, updated_at = datetime('now') WHERE journal_group_id = ? AND state = 'active'";
pub const SELECT_TERMINAL_SQLITE_RESULT_RESERVATION: &str = r#"
    SELECT s.id
      FROM llm_result_receipts AS r
      JOIN file_operation_groups AS g ON g.id = r.journal_group_id
      JOIN data_dir_space_reservations AS s ON s.id = r.sqlite_reservation_id
     WHERE r.job_id = ?
       AND r.state IN ('cleaned', 'discarded', 'failed')
       AND g.state IN ('cleaned', 'rolled_back')
       AND s.class = 'sqlite'
       AND s.owner_kind IN ('llm_result', 'llm_result_cleanup')
       AND s.owner_id = r.job_id
       AND s.state = 'active'
       AND NOT EXISTS (
               SELECT 1 FROM llm_result_staging AS staging
                WHERE staging.job_id = r.job_id
           )
"#;
pub const RELEASE_SQLITE_RESULT_RESERVATION: &str = r#"
    UPDATE data_dir_space_reservations
       SET state = 'released'
         , version = version + 1
         , updated_at = datetime('now')
     WHERE id = ?
       AND state = 'active'
"#;
pub const RELEASE_ROLLED_BACK_SQLITE_RESULT_RESERVATION: &str = r#"
    UPDATE data_dir_space_reservations
       SET state = 'released'
         , version = version + 1
         , updated_at = datetime('now')
     WHERE id = (
               SELECT r.sqlite_reservation_id
                 FROM llm_result_receipts AS r
                 JOIN file_operation_groups AS g ON g.id = r.journal_group_id
                WHERE r.journal_group_id = ?
                  AND r.state = 'discarded'
                  AND g.state = 'rolled_back'
                  AND NOT EXISTS (
                          SELECT 1 FROM llm_result_staging AS staging
                           WHERE staging.job_id = r.job_id
                      )
           )
       AND state = 'active'
"#;
pub const DELETE_REPLAYABLE_RESULT_RECEIPT_AFTER_TERMINATION: &str = r#"
    DELETE FROM llm_result_receipts
     WHERE journal_group_id = ?
       AND state = 'discarded'
       AND NOT EXISTS (
               SELECT 1 FROM llm_result_staging WHERE job_id = llm_result_receipts.job_id
           )
       AND EXISTS (
               SELECT 1 FROM data_dir_space_reservations
                WHERE id = llm_result_receipts.sqlite_reservation_id AND state = 'released'
           )
       AND EXISTS (
               SELECT 1 FROM file_operation_groups
                WHERE id = llm_result_receipts.journal_group_id
                  AND (
                          state = 'rolled_back'
                       OR (state = 'cleaned' AND completion_outcome = 'discarded')
                      )
           )
       AND EXISTS (
               SELECT 1 FROM llm_jobs
                WHERE id = llm_result_receipts.job_id AND status = 'submitted'
           )
"#;
pub const DELETE_RELEASED_RESULT_RESERVATION: &str = r#"
    DELETE FROM data_dir_space_reservations
     WHERE id = ?
       AND class = 'sqlite'
       AND owner_kind = 'llm_result'
       AND state = 'released'
       AND NOT EXISTS (
               SELECT 1 FROM llm_result_receipts
                WHERE sqlite_reservation_id = data_dir_space_reservations.id
           )
"#;
pub const SELECT_REPLAYABLE_TERMINAL_RESULT_RECEIPTS: &str = r#"
    SELECT r.journal_group_id, r.sqlite_reservation_id
      FROM llm_result_receipts AS r
      JOIN file_operation_groups AS g ON g.id = r.journal_group_id
      JOIN llm_jobs AS j ON j.id = r.job_id
      JOIN data_dir_space_reservations AS s ON s.id = r.sqlite_reservation_id
     WHERE r.state = 'discarded'
       AND (?1 IS NULL OR r.job_id = ?1)
       AND NOT EXISTS (
               SELECT 1 FROM llm_result_staging WHERE job_id = r.job_id
           )
       AND (
               g.state = 'rolled_back'
            OR (g.state = 'cleaned' AND g.completion_outcome = 'discarded')
           )
       AND j.status = 'submitted'
       AND s.state = 'released'
     ORDER BY r.job_id
     LIMIT 256
"#;
pub const DELETE_ORPHANED_RELEASED_RESULT_RESERVATIONS_PAGE: &str = r#"
    DELETE FROM data_dir_space_reservations
     WHERE id IN (
               SELECT s.id
                 FROM data_dir_space_reservations AS s
                WHERE s.class = 'sqlite'
                  AND s.owner_kind = 'llm_result'
                  AND s.state = 'released'
                  AND NOT EXISTS (
                          SELECT 1 FROM llm_result_receipts AS r
                           WHERE r.sqlite_reservation_id = s.id
                      )
                ORDER BY s.id
                LIMIT 256
           )
"#;
pub const SELECT_ORPHANED_ACTIVE_RESULT_RESERVATIONS_PAGE: &str = r#"
    SELECT s.id
      FROM data_dir_space_reservations AS s
     WHERE s.class = 'sqlite'
       AND s.owner_kind IN ('llm_result', 'llm_result_cleanup')
       AND s.state = 'active'
       AND NOT EXISTS (
               SELECT 1
                 FROM llm_result_receipts AS r
                WHERE r.sqlite_reservation_id = s.id
           )
  ORDER BY s.id
     LIMIT 256
"#;
pub const SELECT_LINKED_RELEASED_SQLITE_RESULT_RESERVATION: &str = r#"
    SELECT r.sqlite_reservation_id
      FROM llm_result_receipts AS r
      JOIN data_dir_space_reservations AS s ON s.id = r.sqlite_reservation_id
     WHERE r.journal_group_id = ?
       AND s.state = 'released'
"#;
pub const SELECT_ACTIVE_SQLITE_RESULT_RESERVATION: &str = "SELECT s.id, s.class, s.owner_kind, s.owner_id, s.journal_group_id, s.filesystem_id, s.reserved_peak_additional_bytes, s.newly_allocated_blocks, s.version FROM llm_result_receipts AS r JOIN data_dir_space_reservations AS s ON s.id = r.sqlite_reservation_id WHERE r.job_id = ? AND s.class = 'sqlite' AND s.owner_kind IN ('llm_result', 'llm_result_cleanup') AND s.owner_id = r.job_id AND s.state = 'active'";
pub const CONSUME_SQLITE_RESULT_RESERVATION: &str = "UPDATE data_dir_space_reservations SET newly_allocated_blocks = newly_allocated_blocks + ?, version = version + 1, updated_at = datetime('now') WHERE id = ? AND class = 'sqlite' AND owner_kind = 'llm_result' AND owner_id = ? AND state = 'active' AND version = ? AND newly_allocated_blocks + ? <= reserved_peak_additional_bytes";
pub const SHRINK_SQLITE_RESULT_RESERVATION_TO_CLEANUP: &str = "UPDATE data_dir_space_reservations SET owner_kind = 'llm_result_cleanup', newly_allocated_blocks = reserved_peak_additional_bytes - ?, version = version + 1, updated_at = datetime('now') WHERE id = ? AND class = 'sqlite' AND owner_kind = 'llm_result' AND owner_id = ? AND state = 'active' AND version = ? AND reserved_peak_additional_bytes - newly_allocated_blocks >= ?";
pub const SELECT_GROUP_VERSION: &str = "SELECT version FROM file_operation_groups WHERE id = ?";
// The ready-only ordered index is the durable FIFO; never sort the backlog per dequeue.
pub const SELECT_NEXT_GENERIC_RECOVERY_GROUP: &str = "SELECT id, state, version, owner_kind, kind FROM file_operation_groups INDEXED BY idx_file_operation_groups_recovery_queue WHERE product_target IS NULL AND state IN ('publishing', 'files_committed', 'cleanup_pending', 'rollback_pending') AND retry_at <= unixepoch() AND (state NOT IN ('publishing', 'files_committed') OR NOT EXISTS (SELECT 1 FROM import_content_hash_claims AS active_import WHERE active_import.claim_token = file_operation_groups.claim_token)) AND id NOT IN (SELECT value FROM json_each(?)) ORDER BY recovery_order, id LIMIT 1";
pub const SELECT_NEXT_STARTUP_CRITICAL_RECOVERY_GROUP: &str = "SELECT id, state, version, owner_kind, kind FROM file_operation_groups WHERE product_target IS NULL AND (state IN ('publishing', 'files_committed', 'rollback_pending') OR (state = 'cleanup_pending' AND completion_outcome = 'discarded' AND cancel_requested = 1)) AND id NOT IN (SELECT value FROM json_each(?)) ORDER BY recovery_order, id LIMIT 1";
pub const SELECT_NEXT_BLOCKING_RECOVERY_GROUP: &str = "SELECT id, state, version, owner_kind, kind FROM file_operation_groups INDEXED BY idx_file_operation_groups_recovery_queue WHERE product_target IS NULL AND state IN ('publishing', 'files_committed', 'cleanup_pending', 'rollback_pending') AND (state <> 'cleanup_pending' OR cancel_requested = 1) AND retry_at <= unixepoch() AND (state NOT IN ('publishing', 'files_committed') OR NOT EXISTS (SELECT 1 FROM import_content_hash_claims AS active_import WHERE active_import.claim_token = file_operation_groups.claim_token)) AND id NOT IN (SELECT value FROM json_each(?)) ORDER BY recovery_order, id LIMIT 1";
pub const SELECT_NEXT_CLEANUP_RECOVERY_GROUP: &str = "SELECT id, state, version, owner_kind, kind FROM file_operation_groups INDEXED BY idx_file_operation_groups_recovery_queue WHERE product_target IS NULL AND state IN ('publishing', 'files_committed', 'cleanup_pending', 'rollback_pending') AND state = 'cleanup_pending' AND cancel_requested = 0 AND retry_at <= unixepoch() AND (state NOT IN ('publishing', 'files_committed') OR NOT EXISTS (SELECT 1 FROM import_content_hash_claims AS active_import WHERE active_import.claim_token = file_operation_groups.claim_token)) AND id NOT IN (SELECT value FROM json_each(?)) ORDER BY recovery_order, id LIMIT 1";
pub const YIELD_RECOVERY_PROGRESS: &str = "UPDATE file_operation_groups SET version = version + 1, recovery_order = (SELECT COALESCE(MAX(recovery_order), 0) + 1 FROM file_operation_groups), updated_at = datetime('now') WHERE id = ? AND version = ? AND state IN ('cleanup_pending', 'rollback_pending')";
pub const DEFER_RECOVERY: &str = "UPDATE file_operation_groups SET version = version + 1, recovery_order = (SELECT COALESCE(MAX(recovery_order), 0) + 1 FROM file_operation_groups), retry_at = unixepoch() + 1, finalization_error_kind = ?1, finalization_error = ?2, rollback_error_kind = CASE WHEN state = 'rollback_pending' THEN ?1 ELSE rollback_error_kind END, rollback_error = CASE WHEN state = 'rollback_pending' THEN ?2 ELSE rollback_error END, updated_at = datetime('now') WHERE id = ?3 AND version = ?4 AND state IN ('publishing', 'files_committed', 'cleanup_pending', 'rollback_pending')";
pub const NEXT_RECOVERY_DELAY: &str = "SELECT MAX(0, MIN(retry_at) - unixepoch()) FROM file_operation_groups WHERE product_target IS NULL AND state IN ('publishing', 'files_committed', 'cleanup_pending', 'rollback_pending') AND (state NOT IN ('publishing', 'files_committed') OR NOT EXISTS (SELECT 1 FROM import_content_hash_claims AS active_import WHERE active_import.claim_token = file_operation_groups.claim_token))";
pub const SELECT_RETRY_RECEIPT: &str = "SELECT group_id, expected_version, request_hash, response_state, response_version, expires_at > datetime('now') FROM file_operation_retry_requests WHERE retry_request_id = ?";
pub const COUNT_LIVE_RETRY_RECEIPTS: &str = "SELECT COUNT(*) FROM file_operation_retry_requests WHERE group_id = ? AND expires_at > datetime('now')";
pub const SELECT_FAILED_GROUP_FOR_RETRY: &str =
    "SELECT state, version FROM file_operation_groups WHERE id = ?";
pub const RETRY_FAILED_GROUP: &str = "UPDATE file_operation_groups SET state = ?, version = version + 1, recovery_order = (SELECT COALESCE(MAX(recovery_order), 0) + 1 FROM file_operation_groups), finalization_error_kind = NULL, finalization_error = NULL, updated_at = datetime('now') WHERE id = ? AND version = ? AND state = ?";
pub const RESET_PUBLICATION_ENTRY_FAILURES: &str = "UPDATE file_operation_entries SET last_error_kind = NULL, last_error = NULL WHERE group_id = ? AND action IN ('publish', 'move', 'tombstone') AND state = 'prepared'";
pub const RESET_CLEANUP_ENTRY_FAILURES: &str = "UPDATE file_operation_entries SET cleanup_state = 'pending', last_error_kind = NULL, last_error = NULL WHERE group_id = ? AND cleanup_state = 'failed' AND (action = 'cleanup' OR (action = 'publish' AND state = 'committed'))";
pub const INSERT_RETRY_RECEIPT: &str = "INSERT INTO file_operation_retry_requests (retry_request_id, group_id, expected_version, request_hash, response_state, response_version, expires_at) VALUES (?, ?, ?, ?, ?, ?, datetime('now', '+604800 seconds'))";
pub const LIST_GROUPS: &str = "SELECT id, kind, owner_kind, owner_id, state, product_target, product_version, cancel_requested, completion_outcome, finalization_error_kind, finalization_error, rollback_error_kind, rollback_error, entry_count, version, created_at, updated_at, terminal_at FROM file_operation_groups WHERE state IN (SELECT value FROM json_each(?)) AND (? IS NULL OR updated_at < (SELECT updated_at FROM file_operation_groups WHERE id = ?) OR (updated_at = (SELECT updated_at FROM file_operation_groups WHERE id = ?) AND id < ?)) ORDER BY updated_at DESC, id DESC LIMIT ?";
pub const SELECT_GROUP_DETAIL: &str = "SELECT id, kind, owner_kind, owner_id, state, product_target, product_version, cancel_requested, completion_outcome, finalization_error_kind, finalization_error, rollback_error_kind, rollback_error, entry_count, version, created_at, updated_at, terminal_at, detail_level, entry_action_summary, entry_state_summary, cleanup_summary FROM file_operation_groups WHERE id = ?";
pub const SELECT_GROUP_ENTRIES: &str = "SELECT sequence, action, storage_root, source_path, temporary_path, destination_path, tombstone_path, expected_size, expected_sha256, expected_version, state, cleanup_state, last_error_kind, last_error FROM file_operation_entries WHERE group_id = ? ORDER BY sequence";
pub const SELECT_GROUP_CLAIMS: &str = "SELECT sequence, storage_root, relative_path, mode, scope, role, expected_version FROM file_operation_path_claims WHERE group_id = ? ORDER BY sequence";
pub const SELECT_EXPIRED_RETRY_RECEIPTS: &str = "SELECT retry_request_id FROM file_operation_retry_requests WHERE expires_at <= datetime('now') ORDER BY expires_at, retry_request_id LIMIT 256";
pub const DELETE_RETRY_RECEIPT: &str = "DELETE FROM file_operation_retry_requests WHERE retry_request_id = ? AND expires_at <= datetime('now')";
pub const SELECT_EXPIRED_LLM_RESULT_RECEIPTS: &str = "SELECT r.job_id FROM llm_result_receipts AS r JOIN file_operation_groups AS g ON g.id = r.journal_group_id WHERE r.state IN ('cleaned', 'discarded', 'failed') AND r.updated_at <= datetime('now', '-604800 seconds') AND g.state IN ('cleaned', 'rolled_back') ORDER BY r.updated_at, r.job_id LIMIT 64";
pub const DELETE_EXPIRED_LLM_RESULT_RECEIPT: &str = "DELETE FROM llm_result_receipts WHERE job_id = ? AND state IN ('cleaned', 'discarded', 'failed') AND updated_at <= datetime('now', '-604800 seconds') AND journal_group_id IN (SELECT id FROM file_operation_groups WHERE state IN ('cleaned', 'rolled_back'))";
pub const SELECT_COMPACTION_PAGE: &str = "SELECT id, state, version FROM file_operation_groups WHERE detail_level = 'full' AND state IN ('cleaned', 'rolled_back') AND terminal_at IS NOT NULL ORDER BY terminal_at, id LIMIT 32";
pub const COUNT_ENTRY_ACTIONS: &str = "SELECT action, COUNT(*) FROM file_operation_entries WHERE group_id = ? GROUP BY action ORDER BY action";
pub const COUNT_ENTRY_STATES: &str = "SELECT state, COUNT(*) FROM file_operation_entries WHERE group_id = ? GROUP BY state ORDER BY state";
pub const COUNT_CLEANUP_STATES: &str = "SELECT cleanup_state, COUNT(*) FROM file_operation_entries WHERE group_id = ? GROUP BY cleanup_state ORDER BY cleanup_state";
pub const DELETE_GROUP_ENTRIES: &str = "DELETE FROM file_operation_entries WHERE group_id = ?";
pub const DELETE_DIRECTORY_COPY: &str =
    "DELETE FROM directory_copy_constructions WHERE group_id = ?";
pub const DELETE_GROUP_CLAIMS: &str = "DELETE FROM file_operation_path_claims WHERE group_id = ?";
pub const COMPACT_GROUP: &str = "UPDATE file_operation_groups SET detail_level = 'compacted', entry_action_summary = ?, entry_state_summary = ?, cleanup_summary = ?, version = version + 1, updated_at = datetime('now') WHERE id = ? AND state = ? AND version = ? AND detail_level = 'full'";
pub const SELECT_PRUNE_PAGE: &str = "SELECT id FROM file_operation_groups WHERE detail_level = 'compacted' AND state IN ('cleaned', 'rolled_back') AND terminal_at <= datetime('now', '-604800 seconds') ORDER BY terminal_at, id LIMIT 32";
pub const PRUNE_GROUP: &str = "DELETE FROM file_operation_groups WHERE id = ? AND detail_level = 'compacted' AND state IN ('cleaned', 'rolled_back') AND terminal_at <= datetime('now', '-604800 seconds')";
pub const SELECT_GROUP_FOR_CANCELLATION: &str =
    "SELECT state, version, cancel_requested FROM file_operation_groups WHERE id = ?";
pub const REQUEST_PRECOMMIT_ROLLBACK: &str = "UPDATE file_operation_groups SET cancel_requested = 1, state = 'rollback_pending', version = version + 1, recovery_order = (SELECT COALESCE(MAX(recovery_order), 0) + 1 FROM file_operation_groups), updated_at = datetime('now') WHERE id = ? AND version = ? AND state = 'prepared'";
pub const ACTIVATE_METADATA_CLEANUP: &str = r#"
UPDATE file_operation_groups
   SET state = 'cleanup_pending'
     , completion_outcome = 'published'
     , version = version + 1
     , recovery_order = (
           SELECT COALESCE(MAX(recovery_order), 0) + 1
             FROM file_operation_groups
       )
     , updated_at = datetime('now')
     , terminal_at = NULL
 WHERE id = ?
   AND kind = 'metadata_clean'
   AND owner_kind = 'metadata'
   AND owner_id = 'all'
   AND state = 'prepared'
"#;
pub const REQUEST_FORWARD_DISCARD: &str = "UPDATE file_operation_groups SET cancel_requested = 1, version = version + 1, recovery_order = (SELECT COALESCE(MAX(recovery_order), 0) + 1 FROM file_operation_groups), updated_at = datetime('now') WHERE id = ? AND version = ? AND state IN ('publishing', 'publication_failed', 'files_committed', 'finalize_failed')";
pub const DETACH_CANCELLED_DISCARDABLE_PRODUCT: &str = "UPDATE file_operation_groups SET product_target = NULL WHERE id = ? AND owner_kind IN ('llm_result', 'metadata_generation', 'import') AND cancel_requested = 1";
pub const MARK_NON_PUBLISH_ENTRIES_ROLLED_BACK: &str = "UPDATE file_operation_entries SET state = 'rolled_back' WHERE group_id = ? AND action != 'publish' AND state = 'prepared'";
pub const COUNT_PENDING_ROLLBACK_ENTRIES: &str = "SELECT COUNT(*) FROM file_operation_entries WHERE group_id = ? AND action = 'publish' AND state = 'prepared'";
pub const COMPLETE_EMPTY_ROLLBACK: &str = "UPDATE file_operation_groups SET state = 'rolled_back', terminal_at = datetime('now'), updated_at = datetime('now') WHERE id = ? AND state = 'rollback_pending'";
pub const VERIFY_ROLLBACK: &str = "SELECT 1 FROM file_operation_groups WHERE id = ? AND version = ? AND state = 'rollback_pending'";
pub const SELECT_PENDING_ROLLBACK_ENTRIES: &str = r#"
    WITH rollback_entries AS (
        SELECT e.*, g.kind,
               (g.kind = 'import_media_publication' AND g.owner_kind = 'import'
                AND e.storage_root = 'originals'
                AND (e.temporary_path GLOB '.importing/.importing-*'
                     OR e.temporary_path GLOB '.importing/sidecar-*')
                AND EXISTS (
                    SELECT 1 FROM file_operation_path_claims AS claim
                     WHERE claim.group_id = e.group_id
                       AND claim.storage_root = e.storage_root
                       AND claim.relative_path = e.temporary_path
                       AND claim.mode = 'write' AND claim.scope = 'exact'
                       AND claim.role IN ('temporary_original', 'temporary_sidecar')
                )) AS incomplete_import
          FROM file_operation_entries AS e
          JOIN file_operation_groups AS g ON g.id = e.group_id
         WHERE e.group_id = ? AND e.action = 'publish' AND e.state = 'prepared'
    )
    SELECT e.sequence
         , e.action
         , e.storage_root
         , e.source_path
         , e.temporary_path
         , e.destination_path
         , e.tombstone_path
         , CASE WHEN e.kind = 'llm_result_receive' OR e.incomplete_import THEN NULL ELSE e.expected_size END
         , CASE WHEN e.kind = 'llm_result_receive' OR e.incomplete_import THEN NULL ELSE e.expected_sha256 END
         , CASE WHEN e.kind = 'llm_result_receive' THEN NULL ELSE e.expected_version END
      FROM rollback_entries AS e
  ORDER BY e.sequence DESC LIMIT ?
"#;
pub const ROLLBACK_ENTRY: &str = "UPDATE file_operation_entries SET state = 'rolled_back', last_error_kind = NULL, last_error = NULL WHERE group_id = ? AND sequence = ? AND action = 'publish' AND state = 'prepared'";
pub const CHECKPOINT_ROLLBACK: &str = "UPDATE file_operation_groups SET state = ?, version = version + 1, updated_at = datetime('now'), terminal_at = CASE WHEN ? = 'rolled_back' THEN datetime('now') ELSE NULL END WHERE id = ? AND version = ? AND state = 'rollback_pending'";
pub const RECORD_FINALIZE_FAILURE: &str = "UPDATE file_operation_groups SET state = 'finalize_failed', version = version + 1, finalization_error_kind = ?, finalization_error = ?, updated_at = datetime('now') WHERE id = ? AND version = ? AND state = 'files_committed'";
pub const RECORD_ROLLBACK_FAILURE_ENTRY: &str = "UPDATE file_operation_entries SET last_error_kind = ?, last_error = ? WHERE group_id = ? AND sequence = ? AND action = 'publish' AND state = 'prepared'";
pub const RECORD_ROLLBACK_FAILURE_GROUP: &str = "UPDATE file_operation_groups SET version = version + 1, recovery_order = (SELECT COALESCE(MAX(recovery_order), 0) + 1 FROM file_operation_groups), rollback_error_kind = ?, rollback_error = ?, updated_at = datetime('now') WHERE id = ? AND version = ? AND state = 'rollback_pending'";
