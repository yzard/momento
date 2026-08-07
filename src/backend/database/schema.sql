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
    content_hash TEXT UNIQUE,
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

CREATE INDEX IF NOT EXISTS idx_media_content_hash
    ON media (content_hash)
    WHERE content_hash IS NOT NULL;

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

INSERT OR IGNORE INTO media_similarity_dirty (media_id)
SELECT media.id
  FROM media
  LEFT JOIN media_similarity_index ON media_similarity_index.media_id = media.id
 WHERE media_similarity_index.media_id IS NULL;
