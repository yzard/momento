mod defaults;
mod settings;

use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use momento_common::config_file::{replace_config, write_new_config};
use tokio_tungstenite::tungstenite::http::uri::Authority;
use toml_edit::{value, DocumentMut};

static DEFAULT_CONFIG_TEMPLATE: LazyLock<String> =
    LazyLock::new(|| defaults::render_template(include_str!("default.toml")));
const LLM_SERVICE_ADDRESS_PLACEHOLDER: &str = "${LLM_SERVICE_ADDRESS}";
const RESET_ADMIN_PASSWORD_PLACEHOLDER: &str = "${RESET_ADMIN_PASSWORD}";
const SECRET_KEY_PLACEHOLDER: &str = "${SECRET_KEY}";
const LLM_SERVICE_API_KEY_PLACEHOLDER: &str = "${LLM_SERVICE_API_KEY}";

pub fn default_config_template() -> &'static str {
    DEFAULT_CONFIG_TEMPLATE.as_str()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    #[serde(default = "defaults::server_host")]
    pub host: String,
    #[serde(default = "defaults::server_port")]
    pub port: u16,
    #[serde(default = "defaults::server_debug")]
    pub debug: bool,
    #[serde(default = "defaults::server_reset_admin_password")]
    pub reset_admin_password: bool,
    /// Root for the database and every generated media directory.
    #[serde(default = "defaults::server_data_dir")]
    pub data_dir: PathBuf,
    /// Built frontend served as the HTTP fallback.
    #[serde(default = "defaults::server_static_dir")]
    pub static_dir: PathBuf,
    /// Maximum body accepted by buffered API extractors such as JSON.
    #[serde(default = "defaults::server_api_request_body_max_bytes")]
    pub api_request_body_max_bytes: usize,
    /// Maximum POST body bytes retained by request logging before omission.
    #[serde(default = "defaults::server_request_log_body_max_bytes")]
    pub request_log_body_max_bytes: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: defaults::server_host(),
            port: defaults::SERVER_PORT,
            debug: defaults::SERVER_DEBUG,
            reset_admin_password: defaults::SERVER_RESET_ADMIN_PASSWORD,
            data_dir: defaults::server_data_dir(),
            static_dir: defaults::server_static_dir(),
            api_request_body_max_bytes: defaults::SERVER_API_REQUEST_BODY_MAX_BYTES,
            request_log_body_max_bytes: defaults::SERVER_REQUEST_LOG_BODY_MAX_BYTES,
        }
    }
}

