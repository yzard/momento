use std::path::PathBuf;

pub(crate) const SERVER_HOST: &str = "0.0.0.0";
pub(crate) const SERVER_PORT: u16 = 8000;
pub(crate) const SERVER_DEBUG: bool = false;
pub(crate) const SERVER_RESET_ADMIN_PASSWORD: bool = false;
pub(crate) const SERVER_DATA_DIR: &str = "/data";
pub(crate) const SERVER_STATIC_DIR: &str = "/app/static";
pub(crate) const SERVER_API_REQUEST_BODY_MAX_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const SERVER_REQUEST_LOG_BODY_MAX_BYTES: usize = 1024 * 1024;
pub(crate) const ACCESS_TOKEN_EXPIRE_MINUTES: i64 = 30;
pub(crate) const REFRESH_TOKEN_EXPIRE_DAYS: i64 = 7;
pub(crate) const MEDIA_ACCESS_TICKET_EXPIRE_HOURS: i64 = 24;
pub(crate) const SHARE_SESSION_EXPIRE_HOURS: i64 = 24;
pub(crate) const WEBDAV_MOUNT_PATH: &str = "/webdav";
pub(crate) const WEBDAV_REALM: &str = "Momento WebDAV";
pub(crate) const WEBDAV_MAX_UPLOAD_BYTES: u64 = 50 * 1024 * 1024 * 1024;
pub(crate) const WEBDAV_MAX_CONCURRENT_REQUESTS: usize = 16;
pub(crate) const WEBDAV_POLL_INTERVAL_SECONDS: u64 = 1;
pub(crate) const WEBDAV_STABLE_FILE_AGE_SECONDS: u64 = 2;
pub(crate) const WEBDAV_MAX_CONCURRENT_PROCESSING: usize = 4;
pub(crate) const BACKUP_MAX_UPLOAD_BYTES: u64 = 50 * 1024 * 1024 * 1024;
pub(crate) const BACKUP_MAX_CHUNK_BYTES: u64 = 32 * 1024 * 1024;
pub(crate) const BACKUP_MAX_ACTIVE_UPLOADS_PER_USER: usize = 4;
pub(crate) const BACKUP_SESSION_EXPIRY_HOURS: u64 = 24;
pub(crate) const BACKUP_WORKER_POLL_INTERVAL_SECONDS: u64 = 2;
pub(crate) const BACKUP_WORKER_CONCURRENCY: usize = 2;
pub(crate) const METADATA_WORKER_POLL_INTERVAL_SECONDS: u64 = 10;
pub(crate) const METADATA_WORKER_LEASE_SECONDS: u64 = 300;
pub(crate) const METADATA_WORKER_MAX_ATTEMPTS: u32 = 5;
pub(crate) const LLM_SUBMISSION_POLL_INTERVAL_SECONDS: u64 = 5;
pub(crate) const LLM_SUBMISSION_MAX_IN_FLIGHT: usize = 128;
pub(crate) const LLM_RESULT_POLL_INTERVAL_SECONDS: u64 = 1;
pub(crate) const LLM_RESULT_CPU_PROCESSING_CONCURRENCY: usize = 8;
pub(crate) const FACE_GROUP_SIMILARITY_THRESHOLD: f32 = 0.41;
pub(crate) const FACE_REPRESENTATIVE_CONFIDENCE_WEIGHT: f64 = 0.05;
pub(crate) const FACE_REPRESENTATIVE_FACE_SIZE_WEIGHT: f64 = 0.10;
pub(crate) const FACE_REPRESENTATIVE_CENTER_PROXIMITY_WEIGHT: f64 = 0.10;
pub(crate) const FACE_REPRESENTATIVE_FRONTALITY_WEIGHT: f64 = 0.25;
pub(crate) const FACE_REPRESENTATIVE_VISIBILITY_WEIGHT: f64 = 0.30;
pub(crate) const FACE_REPRESENTATIVE_FEATURE_CLARITY_WEIGHT: f64 = 0.20;
pub(crate) const OCR_CRON: &str = "0 1 * * *";
pub(crate) const IMAGE_TAGGING_CRON: &str = "0 2 * * *";
pub(crate) const DEDUPLICATE_CRON: &str = "0 3 * * *";
pub(crate) const FACE_DETECTION_CRON: &str = "0 4 * * *";
pub(crate) const IMAGE_AESTHETICS_CRON: &str = "0 5 * * *";
pub(crate) const SCREENSHOT_DETECTION_CRON: &str = "0 6 * * *";
pub(crate) const DOCUMENT_DETECTION_CRON: &str = "0 7 * * *";

