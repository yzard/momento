macro_rules! media_response_columns {
    () => {
        r#"m.id
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
         , mm.keywords"#
    };
}

pub mod file_operations {
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
        "SELECT 1 FROM file_operation_groups WHERE id = ? AND version = ? AND state = 'publishing' AND (claim_token IS NULL OR EXISTS (SELECT 1 FROM media_metadata_jobs WHERE claim_token = file_operation_groups.claim_token AND status = 'processing') OR EXISTS (SELECT 1 FROM llm_result_receipts WHERE claim_token = file_operation_groups.claim_token AND state = 'processing') OR EXISTS (SELECT 1 FROM import_content_hash_claims WHERE claim_token = file_operation_groups.claim_token))";
    pub const SELECT_PENDING_PUBLICATION_ENTRIES: &str = "SELECT sequence, action, storage_root, source_path, temporary_path, destination_path, tombstone_path, expected_size, expected_sha256, expected_version FROM file_operation_entries WHERE group_id = ? AND action IN ('publish', 'move', 'tombstone') AND state = 'prepared' ORDER BY sequence";
    pub const COMMIT_ENTRY: &str = "UPDATE file_operation_entries SET state = 'committed', last_error_kind = NULL, last_error = NULL WHERE group_id = ? AND sequence = ? AND action IN ('publish', 'move', 'tombstone') AND state = 'prepared'";
    pub const COUNT_UNCOMMITTED_ENTRIES: &str = "SELECT COUNT(*) FROM file_operation_entries WHERE group_id = ? AND action IN ('publish', 'move', 'tombstone') AND state != 'committed'";
    pub const CHECKPOINT_PUBLICATION: &str = "UPDATE file_operation_groups SET state = ?, version = version + 1, recovery_order = (SELECT COALESCE(MAX(recovery_order), 0) + 1 FROM file_operation_groups), updated_at = datetime('now') WHERE id = ? AND version = ? AND state = 'publishing' AND (claim_token IS NULL OR EXISTS (SELECT 1 FROM media_metadata_jobs WHERE claim_token = file_operation_groups.claim_token AND status = 'processing') OR EXISTS (SELECT 1 FROM llm_result_receipts WHERE claim_token = file_operation_groups.claim_token AND state = 'processing') OR EXISTS (SELECT 1 FROM import_content_hash_claims WHERE claim_token = file_operation_groups.claim_token))";
    pub const COMPLETE_PUBLICATION: &str = "UPDATE file_operation_groups SET state = CASE WHEN EXISTS (SELECT 1 FROM file_operation_entries WHERE group_id = file_operation_groups.id AND cleanup_state = 'pending' AND (action = 'cleanup' OR (action = 'publish' AND state = 'committed'))) THEN 'cleanup_pending' ELSE 'completed' END, completion_outcome = CASE WHEN cancel_requested = 1 THEN 'discarded' ELSE 'published' END, version = version + 1, recovery_order = (SELECT COALESCE(MAX(recovery_order), 0) + 1 FROM file_operation_groups), updated_at = datetime('now'), terminal_at = CASE WHEN EXISTS (SELECT 1 FROM file_operation_entries WHERE group_id = file_operation_groups.id AND cleanup_state = 'pending' AND (action = 'cleanup' OR (action = 'publish' AND state = 'committed'))) THEN NULL ELSE datetime('now') END WHERE id = ? AND version = ? AND state = 'files_committed' AND product_target IS NULL AND (claim_token IS NULL OR EXISTS (SELECT 1 FROM media_metadata_jobs WHERE claim_token = file_operation_groups.claim_token AND status = 'processing') OR EXISTS (SELECT 1 FROM llm_result_receipts WHERE claim_token = file_operation_groups.claim_token AND state = 'processing'))";
    pub const VERIFY_OPERATION_CLAIM_OWNER: &str = "SELECT 1 WHERE EXISTS (SELECT 1 FROM media_metadata_jobs WHERE claim_token = ?1 AND status = 'processing') OR EXISTS (SELECT 1 FROM llm_result_receipts WHERE claim_token = ?1 AND state = 'processing') OR EXISTS (SELECT 1 FROM import_content_hash_claims WHERE claim_token = ?1)";
    pub const VERIFY_CLEANUP: &str = "SELECT 1 FROM file_operation_groups WHERE id = ? AND version = ? AND state = 'cleanup_pending'";
    pub const SELECT_PENDING_CLEANUP_ENTRIES: &str = "SELECT e.sequence, 'cleanup', e.storage_root, CASE WHEN e.action = 'publish' AND g.completion_outcome = 'discarded' THEN e.destination_path WHEN e.action = 'publish' THEN e.temporary_path ELSE e.source_path END, NULL, NULL, NULL, CASE WHEN e.action = 'publish' AND g.completion_outcome = 'discarded' THEN e.expected_size WHEN e.action = 'cleanup' THEN e.expected_size ELSE NULL END, CASE WHEN e.action = 'publish' AND g.completion_outcome = 'discarded' THEN e.expected_sha256 WHEN e.action = 'cleanup' THEN e.expected_sha256 ELSE NULL END, CASE WHEN e.action = 'publish' AND g.completion_outcome = 'discarded' THEN e.expected_version WHEN e.action = 'cleanup' THEN e.expected_version ELSE NULL END FROM file_operation_entries AS e JOIN file_operation_groups AS g ON g.id = e.group_id WHERE e.group_id = ? AND e.cleanup_state = 'pending' AND (e.action = 'cleanup' OR (e.action = 'publish' AND e.state = 'committed')) ORDER BY e.sequence";
    pub const CLEAN_ENTRY: &str = "UPDATE file_operation_entries SET cleanup_state = 'cleaned', last_error_kind = NULL, last_error = NULL WHERE group_id = ? AND sequence = ? AND cleanup_state = 'pending' AND (action = 'cleanup' OR (action = 'publish' AND state = 'committed'))";
    pub const COUNT_UNCLEANED_ENTRIES: &str = "SELECT COUNT(*) FROM file_operation_entries WHERE group_id = ? AND cleanup_state != 'cleaned' AND (action = 'cleanup' OR (action = 'publish' AND state = 'committed'))";
    pub const CHECKPOINT_CLEANUP: &str = "UPDATE file_operation_groups SET state = ?, version = version + 1, recovery_order = (SELECT COALESCE(MAX(recovery_order), 0) + 1 FROM file_operation_groups), updated_at = datetime('now'), terminal_at = CASE WHEN ? = 'cleaned' THEN datetime('now') ELSE NULL END WHERE id = ? AND version = ? AND state = 'cleanup_pending'";
    pub const RECORD_PUBLICATION_FAILURE_GROUP: &str = "UPDATE file_operation_groups SET state = 'publication_failed', version = version + 1, finalization_error_kind = ?, finalization_error = ?, updated_at = datetime('now') WHERE id = ? AND version = ? AND state = 'publishing'";
    pub const RECORD_PUBLICATION_FAILURE_ENTRY: &str = "UPDATE file_operation_entries SET last_error_kind = ?, last_error = ? WHERE group_id = ? AND sequence = ? AND action IN ('publish', 'move', 'tombstone') AND state = 'prepared'";
    pub const RECORD_CLEANUP_FAILURE_GROUP: &str = "UPDATE file_operation_groups SET state = 'cleanup_failed', version = version + 1, finalization_error_kind = ?, finalization_error = ?, updated_at = datetime('now') WHERE id = ? AND version = ? AND state = 'cleanup_pending'";
    pub const RECORD_CLEANUP_FAILURE_ENTRY: &str = "UPDATE file_operation_entries SET cleanup_state = 'failed', last_error_kind = ?, last_error = ? WHERE group_id = ? AND sequence = ? AND cleanup_state = 'pending' AND (action = 'cleanup' OR (action = 'publish' AND state = 'committed'))";
    pub const RELEASE_GROUP_CLAIMS: &str =
        "DELETE FROM file_operation_path_claims WHERE group_id = ?";
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
    pub const SELECT_LINKED_RELEASED_SQLITE_RESULT_RESERVATION: &str = "SELECT sqlite_reservation_id FROM llm_result_receipts WHERE journal_group_id = ? AND sqlite_reservation_id IN (SELECT id FROM data_dir_space_reservations WHERE state = 'released')";
    pub const SELECT_ACTIVE_SQLITE_RESULT_RESERVATION: &str = "SELECT s.id, s.class, s.owner_kind, s.owner_id, s.journal_group_id, s.filesystem_id, s.reserved_peak_additional_bytes, s.newly_allocated_blocks, s.version FROM llm_result_receipts AS r JOIN data_dir_space_reservations AS s ON s.id = r.sqlite_reservation_id WHERE r.job_id = ? AND s.class = 'sqlite' AND s.owner_kind IN ('llm_result', 'llm_result_cleanup') AND s.owner_id = r.job_id AND s.state = 'active'";
    pub const CONSUME_SQLITE_RESULT_RESERVATION: &str = "UPDATE data_dir_space_reservations SET newly_allocated_blocks = newly_allocated_blocks + ?, version = version + 1, updated_at = datetime('now') WHERE id = ? AND class = 'sqlite' AND owner_kind = 'llm_result' AND owner_id = ? AND state = 'active' AND version = ? AND newly_allocated_blocks + ? <= reserved_peak_additional_bytes";
    pub const SHRINK_SQLITE_RESULT_RESERVATION_TO_CLEANUP: &str = "UPDATE data_dir_space_reservations SET owner_kind = 'llm_result_cleanup', newly_allocated_blocks = reserved_peak_additional_bytes - ?, version = version + 1, updated_at = datetime('now') WHERE id = ? AND class = 'sqlite' AND owner_kind = 'llm_result' AND owner_id = ? AND state = 'active' AND version = ? AND reserved_peak_additional_bytes - newly_allocated_blocks >= ?";
    pub const SELECT_GROUP_VERSION: &str = "SELECT version FROM file_operation_groups WHERE id = ?";
    pub const SELECT_NEXT_GENERIC_RECOVERY_GROUP: &str = "SELECT id, state, version FROM file_operation_groups WHERE product_target IS NULL AND state IN ('publishing', 'files_committed', 'cleanup_pending', 'rollback_pending') ORDER BY recovery_order, id LIMIT 1";
    pub const SELECT_NEXT_STARTUP_CRITICAL_RECOVERY_GROUP: &str = "SELECT id, state, version FROM file_operation_groups WHERE product_target IS NULL AND state IN ('publishing', 'files_committed', 'rollback_pending') ORDER BY recovery_order, id LIMIT 1";
    pub const YIELD_RECOVERY_PROGRESS: &str = "UPDATE file_operation_groups SET version = version + 1, recovery_order = (SELECT COALESCE(MAX(recovery_order), 0) + 1 FROM file_operation_groups), updated_at = datetime('now') WHERE id = ? AND version = ? AND state IN ('cleanup_pending', 'rollback_pending')";
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
    pub const SELECT_COMPACTION_CANDIDATE: &str = "SELECT id, state, version FROM file_operation_groups WHERE detail_level = 'full' AND state IN ('cleaned', 'rolled_back') AND terminal_at IS NOT NULL ORDER BY terminal_at, id LIMIT 1";
    pub const COUNT_ENTRY_ACTIONS: &str = "SELECT action, COUNT(*) FROM file_operation_entries WHERE group_id = ? GROUP BY action ORDER BY action";
    pub const COUNT_ENTRY_STATES: &str = "SELECT state, COUNT(*) FROM file_operation_entries WHERE group_id = ? GROUP BY state ORDER BY state";
    pub const COUNT_CLEANUP_STATES: &str = "SELECT cleanup_state, COUNT(*) FROM file_operation_entries WHERE group_id = ? GROUP BY cleanup_state ORDER BY cleanup_state";
    pub const DELETE_GROUP_ENTRIES: &str = "DELETE FROM file_operation_entries WHERE group_id = ?";
    pub const DELETE_DIRECTORY_COPY: &str =
        "DELETE FROM directory_copy_constructions WHERE group_id = ?";
    pub const DELETE_GROUP_CLAIMS: &str =
        "DELETE FROM file_operation_path_claims WHERE group_id = ?";
    pub const COMPACT_GROUP: &str = "UPDATE file_operation_groups SET detail_level = 'compacted', entry_action_summary = ?, entry_state_summary = ?, cleanup_summary = ?, version = version + 1, updated_at = datetime('now') WHERE id = ? AND state = ? AND version = ? AND detail_level = 'full'";
    pub const SELECT_PRUNE_CANDIDATE: &str = "SELECT id FROM file_operation_groups WHERE detail_level = 'compacted' AND state IN ('cleaned', 'rolled_back') AND terminal_at <= datetime('now', '-604800 seconds') ORDER BY terminal_at, id LIMIT 1";
    pub const PRUNE_GROUP: &str = "DELETE FROM file_operation_groups WHERE id = ? AND detail_level = 'compacted' AND state IN ('cleaned', 'rolled_back') AND terminal_at <= datetime('now', '-604800 seconds')";
    pub const SELECT_GROUP_FOR_CANCELLATION: &str =
        "SELECT state, version, cancel_requested FROM file_operation_groups WHERE id = ?";
    pub const REQUEST_PRECOMMIT_ROLLBACK: &str = "UPDATE file_operation_groups SET cancel_requested = 1, state = 'rollback_pending', version = version + 1, recovery_order = (SELECT COALESCE(MAX(recovery_order), 0) + 1 FROM file_operation_groups), updated_at = datetime('now') WHERE id = ? AND version = ? AND state = 'prepared'";
    pub const ACTIVATE_METADATA_RESET_CLEANUP: &str = r#"
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
       AND kind = 'metadata_reset'
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
        SELECT e.sequence
             , e.action
             , e.storage_root
             , e.source_path
             , e.temporary_path
             , e.destination_path
             , e.tombstone_path
             , CASE WHEN g.kind = 'llm_result_receive' THEN NULL ELSE e.expected_size END
             , CASE WHEN g.kind = 'llm_result_receive' THEN NULL ELSE e.expected_sha256 END
             , CASE WHEN g.kind = 'llm_result_receive' THEN NULL ELSE e.expected_version END
          FROM file_operation_entries AS e
          JOIN file_operation_groups AS g ON g.id = e.group_id
         WHERE e.group_id = ? AND e.action = 'publish' AND e.state = 'prepared'
      ORDER BY e.sequence DESC
    "#;
    pub const ROLLBACK_ENTRY: &str = "UPDATE file_operation_entries SET state = 'rolled_back', last_error_kind = NULL, last_error = NULL WHERE group_id = ? AND sequence = ? AND action = 'publish' AND state = 'prepared'";
    pub const CHECKPOINT_ROLLBACK: &str = "UPDATE file_operation_groups SET state = ?, version = version + 1, recovery_order = (SELECT COALESCE(MAX(recovery_order), 0) + 1 FROM file_operation_groups), updated_at = datetime('now'), terminal_at = CASE WHEN ? = 'rolled_back' THEN datetime('now') ELSE NULL END WHERE id = ? AND version = ? AND state = 'rollback_pending'";
    pub const RECORD_FINALIZE_FAILURE: &str = "UPDATE file_operation_groups SET state = 'finalize_failed', version = version + 1, finalization_error_kind = ?, finalization_error = ?, updated_at = datetime('now') WHERE id = ? AND version = ? AND state = 'files_committed'";
    pub const RECORD_ROLLBACK_FAILURE_ENTRY: &str = "UPDATE file_operation_entries SET last_error_kind = ?, last_error = ? WHERE group_id = ? AND sequence = ? AND action = 'publish' AND state = 'prepared'";
    pub const RECORD_ROLLBACK_FAILURE_GROUP: &str = "UPDATE file_operation_groups SET version = version + 1, recovery_order = (SELECT COALESCE(MAX(recovery_order), 0) + 1 FROM file_operation_groups), rollback_error_kind = ?, rollback_error = ?, updated_at = datetime('now') WHERE id = ? AND version = ? AND state = 'rollback_pending'";
}

