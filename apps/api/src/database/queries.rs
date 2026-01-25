pub mod media {
    pub const INSERT: &str = r#"
    INSERT INTO media (
        user_id
      , filename
      , original_filename
      , file_path
      , thumbnail_path
      , media_type
      , mime_type
      , width
      , height
      , file_size
      , duration_seconds
      , date_taken
      , gps_latitude
      , gps_longitude
      , camera_make
      , camera_model
      , lens_make
      , lens_model
      , iso
      , exposure_time
      , f_number
      , focal_length
      , focal_length_35mm
      , gps_altitude
      , location_city
      , location_state
      , location_country
      , video_codec
      , keywords
      , content_hash
      , geohash
    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
         , m.width
         , m.height
         , m.file_size
         , m.duration_seconds
         , m.date_taken
         , m.gps_latitude
         , m.gps_longitude
         , m.camera_make
         , m.camera_model
         , m.lens_make
         , m.lens_model
         , m.iso
         , m.exposure_time
         , m.f_number
         , m.focal_length
         , m.focal_length_35mm
         , m.gps_altitude
         , m.location_city
         , m.location_state
         , m.location_country
         , m.video_codec
         , m.keywords
         , m.created_at
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
     WHERE ma.user_id = ?
       AND ma.deleted_at IS NULL
     ORDER BY m.date_taken DESC, m.id DESC
    "#;

    pub const SELECT_PAGINATED_FOR_USER: &str = r#"
    SELECT m.id
         , m.filename
         , m.original_filename
         , m.media_type
         , m.mime_type
         , m.width
         , m.height
         , m.file_size
         , m.duration_seconds
         , m.date_taken
         , m.gps_latitude
         , m.gps_longitude
         , m.camera_make
         , m.camera_model
         , m.lens_make
         , m.lens_model
         , m.iso
         , m.exposure_time
         , m.f_number
         , m.focal_length
         , m.focal_length_35mm
         , m.gps_altitude
         , m.location_city
         , m.location_state
         , m.location_country
         , m.video_codec
         , m.keywords
         , m.created_at
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
     WHERE ma.user_id = ?
       AND ma.deleted_at IS NULL
       AND (m.date_taken < ? OR (m.date_taken = ? AND m.id < ?))
     ORDER BY m.date_taken DESC, m.id DESC
     LIMIT ?
    "#;

    pub const SELECT_BY_ID: &str = r#"
    SELECT id
         , filename
         , original_filename
         , media_type
         , mime_type
         , width
         , height
         , file_size
         , duration_seconds
         , date_taken
         , gps_latitude
         , gps_longitude
         , camera_make
         , camera_model
         , lens_make
         , lens_model
         , iso
         , exposure_time
         , f_number
         , focal_length
         , focal_length_35mm
         , gps_altitude
         , location_city
         , location_state
         , location_country
         , video_codec
         , keywords
         , created_at
      FROM media
     WHERE id = ?
    "#;

    pub const SELECT_BY_ID_AND_USER: &str = r#"
    SELECT m.id
         , m.filename
         , m.original_filename
         , m.media_type
         , m.mime_type
         , m.width
         , m.height
         , m.file_size
         , m.duration_seconds
         , m.date_taken
         , m.gps_latitude
         , m.gps_longitude
         , m.camera_make
         , m.camera_model
         , m.lens_make
         , m.lens_model
         , m.iso
         , m.exposure_time
         , m.f_number
         , m.focal_length
         , m.focal_length_35mm
         , m.gps_altitude
         , m.location_city
         , m.location_state
         , m.location_country
         , m.video_codec
         , m.keywords
         , m.created_at
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
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
         , m.width
         , m.height
         , m.file_size
         , m.duration_seconds
         , m.date_taken
         , m.gps_latitude
         , m.gps_longitude
         , m.camera_make
         , m.camera_model
         , m.lens_make
         , m.lens_model
         , m.iso
         , m.exposure_time
         , m.f_number
         , m.focal_length
         , m.focal_length_35mm
         , m.gps_altitude
         , m.location_city
         , m.location_state
         , m.location_country
         , m.video_codec
         , m.keywords
         , m.created_at
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
     WHERE ma.user_id = ?
       AND ma.deleted_at IS NULL
       AND m.gps_latitude IS NOT NULL
       AND m.gps_longitude IS NOT NULL
    "#;

    pub const SELECT_THUMBNAIL_BATCH: &str = r#"
    SELECT m.id
         , m.thumbnail_path
         , m.file_path
         , m.media_type
         , ma.user_id
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
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
}

pub mod timeline {
    pub const SELECT_DEFAULT: &str = r#"
    SELECT m.id
         , m.filename
         , m.original_filename
         , m.media_type
         , m.mime_type
         , m.width
         , m.height
         , m.file_size
         , m.duration_seconds
         , m.date_taken
         , m.gps_latitude
         , m.gps_longitude
         , m.camera_make
         , m.camera_model
         , m.lens_make
         , m.lens_model
         , m.iso
         , m.exposure_time
         , m.f_number
         , m.focal_length
         , m.focal_length_35mm
         , m.gps_altitude
         , m.location_city
         , m.location_state
         , m.location_country
         , m.video_codec
         , m.keywords
         , m.created_at
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
     WHERE ma.user_id = ?
       AND ma.deleted_at IS NULL
     ORDER BY m.date_taken DESC, m.id DESC
     LIMIT ?
    "#;

    pub const SELECT_PAGINATED: &str = r#"
    SELECT m.id
         , m.filename
         , m.original_filename
         , m.media_type
         , m.mime_type
         , m.width
         , m.height
         , m.file_size
         , m.duration_seconds
         , m.date_taken
         , m.gps_latitude
         , m.gps_longitude
         , m.camera_make
         , m.camera_model
         , m.lens_make
         , m.lens_model
         , m.iso
         , m.exposure_time
         , m.f_number
         , m.focal_length
         , m.focal_length_35mm
         , m.gps_altitude
         , m.location_city
         , m.location_state
         , m.location_country
         , m.video_codec
         , m.keywords
         , m.created_at
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
     WHERE ma.user_id = ?
       AND ma.deleted_at IS NULL
       AND (m.date_taken < ? OR (m.date_taken = ? AND m.id < ?))
     ORDER BY m.date_taken DESC, m.id DESC
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
    SELECT id
         , thumbnail_path
      FROM media
    "#;

    pub const CLEAR_METADATA: &str = r#"
    UPDATE media
       SET thumbnail_path = NULL
         , width = NULL
         , height = NULL
         , duration_seconds = NULL
         , date_taken = NULL
         , gps_latitude = NULL
         , gps_longitude = NULL
         , gps_altitude = NULL
         , camera_make = NULL
         , camera_model = NULL
         , lens_make = NULL
         , lens_model = NULL
         , iso = NULL
         , exposure_time = NULL
         , f_number = NULL
         , focal_length = NULL
         , focal_length_35mm = NULL
         , location_city = NULL
         , location_state = NULL
         , location_country = NULL
         , video_codec = NULL
         , keywords = NULL
     WHERE id = ?
    "#;

    pub const SELECT_MISSING_METADATA: &str = r#"
    SELECT id
         , -1 as user_id
         , file_path
         , thumbnail_path
         , media_type
         , width
         , height
         , duration_seconds
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
      FROM media
     WHERE thumbnail_path IS NULL
        OR width IS NULL
        OR height IS NULL
     ORDER BY id
    "#;

    pub const UPDATE_METADATA: &str = r#"
    UPDATE media
       SET width = ?
         , height = ?
         , date_taken = ?
         , gps_latitude = ?
         , gps_longitude = ?
         , gps_altitude = ?
         , camera_make = ?
         , camera_model = ?
         , lens_make = ?
         , lens_model = ?
         , iso = ?
         , exposure_time = ?
         , f_number = ?
         , focal_length = ?
         , focal_length_35mm = ?
         , location_city = ?
         , location_state = ?
         , location_country = ?
         , video_codec = ?
         , keywords = ?
         , duration_seconds = ?
     WHERE id = ?
    "#;

    pub const UPDATE_THUMBNAIL: &str = r#"
    UPDATE media
       SET thumbnail_path = ?
     WHERE id = ?
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
         , m.width
         , m.height
         , m.file_size
         , m.duration_seconds
         , m.date_taken
         , m.gps_latitude
         , m.gps_longitude
         , m.camera_make
         , m.camera_model
         , m.lens_make
         , m.lens_model
         , m.iso
         , m.exposure_time
         , m.f_number
         , m.focal_length
         , m.focal_length_35mm
         , m.gps_altitude
         , m.location_city
         , m.location_state
         , m.location_country
         , m.video_codec
         , m.keywords
         , m.created_at
      FROM media AS m
      JOIN album_media AS am ON m.id = am.media_id
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
         , m.width
         , m.height
         , m.file_size
         , m.duration_seconds
         , m.date_taken
         , m.gps_latitude
         , m.gps_longitude
         , m.camera_make
         , m.camera_model
         , m.lens_make
         , m.lens_model
         , m.iso
         , m.exposure_time
         , m.f_number
         , m.focal_length
         , m.focal_length_35mm
         , m.gps_altitude
         , m.location_city
         , m.location_state
         , m.location_country
         , m.video_codec
         , m.keywords
         , m.created_at
      FROM media AS m
      JOIN album_media AS am ON m.id = am.media_id
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
      FROM media
     WHERE id = ?
    "#;
}

pub mod trash {
    pub const SELECT_DELETED: &str = r#"
    SELECT m.id
         , m.filename
         , m.original_filename
         , m.media_type
         , m.mime_type
         , m.width
         , m.height
         , m.file_size
         , m.duration_seconds
         , m.date_taken
         , ma.deleted_at
         , m.created_at
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
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
         , m.thumbnail_path
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
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
         , m.thumbnail_path
      FROM media AS m
      JOIN media_access AS ma ON m.id = ma.media_id
     WHERE ma.user_id = ?
       AND ma.deleted_at IS NOT NULL
    "#;

    pub const SELECT_OLD_DELETED: &str = r#"
    SELECT m.id
         , m.file_path
         , m.thumbnail_path
         , ma.user_id
      FROM media_access AS ma
      JOIN media AS m ON ma.media_id = m.id
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
