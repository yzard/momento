CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT UNIQUE NOT NULL,
    email TEXT UNIQUE NOT NULL,
    hashed_password TEXT NOT NULL,
    role TEXT CHECK(role IN ('admin', 'user')) DEFAULT 'user',
    must_change_password INTEGER DEFAULT 1,
    is_active INTEGER DEFAULT 1,
    created_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS media (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER,
    filename TEXT NOT NULL,
    original_filename TEXT NOT NULL,
    file_path TEXT NOT NULL,
    media_type TEXT CHECK(media_type IN ('image', 'video')) NOT NULL,
    mime_type TEXT,
    file_size INTEGER,
    content_hash TEXT,
    import_state TEXT NOT NULL DEFAULT 'imported' CHECK(import_state IN ('importing', 'imported', 'failed')),
    import_source TEXT NOT NULL DEFAULT 'local' CHECK(import_source IN ('local', 'webdav', 'mobile_backup')),
    import_error TEXT,
    imported_at TEXT,
    created_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS import_content_hash_claims (
    content_hash TEXT PRIMARY KEY NOT NULL CHECK(length(content_hash) = 64),
    claim_token TEXT UNIQUE NOT NULL CHECK(length(claim_token) = 36),
    import_source TEXT NOT NULL CHECK(import_source IN ('local', 'webdav', 'mobile_backup')),
    claimed_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS media_metadata (
    media_id INTEGER PRIMARY KEY,
    thumbnail_path TEXT,
    preview_path TEXT,
    artifact_version INTEGER NOT NULL DEFAULT 0 CHECK(artifact_version >= 0),
    width INTEGER,
    height INTEGER,
    duration_seconds REAL,
    date_taken TEXT,
    gps_latitude REAL,
    gps_longitude REAL,
    gps_altitude REAL,
    geohash TEXT,
    location_city TEXT,
    location_state TEXT,
    location_country TEXT,
    camera_make TEXT,
    camera_model TEXT,
    lens_make TEXT,
    lens_model TEXT,
    iso INTEGER,
    exposure_time TEXT,
    f_number REAL,
    focal_length REAL,
    focal_length_35mm REAL,
    video_codec TEXT,
    keywords TEXT,
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS media_metadata_sources (
    media_id INTEGER NOT NULL,
    source_type TEXT NOT NULL CHECK(source_type IN ('exiftool', 'ffprobe', 'supplemental_sidecar')),
    schema_version INTEGER NOT NULL CHECK(schema_version = 1),
    payload_json TEXT NOT NULL CHECK(json_valid(payload_json)),
    captured_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (media_id, source_type),
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS albums (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    cover_media_id INTEGER,
    created_at TEXT DEFAULT (datetime('now')),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (cover_media_id) REFERENCES media(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS album_media (
    album_id INTEGER NOT NULL,
    media_id INTEGER NOT NULL,
    position INTEGER DEFAULT 0,
    added_at TEXT DEFAULT (datetime('now')),
    PRIMARY KEY (album_id, media_id),
    FOREIGN KEY (album_id) REFERENCES albums(id) ON DELETE CASCADE,
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS share_links (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    media_id INTEGER,
    album_id INTEGER,
    token TEXT UNIQUE NOT NULL,
    password_hash TEXT,
    expires_at TEXT,
    view_count INTEGER DEFAULT 0,
    created_at TEXT DEFAULT (datetime('now')),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE,
    FOREIGN KEY (album_id) REFERENCES albums(id) ON DELETE CASCADE,
    CHECK (
        (media_id IS NOT NULL AND album_id IS NULL)
        OR (media_id IS NULL AND album_id IS NOT NULL)
    )
);

CREATE TABLE IF NOT EXISTS refresh_tokens (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    token_hash TEXT UNIQUE NOT NULL,
    user_id INTEGER NOT NULL,
    expires_at TEXT NOT NULL,
    revoked INTEGER DEFAULT 0,
    created_at TEXT DEFAULT (datetime('now')),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS auth_attempt_buckets (
    bucket_key BLOB PRIMARY KEY NOT NULL CHECK (length(bucket_key) = 32),
    bucket_kind TEXT NOT NULL CHECK (bucket_kind IN ('source', 'identity')),
    window_started_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    attempts INTEGER NOT NULL CHECK (attempts >= 0),
    locked_until INTEGER
);

CREATE TABLE IF NOT EXISTS backup_devices (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    device_id TEXT NOT NULL,
    device_name TEXT NOT NULL,
    registered_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_seen_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (user_id, device_id),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS backup_assets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    device_id TEXT NOT NULL,
    client_asset_id TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    original_filename TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    byte_size INTEGER NOT NULL CHECK(byte_size > 0),
    source_modified_at TEXT,
    status TEXT NOT NULL CHECK(status IN ('uploading', 'queued', 'processing', 'completed', 'failed', 'cancelled', 'expired')),
    staged_path TEXT NOT NULL,
    content_hash TEXT,
    media_id INTEGER,
    error TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    completed_at TEXT,
    UNIQUE (user_id, device_id, client_asset_id),
    UNIQUE (user_id, operation_id),
    FOREIGN KEY (user_id, device_id) REFERENCES backup_devices(user_id, device_id) ON DELETE CASCADE,
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS backup_upload_sessions (
    upload_id TEXT PRIMARY KEY,
    asset_id INTEGER NOT NULL UNIQUE,
    user_id INTEGER NOT NULL,
    expected_size INTEGER NOT NULL CHECK(expected_size > 0),
    uploaded_size INTEGER NOT NULL DEFAULT 0 CHECK(uploaded_size >= 0),
    status TEXT NOT NULL CHECK(status IN ('uploading', 'writing', 'queued', 'processing', 'completed', 'failed', 'cancelled', 'expired')),
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (asset_id) REFERENCES backup_assets(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS backup_asset_manifests (
    asset_id INTEGER PRIMARY KEY,
    protocol_version INTEGER NOT NULL CHECK(protocol_version = 2),
    content_hash TEXT NOT NULL CHECK(length(content_hash) = 64),
    metadata_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (asset_id) REFERENCES backup_assets(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_backup_upload_sessions_user_active
    ON backup_upload_sessions (user_id, status, expires_at);

CREATE INDEX IF NOT EXISTS idx_backup_assets_claim
    ON backup_assets (status, id);

CREATE TABLE IF NOT EXISTS media_access (
    media_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    access_level INTEGER NOT NULL CHECK (access_level IN (1, 2)),
    created_at TEXT DEFAULT (datetime('now')),
    deleted_at TEXT DEFAULT NULL,
    PRIMARY KEY (media_id, user_id),
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS album_access (
    album_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    access_level INTEGER NOT NULL CHECK (access_level IN (1, 2)),
    created_at TEXT DEFAULT (datetime('now')),
    PRIMARY KEY (album_id, user_id),
    FOREIGN KEY (album_id) REFERENCES albums(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE VIRTUAL TABLE IF NOT EXISTS media_rtree USING rtree (
    media_id,
    min_lat,
    max_lat,
    min_lon,
    max_lon
);

CREATE TABLE IF NOT EXISTS media_text (
    media_id INTEGER NOT NULL,
    model_type TEXT NOT NULL,
    model_version TEXT NOT NULL,
    string TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (media_id, model_type),
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS media_text_inputs (
    media_id INTEGER NOT NULL,
    model_type TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    frame_timestamp_ms INTEGER,
    model_version TEXT NOT NULL,
    string TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (media_id, model_type, sequence),
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS media_aesthetics (
    media_id INTEGER PRIMARY KEY,
    model_type TEXT NOT NULL CHECK(model_type = 'image_aesthetics'),
    model_version TEXT NOT NULL,
    aesthetic_score REAL NOT NULL CHECK(aesthetic_score >= 0.0 AND aesthetic_score <= 1.0),
    scenic_score REAL NOT NULL CHECK(scenic_score >= 0.0 AND scenic_score <= 1.0),
    simplicity_score REAL NOT NULL CHECK(simplicity_score >= 0.0 AND simplicity_score <= 1.0),
    landscape_score REAL NOT NULL CHECK(landscape_score >= 0.0 AND landscape_score <= 1.0),
    technical_quality_score REAL NOT NULL CHECK(technical_quality_score >= 0.0 AND technical_quality_score <= 1.0),
    completed_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS media_aesthetic_inputs (
    media_id INTEGER NOT NULL,
    sequence INTEGER NOT NULL,
    frame_timestamp_ms INTEGER,
    model_type TEXT NOT NULL CHECK(model_type = 'image_aesthetics'),
    model_version TEXT NOT NULL,
    aesthetic_score REAL NOT NULL CHECK(aesthetic_score >= 0.0 AND aesthetic_score <= 1.0),
    scenic_score REAL NOT NULL CHECK(scenic_score >= 0.0 AND scenic_score <= 1.0),
    simplicity_score REAL NOT NULL CHECK(simplicity_score >= 0.0 AND simplicity_score <= 1.0),
    landscape_score REAL NOT NULL CHECK(landscape_score >= 0.0 AND landscape_score <= 1.0),
    technical_quality_score REAL NOT NULL CHECK(technical_quality_score >= 0.0 AND technical_quality_score <= 1.0),
    completed_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (media_id, sequence),
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS media_screenshot_classifications (
    media_id INTEGER PRIMARY KEY,
    model_type TEXT NOT NULL CHECK(model_type = 'screenshot_detection'),
    model_version TEXT NOT NULL,
    is_screenshot INTEGER NOT NULL CHECK(is_screenshot IN (0, 1)),
    confidence REAL NOT NULL CHECK(confidence >= 0.0 AND confidence <= 1.0),
    completed_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS media_screenshot_classification_inputs (
    media_id INTEGER NOT NULL,
    sequence INTEGER NOT NULL,
    frame_timestamp_ms INTEGER,
    model_type TEXT NOT NULL CHECK(model_type = 'screenshot_detection'),
    model_version TEXT NOT NULL,
    is_screenshot INTEGER NOT NULL CHECK(is_screenshot IN (0, 1)),
    confidence REAL NOT NULL CHECK(confidence >= 0.0 AND confidence <= 1.0),
    completed_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (media_id, sequence),
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS media_document_classifications (
    media_id INTEGER PRIMARY KEY,
    model_type TEXT NOT NULL CHECK(model_type = 'document_detection'),
    model_version TEXT NOT NULL,
    is_document INTEGER NOT NULL CHECK(is_document IN (0, 1)),
    confidence REAL NOT NULL CHECK(confidence >= 0.0 AND confidence <= 1.0),
    completed_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS media_document_classification_inputs (
    media_id INTEGER NOT NULL,
    sequence INTEGER NOT NULL,
    frame_timestamp_ms INTEGER,
    model_type TEXT NOT NULL CHECK(model_type = 'document_detection'),
    model_version TEXT NOT NULL,
    is_document INTEGER NOT NULL CHECK(is_document IN (0, 1)),
    confidence REAL NOT NULL CHECK(confidence >= 0.0 AND confidence <= 1.0),
    completed_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (media_id, sequence),
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS media_metadata_jobs (
    media_id INTEGER PRIMARY KEY,
    status TEXT NOT NULL CHECK(status IN ('queued', 'processing', 'completed', 'failed')),
    claim_token TEXT UNIQUE,
    attempts INTEGER NOT NULL DEFAULT 0,
    rerun_requested INTEGER NOT NULL DEFAULT 0 CHECK(rerun_requested IN (0, 1)),
    available_at TEXT NOT NULL DEFAULT (datetime('now')),
    claimed_at TEXT,
    completed_at TEXT,
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    CHECK(
        (status = 'processing' AND claim_token IS NOT NULL AND length(claim_token) = 36)
        OR (status <> 'processing' AND claim_token IS NULL)
    ),
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS metadata_reset_operations (
    id INTEGER PRIMARY KEY CHECK(id = 1),
    cleanup_group_id TEXT NOT NULL UNIQUE,
    phase TEXT NOT NULL CHECK(phase IN (
        'metadata_jobs', 'llm_result_groups', 'llm_result_staging', 'llm_result_receipts',
        'llm_reservations', 'llm_job_cancellations', 'llm_cancellation_scopes',
        'llm_jobs', 'text_inputs', 'text',
        'aesthetic_inputs', 'aesthetics', 'screenshot_inputs', 'screenshots',
        'document_inputs', 'documents', 'face_finalization_faces',
        'face_finalization_anchors', 'face_finalization_groups',
        'face_representatives', 'face_members', 'face_groups',
        'face_finalizations', 'face_generation_state', 'face_manual_state', 'face_generations',
        'face_runs', 'face_results', 'media_faces',
        'similarity_cluster_members', 'similarity_clusters',
        'similarity_dirty_snapshot', 'similarity_edges', 'similarity_labels',
        'similarity_finalizations', 'similarity_generation_state',
        'similarity_generations', 'similarity_bands', 'similarity_index',
        'similarity_dirty', 'similarity_runs', 'ai_inputs', 'rtree',
        'metadata_sources', 'metadata', 'queue_imported', 'dirty_imported',
        'activate_cleanup'
    )),
    media_cursor INTEGER NOT NULL DEFAULT 0 CHECK(media_cursor >= 0),
    media_count INTEGER NOT NULL CHECK(media_count >= 0),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (cleanup_group_id) REFERENCES file_operation_groups(id) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS import_jobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source TEXT NOT NULL CHECK(source IN ('local', 'webdav')),
    status TEXT NOT NULL CHECK(status IN ('running', 'completed', 'failed')),
    total_files INTEGER NOT NULL DEFAULT 0,
    processed_files INTEGER NOT NULL DEFAULT 0,
    successful_imports INTEGER NOT NULL DEFAULT 0,
    failed_imports INTEGER NOT NULL DEFAULT 0,
    started_at TEXT NOT NULL DEFAULT (datetime('now')),
    completed_at TEXT,
    last_error TEXT
);

CREATE TABLE IF NOT EXISTS import_job_errors (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    import_job_id INTEGER NOT NULL,
    error TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (import_job_id) REFERENCES import_jobs(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_import_job_errors_job_id
ON import_job_errors(import_job_id, id DESC);

CREATE TABLE IF NOT EXISTS webdav_ready_files (
    user_id INTEGER NOT NULL,
    file_path TEXT NOT NULL,
    completed_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (user_id, file_path),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS media_ai_inputs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    media_id INTEGER NOT NULL,
    task TEXT NOT NULL CHECK(task IN ('ocr', 'image_tagging', 'image_clustering', 'image_aesthetics', 'screenshot_detection', 'document_detection', 'face_detection')),
    sequence INTEGER NOT NULL,
    input_kind TEXT NOT NULL CHECK(input_kind IN ('image', 'video_frame')),
    storage_root TEXT NOT NULL CHECK(storage_root IN ('originals', 'previews')),
    file_path TEXT NOT NULL,
    filename TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    byte_size INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    frame_timestamp_ms INTEGER,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (media_id, task, sequence),
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS face_grouping_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    status TEXT NOT NULL CHECK(status IN ('running', 'cancelling', 'completed', 'failed', 'cancelled')),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    completed_at TEXT,
    error TEXT
);

CREATE TABLE IF NOT EXISTS face_group_generations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id INTEGER NOT NULL UNIQUE,
    status TEXT NOT NULL CHECK(status IN ('building', 'active', 'retired', 'cancelled')),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    published_at TEXT,
    FOREIGN KEY (run_id) REFERENCES face_grouping_runs(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS face_group_generation_state (
    id INTEGER PRIMARY KEY CHECK(id = 1),
    active_generation_id INTEGER NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (active_generation_id) REFERENCES face_group_generations(id)
);

CREATE TABLE IF NOT EXISTS face_group_manual_state (
    id INTEGER PRIMARY KEY CHECK(id = 1),
    revision INTEGER NOT NULL CHECK(revision >= 0),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS media_faces (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    media_id INTEGER NOT NULL,
    input_sequence INTEGER NOT NULL,
    face_index INTEGER NOT NULL,
    x REAL NOT NULL,
    y REAL NOT NULL,
    width REAL NOT NULL,
    height REAL NOT NULL,
    confidence REAL NOT NULL CHECK(confidence >= 0.0 AND confidence <= 1.0),
    face_size_score REAL NOT NULL CHECK(face_size_score >= 0.0 AND face_size_score <= 1.0),
    frontality_score REAL NOT NULL CHECK(frontality_score >= 0.0 AND frontality_score <= 1.0),
    visibility_score REAL NOT NULL CHECK(visibility_score >= 0.0 AND visibility_score <= 1.0),
    feature_clarity_score REAL NOT NULL CHECK(feature_clarity_score >= 0.0 AND feature_clarity_score <= 1.0),
    embedding BLOB NOT NULL,
    crop_path TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(media_id, input_sequence, face_index),
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS media_face_detection_results (
    media_id INTEGER PRIMARY KEY,
    model_type TEXT NOT NULL CHECK(model_type = 'face_detection'),
    model_version TEXT NOT NULL,
    completed_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS face_groups (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    representative_face_id INTEGER,
    manual_curated INTEGER NOT NULL DEFAULT 0 CHECK(manual_curated IN (0, 1)),
    automatic_generation_id INTEGER,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (representative_face_id) REFERENCES media_faces(id) ON DELETE SET NULL,
    FOREIGN KEY (automatic_generation_id) REFERENCES face_group_generations(id) ON DELETE CASCADE,
    CHECK(manual_curated = 0 OR automatic_generation_id IS NULL)
);

CREATE TABLE IF NOT EXISTS face_group_members (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    face_group_id INTEGER NOT NULL,
    face_id INTEGER NOT NULL,
    manual_anchor INTEGER NOT NULL CHECK(manual_anchor IN (0, 1)),
    automatic_generation_id INTEGER,
    FOREIGN KEY (face_group_id) REFERENCES face_groups(id) ON DELETE CASCADE,
    FOREIGN KEY (face_id) REFERENCES media_faces(id) ON DELETE CASCADE,
    FOREIGN KEY (automatic_generation_id) REFERENCES face_group_generations(id) ON DELETE CASCADE,
    CHECK(manual_anchor = 0 OR automatic_generation_id IS NULL)
);

CREATE TABLE IF NOT EXISTS face_group_finalizations (
    run_id INTEGER PRIMARY KEY,
    generation_id INTEGER NOT NULL UNIQUE,
    phase TEXT NOT NULL CHECK(phase IN ('face_snapshot', 'manual_snapshot', 'grouping', 'representatives', 'publishing', 'cleanup', 'restart_cleanup')),
    manual_revision INTEGER NOT NULL CHECK(manual_revision >= 0),
    face_snapshot_cursor INTEGER NOT NULL DEFAULT 0,
    manual_snapshot_cursor INTEGER NOT NULL DEFAULT 0,
    face_cursor INTEGER NOT NULL DEFAULT 0,
    current_face_id INTEGER,
    candidate_kind TEXT NOT NULL DEFAULT 'manual' CHECK(candidate_kind IN ('manual', 'automatic')),
    candidate_cursor INTEGER NOT NULL DEFAULT 0,
    best_group_id INTEGER,
    best_similarity REAL,
    group_cursor INTEGER NOT NULL DEFAULT 0,
    current_group_id INTEGER,
    representative_cursor INTEGER NOT NULL DEFAULT 0,
    best_representative_face_id INTEGER,
    best_representative_score REAL,
    completion_error TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (run_id) REFERENCES face_grouping_runs(id) ON DELETE CASCADE,
    FOREIGN KEY (generation_id) REFERENCES face_group_generations(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS face_group_finalization_faces (
    generation_id INTEGER NOT NULL,
    face_id INTEGER NOT NULL,
    embedding BLOB NOT NULL,
    PRIMARY KEY (generation_id, face_id),
    FOREIGN KEY (generation_id) REFERENCES face_group_generations(id) ON DELETE CASCADE,
    FOREIGN KEY (face_id) REFERENCES media_faces(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS face_group_finalization_manual_anchors (
    generation_id INTEGER NOT NULL,
    face_id INTEGER NOT NULL,
    face_group_id INTEGER NOT NULL,
    embedding BLOB NOT NULL,
    PRIMARY KEY (generation_id, face_id),
    FOREIGN KEY (generation_id) REFERENCES face_group_generations(id) ON DELETE CASCADE,
    FOREIGN KEY (face_id) REFERENCES media_faces(id) ON DELETE CASCADE,
    FOREIGN KEY (face_group_id) REFERENCES face_groups(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS face_group_finalization_groups (
    generation_id INTEGER NOT NULL,
    face_group_id INTEGER NOT NULL,
    representative_face_id INTEGER,
    representative_score REAL,
    complete INTEGER NOT NULL DEFAULT 0 CHECK(complete IN (0, 1)),
    PRIMARY KEY (generation_id, face_group_id),
    FOREIGN KEY (generation_id) REFERENCES face_group_generations(id) ON DELETE CASCADE,
    FOREIGN KEY (face_group_id) REFERENCES face_groups(id) ON DELETE CASCADE,
    FOREIGN KEY (representative_face_id) REFERENCES media_faces(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS face_group_representatives (
    generation_id INTEGER NOT NULL,
    face_group_id INTEGER NOT NULL,
    face_id INTEGER NOT NULL,
    PRIMARY KEY (generation_id, face_group_id),
    FOREIGN KEY (generation_id) REFERENCES face_group_generations(id) ON DELETE CASCADE,
    FOREIGN KEY (face_group_id) REFERENCES face_groups(id) ON DELETE CASCADE,
    FOREIGN KEY (face_id) REFERENCES media_faces(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS llm_jobs (
    id TEXT PRIMARY KEY,
    media_id INTEGER NOT NULL,
    deduplicate_run_id INTEGER,
    face_grouping_run_id INTEGER,
    task TEXT NOT NULL CHECK(task IN ('ocr', 'image_tagging', 'image_clustering', 'image_aesthetics', 'screenshot_detection', 'document_detection', 'face_detection')),
    status TEXT NOT NULL CHECK(status IN ('queued', 'submitting', 'submitted', 'completed', 'failed', 'cancelled')),
    state_version INTEGER NOT NULL DEFAULT 1 CHECK(state_version > 0),
    attempts INTEGER NOT NULL DEFAULT 0,
    available_at TEXT NOT NULL DEFAULT (datetime('now')),
    claimed_at TEXT,
    submitted_at TEXT,
    completed_at TEXT,
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE,
    FOREIGN KEY (deduplicate_run_id) REFERENCES media_similarity_runs(id) ON DELETE CASCADE,
    FOREIGN KEY (face_grouping_run_id) REFERENCES face_grouping_runs(id) ON DELETE CASCADE,
    CHECK(
        (task IN ('ocr', 'image_tagging', 'image_aesthetics', 'screenshot_detection', 'document_detection') AND deduplicate_run_id IS NULL AND face_grouping_run_id IS NULL)
        OR (task = 'image_clustering' AND deduplicate_run_id IS NOT NULL AND face_grouping_run_id IS NULL)
        OR (task = 'face_detection' AND deduplicate_run_id IS NULL AND face_grouping_run_id IS NOT NULL)
    )
);

CREATE TABLE IF NOT EXISTS llm_job_inputs (
    job_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    input_kind TEXT NOT NULL CHECK(input_kind IN ('image', 'video_frame')),
    storage_root TEXT NOT NULL CHECK(storage_root IN ('originals', 'previews')),
    file_path TEXT NOT NULL,
    filename TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    byte_size INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    frame_timestamp_ms INTEGER,
    PRIMARY KEY (job_id, sequence),
    FOREIGN KEY (job_id) REFERENCES llm_jobs(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS llm_job_cancellations (
    job_id TEXT PRIMARY KEY,
    task TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS llm_cancellation_scopes (
    scope TEXT NOT NULL CHECK (scope IN ('all', 'task')),
    task TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (scope, task),
    CHECK ((scope = 'all' AND task = '') OR (scope = 'task' AND task <> ''))
);

CREATE TABLE IF NOT EXISTS media_similarity_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    trigger TEXT NOT NULL CHECK(trigger IN ('startup', 'scheduled', 'manual')),
    status TEXT NOT NULL CHECK(status IN ('running', 'cancelling', 'completed', 'failed', 'cancelled', 'interrupted')),
    scheduled_for TEXT,
    started_at TEXT NOT NULL DEFAULT (datetime('now')),
    completed_at TEXT,
    indexed_media INTEGER NOT NULL DEFAULT 0,
    processed_media INTEGER NOT NULL DEFAULT 0,
    candidate_comparisons INTEGER NOT NULL DEFAULT 0,
    clusters_created INTEGER NOT NULL DEFAULT 0,
    error TEXT
);

CREATE TABLE IF NOT EXISTS media_similarity_index (
    media_id INTEGER PRIMARY KEY,
    content_hash TEXT NOT NULL,
    model_version TEXT NOT NULL,
    preprocessing_version TEXT NOT NULL,
    embedding BLOB NOT NULL,
    perceptual_hash INTEGER NOT NULL,
    capture_time_seconds INTEGER,
    indexed_at TEXT NOT NULL DEFAULT (datetime('now')),
    processing_status INTEGER NOT NULL DEFAULT 1 CHECK(processing_status IN (-1, 1)),
    processing_error TEXT,
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS media_similarity_hash_bands (
    media_id INTEGER NOT NULL,
    band_index INTEGER NOT NULL,
    band_value INTEGER NOT NULL,
    PRIMARY KEY (media_id, band_index),
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS media_similarity_dirty (
    media_id INTEGER PRIMARY KEY,
    marked_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS media_similarity_generations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id INTEGER UNIQUE,
    status TEXT NOT NULL CHECK(status IN ('building', 'active', 'retiring')),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    published_at TEXT,
    FOREIGN KEY (run_id) REFERENCES media_similarity_runs(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS media_similarity_generation_state (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    active_generation_id INTEGER NOT NULL,
    FOREIGN KEY (active_generation_id) REFERENCES media_similarity_generations(id)
);

CREATE TABLE IF NOT EXISTS media_similarity_finalizations (
    run_id INTEGER PRIMARY KEY,
    generation_id INTEGER NOT NULL UNIQUE,
    phase TEXT NOT NULL CHECK(phase IN ('dirty_snapshot', 'comparison', 'label_initialization', 'label_propagation', 'grouping', 'publishing', 'cleanup')),
    source_media_id INTEGER,
    source_cursor INTEGER NOT NULL DEFAULT 0,
    candidate_kind TEXT NOT NULL DEFAULT 'near_duplicate' CHECK(candidate_kind IN ('near_duplicate', 'burst')),
    candidate_cursor INTEGER NOT NULL DEFAULT 0,
    label_kind TEXT NOT NULL DEFAULT 'near_duplicate' CHECK(label_kind IN ('near_duplicate', 'burst')),
    label_media_cursor INTEGER NOT NULL DEFAULT 0,
    label_edge_left_cursor INTEGER NOT NULL DEFAULT 0,
    label_edge_right_cursor INTEGER NOT NULL DEFAULT 0,
    label_pass_changed INTEGER NOT NULL DEFAULT 0 CHECK(label_pass_changed IN (0, 1)),
    group_kind TEXT NOT NULL DEFAULT 'near_duplicate' CHECK(group_kind IN ('near_duplicate', 'burst')),
    group_label_cursor INTEGER NOT NULL DEFAULT 0,
    group_member_cursor INTEGER NOT NULL DEFAULT 0,
    group_cluster_id INTEGER,
    dirty_cursor INTEGER NOT NULL DEFAULT 0,
    completion_error TEXT,
    FOREIGN KEY (run_id) REFERENCES media_similarity_runs(id) ON DELETE CASCADE,
    FOREIGN KEY (generation_id) REFERENCES media_similarity_generations(id) ON DELETE CASCADE,
    FOREIGN KEY (source_media_id) REFERENCES media_similarity_index(media_id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS media_similarity_finalization_dirty (
    run_id INTEGER NOT NULL,
    media_id INTEGER NOT NULL,
    marked_at TEXT NOT NULL,
    PRIMARY KEY (run_id, media_id),
    FOREIGN KEY (run_id) REFERENCES media_similarity_runs(id) ON DELETE CASCADE,
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS media_similarity_edges (
    run_id INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('near_duplicate', 'burst')),
    left_media_id INTEGER NOT NULL,
    right_media_id INTEGER NOT NULL,
    cosine_similarity REAL NOT NULL,
    perceptual_hash_distance INTEGER NOT NULL,
    PRIMARY KEY (run_id, kind, left_media_id, right_media_id),
    CHECK(left_media_id < right_media_id),
    FOREIGN KEY (run_id) REFERENCES media_similarity_runs(id) ON DELETE CASCADE,
    FOREIGN KEY (left_media_id) REFERENCES media(id) ON DELETE CASCADE,
    FOREIGN KEY (right_media_id) REFERENCES media(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS media_similarity_labels (
    run_id INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('near_duplicate', 'burst')),
    media_id INTEGER NOT NULL,
    component_label INTEGER NOT NULL,
    PRIMARY KEY (run_id, kind, media_id),
    FOREIGN KEY (run_id) REFERENCES media_similarity_runs(id) ON DELETE CASCADE,
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS media_similarity_clusters (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    generation_id INTEGER,
    kind TEXT NOT NULL CHECK(kind IN ('near_duplicate', 'burst')),
    representative_media_id INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (generation_id) REFERENCES media_similarity_generations(id) ON DELETE CASCADE,
    FOREIGN KEY (representative_media_id) REFERENCES media(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS media_similarity_cluster_members (
    cluster_id INTEGER NOT NULL,
    media_id INTEGER NOT NULL,
    cosine_similarity REAL NOT NULL,
    perceptual_hash_distance INTEGER NOT NULL,
    PRIMARY KEY (cluster_id, media_id),
    FOREIGN KEY (cluster_id) REFERENCES media_similarity_clusters(id) ON DELETE CASCADE,
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
);

CREATE TRIGGER IF NOT EXISTS delete_media_rtree_after_media_delete
    AFTER DELETE ON media
BEGIN
    DELETE FROM media_rtree
     WHERE media_id = OLD.id;
END;

CREATE TRIGGER IF NOT EXISTS prevent_reserved_admin_deactivation
    BEFORE UPDATE OF is_active ON users
    WHEN OLD.username = 'admin'
     AND NEW.is_active = 0
BEGIN
    SELECT RAISE(ABORT, 'the reserved admin account cannot be deactivated');
END;

CREATE TRIGGER IF NOT EXISTS prevent_reserved_admin_deletion
    BEFORE DELETE ON users
    WHEN OLD.username = 'admin'
BEGIN
    SELECT RAISE(ABORT, 'the reserved admin account cannot be deleted');
END;

CREATE TRIGGER IF NOT EXISTS mark_similarity_dirty_after_media_insert
    AFTER INSERT ON media
BEGIN
    INSERT INTO media_similarity_dirty (media_id, marked_at)
    VALUES (NEW.id, datetime('now'))
    ON CONFLICT(media_id) DO UPDATE SET marked_at = excluded.marked_at;
END;

CREATE TRIGGER IF NOT EXISTS mark_similarity_dirty_after_metadata_insert
    AFTER INSERT ON media_metadata
BEGIN
    INSERT INTO media_similarity_dirty (media_id, marked_at)
    VALUES (NEW.media_id, datetime('now'))
    ON CONFLICT(media_id) DO UPDATE SET marked_at = excluded.marked_at;
END;

CREATE TRIGGER IF NOT EXISTS mark_similarity_dirty_after_metadata_update
    AFTER UPDATE ON media_metadata
BEGIN
    INSERT INTO media_similarity_dirty (media_id, marked_at)
    VALUES (NEW.media_id, datetime('now'))
    ON CONFLICT(media_id) DO UPDATE SET marked_at = excluded.marked_at;
END;

CREATE TRIGGER IF NOT EXISTS mark_similarity_dirty_after_access_insert
    AFTER INSERT ON media_access
BEGIN
    INSERT INTO media_similarity_dirty (media_id, marked_at)
    VALUES (NEW.media_id, datetime('now'))
    ON CONFLICT(media_id) DO UPDATE SET marked_at = excluded.marked_at;
END;

CREATE TRIGGER IF NOT EXISTS mark_similarity_dirty_after_access_update
    AFTER UPDATE ON media_access
BEGIN
    INSERT INTO media_similarity_dirty (media_id, marked_at)
    VALUES (NEW.media_id, datetime('now'))
    ON CONFLICT(media_id) DO UPDATE SET marked_at = excluded.marked_at;
END;

CREATE TRIGGER IF NOT EXISTS remove_similarity_clusters_before_media_delete
    BEFORE DELETE ON media
BEGIN
    INSERT INTO media_similarity_dirty (media_id, marked_at)
    SELECT members.media_id
         , datetime('now')
      FROM media_similarity_cluster_members AS deleted_member
      JOIN media_similarity_cluster_members AS members
        ON members.cluster_id = deleted_member.cluster_id
     WHERE deleted_member.media_id = OLD.id
       AND members.media_id <> OLD.id
    ON CONFLICT(media_id) DO UPDATE SET marked_at = excluded.marked_at;

    DELETE FROM media_similarity_clusters
     WHERE id IN (
        SELECT cluster_id
          FROM media_similarity_cluster_members
         WHERE media_id = OLD.id
     );
END;

CREATE INDEX IF NOT EXISTS idx_media_pagination
    ON media_metadata (date_taken DESC, media_id DESC);

CREATE INDEX IF NOT EXISTS idx_media_gps
    ON media_metadata (gps_latitude, gps_longitude)
    WHERE gps_latitude IS NOT NULL
      AND gps_longitude IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_media_file_path
    ON media (file_path);

CREATE UNIQUE INDEX IF NOT EXISTS idx_media_content_hash
    ON media (content_hash)
    WHERE content_hash IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_media_import_state
    ON media (import_state, id);

CREATE INDEX IF NOT EXISTS idx_media_metadata_jobs_claim
    ON media_metadata_jobs (status, available_at, media_id);

CREATE UNIQUE INDEX IF NOT EXISTS idx_import_jobs_single_running
    ON import_jobs ((1))
    WHERE status = 'running';

CREATE INDEX IF NOT EXISTS idx_media_ai_inputs_media_task
    ON media_ai_inputs (media_id, task, sequence);

CREATE INDEX IF NOT EXISTS idx_llm_jobs_claim
    ON llm_jobs (status, available_at, created_at);

CREATE INDEX IF NOT EXISTS idx_llm_job_inputs_job
    ON llm_job_inputs (job_id, sequence);

CREATE UNIQUE INDEX IF NOT EXISTS idx_llm_jobs_active_media_task
    ON llm_jobs (media_id, task)
    WHERE task IN ('ocr', 'image_tagging', 'image_aesthetics', 'screenshot_detection', 'document_detection', 'face_detection')
      AND status IN ('queued', 'submitting', 'submitted');

CREATE UNIQUE INDEX IF NOT EXISTS idx_llm_jobs_active_clustering
    ON llm_jobs (deduplicate_run_id, media_id)
    WHERE task = 'image_clustering'
      AND status IN ('queued', 'submitting', 'submitted');

CREATE INDEX IF NOT EXISTS idx_albums_user
    ON albums (user_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_album_media_order
    ON album_media (album_id, position);

CREATE INDEX IF NOT EXISTS idx_share_token
    ON share_links (token);

CREATE INDEX IF NOT EXISTS idx_refresh_tokens_user
    ON refresh_tokens (user_id, revoked);

CREATE INDEX IF NOT EXISTS idx_refresh_tokens_cleanup
    ON refresh_tokens (revoked, expires_at);

CREATE INDEX IF NOT EXISTS idx_auth_attempt_buckets_expiry
    ON auth_attempt_buckets (last_seen_at, locked_until);

CREATE INDEX IF NOT EXISTS idx_media_access_user_deleted
    ON media_access (user_id, deleted_at)
    WHERE deleted_at IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_media_access_user_active
    ON media_access (user_id, media_id)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_media_access_media
    ON media_access (media_id);

CREATE INDEX IF NOT EXISTS idx_album_access_user
    ON album_access (user_id);

CREATE INDEX IF NOT EXISTS idx_media_geohash
    ON media_metadata (geohash);

CREATE INDEX IF NOT EXISTS idx_media_metadata_places
    ON media_metadata (location_city, location_state, location_country, media_id);

CREATE UNIQUE INDEX IF NOT EXISTS idx_media_similarity_single_running
    ON media_similarity_runs ((1))
    WHERE status IN ('running', 'cancelling');

CREATE INDEX IF NOT EXISTS idx_media_similarity_index_versions
    ON media_similarity_index (content_hash, model_version, preprocessing_version);

CREATE INDEX IF NOT EXISTS idx_media_similarity_bands_lookup
    ON media_similarity_hash_bands (band_index, band_value, media_id);

CREATE INDEX IF NOT EXISTS idx_media_similarity_capture_time
    ON media_similarity_index (capture_time_seconds, media_id);

CREATE INDEX IF NOT EXISTS idx_media_similarity_members_media
    ON media_similarity_cluster_members (media_id, cluster_id);

CREATE INDEX IF NOT EXISTS idx_media_similarity_members_cluster
    ON media_similarity_cluster_members (cluster_id, media_id);

CREATE INDEX IF NOT EXISTS idx_media_similarity_clusters_generation
    ON media_similarity_clusters (generation_id, kind, representative_media_id);

CREATE INDEX IF NOT EXISTS idx_media_similarity_edges_page
    ON media_similarity_edges (run_id, kind, left_media_id, right_media_id);

CREATE INDEX IF NOT EXISTS idx_media_similarity_labels_component
    ON media_similarity_labels (run_id, kind, component_label, media_id);

CREATE INDEX IF NOT EXISTS idx_media_similarity_generations_status
    ON media_similarity_generations (status, id);

CREATE INDEX IF NOT EXISTS idx_media_faces_media
    ON media_faces (media_id, input_sequence, face_index);

CREATE UNIQUE INDEX IF NOT EXISTS idx_face_grouping_single_active
    ON face_grouping_runs ((1))
    WHERE status IN ('running', 'cancelling');

CREATE INDEX IF NOT EXISTS idx_face_group_members_face
    ON face_group_members (face_id, face_group_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_face_group_members_manual_unique
    ON face_group_members (face_id)
 WHERE manual_anchor = 1;
CREATE UNIQUE INDEX IF NOT EXISTS idx_face_group_members_automatic_unique
    ON face_group_members (automatic_generation_id, face_id)
 WHERE manual_anchor = 0;
CREATE INDEX IF NOT EXISTS idx_face_groups_generation
    ON face_groups (automatic_generation_id, id);
CREATE INDEX IF NOT EXISTS idx_face_group_members_generation
    ON face_group_members (automatic_generation_id, face_group_id, face_id);
CREATE INDEX IF NOT EXISTS idx_face_group_finalization_manual_page
    ON face_group_finalization_manual_anchors (generation_id, face_id);
CREATE INDEX IF NOT EXISTS idx_face_group_finalization_group_page
    ON face_group_finalization_groups (generation_id, complete, face_group_id);

CREATE TABLE IF NOT EXISTS file_operation_groups (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    owner_kind TEXT NOT NULL,
    owner_id TEXT NOT NULL,
    claim_token TEXT CHECK(claim_token IS NULL OR length(claim_token) = 36),
    state TEXT NOT NULL CHECK(state IN (
        'prepared', 'publishing', 'publication_failed', 'files_committed',
        'finalize_failed', 'completed', 'rollback_pending', 'rolled_back',
        'cleanup_pending', 'cleanup_failed', 'cleaned'
    )),
    product_target TEXT,
    product_version INTEGER,
    cancel_requested INTEGER NOT NULL DEFAULT 0 CHECK(cancel_requested IN (0, 1)),
    completion_outcome TEXT CHECK(completion_outcome IN ('published', 'discarded')),
    finalization_error_kind TEXT,
    finalization_error TEXT,
    rollback_error_kind TEXT,
    rollback_error TEXT,
    detail_level TEXT NOT NULL DEFAULT 'full' CHECK(detail_level IN ('full', 'compacted')),
    entry_action_summary TEXT,
    entry_state_summary TEXT,
    cleanup_summary TEXT,
    entry_count INTEGER NOT NULL CHECK(entry_count BETWEEN 1 AND 256),
    version INTEGER NOT NULL DEFAULT 1 CHECK(version > 0),
    recovery_order INTEGER NOT NULL DEFAULT 1 CHECK(recovery_order > 0),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    terminal_at TEXT
);

CREATE TABLE IF NOT EXISTS file_operation_entries (
    group_id TEXT NOT NULL REFERENCES file_operation_groups(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL CHECK(sequence BETWEEN 0 AND 255),
    action TEXT NOT NULL CHECK(action IN ('publish', 'move', 'tombstone', 'cleanup')),
    storage_root TEXT NOT NULL CHECK(storage_root IN (
        'originals', 'thumbnails', 'tiny_thumbnails', 'place_thumbnails', 'previews',
        'imports', 'albums', 'trash', 'webdav', 'backups', 'logs', 'journal', 'static'
    )),
    source_path TEXT,
    temporary_path TEXT,
    destination_path TEXT,
    tombstone_path TEXT,
    expected_size INTEGER CHECK(expected_size IS NULL OR expected_size >= 0),
    expected_sha256 BLOB CHECK(expected_sha256 IS NULL OR length(expected_sha256) = 32),
    expected_version TEXT,
    state TEXT NOT NULL DEFAULT 'prepared' CHECK(state IN ('prepared', 'committed', 'rolled_back')),
    cleanup_state TEXT NOT NULL DEFAULT 'pending' CHECK(cleanup_state IN ('pending', 'cleaned', 'failed')),
    last_error_kind TEXT,
    last_error TEXT,
    PRIMARY KEY (group_id, sequence)
);

CREATE TABLE IF NOT EXISTS file_operation_path_claims (
    group_id TEXT NOT NULL REFERENCES file_operation_groups(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL CHECK(sequence BETWEEN 0 AND 511),
    storage_root TEXT NOT NULL,
    relative_path TEXT NOT NULL,
    path_key BLOB NOT NULL CHECK(length(path_key) BETWEEN 1 AND 5120),
    mode TEXT NOT NULL CHECK(mode IN ('read', 'write')),
    scope TEXT NOT NULL CHECK(scope IN ('exact', 'subtree')),
    role TEXT NOT NULL,
    expected_version TEXT,
    PRIMARY KEY (group_id, sequence)
);

CREATE TABLE IF NOT EXISTS directory_copy_constructions (
    group_id TEXT PRIMARY KEY REFERENCES file_operation_groups(id) ON DELETE CASCADE,
    storage_root TEXT NOT NULL CHECK(storage_root = 'webdav'),
    source_root TEXT NOT NULL,
    temporary_root TEXT NOT NULL,
    expected_file_bytes INTEGER NOT NULL CHECK(expected_file_bytes >= 0),
    expected_entry_count INTEGER NOT NULL CHECK(expected_entry_count >= 0),
    expected_fingerprint BLOB NOT NULL CHECK(length(expected_fingerprint) = 32),
    copied_file_bytes INTEGER NOT NULL DEFAULT 0 CHECK(copied_file_bytes >= 0),
    copied_entry_count INTEGER NOT NULL DEFAULT 0 CHECK(copied_entry_count >= 0),
    copied_fingerprint BLOB NOT NULL DEFAULT (zeroblob(32)) CHECK(length(copied_fingerprint) = 32),
    state TEXT NOT NULL DEFAULT 'building' CHECK(state IN ('building', 'complete')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS directory_copy_cursors (
    group_id TEXT NOT NULL REFERENCES directory_copy_constructions(group_id) ON DELETE CASCADE,
    depth INTEGER NOT NULL CHECK(depth BETWEEN 0 AND 255),
    source_path TEXT NOT NULL,
    temporary_path TEXT NOT NULL,
    resume_offset INTEGER NOT NULL DEFAULT 0 CHECK(resume_offset >= 0),
    PRIMARY KEY (group_id, depth)
);

CREATE INDEX IF NOT EXISTS idx_directory_copy_constructions_state
    ON directory_copy_constructions (state, group_id);

CREATE TABLE IF NOT EXISTS data_dir_space_reservations (
    id TEXT PRIMARY KEY,
    class TEXT NOT NULL CHECK(class IN ('journal', 'sqlite', 'log')),
    owner_kind TEXT NOT NULL,
    owner_id TEXT NOT NULL,
    journal_group_id TEXT REFERENCES file_operation_groups(id) ON DELETE CASCADE,
    filesystem_id TEXT NOT NULL,
    reserved_peak_additional_bytes INTEGER NOT NULL CHECK(reserved_peak_additional_bytes >= 0),
    newly_allocated_blocks INTEGER NOT NULL DEFAULT 0 CHECK(newly_allocated_blocks >= 0),
    state TEXT NOT NULL CHECK(state IN ('provisional', 'active', 'releasing', 'released')),
    version INTEGER NOT NULL DEFAULT 1 CHECK(version > 0),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (class, owner_kind, owner_id)
);

CREATE TABLE IF NOT EXISTS file_operation_retry_requests (
    retry_request_id TEXT PRIMARY KEY,
    group_id TEXT NOT NULL REFERENCES file_operation_groups(id) ON DELETE CASCADE,
    expected_version INTEGER NOT NULL CHECK(expected_version > 0),
    request_hash BLOB NOT NULL CHECK(length(request_hash) = 32),
    response_state TEXT NOT NULL,
    response_version INTEGER NOT NULL CHECK(response_version > 0),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS llm_result_receipts (
    job_id TEXT PRIMARY KEY REFERENCES llm_jobs(id) ON DELETE CASCADE,
    attempt INTEGER NOT NULL CHECK(attempt > 0),
    job_version INTEGER NOT NULL CHECK(job_version > 0),
    media_id INTEGER NOT NULL REFERENCES media(id) ON DELETE CASCADE,
    task TEXT NOT NULL CHECK(task IN (
        'ocr', 'image_tagging', 'image_clustering', 'image_aesthetics',
        'screenshot_detection', 'document_detection', 'face_detection'
    )),
    result_status TEXT NOT NULL CHECK(result_status IN ('completed', 'failed')),
    model_type TEXT CHECK(model_type IS NULL OR length(model_type) BETWEEN 1 AND 63),
    model_version TEXT CHECK(model_version IS NULL OR length(model_version) BETWEEN 1 AND 255),
    encoding TEXT NOT NULL CHECK(encoding = 'momento-result-records-v1'),
    record_count INTEGER NOT NULL CHECK(record_count BETWEEN 1 AND 1000000),
    byte_size INTEGER NOT NULL CHECK(byte_size BETWEEN 24 AND 1073741824),
    content_hash TEXT NOT NULL CHECK(
        length(content_hash) = 64
        AND content_hash = lower(content_hash)
        AND content_hash NOT GLOB '*[^0-9a-f]*'
    ),
    journal_group_id TEXT NOT NULL UNIQUE REFERENCES file_operation_groups(id) ON DELETE RESTRICT,
    sqlite_reservation_id TEXT NOT NULL UNIQUE REFERENCES data_dir_space_reservations(id) ON DELETE RESTRICT,
    inbox_path TEXT NOT NULL CHECK(length(inbox_path) BETWEEN 1 AND 4096),
    receive_token TEXT NOT NULL CHECK(length(receive_token) = 36),
    state TEXT NOT NULL CHECK(state IN (
        'receiving', 'received', 'processing', 'discarded', 'cleanup_pending',
        'file_cleanup_pending', 'cleaned', 'failed'
    )),
    claim_token TEXT CHECK(claim_token IS NULL OR length(claim_token) = 36),
    next_record_sequence INTEGER NOT NULL DEFAULT 0 CHECK(
        next_record_sequence BETWEEN 0 AND record_count
    ),
    next_byte_offset INTEGER NOT NULL DEFAULT 0 CHECK(
        next_byte_offset BETWEEN 0 AND byte_size
    ),
    result_product_version INTEGER NOT NULL CHECK(result_product_version > 0),
    cancel_requested INTEGER NOT NULL DEFAULT 0 CHECK(cancel_requested IN (0, 1)),
    last_error TEXT CHECK(last_error IS NULL OR length(last_error) <= 4096),
    received_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(job_id, attempt),
    CHECK(
        (result_status = 'completed' AND model_type IS NOT NULL AND model_version IS NOT NULL)
        OR (result_status = 'failed' AND model_type IS NULL AND model_version IS NULL)
    ),
    CHECK(
        (state = 'processing' AND claim_token IS NOT NULL)
        OR (state != 'processing' AND claim_token IS NULL)
    ),
    CHECK((state = 'received' AND received_at IS NOT NULL) OR state != 'received')
);

CREATE TABLE IF NOT EXISTS llm_result_staging (
    job_id TEXT NOT NULL REFERENCES llm_result_receipts(job_id) ON DELETE CASCADE,
    attempt INTEGER NOT NULL CHECK(attempt > 0),
    record_sequence INTEGER NOT NULL CHECK(record_sequence BETWEEN 0 AND 999999),
    input_sequence INTEGER CHECK(input_sequence IS NULL OR input_sequence BETWEEN 0 AND 4294967295),
    kind TEXT NOT NULL CHECK(kind IN (
        'failure', 'input_started', 'ocr_text', 'image_tags', 'image_clustering',
        'image_aesthetics', 'face', 'screenshot_detection', 'document_detection',
        'input_finished', 'ocr_text_continuation', 'image_tags_continuation'
    )),
    byte_offset INTEGER NOT NULL CHECK(byte_offset >= 0),
    encoded_size INTEGER NOT NULL CHECK(encoded_size BETWEEN 24 AND 1048600),
    normalized_payload BLOB NOT NULL CHECK(length(normalized_payload) <= 1048576),
    PRIMARY KEY (job_id, attempt, record_sequence),
    FOREIGN KEY (job_id, attempt) REFERENCES llm_result_receipts(job_id, attempt) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_llm_result_receipts_state
    ON llm_result_receipts (state, updated_at, job_id);

CREATE INDEX IF NOT EXISTS idx_llm_result_staging_input
    ON llm_result_staging (job_id, attempt, input_sequence, record_sequence);

CREATE TRIGGER IF NOT EXISTS trg_llm_result_receipt_cleanup_completed
AFTER UPDATE OF state ON file_operation_groups
WHEN NEW.kind = 'llm_result_receive' AND NEW.state = 'cleaned'
BEGIN
    UPDATE llm_result_receipts
       SET state = 'cleaned', updated_at = datetime('now')
     WHERE journal_group_id = NEW.id AND state = 'file_cleanup_pending';
END;

CREATE TRIGGER IF NOT EXISTS trg_llm_result_receipt_group_cancelled
AFTER UPDATE OF cancel_requested ON file_operation_groups
WHEN NEW.kind = 'llm_result_receive' AND NEW.cancel_requested = 1
BEGIN
    UPDATE llm_result_receipts
       SET state = 'discarded', cancel_requested = 1, claim_token = NULL,
           updated_at = datetime('now')
     WHERE journal_group_id = NEW.id
       AND state IN ('receiving', 'received', 'processing');
END;

CREATE INDEX IF NOT EXISTS idx_file_operation_groups_state
    ON file_operation_groups (state, recovery_order, id);

CREATE INDEX IF NOT EXISTS idx_file_operation_groups_owner
    ON file_operation_groups (owner_kind, owner_id, state);

CREATE INDEX IF NOT EXISTS idx_file_operation_path_claims_path
    ON file_operation_path_claims (storage_root, path_key, mode, scope, group_id);

CREATE INDEX IF NOT EXISTS idx_file_operation_path_claims_group
    ON file_operation_path_claims (group_id, sequence);

CREATE INDEX IF NOT EXISTS idx_data_dir_space_reservations_state
    ON data_dir_space_reservations (class, state, filesystem_id);

CREATE INDEX IF NOT EXISTS idx_file_operation_retry_requests_group
    ON file_operation_retry_requests (group_id, expires_at, retry_request_id);

CREATE INDEX IF NOT EXISTS idx_file_operation_retry_requests_expiry
    ON file_operation_retry_requests (expires_at, retry_request_id);