macro_rules! timeline_media_filters {
    () => {
        r#"AND (
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
        )"#
    };
}

macro_rules! timeline_window_prefix {
    () => {
        concat!(
            "SELECT ",
            media_response_columns!(),
            r#"
         , m.created_at
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE ma.user_id = ?
       AND ma.deleted_at IS NULL
       AND mm.date_taken >= ?
       AND mm.date_taken <= ?
       "#
        )
    };
}

pub mod import {
    pub const INSERT_CONTENT_HASH_CLAIM: &str = r#"
    INSERT INTO import_content_hash_claims (content_hash, claim_token, import_source)
    VALUES (?, ?, ?)
    ON CONFLICT(content_hash) DO NOTHING
    "#;

    pub const RELEASE_CONTENT_HASH_CLAIM: &str = r#"
    DELETE FROM import_content_hash_claims
     WHERE content_hash = ?
       AND claim_token = ?
    "#;

    pub const RECOVER_CONTENT_HASH_CLAIMS: &str = "DELETE FROM import_content_hash_claims";

    pub const FINALIZE_PRODUCT: &str = r#"
    UPDATE file_operation_groups
       SET product_target = NULL
         , state = 'cleanup_pending'
         , completion_outcome = 'published'
         , version = version + 1
         , recovery_order = (
               SELECT COALESCE(MAX(recovery_order), 0) + 1
                 FROM file_operation_groups
           )
         , updated_at = datetime('now')
         , terminal_at = NULL
     WHERE id = ?
       AND version = ?
       AND state = 'files_committed'
       AND owner_kind = 'import'
       AND owner_id = CAST(? AS TEXT)
       AND product_target = 'import_media'
       AND product_version = 1
       AND claim_token = ?
       AND EXISTS (
               SELECT 1
                 FROM import_content_hash_claims
                WHERE import_content_hash_claims.claim_token = file_operation_groups.claim_token
           )
    "#;

    pub const SELECT_INTERRUPTED_PAGE: &str = r#"
    SELECT id
      FROM media
     WHERE import_state = 'importing'
       AND id > ?
     ORDER BY id
     LIMIT ?
    "#;

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
    pub const RECORD_INTERRUPTED_JOB_ERRORS: &str = r#"
    INSERT INTO import_job_errors (import_job_id, error)
    SELECT id, 'import interrupted by service restart'
      FROM import_jobs
     WHERE status = 'running'
    "#;
    pub const INSERT_JOB: &str = "INSERT INTO import_jobs (source, status) VALUES (?, 'running')";
    pub const SELECT_LATEST_JOB_FOR_SOURCE: &str = "SELECT id, status, total_files, processed_files, successful_imports, failed_imports, started_at, completed_at, last_error FROM import_jobs WHERE source = ? ORDER BY id DESC LIMIT 1";
    pub const SELECT_JOB_ERRORS: &str =
        "SELECT error FROM import_job_errors WHERE import_job_id = ? ORDER BY id DESC LIMIT 100";
    pub const INSERT_JOB_ERROR: &str =
        "INSERT INTO import_job_errors (import_job_id, error) VALUES (?, ?)";
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
    pub const SELECT_MANIFEST_FOR_ASSET: &str =
        "SELECT content_hash, metadata_json FROM backup_asset_manifests WHERE asset_id = ?";
    pub const SELECT_STAGED_PATH_FOR_ASSET: &str =
        "SELECT staged_path FROM backup_assets WHERE id = ?";
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
    pub const EXPIRE_SESSIONS: &str = "UPDATE backup_upload_sessions SET status = 'expired', updated_at = datetime('now') WHERE status IN ('uploading', 'writing') AND expires_at <= datetime('now')";
    pub const EXPIRE_ASSETS: &str = "UPDATE backup_assets SET status = 'expired', updated_at = datetime('now') WHERE id IN (SELECT asset_id FROM backup_upload_sessions WHERE status = 'expired') AND status = 'uploading'";
    pub const SELECT_NEXT_EXPIRATION_SECONDS: &str = "SELECT CAST((julianday(MIN(expires_at)) - julianday('now')) * 86400 AS INTEGER) + 1 FROM backup_upload_sessions WHERE status IN ('uploading', 'writing')";
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
    pub const SELECT_IMPORT_PAGE: &str = r#"
    SELECT wr.user_id, u.username, wr.file_path
      FROM webdav_ready_files AS wr
      JOIN users AS u ON u.id = wr.user_id
     WHERE wr.user_id > ?
        OR (wr.user_id = ? AND wr.file_path > ?)
     ORDER BY wr.user_id, wr.file_path
     LIMIT ?
    "#;
}

pub mod metadata_jobs {
    pub const COUNT_IMPORTED_MEDIA: &str =
        "SELECT COUNT(*) FROM media WHERE import_state = 'imported'";
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
      , claim_token = CASE
            WHEN media_metadata_jobs.status = 'processing' THEN media_metadata_jobs.claim_token
            ELSE NULL
        END
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
    pub const SELECT_RESET_STATE: &str = r#"
    SELECT cleanup_group_id, phase, media_cursor, media_count
      FROM metadata_reset_operations
     WHERE id = 1
    "#;
    pub const IS_RESET_ACTIVE: &str =
        "SELECT EXISTS (SELECT 1 FROM metadata_reset_operations WHERE id = 1)";
    pub const INSERT_RESET_STATE: &str = r#"
    INSERT INTO metadata_reset_operations (
        id, cleanup_group_id, phase, media_cursor, media_count
    )
    VALUES (
        1, ?, 'metadata_jobs', 0,
        (SELECT COUNT(*) FROM media WHERE import_state = 'imported')
    )
    "#;
    pub const CANCEL_LLM_JOBS_FOR_RESET: &str = r#"
    UPDATE llm_jobs
       SET status = 'cancelled'
         , state_version = state_version + 1
         , attempts = attempts + CASE WHEN status = 'submitting' THEN 1 ELSE 0 END
         , completed_at = datetime('now')
         , updated_at = datetime('now')
     WHERE status IN ('queued', 'submitting', 'submitted')
    "#;
    pub const DISCARD_LLM_RESULT_RECEIPTS_FOR_RESET: &str = r#"
    UPDATE llm_result_receipts
       SET state = 'discarded'
         , cancel_requested = 1
         , claim_token = NULL
         , last_error = 'metadata reset superseded this result'
         , updated_at = datetime('now')
     WHERE state IN ('received', 'processing')
    "#;
    pub const SELECT_LLM_RESULT_GROUPS_PAGE: &str = r#"
    SELECT DISTINCT g.id
      FROM file_operation_groups AS g
      JOIN llm_result_receipts AS r ON r.journal_group_id = g.id
     WHERE g.state != 'cleaned'
     ORDER BY g.id
     LIMIT ?
    "#;
    pub const RETIRE_LLM_RESULT_GROUP_ENTRIES: &str = r#"
    UPDATE file_operation_entries
       SET state = CASE WHEN action = 'publish' THEN 'committed' ELSE state END
         , cleanup_state = 'cleaned'
         , last_error_kind = NULL
         , last_error = NULL
     WHERE group_id = ?
    "#;
    pub const RETIRE_LLM_RESULT_GROUP: &str = r#"
    UPDATE file_operation_groups
       SET state = 'cleaned'
         , product_target = NULL
         , completion_outcome = 'discarded'
         , cancel_requested = 1
         , version = version + 1
         , updated_at = datetime('now')
         , terminal_at = datetime('now')
     WHERE id = ? AND state != 'cleaned'
    "#;
    pub const RELEASE_LLM_RESULT_GROUP_CLAIMS: &str =
        "DELETE FROM file_operation_path_claims WHERE group_id = ?";
    pub const RELEASE_LLM_RESULT_GROUP_RESERVATIONS: &str = "UPDATE data_dir_space_reservations SET state = 'released', version = version + 1, updated_at = datetime('now') WHERE journal_group_id = ? AND state IN ('active', 'releasing')";
    pub const SELECT_IMPORTED_PAGE: &str = r#"
    SELECT id
      FROM media
     WHERE import_state = 'imported'
       AND id > ?
     ORDER BY id
     LIMIT ?
    "#;
    pub const RESET_JOB_FOR_MEDIA: &str = r#"
    INSERT INTO media_metadata_jobs (
        media_id, status, claim_token, attempts, rerun_requested, available_at,
        claimed_at, completed_at, last_error, updated_at
    )
    VALUES (?, 'queued', NULL, 0, 0, datetime('now'), NULL, NULL, NULL, datetime('now'))
    ON CONFLICT(media_id) DO UPDATE SET
        status = 'queued'
      , claim_token = NULL
      , attempts = 0
      , rerun_requested = 0
      , available_at = datetime('now')
      , claimed_at = NULL
      , completed_at = NULL
      , last_error = NULL
      , updated_at = datetime('now')
    "#;
    pub const MARK_MEDIA_DIRTY: &str = r#"
    INSERT INTO media_similarity_dirty (media_id, marked_at)
    VALUES (?, datetime('now'))
    ON CONFLICT(media_id) DO UPDATE SET marked_at = excluded.marked_at
    "#;
    pub const UPDATE_RESET_CURSOR: &str = r#"
    UPDATE metadata_reset_operations
       SET media_cursor = ?, updated_at = datetime('now')
     WHERE id = 1 AND phase = ?
    "#;
    pub const ADVANCE_RESET_PHASE: &str = r#"
    UPDATE metadata_reset_operations
       SET phase = ?, media_cursor = 0, updated_at = datetime('now')
     WHERE id = 1 AND phase = ?
    "#;
    pub const DELETE_RESET_STATE: &str =
        "DELETE FROM metadata_reset_operations WHERE id = 1 AND phase = 'activate_cleanup'";
    pub const DELETE_LLM_RESULT_STAGING_PAGE: &str = "DELETE FROM llm_result_staging WHERE rowid IN (SELECT rowid FROM llm_result_staging ORDER BY rowid LIMIT ?)";
    pub const DELETE_LLM_RESULT_RECEIPTS_PAGE: &str = "DELETE FROM llm_result_receipts WHERE rowid IN (SELECT rowid FROM llm_result_receipts ORDER BY rowid LIMIT ?)";
    pub const RELEASE_LLM_RESERVATIONS_PAGE: &str = "UPDATE data_dir_space_reservations SET state = 'released', version = version + 1, updated_at = datetime('now') WHERE id IN (SELECT id FROM data_dir_space_reservations WHERE state = 'active' AND owner_kind IN ('llm_result', 'llm_result_cleanup') ORDER BY id LIMIT ?)";
    pub const DELETE_LLM_JOB_CANCELLATIONS_PAGE: &str = "DELETE FROM llm_job_cancellations WHERE rowid IN (SELECT rowid FROM llm_job_cancellations ORDER BY rowid LIMIT ?)";
    pub const DELETE_LLM_CANCELLATION_SCOPES_PAGE: &str = "DELETE FROM llm_cancellation_scopes WHERE rowid IN (SELECT rowid FROM llm_cancellation_scopes ORDER BY rowid LIMIT ?)";
    pub const DELETE_LLM_JOBS_PAGE: &str =
        "DELETE FROM llm_jobs WHERE rowid IN (SELECT rowid FROM llm_jobs ORDER BY rowid LIMIT ?)";
    pub const DELETE_TEXT_INPUTS_PAGE: &str = "DELETE FROM media_text_inputs WHERE rowid IN (SELECT rowid FROM media_text_inputs ORDER BY rowid LIMIT ?)";
    pub const DELETE_TEXT_PAGE: &str = "DELETE FROM media_text WHERE rowid IN (SELECT rowid FROM media_text ORDER BY rowid LIMIT ?)";
    pub const DELETE_AESTHETIC_INPUTS_PAGE: &str = "DELETE FROM media_aesthetic_inputs WHERE rowid IN (SELECT rowid FROM media_aesthetic_inputs ORDER BY rowid LIMIT ?)";
    pub const DELETE_AESTHETICS_PAGE: &str = "DELETE FROM media_aesthetics WHERE rowid IN (SELECT rowid FROM media_aesthetics ORDER BY rowid LIMIT ?)";
    pub const DELETE_SCREENSHOT_INPUTS_PAGE: &str = "DELETE FROM media_screenshot_classification_inputs WHERE rowid IN (SELECT rowid FROM media_screenshot_classification_inputs ORDER BY rowid LIMIT ?)";
    pub const DELETE_SCREENSHOTS_PAGE: &str = "DELETE FROM media_screenshot_classifications WHERE rowid IN (SELECT rowid FROM media_screenshot_classifications ORDER BY rowid LIMIT ?)";
    pub const DELETE_DOCUMENT_INPUTS_PAGE: &str = "DELETE FROM media_document_classification_inputs WHERE rowid IN (SELECT rowid FROM media_document_classification_inputs ORDER BY rowid LIMIT ?)";
    pub const DELETE_DOCUMENTS_PAGE: &str = "DELETE FROM media_document_classifications WHERE rowid IN (SELECT rowid FROM media_document_classifications ORDER BY rowid LIMIT ?)";
    pub const DELETE_FACE_FINALIZATION_FACES_PAGE: &str = "DELETE FROM face_group_finalization_faces WHERE rowid IN (SELECT rowid FROM face_group_finalization_faces ORDER BY rowid LIMIT ?)";
    pub const DELETE_FACE_FINALIZATION_ANCHORS_PAGE: &str = "DELETE FROM face_group_finalization_manual_anchors WHERE rowid IN (SELECT rowid FROM face_group_finalization_manual_anchors ORDER BY rowid LIMIT ?)";
    pub const DELETE_FACE_FINALIZATION_GROUPS_PAGE: &str = "DELETE FROM face_group_finalization_groups WHERE rowid IN (SELECT rowid FROM face_group_finalization_groups ORDER BY rowid LIMIT ?)";
    pub const DELETE_FACE_REPRESENTATIVES_PAGE: &str = "DELETE FROM face_group_representatives WHERE rowid IN (SELECT rowid FROM face_group_representatives ORDER BY rowid LIMIT ?)";
    pub const DELETE_FACE_MEMBERS_PAGE: &str = "DELETE FROM face_group_members WHERE rowid IN (SELECT rowid FROM face_group_members ORDER BY rowid LIMIT ?)";
    pub const DELETE_FACE_GROUPS_PAGE: &str = "DELETE FROM face_groups WHERE rowid IN (SELECT rowid FROM face_groups ORDER BY rowid LIMIT ?)";
    pub const DELETE_FACE_FINALIZATIONS_PAGE: &str = "DELETE FROM face_group_finalizations WHERE rowid IN (SELECT rowid FROM face_group_finalizations ORDER BY rowid LIMIT ?)";
    pub const DELETE_FACE_GENERATION_STATE_PAGE: &str = "DELETE FROM face_group_generation_state WHERE rowid IN (SELECT rowid FROM face_group_generation_state ORDER BY rowid LIMIT ?)";
    pub const DELETE_FACE_MANUAL_STATE_PAGE: &str = "DELETE FROM face_group_manual_state WHERE rowid IN (SELECT rowid FROM face_group_manual_state ORDER BY rowid LIMIT ?)";
    pub const DELETE_FACE_GENERATIONS_PAGE: &str = "DELETE FROM face_group_generations WHERE rowid IN (SELECT rowid FROM face_group_generations ORDER BY rowid LIMIT ?)";
    pub const DELETE_FACE_RUNS_PAGE: &str = "DELETE FROM face_grouping_runs WHERE rowid IN (SELECT rowid FROM face_grouping_runs ORDER BY rowid LIMIT ?)";
    pub const DELETE_MEDIA_FACES_PAGE: &str = "DELETE FROM media_faces WHERE rowid IN (SELECT rowid FROM media_faces ORDER BY rowid LIMIT ?)";
    pub const DELETE_FACE_RESULTS_PAGE: &str = "DELETE FROM media_face_detection_results WHERE rowid IN (SELECT rowid FROM media_face_detection_results ORDER BY rowid LIMIT ?)";
    pub const DELETE_SIMILARITY_CLUSTER_MEMBERS_PAGE: &str = "DELETE FROM media_similarity_cluster_members WHERE rowid IN (SELECT rowid FROM media_similarity_cluster_members ORDER BY rowid LIMIT ?)";
    pub const DELETE_SIMILARITY_CLUSTERS_PAGE: &str = "DELETE FROM media_similarity_clusters WHERE rowid IN (SELECT rowid FROM media_similarity_clusters ORDER BY rowid LIMIT ?)";
    pub const DELETE_SIMILARITY_DIRTY_SNAPSHOT_PAGE: &str = "DELETE FROM media_similarity_finalization_dirty WHERE rowid IN (SELECT rowid FROM media_similarity_finalization_dirty ORDER BY rowid LIMIT ?)";
    pub const DELETE_SIMILARITY_EDGES_PAGE: &str = "DELETE FROM media_similarity_edges WHERE rowid IN (SELECT rowid FROM media_similarity_edges ORDER BY rowid LIMIT ?)";
    pub const DELETE_SIMILARITY_LABELS_PAGE: &str = "DELETE FROM media_similarity_labels WHERE rowid IN (SELECT rowid FROM media_similarity_labels ORDER BY rowid LIMIT ?)";
    pub const DELETE_SIMILARITY_FINALIZATIONS_PAGE: &str = "DELETE FROM media_similarity_finalizations WHERE rowid IN (SELECT rowid FROM media_similarity_finalizations ORDER BY rowid LIMIT ?)";
    pub const DELETE_SIMILARITY_GENERATION_STATE_PAGE: &str = "DELETE FROM media_similarity_generation_state WHERE rowid IN (SELECT rowid FROM media_similarity_generation_state ORDER BY rowid LIMIT ?)";
    pub const DELETE_SIMILARITY_GENERATIONS_PAGE: &str = "DELETE FROM media_similarity_generations WHERE rowid IN (SELECT rowid FROM media_similarity_generations ORDER BY rowid LIMIT ?)";
    pub const DELETE_SIMILARITY_BANDS_PAGE: &str = "DELETE FROM media_similarity_hash_bands WHERE rowid IN (SELECT rowid FROM media_similarity_hash_bands ORDER BY rowid LIMIT ?)";
    pub const DELETE_SIMILARITY_INDEX_PAGE: &str = "DELETE FROM media_similarity_index WHERE rowid IN (SELECT rowid FROM media_similarity_index ORDER BY rowid LIMIT ?)";
    pub const DELETE_SIMILARITY_DIRTY_PAGE: &str = "DELETE FROM media_similarity_dirty WHERE rowid IN (SELECT rowid FROM media_similarity_dirty ORDER BY rowid LIMIT ?)";
    pub const DELETE_SIMILARITY_RUNS_PAGE: &str = "DELETE FROM media_similarity_runs WHERE rowid IN (SELECT rowid FROM media_similarity_runs ORDER BY rowid LIMIT ?)";
    pub const DELETE_AI_INPUTS_PAGE: &str = "DELETE FROM media_ai_inputs WHERE rowid IN (SELECT rowid FROM media_ai_inputs ORDER BY rowid LIMIT ?)";
    pub const DELETE_RTREE_PAGE: &str = "DELETE FROM media_rtree WHERE rowid IN (SELECT rowid FROM media_rtree ORDER BY rowid LIMIT ?)";
    pub const DELETE_METADATA_SOURCES_PAGE: &str = "DELETE FROM media_metadata_sources WHERE rowid IN (SELECT rowid FROM media_metadata_sources ORDER BY rowid LIMIT ?)";
    pub const DELETE_METADATA_PAGE: &str = "DELETE FROM media_metadata WHERE rowid IN (SELECT rowid FROM media_metadata ORDER BY rowid LIMIT ?)";
    pub const SELECT_INPUT_PATHS: &str =
        "SELECT storage_root, file_path FROM media_ai_inputs WHERE media_id = ? AND task = ? ORDER BY sequence";
    pub const CLAIM_NEXT_QUEUED: &str = r#"
    UPDATE media_metadata_jobs
       SET status = 'processing'
         , claim_token = ?
         , claimed_at = datetime('now')
         , attempts = attempts + 1
         , updated_at = datetime('now')
     WHERE media_id = (
               SELECT media_id
                 FROM media_metadata_jobs
                WHERE status = 'queued'
                  AND available_at <= datetime('now')
                  AND NOT EXISTS (SELECT 1 FROM metadata_reset_operations)
                ORDER BY media_id
                LIMIT 1
           )
       AND status = 'queued'
    RETURNING media_id, claim_token
    "#;
    pub const NEXT_AVAILABLE_DELAY_SECONDS: &str = r#"
    SELECT CAST(
               MAX(1, unixepoch(MIN(available_at)) - unixepoch('now'))
               AS INTEGER
           )
      FROM media_metadata_jobs
     WHERE status = 'queued'
       AND available_at > datetime('now')
       AND NOT EXISTS (SELECT 1 FROM metadata_reset_operations)
    "#;
    pub const MARK_COMPLETED: &str = "UPDATE media_metadata_jobs SET status = CASE WHEN rerun_requested = 1 THEN 'queued' ELSE 'completed' END, claim_token = NULL, attempts = CASE WHEN rerun_requested = 1 THEN 0 ELSE attempts END, rerun_requested = 0, available_at = CASE WHEN rerun_requested = 1 THEN datetime('now') ELSE available_at END, claimed_at = NULL, completed_at = CASE WHEN rerun_requested = 1 THEN NULL ELSE datetime('now') END, last_error = NULL, updated_at = datetime('now') WHERE media_id = ? AND claim_token = ? AND status = 'processing'";
    pub const MARK_RETRY: &str = "UPDATE media_metadata_jobs SET status = 'queued', claim_token = NULL, attempts = CASE WHEN rerun_requested = 1 THEN 0 ELSE attempts END, rerun_requested = 0, available_at = CASE WHEN rerun_requested = 1 THEN datetime('now') ELSE datetime('now', '+30 seconds') END, claimed_at = NULL, last_error = CASE WHEN rerun_requested = 1 THEN NULL ELSE ? END, updated_at = datetime('now') WHERE media_id = ? AND claim_token = ? AND status = 'processing'";
    pub const RECOVER_ORPHANED_CLAIMS: &str = "UPDATE media_metadata_jobs SET status = 'queued', claim_token = NULL, available_at = datetime('now'), claimed_at = NULL, updated_at = datetime('now') WHERE status = 'processing' AND claim_token IS NOT NULL";
    pub const VERIFY_CLAIM: &str = "SELECT 1 FROM media_metadata_jobs WHERE media_id = ? AND claim_token = ? AND status = 'processing'";
    pub const SELECT_FAILURES: &str = "SELECT last_error FROM media_metadata_jobs WHERE status = 'failed' AND last_error IS NOT NULL ORDER BY updated_at DESC LIMIT 100";
}