pub(crate) mod fallback {
    pub(crate) const SECURITY_SECRET_KEY: &str = "change-me-in-production-use-openssl-rand-hex-32";
    pub(crate) const THUMBNAILS_MAX_SIZE: u32 = 400;
    pub(crate) const THUMBNAILS_TINY_SIZE: u32 = 48;
    pub(crate) const THUMBNAILS_QUALITY: u8 = 85;
    pub(crate) const THUMBNAILS_VIDEO_FRAME_QUALITY: u8 = 2;
    pub(crate) const LLM_ENABLED: bool = false;
    pub(crate) const LLM_SERVER_ADDRESS: &str = "127.0.0.1:8100";
    pub(crate) const LLM_CLIENT_ID: &str = "";
    pub(crate) const LLM_API_KEY: &str = "";

    pub(crate) fn metadata_worker_concurrency() -> usize {
        num_cpus::get()
    }

    pub(crate) fn regenerate_num_cpus() -> usize {
        num_cpus::get()
    }
}

mod template {
    pub(super) const SECURITY_SECRET_KEY: &str =
        "playground-only-change-this-secret-before-exposing-the-server";
    pub(super) const THUMBNAILS_MAX_SIZE: u32 = 1200;
    pub(super) const THUMBNAILS_TINY_SIZE: u32 = 300;
    pub(super) const THUMBNAILS_QUALITY: u8 = 85;
    pub(super) const THUMBNAILS_VIDEO_FRAME_QUALITY: u8 = 85;
    pub(super) const METADATA_WORKER_CONCURRENCY: usize = 16;
    pub(super) const LLM_ENABLED: bool = true;
    pub(super) const LLM_SERVER_ADDRESS: &str = "${LLM_SERVICE_ADDRESS}";
    pub(super) const LLM_CLIENT_ID: &str = "playground";
    pub(super) const LLM_API_KEY: &str = "change-me-llm-service-key";
}

pub(crate) fn server_host() -> String {
    SERVER_HOST.to_string()
}

pub(crate) fn server_port() -> u16 {
    SERVER_PORT
}

pub(crate) fn server_debug() -> bool {
    SERVER_DEBUG
}

pub(crate) fn server_reset_admin_password() -> bool {
    SERVER_RESET_ADMIN_PASSWORD
}

pub(crate) fn server_data_dir() -> PathBuf {
    PathBuf::from(SERVER_DATA_DIR)
}

pub(crate) fn server_static_dir() -> PathBuf {
    PathBuf::from(SERVER_STATIC_DIR)
}

pub(crate) fn server_api_request_body_max_bytes() -> usize {
    SERVER_API_REQUEST_BODY_MAX_BYTES
}

pub(crate) fn server_request_log_body_max_bytes() -> usize {
    SERVER_REQUEST_LOG_BODY_MAX_BYTES
}

pub(crate) fn security_secret_key() -> String {
    fallback::SECURITY_SECRET_KEY.to_string()
}

pub(crate) fn access_token_expire_minutes() -> i64 {
    ACCESS_TOKEN_EXPIRE_MINUTES
}

pub(crate) fn refresh_token_expire_days() -> i64 {
    REFRESH_TOKEN_EXPIRE_DAYS
}

pub(crate) fn media_access_ticket_expire_hours() -> i64 {
    MEDIA_ACCESS_TICKET_EXPIRE_HOURS
}

pub(crate) fn share_session_expire_hours() -> i64 {
    SHARE_SESSION_EXPIRE_HOURS
}

pub(crate) fn webdav_mount_path() -> String {
    WEBDAV_MOUNT_PATH.to_string()
}

pub(crate) fn webdav_realm() -> String {
    WEBDAV_REALM.to_string()
}

pub(crate) fn webdav_max_upload_bytes() -> u64 {
    WEBDAV_MAX_UPLOAD_BYTES
}

pub(crate) fn webdav_max_concurrent_requests() -> usize {
    WEBDAV_MAX_CONCURRENT_REQUESTS
}

pub(crate) fn webdav_poll_interval_seconds() -> u64 {
    WEBDAV_POLL_INTERVAL_SECONDS
}

pub(crate) fn webdav_stable_file_age_seconds() -> u64 {
    WEBDAV_STABLE_FILE_AGE_SECONDS
}

pub(crate) fn webdav_max_concurrent_processing() -> usize {
    WEBDAV_MAX_CONCURRENT_PROCESSING
}

