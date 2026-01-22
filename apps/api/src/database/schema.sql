CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT
  , username TEXT UNIQUE NOT NULL
  , email TEXT UNIQUE NOT NULL
  , hashed_password TEXT NOT NULL
  , role TEXT CHECK(role IN ('admin', 'user')) DEFAULT 'user'
  , must_change_password INTEGER DEFAULT 1
  , is_active INTEGER DEFAULT 1
  , created_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS media (
    id INTEGER PRIMARY KEY AUTOINCREMENT
  , filename TEXT NOT NULL
  , original_filename TEXT NOT NULL
  , file_path TEXT NOT NULL
  , thumbnail_path TEXT
  , media_type TEXT CHECK(media_type IN ('image', 'video')) NOT NULL
  , mime_type TEXT
  , width INTEGER
  , height INTEGER
  , file_size INTEGER
  , duration_seconds REAL
  , date_taken TEXT
  , gps_latitude REAL
  , gps_longitude REAL
  , camera_make TEXT
  , camera_model TEXT
  , lens_make TEXT
  , lens_model TEXT
  , iso INTEGER
  , exposure_time TEXT
  , f_number REAL
  , focal_length REAL
  , focal_length_35mm REAL
  , gps_altitude REAL
  , location_state TEXT
  , location_country TEXT
  , location_city TEXT
  , video_codec TEXT
  , focal_length_35mm REAL
  , keywords TEXT
  , content_hash TEXT UNIQUE
  , created_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS albums (
    id INTEGER PRIMARY KEY AUTOINCREMENT
  , user_id INTEGER NOT NULL
  , name TEXT NOT NULL
  , description TEXT
  , cover_media_id INTEGER
  , created_at TEXT DEFAULT (datetime('now'))
  , FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
  , FOREIGN KEY (cover_media_id) REFERENCES media(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS album_media (
    album_id INTEGER NOT NULL
  , media_id INTEGER NOT NULL
  , position INTEGER DEFAULT 0
  , added_at TEXT DEFAULT (datetime('now'))
  , PRIMARY KEY (album_id, media_id)
  , FOREIGN KEY (album_id) REFERENCES albums(id) ON DELETE CASCADE
  , FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT
  , name TEXT UNIQUE NOT NULL
  , created_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS media_tags (
    media_id INTEGER NOT NULL
  , tag_id INTEGER NOT NULL
  , PRIMARY KEY (media_id, tag_id)
  , FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
  , FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS share_links (
    id INTEGER PRIMARY KEY AUTOINCREMENT
  , user_id INTEGER NOT NULL
  , media_id INTEGER
  , album_id INTEGER
  , token TEXT UNIQUE NOT NULL
  , password_hash TEXT
  , expires_at TEXT
  , view_count INTEGER DEFAULT 0
  , created_at TEXT DEFAULT (datetime('now'))
  , FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
  , FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
  , FOREIGN KEY (album_id) REFERENCES albums(id) ON DELETE CASCADE
  , CHECK (
      (media_id IS NOT NULL AND album_id IS NULL) OR
      (media_id IS NULL AND album_id IS NOT NULL)
    )
);

CREATE TABLE IF NOT EXISTS refresh_tokens (
    id INTEGER PRIMARY KEY AUTOINCREMENT
  , token_hash TEXT UNIQUE NOT NULL
  , user_id INTEGER NOT NULL
  , expires_at TEXT NOT NULL
  , revoked INTEGER DEFAULT 0
  , created_at TEXT DEFAULT (datetime('now'))
  , FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS media_access (
    media_id INTEGER NOT NULL
  , user_id INTEGER NOT NULL
  , access_level INTEGER NOT NULL
  , created_at TEXT DEFAULT (datetime('now'))
  , deleted_at TEXT DEFAULT NULL
  , PRIMARY KEY (media_id, user_id)
  , FOREIGN KEY (media_id) REFERENCES media(id) ON DELETE CASCADE
  , FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS album_access (
    album_id INTEGER NOT NULL
  , user_id INTEGER NOT NULL
  , access_level INTEGER NOT NULL
  , created_at TEXT DEFAULT (datetime('now'))
  , PRIMARY KEY (album_id, user_id)
  , FOREIGN KEY (album_id) REFERENCES albums(id) ON DELETE CASCADE
  , FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_media_pagination 
ON media (date_taken DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_media_date_taken 
ON media (date_taken DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_media_gps 
ON media (gps_latitude, gps_longitude) 
WHERE gps_latitude IS NOT NULL AND gps_longitude IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_media_file_path 
ON media (file_path);

CREATE INDEX IF NOT EXISTS idx_media_content_hash 
ON media (content_hash) 
WHERE content_hash IS NOT NULL;


CREATE INDEX IF NOT EXISTS idx_albums_user 
ON albums (user_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_album_media_order 
ON album_media (album_id, position);

CREATE INDEX IF NOT EXISTS idx_tags_name 
ON tags (name);

CREATE INDEX IF NOT EXISTS idx_media_tags_tag 
ON media_tags (tag_id);

CREATE INDEX IF NOT EXISTS idx_share_token 
ON share_links (token);

CREATE INDEX IF NOT EXISTS idx_refresh_tokens_user
ON refresh_tokens (user_id, revoked);

CREATE INDEX IF NOT EXISTS idx_media_access_user_deleted
ON media_access (user_id, deleted_at)
WHERE deleted_at IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_media_access_media
ON media_access (media_id);

CREATE INDEX IF NOT EXISTS idx_album_access_user 
ON album_access(user_id);