impl ServerConfig {
    fn validate(&self) -> std::io::Result<()> {
        if self.api_request_body_max_bytes == 0 || self.request_log_body_max_bytes == 0 {
            return Err(std::io::Error::other(
                "server API request and request-log body limits must be positive",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityConfig {
    #[serde(default = "defaults::security_secret_key")]
    pub secret_key: String,
    #[serde(default = "defaults::access_token_expire_minutes")]
    pub access_token_expire_minutes: i64,
    #[serde(default = "defaults::refresh_token_expire_days")]
    pub refresh_token_expire_days: i64,
    #[serde(default = "defaults::media_access_ticket_expire_hours")]
    pub media_access_ticket_expire_hours: i64,
    #[serde(default = "defaults::share_session_expire_hours")]
    pub share_session_expire_hours: i64,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            secret_key: defaults::security_secret_key(),
            access_token_expire_minutes: defaults::ACCESS_TOKEN_EXPIRE_MINUTES,
            refresh_token_expire_days: defaults::REFRESH_TOKEN_EXPIRE_DAYS,
            media_access_ticket_expire_hours: defaults::MEDIA_ACCESS_TICKET_EXPIRE_HOURS,
            share_session_expire_hours: defaults::SHARE_SESSION_EXPIRE_HOURS,
        }
    }
}

impl SecurityConfig {
    fn validate(&self) -> std::io::Result<()> {
        if self.secret_key.trim().is_empty() {
            return Err(std::io::Error::other(
                "security secret_key must not be empty",
            ));
        }
        if self.access_token_expire_minutes <= 0 || self.refresh_token_expire_days <= 0 {
            return Err(std::io::Error::other(
                "security access-token and refresh-token expirations must be positive",
            ));
        }
        if !(1..=168).contains(&self.media_access_ticket_expire_hours)
            || !(1..=168).contains(&self.share_session_expire_hours)
        {
            return Err(std::io::Error::other(
                "security media ticket and share session expirations must be within 1..=168 hours",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebDAVConfig {
    #[serde(default = "defaults::webdav_mount_path")]
    pub mount_path: String,
    #[serde(default = "defaults::webdav_realm")]
    pub realm: String,
    #[serde(default = "defaults::webdav_max_upload_bytes")]
    pub max_upload_bytes: u64,
    #[serde(default = "defaults::webdav_max_concurrent_requests")]
    pub max_concurrent_requests: usize,
    #[serde(default = "defaults::webdav_poll_interval_seconds")]
    pub poll_interval_seconds: u64,
    #[serde(default = "defaults::webdav_stable_file_age_seconds")]
    pub stable_file_age_seconds: u64,
    #[serde(default = "defaults::webdav_max_concurrent_processing")]
    pub max_concurrent_processing: usize,
}

impl Default for WebDAVConfig {
    fn default() -> Self {
        Self {
            mount_path: defaults::webdav_mount_path(),
            realm: defaults::webdav_realm(),
            max_upload_bytes: defaults::WEBDAV_MAX_UPLOAD_BYTES,
            max_concurrent_requests: defaults::WEBDAV_MAX_CONCURRENT_REQUESTS,
            poll_interval_seconds: defaults::WEBDAV_POLL_INTERVAL_SECONDS,
            stable_file_age_seconds: defaults::WEBDAV_STABLE_FILE_AGE_SECONDS,
            max_concurrent_processing: defaults::WEBDAV_MAX_CONCURRENT_PROCESSING,
        }
    }
}

impl WebDAVConfig {
    fn validate(&self) -> std::io::Result<()> {
        if self.max_upload_bytes == 0 {
            return Err(std::io::Error::other(
                "webdav max_upload_bytes must be positive",
            ));
        }
        if self.max_concurrent_requests == 0 || self.max_concurrent_requests > u32::MAX as usize {
            return Err(std::io::Error::other(
                "webdav max_concurrent_requests must be within 1..=u32::MAX",
            ));
        }
        if self.poll_interval_seconds == 0 || self.max_concurrent_processing == 0 {
            return Err(std::io::Error::other(
                "webdav poll interval and max concurrent processing must be positive",
            ));
        }
        let mount_segment = self.mount_path.strip_prefix('/').unwrap_or_default();
        if mount_segment.is_empty()
            || matches!(mount_segment, "." | "..")
            || !self.mount_path.starts_with('/')
            || self.mount_path.ends_with('/')
            || !mount_segment
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "-._~".contains(character))
        {
            return Err(std::io::Error::other(
                "webdav mount_path must be one unreserved literal path segment beginning with '/'",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupConfig {
    #[serde(default = "defaults::backup_max_upload_bytes")]
    pub max_upload_bytes: u64,
    #[serde(default = "defaults::backup_max_chunk_bytes")]
    pub max_chunk_bytes: u64,
    #[serde(default = "defaults::backup_max_active_uploads_per_user")]
    pub max_active_uploads_per_user: usize,
    #[serde(default = "defaults::backup_session_expiry_hours")]
    pub session_expiry_hours: u64,
    #[serde(default = "defaults::backup_worker_poll_interval_seconds")]
    pub worker_poll_interval_seconds: u64,
    #[serde(default = "defaults::backup_worker_concurrency")]
    pub worker_concurrency: usize,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            max_upload_bytes: defaults::BACKUP_MAX_UPLOAD_BYTES,
            max_chunk_bytes: defaults::BACKUP_MAX_CHUNK_BYTES,
            max_active_uploads_per_user: defaults::BACKUP_MAX_ACTIVE_UPLOADS_PER_USER,
            session_expiry_hours: defaults::BACKUP_SESSION_EXPIRY_HOURS,
            worker_poll_interval_seconds: defaults::BACKUP_WORKER_POLL_INTERVAL_SECONDS,
            worker_concurrency: defaults::BACKUP_WORKER_CONCURRENCY,
        }
    }
}

impl BackupConfig {
    fn validate(&self) -> std::io::Result<()> {
        if self.max_upload_bytes == 0
            || self.max_chunk_bytes == 0
            || self.max_chunk_bytes > self.max_upload_bytes
        {
            return Err(std::io::Error::other(
                "backup upload and chunk limits must be positive and chunk must not exceed upload",
            ));
        }
        if self.max_active_uploads_per_user == 0
            || self.session_expiry_hours == 0
            || self.worker_poll_interval_seconds == 0
            || self.worker_concurrency == 0
        {
            return Err(std::io::Error::other(
                "backup active uploads, expiry, worker poll interval, and concurrency must be positive",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetadataConfig {
    #[serde(default = "defaults::thumbnails_max_size")]
    pub thumbnails_max_size: u32,
    #[serde(default = "defaults::thumbnails_tiny_size")]
    pub thumbnails_tiny_size: u32,
    #[serde(default = "defaults::thumbnails_quality")]
    pub thumbnails_quality: u8,
    #[serde(default = "defaults::thumbnails_video_frame_quality")]
    pub thumbnails_video_frame_quality: u8,
}

impl Default for MetadataConfig {
    fn default() -> Self {
        Self {
            thumbnails_max_size: defaults::fallback::THUMBNAILS_MAX_SIZE,
            thumbnails_tiny_size: defaults::fallback::THUMBNAILS_TINY_SIZE,
            thumbnails_quality: defaults::fallback::THUMBNAILS_QUALITY,
            thumbnails_video_frame_quality: defaults::fallback::THUMBNAILS_VIDEO_FRAME_QUALITY,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegenerateConfig {
    #[serde(default = "defaults::fallback::regenerate_num_cpus")]
    pub num_cpus: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetadataWorkerConfig {
    #[serde(default = "defaults::metadata_worker_poll_interval_seconds")]
    pub poll_interval_seconds: u64,
    #[serde(default = "defaults::fallback::metadata_worker_concurrency")]
    pub concurrency: usize,
    #[serde(default = "defaults::metadata_worker_lease_seconds")]
    pub lease_seconds: u64,
    #[serde(default = "defaults::metadata_worker_max_attempts")]
    pub max_attempts: u32,
}

impl Default for MetadataWorkerConfig {
    fn default() -> Self {
        Self {
            poll_interval_seconds: defaults::METADATA_WORKER_POLL_INTERVAL_SECONDS,
            concurrency: defaults::fallback::metadata_worker_concurrency(),
            lease_seconds: defaults::METADATA_WORKER_LEASE_SECONDS,
            max_attempts: defaults::METADATA_WORKER_MAX_ATTEMPTS,
        }
    }
}

impl Default for RegenerateConfig {
    fn default() -> Self {
        Self {
            num_cpus: defaults::fallback::regenerate_num_cpus(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlmConfig {
    #[serde(default = "defaults::llm_enabled")]
    pub enabled: bool,
    #[serde(default = "defaults::llm_server_address")]
    pub server_address: String,
    #[serde(default = "defaults::llm_client_id")]
    pub client_id: String,
    #[serde(default = "defaults::llm_api_key")]
    pub api_key: String,
    #[serde(default = "defaults::image_tagging_enabled")]
    pub image_tagging_enabled: bool,
    #[serde(default = "defaults::deduplicate_enabled")]
    pub deduplicate_enabled: bool,
    #[serde(default = "defaults::face_detection_enabled")]
    pub face_detection_enabled: bool,
    #[serde(default = "defaults::image_aesthetics_enabled")]
    pub image_aesthetics_enabled: bool,
    #[serde(default = "defaults::screenshot_detection_enabled")]
    pub screenshot_detection_enabled: bool,
    #[serde(default = "defaults::document_detection_enabled")]
    pub document_detection_enabled: bool,
    #[serde(default = "defaults::face_group_similarity_threshold")]
    pub face_group_similarity_threshold: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CronjobConfig {
    #[serde(default = "defaults::cronjob_timezone")]
    pub timezone: String,
    #[serde(default = "defaults::ocr_cron")]
    pub ocr_cron: String,
    #[serde(default = "defaults::image_tagging_cron")]
    pub image_tagging_cron: String,
    #[serde(default = "defaults::deduplicate_cron")]
    pub deduplicate_cron: String,
    #[serde(default = "defaults::face_detection_cron")]
    pub face_detection_cron: String,
    #[serde(default = "defaults::image_aesthetics_cron")]
    pub image_aesthetics_cron: String,
    #[serde(default = "defaults::screenshot_detection_cron")]
    pub screenshot_detection_cron: String,
    #[serde(default = "defaults::document_detection_cron")]
    pub document_detection_cron: String,
}

impl Default for CronjobConfig {
    fn default() -> Self {
        Self {
            timezone: defaults::cronjob_timezone(),
            ocr_cron: defaults::ocr_cron(),
            image_tagging_cron: defaults::image_tagging_cron(),
            deduplicate_cron: defaults::deduplicate_cron(),
            face_detection_cron: defaults::face_detection_cron(),
            image_aesthetics_cron: defaults::image_aesthetics_cron(),
            screenshot_detection_cron: defaults::screenshot_detection_cron(),
            document_detection_cron: defaults::document_detection_cron(),
        }
    }
}

impl CronjobConfig {
    fn validate(&self) -> std::io::Result<()> {
        self.timezone
            .parse::<chrono_tz::Tz>()
            .map_err(|error| std::io::Error::other(format!("invalid cronjob timezone: {error}")))?;
        for (name, expression) in [
            ("ocr", &self.ocr_cron),
            ("image_tagging", &self.image_tagging_cron),
            ("deduplicate", &self.deduplicate_cron),
            ("face_detection", &self.face_detection_cron),
            ("image_aesthetics", &self.image_aesthetics_cron),
            ("screenshot_detection", &self.screenshot_detection_cron),
            ("document_detection", &self.document_detection_cron),
        ] {
            let normalized_cron = format!("0 {expression} *");
            normalized_cron.parse::<cron::Schedule>().map_err(|error| {
                std::io::Error::other(format!("invalid {name} cronjob: {error}"))
            })?;
        }
        Ok(())
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            enabled: defaults::fallback::LLM_ENABLED,
            server_address: defaults::llm_server_address(),
            client_id: defaults::fallback::LLM_CLIENT_ID.to_string(),
            api_key: defaults::fallback::LLM_API_KEY.to_string(),
            image_tagging_enabled: defaults::fallback::IMAGE_TAGGING_ENABLED,
            deduplicate_enabled: defaults::fallback::DEDUPLICATE_ENABLED,
            face_detection_enabled: defaults::fallback::FACE_DETECTION_ENABLED,
            image_aesthetics_enabled: defaults::fallback::IMAGE_AESTHETICS_ENABLED,
            screenshot_detection_enabled: defaults::fallback::SCREENSHOT_DETECTION_ENABLED,
            document_detection_enabled: defaults::fallback::DOCUMENT_DETECTION_ENABLED,
            face_group_similarity_threshold: defaults::FACE_GROUP_SIMILARITY_THRESHOLD,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlmSubmissionWorkerConfig {
    #[serde(default = "defaults::llm_submission_poll_interval_seconds")]
    pub poll_interval_seconds: u64,
    #[serde(default = "defaults::llm_submission_max_in_flight")]
    pub max_in_flight: usize,
}

impl Default for LlmSubmissionWorkerConfig {
    fn default() -> Self {
        Self {
            poll_interval_seconds: defaults::LLM_SUBMISSION_POLL_INTERVAL_SECONDS,
            max_in_flight: defaults::LLM_SUBMISSION_MAX_IN_FLIGHT,
        }
    }
}

impl LlmSubmissionWorkerConfig {
    fn validate(&self) -> std::io::Result<()> {
        if self.poll_interval_seconds == 0 || self.max_in_flight == 0 {
            return Err(std::io::Error::other(
                "llm submission poll interval and max in-flight submissions must be positive",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlmResultWorkerConfig {
    #[serde(default = "defaults::llm_result_poll_interval_seconds")]
    pub poll_interval_seconds: u64,
    #[serde(default = "defaults::llm_result_cpu_processing_concurrency")]
    pub cpu_processing_concurrency: usize,
}

impl Default for LlmResultWorkerConfig {
    fn default() -> Self {
        Self {
            poll_interval_seconds: defaults::LLM_RESULT_POLL_INTERVAL_SECONDS,
            cpu_processing_concurrency: defaults::LLM_RESULT_CPU_PROCESSING_CONCURRENCY,
        }
    }
}

impl LlmResultWorkerConfig {
    fn validate(&self) -> std::io::Result<()> {
        if self.poll_interval_seconds == 0 || self.cpu_processing_concurrency == 0 {
            return Err(std::io::Error::other(
                "llm result poll interval and CPU processing concurrency must be positive",
            ));
        }
        Ok(())
    }
}

impl LlmConfig {
    fn validate(&self) -> std::io::Result<()> {
        if !self.face_group_similarity_threshold.is_finite()
            || !(0.0..=1.0).contains(&self.face_group_similarity_threshold)
        {
            return Err(std::io::Error::other(
                "llm face_group_similarity_threshold must be within [0, 1]",
            ));
        }
        if !self.enabled {
            return Ok(());
        }
        if self.server_address.is_empty() {
            return Err(std::io::Error::other(
                "llm server_address is required when LLM is enabled",
            ));
        }
        let server_address = self
            .server_address
            .parse::<Authority>()
            .map_err(|_| std::io::Error::other("llm server_address must contain only host:port"))?;
        if server_address.host().is_empty()
            || server_address.port_u16().is_none_or(|port| port == 0)
        {
            return Err(std::io::Error::other(
                "llm server_address must contain a host and positive port",
            ));
        }
        if !momento_common::llm::is_valid_client_id(&self.client_id) {
            return Err(std::io::Error::other(
                "llm client_id must contain 1 to 128 letters, numbers, hyphens, or underscores",
            ));
        }
        if self.api_key.trim().is_empty() {
            return Err(std::io::Error::other(
                "llm api_key is required when LLM is enabled",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaceGroupRepresentativeConfig {
    #[serde(default = "defaults::face_representative_confidence_weight")]
    pub confidence_weight: f64,
    #[serde(default = "defaults::face_representative_face_size_weight")]
    pub face_size_weight: f64,
    #[serde(default = "defaults::face_representative_center_proximity_weight")]
    pub center_proximity_weight: f64,
    #[serde(default = "defaults::face_representative_frontality_weight")]
    pub frontality_weight: f64,
    #[serde(default = "defaults::face_representative_visibility_weight")]
    pub visibility_weight: f64,
    #[serde(default = "defaults::face_representative_feature_clarity_weight")]
    pub feature_clarity_weight: f64,
}

impl Default for FaceGroupRepresentativeConfig {
    fn default() -> Self {
        Self {
            confidence_weight: defaults::FACE_REPRESENTATIVE_CONFIDENCE_WEIGHT,
            face_size_weight: defaults::FACE_REPRESENTATIVE_FACE_SIZE_WEIGHT,
            center_proximity_weight: defaults::FACE_REPRESENTATIVE_CENTER_PROXIMITY_WEIGHT,
            frontality_weight: defaults::FACE_REPRESENTATIVE_FRONTALITY_WEIGHT,
            visibility_weight: defaults::FACE_REPRESENTATIVE_VISIBILITY_WEIGHT,
            feature_clarity_weight: defaults::FACE_REPRESENTATIVE_FEATURE_CLARITY_WEIGHT,
        }
    }
}

impl FaceGroupRepresentativeConfig {
    fn validate(&self) -> std::io::Result<()> {
        let weights = [
            ("confidence_weight", self.confidence_weight),
            ("face_size_weight", self.face_size_weight),
            ("center_proximity_weight", self.center_proximity_weight),
            ("frontality_weight", self.frontality_weight),
            ("visibility_weight", self.visibility_weight),
            ("feature_clarity_weight", self.feature_clarity_weight),
        ];
        for &(name, weight) in &weights {
            if !weight.is_finite() || weight < 0.0 {
                return Err(std::io::Error::other(format!(
                    "face_group_representative {name} must be finite and non-negative"
                )));
            }
        }
        let total_weight = weights.iter().map(|(_, weight)| weight).sum::<f64>();
        if (total_weight - 1.0).abs() > 1e-6 {
            return Err(std::io::Error::other(
                "face_group_representative weights must sum to 1",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub webdav: WebDAVConfig,
    #[serde(default)]
    pub backup: BackupConfig,
    #[serde(default)]
    pub metadata: MetadataConfig,
    #[serde(default)]
    pub regenerate: RegenerateConfig,
    #[serde(default)]
    pub metadata_worker: MetadataWorkerConfig,
    #[serde(default)]
    pub llm_submission_worker: LlmSubmissionWorkerConfig,
    #[serde(default)]
    pub llm_result_worker: LlmResultWorkerConfig,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub face_group_representative: FaceGroupRepresentativeConfig,
    #[serde(default)]
    pub cronjob: CronjobConfig,
}

/// Reads and parses the config file. A missing or malformed file is an error: silently
/// falling back to defaults would start the server against the wrong data directory.
pub fn load_config(config_path: &Path) -> std::io::Result<Config> {
    if !config_path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("config file not found: {}", config_path.display()),
        ));
    }

    let content = fs::read_to_string(config_path)?;
    let llm_service_address = read_environment_variable("LLM_SERVICE_ADDRESS")?;
    let reset_admin_password = read_environment_variable("RESET_ADMIN_PASSWORD")?;
    let secret_key = read_environment_variable("SECRET_KEY")?;
    let api_key = read_environment_variable("LLM_SERVICE_API_KEY")?;
    let content = resolve_config_environment(
        &content,
        llm_service_address.as_deref(),
        reset_admin_password.as_deref(),
        secret_key.as_deref(),
        api_key.as_deref(),
    )?;

    let mut config: Config = toml::from_str(&content).map_err(|e| {
        std::io::Error::other(format!("invalid config at {}: {e}", config_path.display()))
    })?;
    apply_config_environment(
        &mut config,
        reset_admin_password.as_deref(),
        secret_key.as_deref(),
        api_key.as_deref(),
    )?;
    config.server.validate()?;
    config.security.validate()?;
    config.webdav.validate()?;
    config.backup.validate()?;
    config.llm.validate()?;
    config.face_group_representative.validate()?;
    config.llm_submission_worker.validate()?;
    config.llm_result_worker.validate()?;
    config.cronjob.validate()?;
    Ok(config)
}

fn read_environment_variable(name: &str) -> std::io::Result<Option<String>> {
    match env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(std::io::Error::other(format!(
            "{name} must contain valid Unicode"
        ))),
    }
}

pub fn apply_config_environment(
    config: &mut Config,
    reset_admin_password: Option<&str>,
    secret_key: Option<&str>,
    api_key: Option<&str>,
) -> std::io::Result<()> {
    if let Some(reset_admin_password) = reset_admin_password {
        config.server.reset_admin_password = parse_reset_admin_password(reset_admin_password)?;
    }
    if let Some(secret_key) = secret_key {
        validate_environment_secret(secret_key, "SECRET_KEY")?;
        config.security.secret_key = secret_key.to_string();
    }
    if let Some(api_key) = api_key {
        validate_environment_secret(api_key, "LLM_SERVICE_API_KEY")?;
        config.llm.api_key = api_key.to_string();
    }
    Ok(())
}

pub fn resolve_config_environment(
    content: &str,
    llm_service_address: Option<&str>,
    reset_admin_password: Option<&str>,
    secret_key: Option<&str>,
    api_key: Option<&str>,
) -> std::io::Result<String> {
    let mut resolved = content.to_string();
    if resolved.contains(LLM_SERVICE_ADDRESS_PLACEHOLDER) {
        let llm_service_address =
            required_environment_value(llm_service_address, "LLM_SERVICE_ADDRESS")?;
        resolved = replace_toml_string_placeholder(
            &resolved,
            LLM_SERVICE_ADDRESS_PLACEHOLDER,
            llm_service_address,
        );
    }
    if resolved.contains(RESET_ADMIN_PASSWORD_PLACEHOLDER) {
        let reset_admin_password =
            required_environment_value(reset_admin_password, "RESET_ADMIN_PASSWORD")?;
        let reset_admin_password = parse_reset_admin_password(reset_admin_password)?;
        resolved = resolved.replace(
            &format!("\"{RESET_ADMIN_PASSWORD_PLACEHOLDER}\""),
            if reset_admin_password {
                "true"
            } else {
                "false"
            },
        );
    }
    for (placeholder, value, name) in [
        (SECRET_KEY_PLACEHOLDER, secret_key, "SECRET_KEY"),
        (
            LLM_SERVICE_API_KEY_PLACEHOLDER,
            api_key,
            "LLM_SERVICE_API_KEY",
        ),
    ] {
        if resolved.contains(placeholder) {
            let value = required_environment_value(value, name)?;
            validate_environment_secret(value, name)?;
            resolved = replace_toml_string_placeholder(&resolved, placeholder, value);
        }
    }
    Ok(resolved)
}

fn required_environment_value<'a>(value: Option<&'a str>, name: &str) -> std::io::Result<&'a str> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| std::io::Error::other(format!("{name} must not be empty")))
}

fn parse_reset_admin_password(value: &str) -> std::io::Result<bool> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(std::io::Error::other(
            "RESET_ADMIN_PASSWORD must be true or false",
        )),
    }
}

fn validate_environment_secret(value: &str, name: &str) -> std::io::Result<()> {
    if value.trim().is_empty() {
        return Err(std::io::Error::other(format!("{name} must not be empty")));
    }
    Ok(())
}

fn replace_toml_string_placeholder(content: &str, placeholder: &str, value: &str) -> String {
    content.replace(
        &format!("\"{placeholder}\""),
        &toml::Value::String(value.to_string()).to_string(),
    )
}

pub fn save_default_config(config_path: &Path) -> std::io::Result<()> {
    write_new_config(config_path, default_config_template())
}

pub fn consume_admin_password_reset(
    config_path: &Path,
    config: &mut Config,
) -> std::io::Result<bool> {
    if !config.server.reset_admin_password {
        return Ok(false);
    }

    let contents = fs::read_to_string(config_path)?;
    let mut document = contents.parse::<DocumentMut>().map_err(|error| {
        std::io::Error::other(format!(
            "invalid config at {}: {error}",
            config_path.display()
        ))
    })?;
    let server = document
        .get_mut("server")
        .and_then(toml_edit::Item::as_table_mut)
        .ok_or_else(|| std::io::Error::other("config server section is missing"))?;
    server["reset_admin_password"] = value(false);
    replace_config(config_path, &document.to_string())?;
    config.server.reset_admin_password = false;
    Ok(true)
}