pub(crate) fn backup_max_upload_bytes() -> u64 {
    BACKUP_MAX_UPLOAD_BYTES
}

pub(crate) fn backup_max_chunk_bytes() -> u64 {
    BACKUP_MAX_CHUNK_BYTES
}

pub(crate) fn backup_max_active_uploads_per_user() -> usize {
    BACKUP_MAX_ACTIVE_UPLOADS_PER_USER
}

pub(crate) fn backup_session_expiry_hours() -> u64 {
    BACKUP_SESSION_EXPIRY_HOURS
}

pub(crate) fn backup_worker_poll_interval_seconds() -> u64 {
    BACKUP_WORKER_POLL_INTERVAL_SECONDS
}

pub(crate) fn backup_worker_concurrency() -> usize {
    BACKUP_WORKER_CONCURRENCY
}

pub(crate) fn thumbnails_max_size() -> u32 {
    fallback::THUMBNAILS_MAX_SIZE
}

pub(crate) fn thumbnails_tiny_size() -> u32 {
    fallback::THUMBNAILS_TINY_SIZE
}

pub(crate) fn thumbnails_quality() -> u8 {
    fallback::THUMBNAILS_QUALITY
}

pub(crate) fn thumbnails_video_frame_quality() -> u8 {
    fallback::THUMBNAILS_VIDEO_FRAME_QUALITY
}

pub(crate) fn metadata_worker_poll_interval_seconds() -> u64 {
    METADATA_WORKER_POLL_INTERVAL_SECONDS
}

pub(crate) fn metadata_worker_lease_seconds() -> u64 {
    METADATA_WORKER_LEASE_SECONDS
}

pub(crate) fn metadata_worker_max_attempts() -> u32 {
    METADATA_WORKER_MAX_ATTEMPTS
}

pub(crate) fn ocr_cron() -> String {
    OCR_CRON.to_string()
}

pub(crate) fn image_tagging_cron() -> String {
    IMAGE_TAGGING_CRON.to_string()
}

pub(crate) fn deduplicate_cron() -> String {
    DEDUPLICATE_CRON.to_string()
}

pub(crate) fn face_detection_cron() -> String {
    FACE_DETECTION_CRON.to_string()
}

pub(crate) fn image_aesthetics_cron() -> String {
    IMAGE_AESTHETICS_CRON.to_string()
}

pub(crate) fn screenshot_detection_cron() -> String {
    SCREENSHOT_DETECTION_CRON.to_string()
}

pub(crate) fn document_detection_cron() -> String {
    DOCUMENT_DETECTION_CRON.to_string()
}

pub(crate) fn llm_server_address() -> String {
    fallback::LLM_SERVER_ADDRESS.to_string()
}

pub(crate) fn llm_enabled() -> bool {
    fallback::LLM_ENABLED
}

pub(crate) fn llm_client_id() -> String {
    fallback::LLM_CLIENT_ID.to_string()
}

pub(crate) fn llm_api_key() -> String {
    fallback::LLM_API_KEY.to_string()
}

pub(crate) fn face_group_similarity_threshold() -> f32 {
    FACE_GROUP_SIMILARITY_THRESHOLD
}

pub(crate) fn face_representative_confidence_weight() -> f64 {
    FACE_REPRESENTATIVE_CONFIDENCE_WEIGHT
}

pub(crate) fn face_representative_face_size_weight() -> f64 {
    FACE_REPRESENTATIVE_FACE_SIZE_WEIGHT
}

pub(crate) fn face_representative_center_proximity_weight() -> f64 {
    FACE_REPRESENTATIVE_CENTER_PROXIMITY_WEIGHT
}

pub(crate) fn face_representative_frontality_weight() -> f64 {
    FACE_REPRESENTATIVE_FRONTALITY_WEIGHT
}

pub(crate) fn face_representative_visibility_weight() -> f64 {
    FACE_REPRESENTATIVE_VISIBILITY_WEIGHT
}

pub(crate) fn face_representative_feature_clarity_weight() -> f64 {
    FACE_REPRESENTATIVE_FEATURE_CLARITY_WEIGHT
}

pub(crate) fn llm_submission_poll_interval_seconds() -> u64 {
    LLM_SUBMISSION_POLL_INTERVAL_SECONDS
}

pub(crate) fn llm_submission_max_in_flight() -> usize {
    LLM_SUBMISSION_MAX_IN_FLIGHT
}

pub(crate) fn llm_result_poll_interval_seconds() -> u64 {
    LLM_RESULT_POLL_INTERVAL_SECONDS
}

