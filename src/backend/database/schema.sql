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

CREATE TABLE IF NOT EXISTS media_metadata (
    media_id INTEGER PRIMARY KEY,
    thumbnail_path TEXT,
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

CREATE INDEX IF NOT EXISTS idx_backup_upload_sessions_user_active
    ON backup_upload_sessions (user_id, status, expires_at);

CREATE INDEX IF NOT EXISTS idx_backup_assets_claim
    ON backup_assets (status, id);

CREATE TABLE IF NOT EXISTS media_access (
    media_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    access_level INTEGER NOT NULL,
    created_at TEXT DEFAULT (datetime('now')),
    deleted_at TEXT DEFAULT NULL,
    PRIMARY KEY (media_id, user_id),
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS album_access (
    album_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    access_level INTEGER NOT NULL,
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
    attempts INTEGER NOT NULL DEFAULT 0,
    rerun_requested INTEGER NOT NULL DEFAULT 0 CHECK(rerun_requested IN (0, 1)),
    available_at TEXT NOT NULL DEFAULT (datetime('now')),
    claimed_at TEXT,
    completed_at TEXT,
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
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

CREATE TABLE IF NOT EXISTS media_faces (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    media_id INTEGER NOT NULL,
    input_sequence INTEGER NOT NULL,
    face_index INTEGER NOT NULL,
    x REAL NOT NULL,
    y REAL NOT NULL,
    width REAL NOT NULL,
    height REAL NOT NULL,
    confidence REAL NOT NULL,
    quality REAL NOT NULL,
    frontality REAL NOT NULL CHECK(frontality >= 0.0 AND frontality <= 1.0),
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
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (representative_face_id) REFERENCES media_faces(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS face_group_members (
    face_group_id INTEGER NOT NULL,
    face_id INTEGER NOT NULL,
    PRIMARY KEY (face_group_id, face_id),
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

CREATE TABLE IF NOT EXISTS media_similarity_clusters (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL CHECK(kind IN ('near_duplicate', 'burst')),
    representative_media_id INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
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

CREATE INDEX IF NOT EXISTS idx_media_faces_media
    ON media_faces (media_id, input_sequence, face_index);

CREATE UNIQUE INDEX IF NOT EXISTS idx_face_grouping_single_active
    ON face_grouping_runs ((1))
    WHERE status IN ('running', 'cancelling');

CREATE INDEX IF NOT EXISTS idx_face_group_members_face
    ON face_group_members (face_id, face_group_id);
