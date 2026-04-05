pub mod schema {
    pub const TABLE_EXISTS: &str = r#"
    SELECT COUNT(*)
      FROM sqlite_master
     WHERE type = 'table'
       AND name = ?
    "#;
}

pub mod media {
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
    "#;

    pub const INSERT_METADATA: &str = r#"
    INSERT INTO media_metadata (
        media_id
      , thumbnail_path
      , width
      , height
      , duration_seconds
      , date_taken
      , gps_latitude
      , gps_longitude
      , gps_altitude
      , geohash
      , location_city
      , location_state
      , location_country
      , camera_make
      , camera_model
      , lens_make
      , lens_model
      , iso
      , exposure_time
      , f_number
      , focal_length
      , focal_length_35mm
      , video_codec
      , keywords
    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    "#;

    pub const SELECT_BY_CONTENT_HASH: &str = r#"
    SELECT id
      FROM media
     WHERE content_hash = ?
     LIMIT 1
    "#;

    pub const SELECT_ALL_FOR_USER: &str = r#"
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
     ORDER BY mm.date_taken DESC, m.id DESC
    "#;

    pub const SELECT_PAGINATED_FOR_USER: &str = r#"
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
       AND (mm.date_taken < ? OR (mm.date_taken = ? AND m.id < ?))
     ORDER BY mm.date_taken DESC, m.id DESC
     LIMIT ?
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

    pub const SELECT_THUMBNAIL_BATCH: &str = r#"
    SELECT m.id
         , mm.thumbnail_path
         , m.file_path
         , m.media_type
         , ma.user_id
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
      LEFT JOIN media_metadata AS mm ON m.id = mm.media_id
     WHERE ma.user_id = ?
    "#;

    pub const SELECT_PREVIEW_BATCH: &str = r#"
    SELECT m.id
         , m.file_path
         , m.media_type
         , m.mime_type
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
     WHERE ma.user_id = ?
    "#;

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
        let placeholders = (0..count).map(|_| "?").collect::<Vec<_>>().join(", ");

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
}

pub mod timeline {
    pub const SELECT_DEFAULT: &str = r#"
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
     ORDER BY mm.date_taken DESC, m.id DESC
     LIMIT ?
    "#;

    pub const SELECT_PAGINATED: &str = r#"
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
       AND (mm.date_taken < ? OR (mm.date_taken = ? AND m.id < ?))
     ORDER BY mm.date_taken DESC, m.id DESC
     LIMIT ?
    "#;
}

pub mod regenerator {
    pub const SELECT_TAG_ID: &str = r#"
    SELECT id
      FROM tags
     WHERE name = ?
    "#;

    pub const INSERT_TAG: &str = r#"
    INSERT INTO tags (name)
    VALUES (?)
    "#;

    pub const INSERT_MEDIA_TAG: &str = r#"
    INSERT OR IGNORE INTO media_tags (media_id, tag_id)
    VALUES (?, ?)
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

    pub const SELECT_ALL_FOR_USER: &str = r#"
    SELECT a.id
         , a.name
         , a.description
         , a.cover_media_id
         , COUNT(am.media_id) as media_count
         , a.created_at
      FROM albums AS a
      JOIN album_access AS aa ON a.id = aa.album_id
      LEFT JOIN album_media AS am ON a.id = am.album_id
     WHERE aa.user_id = ?
     GROUP BY a.id
     ORDER BY a.created_at DESC
    "#;

    pub const CHECK_OWNERSHIP: &str = r#"
    SELECT a.id
      FROM albums AS a
      JOIN album_access AS aa ON a.id = aa.album_id
     WHERE a.id = ?
       AND aa.user_id = ?
    "#;

    pub const DELETE: &str = r#"
    DELETE FROM albums
     WHERE id = ?
    "#;

    pub const SELECT_MAX_POSITION: &str = r#"
    SELECT COALESCE(MAX(position), -1)
      FROM album_media
     WHERE album_id = ?
    "#;

    pub const ADD_MEDIA: &str = r#"
    INSERT OR IGNORE INTO album_media (
        album_id
      , media_id
      , position
    ) VALUES (?, ?, ?)
    "#;

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
    "#;

    pub const DELETE_ACCESS: &str = r#"
    DELETE FROM album_access
     WHERE album_id = ?
       AND user_id = ?
    "#;

    pub const CHECK_ACCESS_COUNT: &str = r#"
    SELECT COUNT(*) FROM album_access WHERE album_id = ?
    "#;

    pub const SELECT_WITH_COUNT: &str = r#"
    SELECT a.id
         , a.name
         , a.description
         , a.cover_media_id
         , COUNT(am.media_id) as media_count
         , a.created_at
      FROM albums AS a
      LEFT JOIN album_media AS am ON a.id = am.album_id
     WHERE a.id = ?
     GROUP BY a.id
    "#;
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

pub mod tags {
    pub const SELECT_ALL: &str = r#"
    SELECT id
         , name
         , created_at
      FROM tags
     ORDER BY name
    "#;

    pub const SELECT_ID_BY_NAME: &str = r#"
    SELECT id
      FROM tags
     WHERE name = ?
    "#;

    pub const INSERT: &str = r#"
    INSERT INTO tags (name)
    VALUES (?)
    "#;

    pub const SELECT_BY_ID: &str = r#"
    SELECT id
         , name
         , created_at
      FROM tags
     WHERE id = ?
    "#;

    pub const CHECK_EXISTS: &str = r#"
    SELECT id
      FROM tags
     WHERE id = ?
    "#;

    pub const DELETE: &str = r#"
    DELETE FROM tags
     WHERE id = ?
    "#;

    pub const ADD_TO_MEDIA: &str = r#"
    INSERT OR IGNORE INTO media_tags (media_id, tag_id)
    VALUES (?, ?)
    "#;

    pub const REMOVE_FROM_MEDIA: &str = r#"
    DELETE FROM media_tags
     WHERE media_id = ?
       AND tag_id = ?
    "#;
}

pub mod users {
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

    pub const UPDATE_PASSWORD: &str = r#"
    UPDATE users
       SET hashed_password = ?
     WHERE id = ?
    "#;

    pub const UPDATE_PASSWORD_AND_RESET_FLAG: &str = r#"
    UPDATE users
       SET hashed_password = ?
         , must_change_password = 0
     WHERE id = ?
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
    "#;

    pub const REVOKE_REFRESH_TOKEN: &str = r#"
    UPDATE refresh_tokens
       SET revoked = 1
     WHERE id = ?
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
    "#;

    pub const CHECK_ALBUM_OWNERSHIP: &str = r#"
    SELECT a.id
      FROM albums AS a
      JOIN album_access AS aa ON a.id = aa.album_id
     WHERE a.id = ?
       AND aa.user_id = ?
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

    pub const SELECT_PASSWORD_HASH: &str = r#"
    SELECT password_hash
      FROM share_links
     WHERE token = ?
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

    pub const CHECK_MEDIA_ACCESS: &str = r#"
    SELECT access_level FROM media_access WHERE media_id = ? AND user_id = ?
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