pub(crate) fn llm_result_cpu_processing_concurrency() -> usize {
    LLM_RESULT_CPU_PROCESSING_CONCURRENCY
}

pub(crate) fn render_template(source: &str) -> String {
    let replacements = [
        ("{{SERVER_HOST}}", SERVER_HOST.to_string()),
        ("{{SERVER_PORT}}", SERVER_PORT.to_string()),
        ("{{SERVER_DEBUG}}", SERVER_DEBUG.to_string()),
        (
            "{{SERVER_RESET_ADMIN_PASSWORD}}",
            SERVER_RESET_ADMIN_PASSWORD.to_string(),
        ),
        ("{{SERVER_DATA_DIR}}", SERVER_DATA_DIR.to_string()),
        ("{{SERVER_STATIC_DIR}}", SERVER_STATIC_DIR.to_string()),
        (
            "{{SERVER_API_REQUEST_BODY_MAX_BYTES}}",
            SERVER_API_REQUEST_BODY_MAX_BYTES.to_string(),
        ),
        (
            "{{SERVER_REQUEST_LOG_BODY_MAX_BYTES}}",
            SERVER_REQUEST_LOG_BODY_MAX_BYTES.to_string(),
        ),
        (
            "{{SECURITY_SECRET_KEY}}",
            template::SECURITY_SECRET_KEY.to_string(),
        ),
        (
            "{{ACCESS_TOKEN_EXPIRE_MINUTES}}",
            ACCESS_TOKEN_EXPIRE_MINUTES.to_string(),
        ),
        (
            "{{REFRESH_TOKEN_EXPIRE_DAYS}}",
            REFRESH_TOKEN_EXPIRE_DAYS.to_string(),
        ),
        (
            "{{MEDIA_ACCESS_TICKET_EXPIRE_HOURS}}",
            MEDIA_ACCESS_TICKET_EXPIRE_HOURS.to_string(),
        ),
        (
            "{{SHARE_SESSION_EXPIRE_HOURS}}",
            SHARE_SESSION_EXPIRE_HOURS.to_string(),
        ),
        ("{{WEBDAV_MOUNT_PATH}}", WEBDAV_MOUNT_PATH.to_string()),
        ("{{WEBDAV_REALM}}", WEBDAV_REALM.to_string()),
        (
            "{{WEBDAV_MAX_UPLOAD_BYTES}}",
            WEBDAV_MAX_UPLOAD_BYTES.to_string(),
        ),
        (
            "{{WEBDAV_MAX_CONCURRENT_REQUESTS}}",
            WEBDAV_MAX_CONCURRENT_REQUESTS.to_string(),
        ),
        (
            "{{WEBDAV_POLL_INTERVAL_SECONDS}}",
            WEBDAV_POLL_INTERVAL_SECONDS.to_string(),
        ),
        (
            "{{WEBDAV_STABLE_FILE_AGE_SECONDS}}",
            WEBDAV_STABLE_FILE_AGE_SECONDS.to_string(),
        ),
        (
            "{{WEBDAV_MAX_CONCURRENT_PROCESSING}}",
            WEBDAV_MAX_CONCURRENT_PROCESSING.to_string(),
        ),
        (
            "{{BACKUP_MAX_UPLOAD_BYTES}}",
            BACKUP_MAX_UPLOAD_BYTES.to_string(),
        ),
        (
            "{{BACKUP_MAX_CHUNK_BYTES}}",
            BACKUP_MAX_CHUNK_BYTES.to_string(),
        ),
        (
            "{{BACKUP_MAX_ACTIVE_UPLOADS_PER_USER}}",
            BACKUP_MAX_ACTIVE_UPLOADS_PER_USER.to_string(),
        ),
        (
            "{{BACKUP_SESSION_EXPIRY_HOURS}}",
            BACKUP_SESSION_EXPIRY_HOURS.to_string(),
        ),
        (
            "{{BACKUP_WORKER_POLL_INTERVAL_SECONDS}}",
            BACKUP_WORKER_POLL_INTERVAL_SECONDS.to_string(),
        ),
        (
            "{{BACKUP_WORKER_CONCURRENCY}}",
            BACKUP_WORKER_CONCURRENCY.to_string(),
        ),
        (
            "{{THUMBNAILS_MAX_SIZE}}",
            template::THUMBNAILS_MAX_SIZE.to_string(),
        ),
        (
            "{{THUMBNAILS_TINY_SIZE}}",
            template::THUMBNAILS_TINY_SIZE.to_string(),
        ),
        (
            "{{THUMBNAILS_QUALITY}}",
            template::THUMBNAILS_QUALITY.to_string(),
        ),
        (
            "{{THUMBNAILS_VIDEO_FRAME_QUALITY}}",
            template::THUMBNAILS_VIDEO_FRAME_QUALITY.to_string(),
        ),
        (
            "{{METADATA_WORKER_POLL_INTERVAL_SECONDS}}",
            METADATA_WORKER_POLL_INTERVAL_SECONDS.to_string(),
        ),
        (
            "{{METADATA_WORKER_CONCURRENCY}}",
            template::METADATA_WORKER_CONCURRENCY.to_string(),
        ),
        (
            "{{METADATA_WORKER_LEASE_SECONDS}}",
            METADATA_WORKER_LEASE_SECONDS.to_string(),
        ),
        (
            "{{METADATA_WORKER_MAX_ATTEMPTS}}",
            METADATA_WORKER_MAX_ATTEMPTS.to_string(),
        ),
        (
            "{{LLM_SUBMISSION_POLL_INTERVAL_SECONDS}}",
            LLM_SUBMISSION_POLL_INTERVAL_SECONDS.to_string(),
        ),
        (
            "{{LLM_SUBMISSION_MAX_IN_FLIGHT}}",
            LLM_SUBMISSION_MAX_IN_FLIGHT.to_string(),
        ),
        (
            "{{LLM_RESULT_POLL_INTERVAL_SECONDS}}",
            LLM_RESULT_POLL_INTERVAL_SECONDS.to_string(),
        ),
        (
            "{{LLM_RESULT_CPU_PROCESSING_CONCURRENCY}}",
            LLM_RESULT_CPU_PROCESSING_CONCURRENCY.to_string(),
        ),
        ("{{LLM_ENABLED}}", template::LLM_ENABLED.to_string()),
        (
            "{{LLM_SERVER_ADDRESS}}",
            template::LLM_SERVER_ADDRESS.to_string(),
        ),
        ("{{LLM_CLIENT_ID}}", template::LLM_CLIENT_ID.to_string()),
        ("{{LLM_API_KEY}}", template::LLM_API_KEY.to_string()),
        (
            "{{FACE_GROUP_SIMILARITY_THRESHOLD}}",
            FACE_GROUP_SIMILARITY_THRESHOLD.to_string(),
        ),
        (
            "{{FACE_REPRESENTATIVE_CONFIDENCE_WEIGHT}}",
            FACE_REPRESENTATIVE_CONFIDENCE_WEIGHT.to_string(),
        ),
        (
            "{{FACE_REPRESENTATIVE_FACE_SIZE_WEIGHT}}",
            FACE_REPRESENTATIVE_FACE_SIZE_WEIGHT.to_string(),
        ),
        (
            "{{FACE_REPRESENTATIVE_CENTER_PROXIMITY_WEIGHT}}",
            FACE_REPRESENTATIVE_CENTER_PROXIMITY_WEIGHT.to_string(),
        ),
        (
            "{{FACE_REPRESENTATIVE_FRONTALITY_WEIGHT}}",
            FACE_REPRESENTATIVE_FRONTALITY_WEIGHT.to_string(),
        ),
        (
            "{{FACE_REPRESENTATIVE_VISIBILITY_WEIGHT}}",
            FACE_REPRESENTATIVE_VISIBILITY_WEIGHT.to_string(),
        ),
        (
            "{{FACE_REPRESENTATIVE_FEATURE_CLARITY_WEIGHT}}",
            FACE_REPRESENTATIVE_FEATURE_CLARITY_WEIGHT.to_string(),
        ),
        ("{{OCR_CRON}}", OCR_CRON.to_string()),
        ("{{IMAGE_TAGGING_CRON}}", IMAGE_TAGGING_CRON.to_string()),
        ("{{DEDUPLICATE_CRON}}", DEDUPLICATE_CRON.to_string()),
        ("{{FACE_DETECTION_CRON}}", FACE_DETECTION_CRON.to_string()),
        (
            "{{IMAGE_AESTHETICS_CRON}}",
            IMAGE_AESTHETICS_CRON.to_string(),
        ),
        (
            "{{SCREENSHOT_DETECTION_CRON}}",
            SCREENSHOT_DETECTION_CRON.to_string(),
        ),
        (
            "{{DOCUMENT_DETECTION_CRON}}",
            DOCUMENT_DETECTION_CRON.to_string(),
        ),
    ];

    replacements
        .into_iter()
        .fold(source.to_string(), |rendered, (placeholder, value)| {
            rendered.replace(placeholder, &value)
        })
}