pub mod ai_jobs {
    pub const INSERT_ELIGIBLE: &str = "INSERT INTO llm_jobs (id, media_id, task, status) SELECT lower(hex(randomblob(16))), media.id, ?, 'queued' FROM media JOIN media_metadata_jobs ON media_metadata_jobs.media_id = media.id WHERE media.import_state = 'imported' AND media_metadata_jobs.status = 'completed' AND NOT EXISTS (SELECT 1 FROM metadata_reset_operations) AND EXISTS (SELECT 1 FROM media_ai_inputs WHERE media_ai_inputs.media_id = media.id AND media_ai_inputs.task = ?) AND NOT EXISTS (SELECT 1 FROM media_text WHERE media_text.media_id = media.id AND media_text.model_type = ?) AND NOT EXISTS (SELECT 1 FROM llm_jobs WHERE llm_jobs.media_id = media.id AND llm_jobs.task = ? AND llm_jobs.status IN ('queued','submitting','submitted'))";
    pub const INSERT_FACE_ELIGIBLE: &str = "INSERT INTO llm_jobs (id, media_id, face_grouping_run_id, task, status) SELECT lower(hex(randomblob(16))), media.id, ?, 'face_detection', 'queued' FROM media JOIN media_metadata_jobs ON media_metadata_jobs.media_id = media.id WHERE media.import_state = 'imported' AND media_metadata_jobs.status = 'completed' AND NOT EXISTS (SELECT 1 FROM metadata_reset_operations) AND EXISTS (SELECT 1 FROM media_ai_inputs WHERE media_ai_inputs.media_id = media.id AND media_ai_inputs.task = 'face_detection') AND NOT EXISTS (SELECT 1 FROM media_face_detection_results WHERE media_face_detection_results.media_id = media.id) AND NOT EXISTS (SELECT 1 FROM llm_jobs WHERE llm_jobs.media_id = media.id AND llm_jobs.task = 'face_detection' AND llm_jobs.status IN ('queued','submitting','submitted'))";
    pub const INSERT_AESTHETICS_ELIGIBLE: &str = "INSERT INTO llm_jobs (id, media_id, task, status) SELECT lower(hex(randomblob(16))), media.id, 'image_aesthetics', 'queued' FROM media JOIN media_metadata_jobs ON media_metadata_jobs.media_id = media.id WHERE media.import_state = 'imported' AND media_metadata_jobs.status = 'completed' AND NOT EXISTS (SELECT 1 FROM metadata_reset_operations) AND EXISTS (SELECT 1 FROM media_ai_inputs WHERE media_ai_inputs.media_id = media.id AND media_ai_inputs.task = 'image_aesthetics') AND NOT EXISTS (SELECT 1 FROM media_aesthetics WHERE media_aesthetics.media_id = media.id) AND NOT EXISTS (SELECT 1 FROM llm_jobs WHERE llm_jobs.media_id = media.id AND llm_jobs.task = 'image_aesthetics' AND llm_jobs.status IN ('queued','submitting','submitted'))";
    pub const INSERT_SCREENSHOT_ELIGIBLE: &str = "INSERT INTO llm_jobs (id, media_id, task, status) SELECT lower(hex(randomblob(16))), media.id, 'screenshot_detection', 'queued' FROM media JOIN media_metadata_jobs ON media_metadata_jobs.media_id = media.id WHERE media.import_state = 'imported' AND media.media_type = 'image' AND media_metadata_jobs.status = 'completed' AND NOT EXISTS (SELECT 1 FROM metadata_reset_operations) AND EXISTS (SELECT 1 FROM media_ai_inputs WHERE media_ai_inputs.media_id = media.id AND media_ai_inputs.task = 'screenshot_detection') AND NOT EXISTS (SELECT 1 FROM media_screenshot_classifications WHERE media_screenshot_classifications.media_id = media.id) AND NOT EXISTS (SELECT 1 FROM llm_jobs WHERE llm_jobs.media_id = media.id AND llm_jobs.task = 'screenshot_detection' AND llm_jobs.status IN ('queued','submitting','submitted'))";
    pub const INSERT_DOCUMENT_ELIGIBLE: &str = "INSERT INTO llm_jobs (id, media_id, task, status) SELECT lower(hex(randomblob(16))), media.id, 'document_detection', 'queued' FROM media JOIN media_metadata_jobs ON media_metadata_jobs.media_id = media.id WHERE media.import_state = 'imported' AND media.media_type = 'image' AND media_metadata_jobs.status = 'completed' AND NOT EXISTS (SELECT 1 FROM metadata_reset_operations) AND EXISTS (SELECT 1 FROM media_ai_inputs WHERE media_ai_inputs.media_id = media.id AND media_ai_inputs.task = 'document_detection') AND NOT EXISTS (SELECT 1 FROM media_document_classifications WHERE media_document_classifications.media_id = media.id) AND NOT EXISTS (SELECT 1 FROM llm_jobs WHERE llm_jobs.media_id = media.id AND llm_jobs.task = 'document_detection' AND llm_jobs.status IN ('queued','submitting','submitted'))";
    pub const SELECT_QUEUED: &str = "SELECT id, media_id, task, attempts FROM llm_jobs WHERE status = 'queued' AND available_at <= datetime('now') AND NOT EXISTS (SELECT 1 FROM llm_cancellation_scopes WHERE llm_cancellation_scopes.scope = 'all' OR (llm_cancellation_scopes.scope = 'task' AND llm_cancellation_scopes.task = llm_jobs.task)) ORDER BY created_at LIMIT ?";
    pub const NEXT_AVAILABLE_DELAY_SECONDS: &str = r#"
    WITH future_work(ready_at) AS (
        SELECT available_at
          FROM llm_jobs
         WHERE status = 'queued'
           AND available_at > datetime('now')
           AND NOT EXISTS (SELECT 1 FROM metadata_reset_operations)
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
    pub const CLAIM: &str = "UPDATE llm_jobs SET status = 'submitting', state_version = state_version + 1, claimed_at = datetime('now'), updated_at = datetime('now') WHERE id = ? AND status = 'queued' AND NOT EXISTS (SELECT 1 FROM metadata_reset_operations)";
    pub const MARK_SUBMITTED: &str = "UPDATE llm_jobs SET status = 'submitted', state_version = state_version + 1, attempts = attempts + 1, submitted_at = datetime('now'), updated_at = datetime('now') WHERE id = ? AND status = 'submitting' AND attempts + 1 = ?";
    pub const REQUEUE_AMBIGUOUS: &str = "UPDATE llm_jobs SET status = 'queued', state_version = state_version + 1, claimed_at = NULL, available_at = datetime('now'), updated_at = datetime('now') WHERE id = ? AND status = 'submitting'";
    pub const REQUEUE_DEFERRED: &str = "UPDATE llm_jobs SET status = 'queued', state_version = state_version + 1, claimed_at = NULL, available_at = datetime('now', '+' || ? || ' seconds'), last_error = NULL, updated_at = datetime('now') WHERE id = ? AND status = 'submitting'";
    pub const SNAPSHOT_QUEUED_INPUTS: &str = "INSERT OR IGNORE INTO llm_job_inputs (job_id, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash, frame_timestamp_ms) SELECT llm_jobs.id, media_ai_inputs.sequence, media_ai_inputs.input_kind, media_ai_inputs.storage_root, media_ai_inputs.file_path, media_ai_inputs.filename, media_ai_inputs.mime_type, media_ai_inputs.byte_size, media_ai_inputs.content_hash, media_ai_inputs.frame_timestamp_ms FROM llm_jobs JOIN media_ai_inputs ON media_ai_inputs.media_id = llm_jobs.media_id AND media_ai_inputs.task = llm_jobs.task WHERE llm_jobs.status = 'queued'";
    pub const SELECT_INPUTS: &str = "SELECT sequence, storage_root, file_path, filename, mime_type, byte_size, content_hash, input_kind, frame_timestamp_ms FROM llm_job_inputs WHERE job_id = ? ORDER BY sequence";
    pub const RECLAIM_STALE: &str = "UPDATE llm_jobs SET status = 'queued', state_version = state_version + 1, claimed_at = NULL, updated_at = datetime('now') WHERE status = 'submitting' AND (claimed_at IS NULL OR claimed_at <= datetime('now', '-5 minutes'))";
    pub const RETRY_OR_FAIL: &str = "UPDATE llm_jobs SET status = CASE WHEN attempts + 1 >= 5 THEN 'failed' ELSE 'queued' END, state_version = state_version + 1, attempts = attempts + 1, available_at = datetime('now', '+30 seconds'), last_error = ?, completed_at = CASE WHEN attempts + 1 >= 5 THEN datetime('now') ELSE NULL END, updated_at = datetime('now') WHERE id = ? AND status = 'submitting'";
    pub const MARK_FAILED: &str = "UPDATE llm_jobs SET status = 'failed', state_version = state_version + 1, last_error = ?, completed_at = datetime('now'), updated_at = datetime('now') WHERE id = ? AND status = 'submitting'";
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
    pub const COUNT_GROUPS: &str = "SELECT COUNT(*) FROM face_groups WHERE manual_curated = 1 OR automatic_generation_id = (SELECT active_generation_id FROM face_group_generation_state WHERE id = 1) OR (automatic_generation_id IS NULL AND NOT EXISTS (SELECT 1 FROM face_group_generation_state WHERE id = 1))";
    pub const INSERT_GROUPING_RUN: &str =
        "INSERT INTO face_grouping_runs (status) VALUES ('running')";
    pub const SELECT_ACTIVE_RUN: &str = "SELECT id, status FROM face_grouping_runs WHERE status IN ('running', 'cancelling') ORDER BY id DESC LIMIT 1";
    pub const COUNT_PENDING_JOBS: &str = "SELECT COUNT(*) FROM llm_jobs WHERE face_grouping_run_id = ? AND task = 'face_detection' AND status IN ('queued', 'submitting', 'submitted')";
    pub const COUNT_FAILED_JOBS: &str =
        "SELECT COUNT(*) FROM llm_jobs WHERE face_grouping_run_id = ? AND task = 'face_detection' AND status = 'failed'";
    pub const MARK_RUN: &str = "UPDATE face_grouping_runs SET status = ?, completed_at = datetime('now'), error = ? WHERE id = ? AND status IN ('running', 'cancelling')";
    pub const CANCEL_ACTIVE: &str = "UPDATE llm_jobs SET status = 'cancelled', state_version = state_version + 1, completed_at = datetime('now'), updated_at = datetime('now') WHERE task = 'face_detection' AND status IN ('queued', 'submitting', 'submitted')";
    pub const SELECT_FINALIZATION_STATE: &str = r#"
    SELECT generation_id
         , phase
         , manual_revision
         , face_snapshot_cursor
         , manual_snapshot_cursor
         , face_cursor
         , current_face_id
         , candidate_kind
         , candidate_cursor
         , best_group_id
         , best_similarity
         , group_cursor
         , current_group_id
         , representative_cursor
         , best_representative_face_id
         , best_representative_score
         , completion_error
      FROM face_group_finalizations
     WHERE run_id = ?
    "#;
    pub const INSERT_GENERATION: &str =
        "INSERT INTO face_group_generations (run_id, status) VALUES (?, 'building')";
    pub const INSERT_FINALIZATION: &str = r#"
    INSERT INTO face_group_finalizations
      ( run_id
      , generation_id
      , phase
      , manual_revision
      , completion_error
    ) VALUES (?, ?, 'face_snapshot', ?, ?)
    "#;
    pub const SELECT_MANUAL_REVISION: &str =
        "SELECT COALESCE((SELECT revision FROM face_group_manual_state WHERE id = 1), 0)";
    pub const INCREMENT_MANUAL_REVISION: &str = r#"
    INSERT INTO face_group_manual_state (id, revision)
    VALUES (1, 1)
    ON CONFLICT(id) DO UPDATE SET
        revision = revision + 1,
        updated_at = datetime('now')
    "#;
    pub const SELECT_FACE_SNAPSHOT_PAGE: &str = r#"
    SELECT media_faces.id
         , media_faces.embedding
      FROM media_faces
     WHERE media_faces.id > ?
       AND NOT EXISTS (
               SELECT 1
                 FROM face_group_members
                WHERE face_group_members.face_id = media_faces.id
                  AND face_group_members.manual_anchor = 1
           )
     ORDER BY media_faces.id
     LIMIT ?
    "#;
    pub const INSERT_FINALIZATION_FACE: &str = r#"
    INSERT INTO face_group_finalization_faces
      ( generation_id
      , face_id
      , embedding
    ) VALUES (?, ?, ?)
    "#;
    pub const ADVANCE_FACE_SNAPSHOT: &str =
        "UPDATE face_group_finalizations SET face_snapshot_cursor = ?, updated_at = datetime('now') WHERE run_id = ? AND phase = 'face_snapshot'";
    pub const FINISH_FACE_SNAPSHOT: &str =
        "UPDATE face_group_finalizations SET phase = 'manual_snapshot', updated_at = datetime('now') WHERE run_id = ? AND phase = 'face_snapshot'";
    pub const SELECT_MANUAL_SNAPSHOT_PAGE: &str = r#"
    SELECT face_group_members.face_id
         , face_group_members.face_group_id
         , media_faces.embedding
      FROM face_group_members
      JOIN face_groups
        ON face_groups.id = face_group_members.face_group_id
      JOIN media_faces
        ON media_faces.id = face_group_members.face_id
     WHERE face_group_members.manual_anchor = 1
       AND face_groups.manual_curated = 1
       AND face_group_members.face_id > ?
     ORDER BY face_group_members.face_id
     LIMIT ?
    "#;
    pub const INSERT_FINALIZATION_MANUAL_ANCHOR: &str = r#"
    INSERT INTO face_group_finalization_manual_anchors
      ( generation_id
      , face_id
      , face_group_id
      , embedding
    ) VALUES (?, ?, ?, ?)
    "#;
    pub const ADVANCE_MANUAL_SNAPSHOT: &str =
        "UPDATE face_group_finalizations SET manual_snapshot_cursor = ?, updated_at = datetime('now') WHERE run_id = ? AND phase = 'manual_snapshot'";
    pub const FINISH_MANUAL_SNAPSHOT: &str =
        "UPDATE face_group_finalizations SET phase = 'grouping', updated_at = datetime('now') WHERE run_id = ? AND phase = 'manual_snapshot'";
    pub const SELECT_NEXT_FINALIZATION_FACE: &str =
        "SELECT face_id FROM face_group_finalization_faces WHERE generation_id = ? AND face_id > ? ORDER BY face_id LIMIT 1";
    pub const START_FINALIZATION_FACE: &str =
        "UPDATE face_group_finalizations SET current_face_id = ?, candidate_kind = 'manual', candidate_cursor = 0, best_group_id = NULL, best_similarity = NULL, updated_at = datetime('now') WHERE run_id = ? AND phase = 'grouping' AND current_face_id IS NULL";
    pub const SELECT_FINALIZATION_FACE: &str =
        "SELECT embedding FROM face_group_finalization_faces WHERE generation_id = ? AND face_id = ?";
    pub const SELECT_MANUAL_CANDIDATE_PAGE: &str = r#"
    SELECT face_group_id
         , face_id
         , embedding
      FROM face_group_finalization_manual_anchors
     WHERE generation_id = ?
       AND face_id > ?
     ORDER BY face_id
     LIMIT ?
    "#;
    pub const SELECT_AUTOMATIC_CANDIDATE_PAGE: &str = r#"
    SELECT face_groups.id
         , media_faces.id
         , media_faces.embedding
      FROM face_groups
      JOIN media_faces
        ON media_faces.id = face_groups.representative_face_id
     WHERE face_groups.automatic_generation_id = ?
       AND face_groups.id > ?
     ORDER BY face_groups.id
     LIMIT ?
    "#;
    pub const ADVANCE_COMPARISON_PAGE: &str = r#"
    UPDATE face_group_finalizations
       SET candidate_cursor = ?
         , best_group_id = ?
         , best_similarity = ?
         , updated_at = datetime('now')
     WHERE run_id = ?
       AND generation_id = ?
       AND phase = 'grouping'
       AND current_face_id = ?
       AND candidate_kind = ?
    "#;
    pub const SWITCH_TO_AUTOMATIC_CANDIDATES: &str = r#"
    UPDATE face_group_finalizations
       SET candidate_kind = 'automatic'
         , candidate_cursor = 0
         , best_group_id = NULL
         , best_similarity = NULL
         , updated_at = datetime('now')
     WHERE run_id = ?
       AND generation_id = ?
       AND phase = 'grouping'
       AND current_face_id = ?
       AND candidate_kind = 'manual'
    "#;
    pub const INSERT_GENERATION_GROUP: &str = r#"
    INSERT INTO face_groups
      ( representative_face_id
      , manual_curated
      , automatic_generation_id
    ) VALUES (?, 0, ?)
    "#;
    pub const INSERT_GENERATION_MEMBER: &str = r#"
    INSERT INTO face_group_members
      ( face_group_id
      , face_id
      , manual_anchor
      , automatic_generation_id
    ) VALUES (?, ?, 0, ?)
    ON CONFLICT(automatic_generation_id, face_id)
        WHERE manual_anchor = 0
    DO UPDATE SET
        face_group_id = excluded.face_group_id
    "#;
    pub const TRACK_FINALIZATION_GROUP: &str = r#"
    INSERT INTO face_group_finalization_groups
      ( generation_id
      , face_group_id
    ) VALUES (?, ?)
    ON CONFLICT(generation_id, face_group_id) DO NOTHING
    "#;
    pub const FINISH_FINALIZATION_FACE: &str = r#"
    UPDATE face_group_finalizations
       SET face_cursor = ?
         , current_face_id = NULL
         , candidate_kind = 'manual'
         , candidate_cursor = 0
         , best_group_id = NULL
         , best_similarity = NULL
         , updated_at = datetime('now')
     WHERE run_id = ?
       AND generation_id = ?
       AND phase = 'grouping'
       AND current_face_id = ?
    "#;
    pub const FINISH_FACE_GROUPING: &str =
        "UPDATE face_group_finalizations SET phase = 'representatives', updated_at = datetime('now') WHERE run_id = ? AND phase = 'grouping' AND current_face_id IS NULL";
    pub const SELECT_NEXT_FINALIZATION_GROUP: &str =
        "SELECT face_group_id FROM face_group_finalization_groups WHERE generation_id = ? AND complete = 0 AND face_group_id > ? ORDER BY face_group_id LIMIT 1";
    pub const START_REPRESENTATIVE_GROUP: &str =
        "UPDATE face_group_finalizations SET current_group_id = ?, representative_cursor = 0, best_representative_face_id = NULL, best_representative_score = NULL, updated_at = datetime('now') WHERE run_id = ? AND phase = 'representatives' AND current_group_id IS NULL";
    pub const SELECT_REPRESENTATIVE_CANDIDATE_PAGE: &str = r#"
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
      FROM media_faces
     WHERE media_faces.id > ?
       AND (
               EXISTS (
                   SELECT 1
                     FROM face_group_finalization_manual_anchors
                    WHERE face_group_finalization_manual_anchors.generation_id = ?
                      AND face_group_finalization_manual_anchors.face_group_id = ?
                      AND face_group_finalization_manual_anchors.face_id = media_faces.id
               )
            OR EXISTS (
                   SELECT 1
                     FROM face_group_members
                    WHERE face_group_members.automatic_generation_id = ?
                      AND face_group_members.face_group_id = ?
                      AND face_group_members.face_id = media_faces.id
               )
           )
     ORDER BY media_faces.id
     LIMIT ?
    "#;
    pub const ADVANCE_REPRESENTATIVE_PAGE: &str = r#"
    UPDATE face_group_finalizations
       SET representative_cursor = ?
         , best_representative_face_id = ?
         , best_representative_score = ?
         , updated_at = datetime('now')
     WHERE run_id = ?
       AND generation_id = ?
       AND phase = 'representatives'
       AND current_group_id = ?
    "#;
    pub const UPSERT_GENERATION_REPRESENTATIVE: &str = r#"
    INSERT INTO face_group_representatives
      ( generation_id
      , face_group_id
      , face_id
    ) VALUES (?, ?, ?)
    ON CONFLICT(generation_id, face_group_id) DO UPDATE SET
        face_id = excluded.face_id
    "#;
    pub const UPDATE_BUILDING_AUTOMATIC_REPRESENTATIVE: &str =
        "UPDATE face_groups SET representative_face_id = ? WHERE id = ? AND automatic_generation_id = ?";
    pub const COMPLETE_FINALIZATION_GROUP: &str =
        "UPDATE face_group_finalization_groups SET representative_face_id = ?, representative_score = ?, complete = 1 WHERE generation_id = ? AND face_group_id = ? AND complete = 0";
    pub const FINISH_REPRESENTATIVE_GROUP: &str =
        "UPDATE face_group_finalizations SET group_cursor = ?, current_group_id = NULL, representative_cursor = 0, best_representative_face_id = NULL, best_representative_score = NULL, updated_at = datetime('now') WHERE run_id = ? AND generation_id = ? AND phase = 'representatives' AND current_group_id = ?";
    pub const FINISH_REPRESENTATIVES: &str =
        "UPDATE face_group_finalizations SET phase = 'publishing', updated_at = datetime('now') WHERE run_id = ? AND phase = 'representatives' AND current_group_id IS NULL";
    pub const COUNT_INCOMPLETE_FINALIZATION_GROUPS: &str =
        "SELECT COUNT(*) FROM face_group_finalization_groups WHERE generation_id = ? AND complete = 0";
    pub const RETIRE_ACTIVE_GENERATION: &str =
        "UPDATE face_group_generations SET status = 'retired' WHERE status = 'active' AND id <> ?";
    pub const ACTIVATE_GENERATION: &str =
        "UPDATE face_group_generations SET status = 'active', published_at = datetime('now') WHERE id = ? AND status = 'building'";
    pub const SWITCH_ACTIVE_GENERATION: &str = r#"
    INSERT INTO face_group_generation_state
      ( id
      , active_generation_id
    ) VALUES (1, ?)
    ON CONFLICT(id) DO UPDATE SET
        active_generation_id = excluded.active_generation_id,
        updated_at = datetime('now')
    "#;
    pub const ENTER_FINALIZATION_CLEANUP: &str =
        "UPDATE face_group_finalizations SET phase = 'cleanup', updated_at = datetime('now') WHERE run_id = ? AND phase = 'publishing'";
    pub const ENTER_RESTART_CLEANUP: &str =
        "UPDATE face_group_finalizations SET phase = 'restart_cleanup', updated_at = datetime('now') WHERE run_id = ?";
    pub const CANCEL_BUILDING_GENERATION: &str =
        "UPDATE face_group_generations SET status = 'cancelled' WHERE run_id = ? AND status = 'building'";
    pub const SELECT_GENERATION_STATUS: &str =
        "SELECT status FROM face_group_generations WHERE id = ?";
    pub const SELECT_FINALIZATION_CLEANUP: &str =
        "SELECT run_id, generation_id, phase FROM face_group_finalizations WHERE phase IN ('cleanup', 'restart_cleanup') ORDER BY run_id LIMIT 1";
    pub const DELETE_FINALIZATION_FACE_PAGE: &str =
        "DELETE FROM face_group_finalization_faces WHERE rowid IN (SELECT rowid FROM face_group_finalization_faces WHERE generation_id = ? ORDER BY face_id LIMIT ?)";
    pub const DELETE_FINALIZATION_MANUAL_PAGE: &str =
        "DELETE FROM face_group_finalization_manual_anchors WHERE rowid IN (SELECT rowid FROM face_group_finalization_manual_anchors WHERE generation_id = ? ORDER BY face_id LIMIT ?)";
    pub const DELETE_FINALIZATION_GROUP_PAGE: &str =
        "DELETE FROM face_group_finalization_groups WHERE rowid IN (SELECT rowid FROM face_group_finalization_groups WHERE generation_id = ? ORDER BY face_group_id LIMIT ?)";
    pub const DELETE_FINALIZATION: &str = "DELETE FROM face_group_finalizations WHERE run_id = ?";
    pub const SELECT_RETIRED_GENERATION: &str = "SELECT id FROM face_group_generations WHERE status IN ('retired', 'cancelled') AND NOT EXISTS (SELECT 1 FROM face_group_finalizations WHERE face_group_finalizations.generation_id = face_group_generations.id) ORDER BY id LIMIT 1";
    pub const DELETE_RETIRED_MEMBER_PAGE: &str =
        "DELETE FROM face_group_members WHERE id IN (SELECT id FROM face_group_members WHERE automatic_generation_id = ? ORDER BY id LIMIT ?)";
    pub const DELETE_RETIRED_REPRESENTATIVE_PAGE: &str =
        "DELETE FROM face_group_representatives WHERE rowid IN (SELECT rowid FROM face_group_representatives WHERE generation_id = ? ORDER BY face_group_id LIMIT ?)";
    pub const DELETE_RETIRED_GROUP_PAGE: &str =
        "DELETE FROM face_groups WHERE id IN (SELECT id FROM face_groups WHERE automatic_generation_id = ? ORDER BY id LIMIT ?)";
    pub const DELETE_RETIRED_GENERATION: &str =
        "DELETE FROM face_group_generations WHERE id = ? AND status IN ('retired', 'cancelled')";
    pub const INSERT_GROUP: &str = "INSERT INTO face_groups (manual_curated) VALUES (0)";
    pub const INSERT_AUTOMATIC_MEMBER: &str = r#"
    INSERT INTO face_group_members
      ( face_group_id
      , face_id
      , manual_anchor
    ) VALUES (?, ?, 0)
    "#;
    pub const INSERT_MANUAL_MEMBER: &str = r#"
    INSERT INTO face_group_members
      ( face_group_id
      , face_id
      , manual_anchor
    ) VALUES (?, ?, 1)
    ON CONFLICT(face_id)
        WHERE manual_anchor = 1
    DO UPDATE SET
        face_group_id = excluded.face_group_id
    "#;
    pub const DELETE_AUTOMATIC_MEMBERSHIP_FOR_FACE: &str =
        "DELETE FROM face_group_members WHERE face_id = ? AND manual_anchor = 0";
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
       AND (
               face_group_members.manual_anchor = 1
            OR face_group_members.automatic_generation_id = (
                   SELECT active_generation_id
                     FROM face_group_generation_state
                    WHERE id = 1
               )
            OR (
                   face_group_members.automatic_generation_id IS NULL
               AND NOT EXISTS (SELECT 1 FROM face_group_generation_state WHERE id = 1)
               )
           )
     ORDER BY media_faces.id
    "#;
    pub const SELECT_GROUP_REPRESENTATIVE_CANDIDATE_PAGE: &str = r#"
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
       AND media_faces.id > ?
       AND (
               face_group_members.manual_anchor = 1
            OR face_group_members.automatic_generation_id = (
                   SELECT active_generation_id
                     FROM face_group_generation_state
                    WHERE id = 1
               )
            OR (
                   face_group_members.automatic_generation_id IS NULL
               AND NOT EXISTS (SELECT 1 FROM face_group_generation_state WHERE id = 1)
               )
           )
     ORDER BY media_faces.id
     LIMIT ?
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
       AND (
               face_group_members.manual_anchor = 1
            OR face_group_members.automatic_generation_id = (
                   SELECT active_generation_id
                     FROM face_group_generation_state
                    WHERE id = 1
               )
            OR (
                   face_group_members.automatic_generation_id IS NULL
               AND NOT EXISTS (SELECT 1 FROM face_group_generation_state WHERE id = 1)
               )
           )
     ORDER BY media_faces.id
    "#;
    pub const SELECT_VISIBLE_STORED_REPRESENTATIVE_CROP: &str = r#"
    SELECT media_faces.crop_path
      FROM face_groups
      JOIN media_faces ON media_faces.id = COALESCE(
             (
                 SELECT face_id
                   FROM face_group_representatives
                  WHERE face_group_representatives.generation_id = (
                            SELECT active_generation_id
                              FROM face_group_generation_state
                             WHERE id = 1
                        )
                    AND face_group_representatives.face_group_id = face_groups.id
             ),
             face_groups.representative_face_id
         )
      JOIN media_access ON media_access.media_id = media_faces.media_id
     WHERE face_groups.id = ?
       AND media_access.user_id = ?
       AND media_access.deleted_at IS NULL
       AND (
               face_groups.manual_curated = 1
            OR face_groups.automatic_generation_id = (
                   SELECT active_generation_id
                     FROM face_group_generation_state
                    WHERE id = 1
               )
            OR (
                   face_groups.automatic_generation_id IS NULL
               AND NOT EXISTS (SELECT 1 FROM face_group_generation_state WHERE id = 1)
               )
           )
    "#;
    pub const UPDATE_GROUP_REPRESENTATIVE_ID: &str =
        "UPDATE face_groups SET representative_face_id = ? WHERE id = ?";
    pub const SELECT_NEXT_GROUP_ID: &str =
        "SELECT id FROM face_groups WHERE id > ? ORDER BY id LIMIT 1";
    pub const SELECT_ALL_GROUP_IDS: &str = "SELECT id FROM face_groups ORDER BY id";
    pub const DELETE_MEDIA_FACES: &str = "DELETE FROM media_faces WHERE media_id = ?";
    pub const CANCEL_RECOVERED_CANCELLING_JOBS: &str = "UPDATE llm_jobs SET status = 'cancelled', state_version = state_version + 1, attempts = attempts + CASE WHEN status = 'submitting' THEN 1 ELSE 0 END, completed_at = datetime('now'), updated_at = datetime('now') WHERE task = 'face_detection' AND status IN ('queued', 'submitting', 'submitted') AND face_grouping_run_id IN (SELECT id FROM face_grouping_runs WHERE status = 'cancelling')";
    pub const QUEUE_RECOVERED_CANCELLATION_SCOPE: &str = "INSERT OR IGNORE INTO llm_cancellation_scopes (scope, task) SELECT 'task', 'face_detection' WHERE EXISTS (SELECT 1 FROM face_grouping_runs WHERE status = 'cancelling')";
    pub const QUEUE_RECOVERED_CANCELLATIONS: &str = "INSERT OR IGNORE INTO llm_job_cancellations (job_id, task) SELECT id, task FROM llm_jobs WHERE task = 'face_detection' AND status IN ('queued', 'submitting', 'submitted') AND face_grouping_run_id IN (SELECT id FROM face_grouping_runs WHERE status = 'cancelling')";
    pub const FINALIZE_RECOVERED_CANCELLING_RUNS: &str = "UPDATE face_grouping_runs SET status = 'cancelled', completed_at = datetime('now'), error = NULL WHERE status = 'cancelling'";
    pub const REQUEST_CANCEL_RUNS: &str =
        "UPDATE face_grouping_runs SET status = 'cancelling' WHERE status = 'running'";
    pub const COUNT_ACTIVE_RUNS: &str =
        "SELECT COUNT(*) FROM face_grouping_runs WHERE status IN ('running', 'cancelling')";
    pub const CLEAN_JOBS: &str = "DELETE FROM llm_jobs WHERE task = 'face_detection'";
    pub const CLEAN_GENERATION_STATE: &str = "DELETE FROM face_group_generation_state";
    pub const CLEAN_GROUPS: &str = "DELETE FROM face_groups";
    pub const CLEAN_GENERATIONS: &str = "DELETE FROM face_group_generations";
    pub const CLEAN_RUNS: &str = "DELETE FROM face_grouping_runs";
    pub const CLEAN_MANUAL_STATE: &str = "DELETE FROM face_group_manual_state";
    pub const CLEAN_FACES: &str = "DELETE FROM media_faces";
    pub const CLEAN_RESULTS: &str = "DELETE FROM media_face_detection_results";
    pub const SELECT_INPUT_CORRELATION: &str = "SELECT sequence, frame_timestamp_ms FROM llm_job_inputs WHERE job_id = ? ORDER BY sequence";
    pub const SELECT_PREPARATION_INPUTS: &str = "SELECT sequence, frame_timestamp_ms, storage_root, file_path, byte_size, content_hash FROM llm_job_inputs WHERE job_id = ? ORDER BY sequence";
    pub const SELECT_INPUT_PATH: &str = "SELECT storage_root, file_path, byte_size, content_hash FROM llm_job_inputs WHERE job_id = ? AND sequence = ?";
    pub const SELECT_MEDIA_CROPS: &str = "SELECT crop_path FROM media_faces WHERE media_id = ?";
    pub const UPSERT_RESULT: &str = "INSERT INTO media_face_detection_results (media_id, model_type, model_version) VALUES (?, 'face_detection', ?) ON CONFLICT(media_id) DO UPDATE SET model_type = excluded.model_type, model_version = excluded.model_version, completed_at = datetime('now')";
    pub const LIST_GROUPS: &str = r#"
    SELECT fg.id
         , COUNT(DISTINCT fgm.face_id)
         , COUNT(DISTINCT mf.media_id) AS media_count
      FROM face_groups AS fg
      JOIN face_group_members AS fgm ON fgm.face_group_id = fg.id
      JOIN media_faces AS mf ON mf.id = fgm.face_id
      JOIN media_access AS ma ON ma.media_id = mf.media_id
     WHERE ma.user_id = ?
       AND ma.deleted_at IS NULL
       AND (fg.manual_curated = 1 OR fg.automatic_generation_id = (SELECT active_generation_id FROM face_group_generation_state WHERE id = 1) OR (fg.automatic_generation_id IS NULL AND NOT EXISTS (SELECT 1 FROM face_group_generation_state WHERE id = 1)))
       AND (fgm.manual_anchor = 1 OR fgm.automatic_generation_id = (SELECT active_generation_id FROM face_group_generation_state WHERE id = 1) OR (fgm.automatic_generation_id IS NULL AND NOT EXISTS (SELECT 1 FROM face_group_generation_state WHERE id = 1)))
     GROUP BY fg.id
     ORDER BY media_count DESC
            , fg.id ASC
     LIMIT ? OFFSET ?
    "#;
    pub const COUNT_VISIBLE_GROUPS: &str = r#"
    SELECT COUNT(*)
      FROM (
               SELECT fg.id
                 FROM face_groups AS fg
                 JOIN face_group_members AS fgm ON fgm.face_group_id = fg.id
                 JOIN media_faces AS mf ON mf.id = fgm.face_id
                 JOIN media_access AS ma ON ma.media_id = mf.media_id
                WHERE ma.user_id = ?
                  AND ma.deleted_at IS NULL
                  AND (fg.manual_curated = 1 OR fg.automatic_generation_id = (SELECT active_generation_id FROM face_group_generation_state WHERE id = 1) OR (fg.automatic_generation_id IS NULL AND NOT EXISTS (SELECT 1 FROM face_group_generation_state WHERE id = 1)))
                  AND (fgm.manual_anchor = 1 OR fgm.automatic_generation_id = (SELECT active_generation_id FROM face_group_generation_state WHERE id = 1) OR (fgm.automatic_generation_id IS NULL AND NOT EXISTS (SELECT 1 FROM face_group_generation_state WHERE id = 1)))
                GROUP BY fg.id
           )
    "#;
    pub const SELECT_GROUP: &str = r#"
    SELECT fg.id
         , COUNT(DISTINCT fgm.face_id)
         , COUNT(DISTINCT mf.media_id)
      FROM face_groups AS fg
      JOIN face_group_members AS fgm ON fgm.face_group_id = fg.id
      JOIN media_faces AS mf ON mf.id = fgm.face_id
      JOIN media_access AS ma ON ma.media_id = mf.media_id
     WHERE fg.id = ?
       AND ma.user_id = ?
       AND ma.deleted_at IS NULL
       AND (fg.manual_curated = 1 OR fg.automatic_generation_id = (SELECT active_generation_id FROM face_group_generation_state WHERE id = 1) OR (fg.automatic_generation_id IS NULL AND NOT EXISTS (SELECT 1 FROM face_group_generation_state WHERE id = 1)))
       AND (fgm.manual_anchor = 1 OR fgm.automatic_generation_id = (SELECT active_generation_id FROM face_group_generation_state WHERE id = 1) OR (fgm.automatic_generation_id IS NULL AND NOT EXISTS (SELECT 1 FROM face_group_generation_state WHERE id = 1)))
     GROUP BY fg.id
    "#;
    pub const SELECT_GROUP_MEDIA: &str = r#"
    SELECT DISTINCT m.id
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
      JOIN media_faces AS mf ON mf.media_id = m.id
      JOIN face_group_members AS fgm ON fgm.face_id = mf.id
      JOIN face_groups AS fg ON fg.id = fgm.face_group_id
      JOIN media_access AS ma ON ma.media_id = m.id
      LEFT JOIN media_metadata AS mm ON mm.media_id = m.id
     WHERE fgm.face_group_id = ?
       AND ma.user_id = ?
       AND ma.deleted_at IS NULL
       AND (fg.manual_curated = 1 OR fg.automatic_generation_id = (SELECT active_generation_id FROM face_group_generation_state WHERE id = 1) OR (fg.automatic_generation_id IS NULL AND NOT EXISTS (SELECT 1 FROM face_group_generation_state WHERE id = 1)))
       AND (fgm.manual_anchor = 1 OR fgm.automatic_generation_id = (SELECT active_generation_id FROM face_group_generation_state WHERE id = 1) OR (fgm.automatic_generation_id IS NULL AND NOT EXISTS (SELECT 1 FROM face_group_generation_state WHERE id = 1)))
     ORDER BY m.id
    "#;
    pub const SELECT_EXISTING_GROUPS: &str = "SELECT id FROM face_groups WHERE id IN (%s) AND (manual_curated = 1 OR automatic_generation_id = (SELECT active_generation_id FROM face_group_generation_state WHERE id = 1) OR (automatic_generation_id IS NULL AND NOT EXISTS (SELECT 1 FROM face_group_generation_state WHERE id = 1)))";
    pub const SELECT_MERGE_MEMBERS: &str = "SELECT DISTINCT face_id FROM face_group_members WHERE face_group_id IN (%s) AND (manual_anchor = 1 OR automatic_generation_id = (SELECT active_generation_id FROM face_group_generation_state WHERE id = 1) OR (automatic_generation_id IS NULL AND NOT EXISTS (SELECT 1 FROM face_group_generation_state WHERE id = 1)))";
    pub const UPDATE_MANUAL_GROUP: &str =
        "UPDATE face_groups SET manual_curated = 1, automatic_generation_id = NULL WHERE id = ?";
    pub const DELETE_GENERATION_REPRESENTATIVES_FOR_GROUP: &str =
        "DELETE FROM face_group_representatives WHERE face_group_id = ?";
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
    pub const SELECT_JOB_FOR_RESULT_RECEIPT: &str =
        "SELECT media_id, task, attempts, status, state_version FROM llm_jobs WHERE id = ?";
    pub const SELECT_JOB_INPUT_CORRELATION: &str =
        "SELECT sequence, frame_timestamp_ms FROM llm_job_inputs WHERE job_id = ? ORDER BY sequence";
    pub const SELECT_RESULT_RECEIPT_STATE: &str =
        "SELECT attempt, job_version, state FROM llm_result_receipts WHERE job_id = ?";
    pub const INSERT_RESULT_RECEIPT: &str = r#"
        INSERT INTO llm_result_receipts (
            job_id, attempt, job_version, media_id, task, result_status,
            model_type, model_version, encoding, record_count, byte_size,
            content_hash, journal_group_id, sqlite_reservation_id, inbox_path, receive_token, state,
            result_product_version
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'receiving', 1)
    "#;
    pub const MARK_RESULT_RECEIPT_RECEIVED: &str = r#"
        UPDATE llm_result_receipts
           SET state = 'received', received_at = datetime('now'), updated_at = datetime('now')
         WHERE job_id = ? AND attempt = ? AND job_version = ? AND journal_group_id = ?
           AND state = 'receiving' AND cancel_requested = 0
    "#;
    pub const SELECT_RESULT_RECEIPT_PROGRESS: &str = r#"
        SELECT attempt, state, claim_token, next_record_sequence, next_byte_offset
          FROM llm_result_receipts
         WHERE job_id = ?
    "#;
    pub const INSERT_RESULT_STAGING_RECORD: &str = r#"
        INSERT INTO llm_result_staging (
            job_id, attempt, record_sequence, input_sequence, kind,
            byte_offset, encoded_size, normalized_payload
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(job_id, attempt, record_sequence) DO UPDATE SET
            input_sequence = excluded.input_sequence,
            kind = excluded.kind,
            byte_offset = excluded.byte_offset,
            encoded_size = excluded.encoded_size,
            normalized_payload = excluded.normalized_payload
    "#;
    pub const ADVANCE_RESULT_RECEIPT_PROGRESS: &str = r#"
        UPDATE llm_result_receipts
           SET next_record_sequence = ?, next_byte_offset = ?, updated_at = datetime('now')
         WHERE job_id = ? AND attempt = ? AND state = 'processing' AND claim_token = ?
           AND next_record_sequence = ? AND next_byte_offset = ?
    "#;
    pub const CLAIM_RESULT_RECEIPT: &str = r#"
        UPDATE llm_result_receipts
           SET state = 'processing', claim_token = ?, updated_at = datetime('now')
         WHERE job_id = ? AND state = 'received' AND claim_token IS NULL
           AND NOT EXISTS (SELECT 1 FROM metadata_reset_operations)
    "#;
    pub const RELEASE_RESULT_RECEIPT_CLAIM: &str = r#"
        UPDATE llm_result_receipts
           SET state = 'received', claim_token = NULL, updated_at = datetime('now')
         WHERE job_id = ? AND state = 'processing' AND claim_token = ?
    "#;
    pub const RECOVER_RESULT_RECEIPT_CLAIMS: &str = r#"
        UPDATE llm_result_receipts
           SET state = 'received', claim_token = NULL, updated_at = datetime('now')
         WHERE state = 'processing' AND claim_token IS NOT NULL
    "#;
    pub const VERIFY_RESULT_RECEIPT_CLAIM: &str = r#"
        SELECT 1 FROM llm_result_receipts
         WHERE job_id = ? AND state = 'processing' AND claim_token = ?
    "#;
    pub const FINALIZE_RESULT_ARTIFACT_GROUP: &str = r#"
        UPDATE file_operation_groups
           SET product_target = NULL, state = 'cleanup_pending',
               completion_outcome = 'published', version = version + 1,
               recovery_order = (SELECT COALESCE(MAX(recovery_order), 0) + 1 FROM file_operation_groups),
               updated_at = datetime('now')
         WHERE id = ? AND version = ? AND state = 'files_committed'
           AND owner_kind = 'llm_result'
           AND product_target = 'llm_result_face_crops' AND product_version = ?
           AND EXISTS (
                   SELECT 1 FROM llm_result_receipts
                    WHERE job_id = file_operation_groups.owner_id
                      AND state = 'processing' AND claim_token = file_operation_groups.claim_token
                      AND result_product_version = file_operation_groups.product_version
               )
    "#;
    pub const SELECT_RESULT_STAGING_CLEANUP: &str = r#"
        SELECT r.job_id
          FROM llm_result_receipts AS r
         WHERE r.state = 'cleanup_pending'
            OR (
                   r.state IN ('file_cleanup_pending', 'cleaned', 'discarded', 'failed')
               AND EXISTS (
                       SELECT 1
                         FROM data_dir_space_reservations AS s
                        WHERE s.id = r.sqlite_reservation_id
                          AND s.state = 'active'
                   )
               AND (
                       r.state != 'file_cleanup_pending'
                    OR EXISTS (
                           SELECT 1
                             FROM file_operation_groups AS g
                            WHERE g.id = r.journal_group_id
                              AND g.state IN ('cleaned', 'rolled_back')
                       )
                   )
               AND NOT (
                       r.state = 'discarded'
                   AND EXISTS (
                           SELECT 1
                             FROM file_operation_groups AS rolled_back_group
                            WHERE rolled_back_group.id = r.journal_group_id
                              AND rolled_back_group.state = 'rolled_back'
                       )
                   )
               )
      ORDER BY r.updated_at, r.job_id
         LIMIT ?
    "#;
    pub const SELECT_RESULT_RECEIPT_STATE_ONLY: &str =
        "SELECT state FROM llm_result_receipts WHERE job_id = ?";
    pub const DELETE_RESULT_STAGING_PAGE: &str = r#"
        DELETE FROM llm_result_staging
         WHERE rowid IN (
                   SELECT rowid
                     FROM (
                           SELECT rowid, attempt, record_sequence,
                                  SUM(encoded_size) OVER (
                                      ORDER BY attempt, record_sequence
                                  ) AS cumulative_bytes
                             FROM llm_result_staging
                            WHERE job_id = ?
                          )
                    WHERE cumulative_bytes <= 4194304
                 ORDER BY attempt, record_sequence
                    LIMIT ?
               )
    "#;
    pub const COUNT_RESULT_STAGING: &str =
        "SELECT COUNT(*) FROM llm_result_staging WHERE job_id = ?";
    pub const RESULT_CLEANUP_IS_TERMINAL: &str = r#"
        SELECT EXISTS (
                   SELECT 1
                     FROM llm_result_receipts AS r
                     JOIN file_operation_groups AS g ON g.id = r.journal_group_id
                    WHERE r.job_id = ?
                      AND r.state IN ('cleaned', 'discarded', 'failed')
                      AND g.state IN ('cleaned', 'rolled_back')
                      AND NOT EXISTS (
                              SELECT 1 FROM llm_result_staging AS staging
                               WHERE staging.job_id = r.job_id
                          )
               )
    "#;
    pub const SELECT_RESULT_STAGING_PAGE: &str = r#"
        SELECT record_sequence, input_sequence, kind, byte_offset, encoded_size, normalized_payload
          FROM llm_result_staging
         WHERE job_id = ? AND attempt = ? AND record_sequence > ?
           AND EXISTS (
                   SELECT 1 FROM llm_result_receipts
                    WHERE llm_result_receipts.job_id = llm_result_staging.job_id
                      AND state = 'processing' AND claim_token = ?
               )
      ORDER BY record_sequence
         LIMIT ?
    "#;
    pub const MARK_RESULT_RECEIPT_FILE_CLEANUP_PENDING: &str = r#"
        UPDATE llm_result_receipts
           SET state = 'file_cleanup_pending', updated_at = datetime('now')
         WHERE job_id = ? AND state = 'cleanup_pending'
    "#;
    pub const MARK_RESULT_RECEIPT_CLEANED_AFTER_FILE: &str = r#"
        UPDATE llm_result_receipts
           SET state = 'cleaned', updated_at = datetime('now')
         WHERE job_id = ? AND state = 'file_cleanup_pending'
           AND EXISTS (
                   SELECT 1 FROM file_operation_groups
                    WHERE id = llm_result_receipts.journal_group_id AND state = 'cleaned'
               )
    "#;
    pub const DISCARD_RESULT_RECEIPT: &str = r#"
        UPDATE llm_result_receipts
           SET state = 'discarded', cancel_requested = 1, claim_token = NULL,
               last_error = ?, updated_at = datetime('now')
         WHERE job_id = ? AND attempt = ?
           AND (? IS NULL OR job_version = ?)
           AND state IN ('receiving', 'received', 'processing')
    "#;
    pub const COMPLETE_RESULT_RECEIPT_GROUP: &str = r#"
        UPDATE file_operation_groups
           SET state = 'completed', completion_outcome = 'published', version = version + 1,
               updated_at = datetime('now'), terminal_at = datetime('now')
         WHERE id = ? AND version = ? AND state = 'files_committed'
           AND product_target = 'llm_result_inbox'
    "#;
    pub const QUEUE_RESULT_RECEIPT_CLEANUP: &str = r#"
        UPDATE file_operation_groups
           SET product_target = NULL, state = 'cleanup_pending',
               completion_outcome = 'discarded', version = version + 1,
               recovery_order = (SELECT COALESCE(MAX(recovery_order), 0) + 1 FROM file_operation_groups),
               updated_at = datetime('now'), terminal_at = NULL
         WHERE id = (SELECT journal_group_id FROM llm_result_receipts WHERE job_id = ?)
           AND kind = 'llm_result_receive' AND product_target = 'llm_result_inbox'
           AND state = 'completed'
    "#;
    pub const MARK_RESULT_RECEIPT_CLEANUP_PENDING: &str = r#"
        UPDATE llm_result_receipts
           SET state = 'cleanup_pending', claim_token = NULL, updated_at = datetime('now')
         WHERE job_id = ? AND state IN ('received', 'processing')
    "#;
    pub const MARK_COMPLETED: &str = "UPDATE llm_jobs SET status = 'completed', state_version = state_version + 1, completed_at = datetime('now'), updated_at = datetime('now') WHERE id = ? AND status = 'submitted' AND attempts = ?";
    pub const MARK_FAILED: &str = "UPDATE llm_jobs SET status = 'failed', state_version = state_version + 1, last_error = ?, completed_at = datetime('now'), updated_at = datetime('now') WHERE id = ? AND status = 'submitted' AND attempts = ?";
    pub const MARK_UNACKNOWLEDGED_RESULT_SUBMITTED: &str = "UPDATE llm_jobs SET status = 'submitted', state_version = state_version + 1, attempts = ?, submitted_at = COALESCE(submitted_at, datetime('now')), claimed_at = NULL, updated_at = datetime('now') WHERE id = ? AND status IN ('queued', 'submitting') AND attempts + 1 = ?";
    pub const MARK_RESULT_CORRELATION_FAILED: &str = "UPDATE llm_jobs SET status = 'failed', state_version = state_version + 1, last_error = ?, completed_at = datetime('now'), updated_at = datetime('now') WHERE id = ? AND status IN ('queued', 'submitting', 'submitted')";
    pub const SELECT_JOURNAL_RESULT_CANDIDATES: &str = r#"
        SELECT job_id
             , media_id
             , task
             , attempt
             , result_status
             , model_type
             , model_version
             , encoding
             , record_count
             , byte_size
             , content_hash
             , inbox_path
             , next_record_sequence
             , next_byte_offset
             , result_product_version
          FROM llm_result_receipts
         WHERE state = 'received'
      ORDER BY updated_at
             , job_id
         LIMIT ?
    "#;
    pub const MARK_RECEIVED_RESULT_FAILED: &str = "UPDATE llm_jobs SET status = 'failed', state_version = state_version + 1, last_error = ?, completed_at = datetime('now'), updated_at = datetime('now') WHERE id = ? AND status = 'submitted'";
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

    pub const SELECT_MEDIA_PAGE: &str = concat!(
        "SELECT ",
        media_response_columns!(),
        r#"
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
    "#
    );
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

    pub const SELECT_BY_ID: &str = concat!(
        "SELECT ",
        media_response_columns!(),
        r#"
         , m.created_at
      FROM media AS m
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE m.id = ?
    "#
    );

    pub const SELECT_BY_ID_AND_USER: &str = concat!(
        "SELECT ",
        media_response_columns!(),
        r#"
         , m.created_at
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE m.id = ?
       AND ma.user_id = ?
       AND ma.deleted_at IS NULL
    "#
    );

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
         , mm.preview_path
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
         , mm.preview_path
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE m.id = ?
       AND ma.user_id = ?
       AND ma.deleted_at IS NOT NULL
    "#;

    pub const SELECT_FOR_MAP: &str = concat!(
        "SELECT ",
        media_response_columns!(),
        r#"
         , m.created_at
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE ma.user_id = ?
       AND ma.deleted_at IS NULL
       AND mm.gps_latitude IS NOT NULL
       AND mm.gps_longitude IS NOT NULL
    "#
    );

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
            SELECT {media_columns}
                 , m.created_at
              FROM media AS m
              JOIN media_access AS ma ON m.id = ma.media_id
              LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
             WHERE ma.user_id = ?
               AND ma.deleted_at IS NULL
               AND m.id IN ({placeholders})
            "#,
            media_columns = media_response_columns!(),
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
    pub const SELECT_MONTH_MARKERS: &str = concat!(
        r#"
     SELECT substr(mm.date_taken, 1, 7)
          , MAX(mm.date_taken)
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
      JOIN media_metadata AS mm ON m.id = mm.media_id
       WHERE ma.user_id = ?
        AND ma.deleted_at IS NULL
        AND mm.date_taken IS NOT NULL
        "#,
        timeline_media_filters!(),
        r#"
     GROUP BY substr(mm.date_taken, 1, 7)
     ORDER BY substr(mm.date_taken, 1, 7) DESC
     "#
    );

    pub const SELECT_WINDOW: &str = concat!(
        timeline_window_prefix!(),
        timeline_media_filters!(),
        r#"
         AND mm.date_taken <= ?
      ORDER BY mm.date_taken DESC, m.id DESC
     LIMIT ?
    "#
    );

    pub const SELECT_PAGINATED_WINDOW: &str = concat!(
        timeline_window_prefix!(),
        timeline_media_filters!(),
        r#"
       AND (mm.date_taken < ? OR (mm.date_taken = ? AND m.id < ?))
     ORDER BY mm.date_taken DESC, m.id DESC
     LIMIT ?
    "#
    );

    pub const SELECT_PAGINATED_WINDOW_ASC: &str = concat!(
        timeline_window_prefix!(),
        timeline_media_filters!(),
        r#"
       AND (mm.date_taken > ? OR (mm.date_taken = ? AND m.id > ?))
     ORDER BY mm.date_taken ASC, m.id ASC
     LIMIT ?
    "#
    );
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
    pub const SELECT_IMPORTED_MEDIA: &str = r#"
    SELECT media.file_path
         , media.media_type
         , media.content_hash
         , media.original_filename
         , media.mime_type
         , COALESCE(media_metadata.artifact_version, 0)
         , media_metadata.thumbnail_path
         , media_metadata.preview_path
      FROM media
      LEFT JOIN media_metadata ON media_metadata.media_id = media.id
     WHERE media.id = ?
       AND media.import_state = 'imported'
    "#;
    pub const DELETE_RTREE_FOR_MEDIA: &str = "DELETE FROM media_rtree WHERE media_id = ?";
    pub const FINALIZE_ARTIFACT_GROUP: &str = r#"
    UPDATE file_operation_groups
       SET product_target = NULL
         , state = 'cleanup_pending'
         , completion_outcome = 'published'
         , version = version + 1
         , recovery_order = (SELECT COALESCE(MAX(recovery_order), 0) + 1 FROM file_operation_groups)
         , updated_at = datetime('now')
         , terminal_at = NULL
     WHERE id = ?
       AND version = ?
       AND state = 'files_committed'
       AND owner_kind = 'metadata_generation'
       AND product_target = 'metadata_artifacts'
       AND product_version = ?
       AND claim_token = ?
       AND EXISTS (
               SELECT 1
                 FROM media_metadata_jobs
                WHERE media_metadata_jobs.claim_token = file_operation_groups.claim_token
                  AND media_metadata_jobs.status = 'processing'
           )
    "#;
    pub const INSERT_RTREE: &str = "INSERT INTO media_rtree (media_id, min_lat, max_lat, min_lon, max_lon) VALUES (?, ?, ?, ?, ?)";
    pub const UPSERT_GEOHASH: &str = "INSERT INTO media_metadata (media_id, geohash) VALUES (?, ?) ON CONFLICT(media_id) DO UPDATE SET geohash = excluded.geohash";
    pub const DELETE_AI_INPUTS_FOR_TASK: &str =
        "DELETE FROM media_ai_inputs WHERE media_id = ? AND task = ?";
    pub const DELETE_AI_INPUTS_FOR_MEDIA: &str = "DELETE FROM media_ai_inputs WHERE media_id = ?";
    pub const INSERT_AI_INPUT: &str = "INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash, frame_timestamp_ms) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";
    pub const DELETE_SOURCES_FOR_MEDIA: &str =
        "DELETE FROM media_metadata_sources WHERE media_id = ?";
    pub const INSERT_SOURCE: &str = r#"
    INSERT INTO media_metadata_sources (
        media_id
      , source_type
      , schema_version
      , payload_json
    ) VALUES (?, ?, ?, ?)
    "#;
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

    pub const UPDATE_ARTIFACT_GENERATION: &str = r#"
    INSERT INTO media_metadata (thumbnail_path, preview_path, artifact_version, media_id)
    VALUES (?, ?, ?, ?)
    ON CONFLICT(media_id) DO UPDATE SET
        thumbnail_path = excluded.thumbnail_path
      , preview_path = excluded.preview_path
      , artifact_version = excluded.artifact_version
    "#;
}

pub mod albums {
    pub const UPDATE: &str = r#"
    UPDATE albums
       SET name = COALESCE(?, name)
         , description = COALESCE(?, description)
         , cover_media_id = COALESCE(?, cover_media_id)
     WHERE id = ?
    "#;
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

    pub const SELECT_MEDIA: &str = concat!(
        "SELECT ",
        media_response_columns!(),
        r#"
         , m.created_at
      FROM media AS m
      JOIN album_media AS am ON m.id = am.media_id
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE am.album_id = ?
     ORDER BY am.position
            , m.id
    "#
    );

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
            SELECT {media_columns}
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
            media_columns = media_response_columns!(),
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

    pub const SELECT_USERNAME_BY_ID: &str = r#"
    SELECT username
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
    pub const PRUNE_ATTEMPT_BUCKETS: &str = r#"
    DELETE FROM auth_attempt_buckets
     WHERE bucket_key IN (
           SELECT bucket_key
             FROM auth_attempt_buckets
            WHERE last_seen_at <= ?
              AND (locked_until IS NULL OR locked_until <= ?)
            ORDER BY last_seen_at, bucket_key
            LIMIT 256
     )
    "#;

    pub const COUNT_ATTEMPT_BUCKETS: &str = "SELECT COUNT(*) FROM auth_attempt_buckets";

    pub const SELECT_ATTEMPT_BUCKET: &str = r#"
    SELECT window_started_at
         , last_seen_at
         , attempts
         , locked_until
      FROM auth_attempt_buckets
     WHERE bucket_key = ?
    "#;

    pub const UPSERT_ATTEMPT_BUCKET: &str = r#"
    INSERT INTO auth_attempt_buckets (
        bucket_key
      , bucket_kind
      , window_started_at
      , last_seen_at
      , attempts
      , locked_until
    ) VALUES (?, ?, ?, ?, ?, ?)
    ON CONFLICT(bucket_key) DO UPDATE SET
        bucket_kind = excluded.bucket_kind
      , window_started_at = excluded.window_started_at
      , last_seen_at = excluded.last_seen_at
      , attempts = excluded.attempts
      , locked_until = excluded.locked_until
    "#;

    pub const DELETE_ATTEMPT_BUCKETS: &str = r#"
    DELETE FROM auth_attempt_buckets
     WHERE bucket_key IN (?, ?)
    "#;

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

    pub const DELETE_REFRESH_TOKEN_BY_HASH: &str = r#"
    DELETE FROM refresh_tokens
     WHERE token_hash = ?
    "#;

    pub const DELETE_ALL_USER_TOKENS: &str = r#"
    DELETE FROM refresh_tokens
     WHERE user_id = ?
    "#;

    pub const DELETE_REVOKED_TOKEN: &str = r#"
    DELETE FROM refresh_tokens
     WHERE revoked = 1
       AND id = ?
    "#;

    pub const DELETE_EXPIRED_OR_REVOKED_TOKENS: &str = r#"
    DELETE FROM refresh_tokens
     WHERE revoked = 1
        OR datetime(expires_at) <= datetime('now')
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

    pub const SELECT_ALBUM_MEDIA: &str = concat!(
        "SELECT ",
        media_response_columns!(),
        r#"
         , m.created_at
      FROM media AS m
      JOIN album_media AS am ON m.id = am.media_id
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE am.album_id = ?
     ORDER BY am.position
    "#
    );

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

    pub const SELECT_ALL_DELETED_PAGE: &str = r#"
    SELECT m.id
         , m.file_path
         , mm.thumbnail_path
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE ma.user_id = ?
       AND ma.deleted_at IS NOT NULL
     ORDER BY m.id
     LIMIT ?
    "#;

    pub const SELECT_OLD_DELETED_PAGE: &str = r#"
    SELECT m.id
         , m.file_path
         , mm.thumbnail_path
         , ma.user_id
      FROM media_access AS ma
      JOIN media AS m ON ma.media_id = m.id
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE ma.deleted_at IS NOT NULL
       AND ma.deleted_at < ?
     ORDER BY ma.deleted_at, m.id, ma.user_id
     LIMIT ?
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
    pub const RECOVER_SUBMITTING_JOBS: &str = "UPDATE llm_jobs SET status = 'queued', state_version = state_version + 1, claimed_at = NULL, updated_at = datetime('now') WHERE task = 'image_clustering' AND status = 'submitting'";
    pub const CANCEL_SUBMITTED_JOBS: &str = "UPDATE llm_jobs SET status = 'cancelled', state_version = state_version + 1, completed_at = datetime('now'), updated_at = datetime('now') WHERE task = 'image_clustering' AND status = 'submitted'";
    pub const FAIL_INTERRUPTED_RUNS: &str = "UPDATE media_similarity_runs SET status = 'failed', completed_at = datetime('now'), error = 'deduplicate inference was interrupted during restart' WHERE status = 'running' AND EXISTS (SELECT 1 FROM llm_jobs WHERE llm_jobs.deduplicate_run_id = media_similarity_runs.id AND llm_jobs.status = 'cancelled')";
    pub const CREATE_CLUSTERING_JOBS: &str = "INSERT INTO llm_jobs (id, media_id, deduplicate_run_id, task, status) SELECT lower(hex(randomblob(16))), media.id, ?, 'image_clustering', 'queued' FROM media JOIN media_metadata_jobs ON media_metadata_jobs.media_id = media.id WHERE media.import_state = 'imported' AND media_metadata_jobs.status = 'completed' AND NOT EXISTS (SELECT 1 FROM metadata_reset_operations) AND EXISTS (SELECT 1 FROM media_ai_inputs WHERE media_ai_inputs.media_id = media.id AND media_ai_inputs.task = 'image_clustering') AND NOT EXISTS (SELECT 1 FROM media_similarity_index WHERE media_similarity_index.media_id = media.id AND media_similarity_index.processing_status = 1) AND NOT EXISTS (SELECT 1 FROM llm_jobs WHERE llm_jobs.deduplicate_run_id = ? AND llm_jobs.media_id = media.id AND llm_jobs.task = 'image_clustering')";
    pub const REQUEUE_MISSING_INPUT_JOBS: &str = "UPDATE llm_jobs SET status = 'queued', state_version = state_version + 1, last_error = NULL, claimed_at = NULL, completed_at = NULL, available_at = datetime('now'), updated_at = datetime('now') WHERE deduplicate_run_id = ? AND task = 'image_clustering' AND status = 'failed' AND last_error = 'missing prepared AI inputs' AND EXISTS (SELECT 1 FROM media_ai_inputs WHERE media_ai_inputs.media_id = llm_jobs.media_id AND media_ai_inputs.task = 'image_clustering')";
    pub const SELECT_ACTIVE_RUNS: &str =
        "SELECT id, status FROM media_similarity_runs WHERE status IN ('running', 'cancelling')";
    pub const CANCEL_UNSUBMITTED_JOBS: &str = "UPDATE llm_jobs SET status = 'cancelled', state_version = state_version + 1, completed_at = datetime('now'), updated_at = datetime('now') WHERE deduplicate_run_id = ? AND status IN ('queued', 'submitting')";
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

    pub const SELECT_LATEST_RUN_STATUS: &str = r#"
    SELECT status
      FROM media_similarity_runs
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

    pub const SELECT_BAND_CANDIDATE_PAGE: &str = r#"
    SELECT candidate.media_id
         , candidate.embedding
         , candidate.perceptual_hash
         , candidate.capture_time_seconds
      FROM media_similarity_index AS candidate
     WHERE candidate.media_id < ?
       AND candidate.media_id > ?
       AND candidate.processing_status = 1
       AND EXISTS (
            SELECT 1
              FROM media_similarity_hash_bands AS source_band
              JOIN media_similarity_hash_bands AS candidate_band
                ON candidate_band.band_index = source_band.band_index
               AND candidate_band.band_value = source_band.band_value
             WHERE source_band.media_id = ?
               AND candidate_band.media_id = candidate.media_id
       )
     ORDER BY candidate.media_id
     LIMIT ?
    "#;

    pub const SELECT_TIME_CANDIDATE_PAGE: &str = r#"
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

    pub const SELECT_FINALIZATION_CLEANUP: &str = r#"
    SELECT run_id
      FROM media_similarity_finalizations
     WHERE phase = 'cleanup'
     ORDER BY run_id
     LIMIT 1
    "#;

    pub const SELECT_RETIRED_GENERATION: &str = r#"
    SELECT id
      FROM media_similarity_generations
     WHERE status = 'retiring'
    UNION ALL
    SELECT -1
     WHERE EXISTS (SELECT 1 FROM media_similarity_generation_state WHERE singleton = 1)
       AND EXISTS (SELECT 1 FROM media_similarity_clusters WHERE generation_id IS NULL)
     ORDER BY id
     LIMIT 1
    "#;

    pub const SELECT_FINALIZATION_STATE: &str = r#"
    SELECT generation_id
         , phase
         , source_media_id
         , source_cursor
         , candidate_kind
         , candidate_cursor
         , label_kind
         , label_media_cursor
         , label_edge_left_cursor
         , label_edge_right_cursor
         , label_pass_changed
         , group_kind
         , group_label_cursor
         , group_member_cursor
         , group_cluster_id
         , completion_error
         , dirty_cursor
      FROM media_similarity_finalizations
     WHERE run_id = ?
    "#;

    pub const SELECT_NEXT_INDEX_ID: &str = r#"
    SELECT media_id
      FROM media_similarity_index
     WHERE media_id > ?
       AND processing_status = 1
     ORDER BY media_id
     LIMIT 1
    "#;

    pub const SELECT_LABEL_INITIALIZATION_PAGE: &str = r#"
    SELECT media_id
      FROM media_similarity_index
     WHERE media_id > ?
       AND processing_status = 1
     ORDER BY media_id
     LIMIT ?
    "#;

    pub const SELECT_EDGE_PAGE: &str = r#"
    SELECT left_media_id
         , right_media_id
      FROM media_similarity_edges
     WHERE run_id = ?
       AND kind = ?
       AND (
            left_media_id > ?
         OR (left_media_id = ? AND right_media_id > ?)
       )
     ORDER BY left_media_id
            , right_media_id
     LIMIT ?
    "#;

    pub const SELECT_NEXT_COMPONENT: &str = r#"
    SELECT component_label
      FROM media_similarity_labels
     WHERE run_id = ?
       AND kind = ?
       AND component_label > ?
     GROUP BY component_label
    HAVING COUNT(*) >= 2
     ORDER BY component_label
     LIMIT 1
    "#;

    pub const SELECT_COMPONENT_MEMBER_PAGE: &str = r#"
    SELECT similarity_index.media_id
         , similarity_index.embedding
         , similarity_index.perceptual_hash
         , similarity_index.capture_time_seconds
      FROM media_similarity_labels AS labels
      JOIN media_similarity_index AS similarity_index ON similarity_index.media_id = labels.media_id
     WHERE labels.run_id = ?
       AND labels.kind = ?
       AND labels.component_label = ?
       AND labels.media_id > ?
     ORDER BY labels.media_id
     LIMIT ?
    "#;

    pub const SELECT_COMPONENT_COUNT: &str = r#"
    SELECT COUNT(*)
      FROM media_similarity_labels
     WHERE run_id = ?
       AND kind = ?
       AND component_label = ?
    "#;

    pub const SELECT_NEAR_COMPONENT_LABEL: &str = r#"
    SELECT component_label
      FROM media_similarity_labels
     WHERE run_id = ?
       AND kind = 'near_duplicate'
       AND media_id = ?
    "#;

    pub const COUNT_COMPONENT_MEMBERS_OUTSIDE_NEAR_LABEL: &str = r#"
    SELECT COUNT(*)
      FROM media_similarity_labels AS burst_labels
     WHERE burst_labels.run_id = ?
       AND burst_labels.kind = 'burst'
       AND burst_labels.component_label = ?
       AND NOT EXISTS (
            SELECT 1
              FROM media_similarity_labels AS near_labels
             WHERE near_labels.run_id = burst_labels.run_id
               AND near_labels.kind = 'near_duplicate'
               AND near_labels.component_label = ?
               AND near_labels.media_id = burst_labels.media_id
       )
    "#;

    pub const DELETE_FINALIZATION_EDGES_PAGE: &str = r#"
    DELETE FROM media_similarity_edges
     WHERE rowid IN (
        SELECT rowid
          FROM media_similarity_edges
         WHERE run_id = ?
         ORDER BY kind
                , left_media_id
                , right_media_id
         LIMIT ?
     )
    "#;

    pub const DELETE_FINALIZATION_LABELS_PAGE: &str = r#"
    DELETE FROM media_similarity_labels
     WHERE rowid IN (
        SELECT rowid
          FROM media_similarity_labels
         WHERE run_id = ?
         ORDER BY kind
                , media_id
         LIMIT ?
     )
    "#;

    pub const DELETE_FINALIZATION_DIRTY_PAGE: &str = r#"
    DELETE FROM media_similarity_finalization_dirty
     WHERE rowid IN (
        SELECT rowid
          FROM media_similarity_finalization_dirty
         WHERE run_id = ?
         ORDER BY media_id
         LIMIT ?
     )
    "#;

    pub const CLEAR_FINALIZATION_DIRTY_PAGE: &str = r#"
    DELETE FROM media_similarity_dirty
     WHERE (media_id, marked_at) IN (
        SELECT media_id
             , marked_at
          FROM media_similarity_finalization_dirty AS snapshot
         WHERE snapshot.run_id = ?
         ORDER BY snapshot.media_id
         LIMIT ?
     )
    "#;

    pub const DELETE_RETIRED_MEMBERS_PAGE: &str = r#"
    DELETE FROM media_similarity_cluster_members
     WHERE rowid IN (
        SELECT members.rowid
          FROM media_similarity_cluster_members AS members
          JOIN media_similarity_clusters AS clusters ON clusters.id = members.cluster_id
         WHERE COALESCE(clusters.generation_id, -1) = ?
         ORDER BY members.cluster_id
                , members.media_id
         LIMIT ?
     )
    "#;

    pub const DELETE_RETIRED_CLUSTERS_PAGE: &str = r#"
    DELETE FROM media_similarity_clusters
     WHERE id IN (
        SELECT id
          FROM media_similarity_clusters
         WHERE COALESCE(generation_id, -1) = ?
         ORDER BY id
         LIMIT ?
     )
    "#;

    pub const DELETE_RETIRED_GENERATION: &str = r#"
    DELETE FROM media_similarity_generations
     WHERE id = ?
       AND status = 'retiring'
       AND NOT EXISTS (
            SELECT 1
              FROM media_similarity_clusters
             WHERE generation_id = media_similarity_generations.id
       )
    "#;

    pub const DELETE_FINALIZATION: &str = r#"
    DELETE FROM media_similarity_finalizations
     WHERE run_id = ?
       AND phase = 'cleanup'
       AND NOT EXISTS (SELECT 1 FROM media_similarity_edges WHERE run_id = ?)
       AND NOT EXISTS (SELECT 1 FROM media_similarity_labels WHERE run_id = ?)
       AND NOT EXISTS (SELECT 1 FROM media_similarity_finalization_dirty WHERE run_id = ?)
    "#;

    pub const INSERT_GENERATION: &str = r#"
    INSERT INTO media_similarity_generations (
        run_id
      , status
    ) VALUES (?, 'building')
    "#;

    pub const INSERT_FINALIZATION_DIRTY: &str = r#"
    INSERT INTO media_similarity_finalization_dirty (
        run_id
      , media_id
      , marked_at
    ) VALUES (?, ?, ?)
    "#;

    pub const SELECT_FINALIZATION_DIRTY_SOURCE_PAGE: &str = r#"
    SELECT media_id
         , marked_at
      FROM media_similarity_dirty
     WHERE media_id > ?
     ORDER BY media_id
     LIMIT ?
    "#;

    pub const ADVANCE_FINALIZATION_DIRTY_CURSOR: &str = r#"
    UPDATE media_similarity_finalizations
       SET dirty_cursor = ?
     WHERE run_id = ?
       AND phase = 'dirty_snapshot'
    "#;

    pub const FINISH_FINALIZATION_DIRTY_SNAPSHOT: &str = r#"
    UPDATE media_similarity_finalizations
       SET phase = 'comparison'
         , source_cursor = 0
     WHERE run_id = ?
       AND phase = 'dirty_snapshot'
    "#;

    pub const INSERT_FINALIZATION: &str = r#"
    INSERT INTO media_similarity_finalizations (
        run_id
      , generation_id
      , phase
      , completion_error
    ) VALUES (?, ?, 'dirty_snapshot', ?)
    "#;

    pub const START_COMPARISON_SOURCE: &str = r#"
    UPDATE media_similarity_finalizations
       SET source_media_id = ?
         , candidate_kind = 'near_duplicate'
         , candidate_cursor = 0
     WHERE run_id = ?
       AND phase = 'comparison'
       AND source_media_id IS NULL
    "#;

    pub const FINISH_COMPARISONS: &str = r#"
    UPDATE media_similarity_finalizations
       SET phase = 'label_initialization'
         , label_kind = 'near_duplicate'
         , label_media_cursor = 0
     WHERE run_id = ?
       AND phase = 'comparison'
       AND source_media_id IS NULL
    "#;

    pub const INSERT_EDGE: &str = r#"
    INSERT OR REPLACE INTO media_similarity_edges (
        run_id
      , kind
      , left_media_id
      , right_media_id
      , cosine_similarity
      , perceptual_hash_distance
    ) VALUES (?, ?, ?, ?, ?, ?)
    "#;

    pub const ADVANCE_COMPARISON_PAGE: &str = r#"
    UPDATE media_similarity_finalizations
       SET candidate_cursor = ?
     WHERE run_id = ?
       AND phase = 'comparison'
       AND source_media_id = ?
       AND candidate_kind = ?
       AND candidate_cursor < ?
    "#;

    pub const ADVANCE_COMPARISON_KIND: &str = r#"
    UPDATE media_similarity_finalizations
       SET candidate_kind = 'burst'
         , candidate_cursor = 0
     WHERE run_id = ?
       AND phase = 'comparison'
       AND source_media_id = ?
       AND candidate_kind = 'near_duplicate'
    "#;

    pub const FINISH_COMPARISON_SOURCE: &str = r#"
    UPDATE media_similarity_finalizations
       SET source_cursor = ?
         , source_media_id = NULL
         , candidate_kind = 'near_duplicate'
         , candidate_cursor = 0
     WHERE run_id = ?
       AND phase = 'comparison'
       AND source_media_id = ?
       AND candidate_kind = 'burst'
    "#;

    pub const INSERT_LABEL: &str = r#"
    INSERT OR IGNORE INTO media_similarity_labels (
        run_id
      , kind
      , media_id
      , component_label
    ) VALUES (?, ?, ?, ?)
    "#;

    pub const ADVANCE_LABEL_INITIALIZATION: &str = r#"
    UPDATE media_similarity_finalizations
       SET label_media_cursor = ?
     WHERE run_id = ?
       AND phase = 'label_initialization'
       AND label_kind = ?
    "#;

    pub const SWITCH_LABEL_INITIALIZATION_KIND: &str = r#"
    UPDATE media_similarity_finalizations
       SET label_kind = 'burst'
         , label_media_cursor = 0
     WHERE run_id = ?
       AND phase = 'label_initialization'
       AND label_kind = 'near_duplicate'
    "#;

    pub const FINISH_LABEL_INITIALIZATION: &str = r#"
    UPDATE media_similarity_finalizations
       SET phase = 'label_propagation'
         , label_kind = 'near_duplicate'
         , label_media_cursor = 0
         , label_edge_left_cursor = 0
         , label_edge_right_cursor = 0
         , label_pass_changed = 0
     WHERE run_id = ?
       AND phase = 'label_initialization'
       AND label_kind = 'burst'
    "#;

    pub const SELECT_LABEL: &str = r#"
    SELECT component_label
      FROM media_similarity_labels
     WHERE run_id = ?
       AND kind = ?
       AND media_id = ?
    "#;

    pub const LOWER_LABEL: &str = r#"
    UPDATE media_similarity_labels
       SET component_label = ?
     WHERE run_id = ?
       AND kind = ?
       AND media_id = ?
       AND component_label > ?
    "#;

    pub const ADVANCE_LABEL_EDGE_PAGE: &str = r#"
    UPDATE media_similarity_finalizations
       SET label_edge_left_cursor = ?
         , label_edge_right_cursor = ?
         , label_pass_changed = CASE WHEN label_pass_changed = 1 OR ? = 1 THEN 1 ELSE 0 END
     WHERE run_id = ?
       AND phase = 'label_propagation'
       AND label_kind = ?
    "#;

    pub const RESTART_LABEL_PASS: &str = r#"
    UPDATE media_similarity_finalizations
       SET label_edge_left_cursor = 0
         , label_edge_right_cursor = 0
         , label_pass_changed = 0
     WHERE run_id = ?
       AND phase = 'label_propagation'
       AND label_kind = ?
       AND label_pass_changed = 1
    "#;

    pub const SWITCH_LABEL_PROPAGATION_KIND: &str = r#"
    UPDATE media_similarity_finalizations
       SET label_kind = 'burst'
         , label_edge_left_cursor = 0
         , label_edge_right_cursor = 0
         , label_pass_changed = 0
     WHERE run_id = ?
       AND phase = 'label_propagation'
       AND label_kind = 'near_duplicate'
       AND label_pass_changed = 0
    "#;

    pub const FINISH_LABEL_PROPAGATION: &str = r#"
    UPDATE media_similarity_finalizations
       SET phase = 'grouping'
         , group_kind = 'near_duplicate'
         , group_label_cursor = 0
         , group_member_cursor = 0
         , group_cluster_id = NULL
     WHERE run_id = ?
       AND phase = 'label_propagation'
       AND label_kind = 'burst'
       AND label_pass_changed = 0
    "#;

    pub const INSERT_GENERATION_CLUSTER: &str = r#"
    INSERT INTO media_similarity_clusters (
        generation_id
      , kind
      , representative_media_id
    ) VALUES (?, ?, ?)
    "#;

    pub const START_GROUP: &str = r#"
    UPDATE media_similarity_finalizations
       SET group_label_cursor = ?
         , group_cluster_id = ?
         , group_member_cursor = 0
     WHERE run_id = ?
       AND phase = 'grouping'
       AND group_kind = ?
       AND group_label_cursor < ?
       AND group_cluster_id IS NULL
    "#;

    pub const SKIP_GROUP: &str = r#"
    UPDATE media_similarity_finalizations
       SET group_label_cursor = ?
         , group_member_cursor = 0
         , group_cluster_id = NULL
     WHERE run_id = ?
       AND phase = 'grouping'
       AND group_kind = ?
       AND group_cluster_id IS NULL
    "#;

    pub const INSERT_GENERATION_MEMBER: &str = r#"
    INSERT OR REPLACE INTO media_similarity_cluster_members (
        cluster_id
      , media_id
      , cosine_similarity
      , perceptual_hash_distance
    ) VALUES (?, ?, ?, ?)
    "#;

    pub const ADVANCE_GROUP_MEMBER_PAGE: &str = r#"
    UPDATE media_similarity_finalizations
       SET group_member_cursor = ?
     WHERE run_id = ?
       AND phase = 'grouping'
       AND group_cluster_id = ?
       AND group_kind = ?
    "#;

    pub const FINISH_GROUP: &str = r#"
    UPDATE media_similarity_finalizations
       SET group_label_cursor = ?
         , group_member_cursor = 0
         , group_cluster_id = NULL
     WHERE run_id = ?
       AND phase = 'grouping'
       AND group_cluster_id = ?
       AND group_kind = ?
    "#;

    pub const SWITCH_GROUP_KIND: &str = r#"
    UPDATE media_similarity_finalizations
       SET group_kind = 'burst'
         , group_label_cursor = 0
         , group_member_cursor = 0
         , group_cluster_id = NULL
     WHERE run_id = ?
       AND phase = 'grouping'
       AND group_kind = 'near_duplicate'
    "#;

    pub const FINISH_GROUPING: &str = r#"
    UPDATE media_similarity_finalizations
       SET phase = 'publishing'
     WHERE run_id = ?
       AND phase = 'grouping'
       AND group_kind = 'burst'
       AND group_cluster_id IS NULL
    "#;

    pub const COUNT_GENERATION_CLUSTERS: &str = r#"
    SELECT COUNT(*)
      FROM media_similarity_clusters
     WHERE generation_id = ?
    "#;

    pub const RETIRE_ACTIVE_GENERATION: &str = r#"
    UPDATE media_similarity_generations
       SET status = 'retiring'
     WHERE id = (
        SELECT active_generation_id
          FROM media_similarity_generation_state
         WHERE singleton = 1
     )
       AND id <> ?
    "#;

    pub const ACTIVATE_GENERATION: &str = r#"
    UPDATE media_similarity_generations
       SET status = 'active'
         , published_at = datetime('now')
     WHERE id = ?
       AND status = 'building'
    "#;

    pub const SWITCH_ACTIVE_GENERATION: &str = r#"
    INSERT INTO media_similarity_generation_state (
        singleton
      , active_generation_id
    ) VALUES (1, ?)
    ON CONFLICT(singleton) DO UPDATE
       SET active_generation_id = excluded.active_generation_id
    "#;

    pub const ENTER_FINALIZATION_CLEANUP: &str = r#"
    UPDATE media_similarity_finalizations
       SET phase = 'cleanup'
     WHERE run_id = ?
       AND phase = 'publishing'
    "#;

    pub const CANCEL_BUILDING_GENERATION: &str = r#"
    UPDATE media_similarity_generations
       SET status = 'retiring'
     WHERE id = (
        SELECT generation_id
          FROM media_similarity_finalizations
         WHERE run_id = ?
     )
       AND status = 'building'
    "#;

    pub const CANCEL_FINALIZATION: &str = r#"
    UPDATE media_similarity_finalizations
       SET phase = 'cleanup'
     WHERE run_id = ?
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

    pub const SELECT_VISIBLE_CLUSTER_PAGE: &str = r#"
    WITH ordered_visible_members AS (
        SELECT members.cluster_id
             , members.media_id
          FROM media_similarity_cluster_members AS members
          JOIN media_similarity_clusters AS clusters ON clusters.id = members.cluster_id
          LEFT JOIN media_similarity_generation_state AS generation_state
            ON generation_state.singleton = 1
          JOIN media_access ON media_access.media_id = members.media_id
         WHERE media_access.user_id = ?
           AND media_access.deleted_at IS NULL
           AND (
                generation_state.active_generation_id = clusters.generation_id
             OR (generation_state.active_generation_id IS NULL AND clusters.generation_id IS NULL)
           )
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
    pub const CLEAN_FINALIZATION_DIRTY: &str = "DELETE FROM media_similarity_finalization_dirty";
    pub const CLEAN_EDGES: &str = "DELETE FROM media_similarity_edges";
    pub const CLEAN_LABELS: &str = "DELETE FROM media_similarity_labels";
    pub const CLEAN_FINALIZATIONS: &str = "DELETE FROM media_similarity_finalizations";
    pub const CLEAN_GENERATION_STATE: &str = "DELETE FROM media_similarity_generation_state";
    pub const CLEAN_GENERATIONS: &str = "DELETE FROM media_similarity_generations";
    pub const CLEAN_JOBS: &str = "DELETE FROM llm_jobs WHERE task = 'image_clustering'";
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
