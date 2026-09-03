mod defaults;
mod settings;

use serde::{Deserialize, Serialize};
use std::env;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use momento_common::config_file::write_new_config;
use tokio::sync::{watch, Mutex};
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
        }
    }
}

impl ServerConfig {
    fn validate(&self) -> std::io::Result<()> {
        if self.api_request_body_max_bytes == 0 {
            return Err(std::io::Error::other(
                "server API request body limit must be positive",
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
    #[serde(default = "defaults::password_attempt_window_seconds")]
    pub password_attempt_window_seconds: u64,
    #[serde(default = "defaults::password_attempts_per_identity")]
    pub password_attempts_per_identity: u32,
    #[serde(default = "defaults::password_attempts_per_source")]
    pub password_attempts_per_source: u32,
    #[serde(default = "defaults::password_lockout_seconds")]
    pub password_lockout_seconds: u64,
    #[serde(default = "defaults::trusted_proxy_ip_addresses")]
    pub trusted_proxy_ip_addresses: Vec<IpAddr>,
    #[serde(default = "defaults::refresh_token_cleanup_interval_seconds")]
    pub refresh_token_cleanup_interval_seconds: u64,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            secret_key: defaults::security_secret_key(),
            access_token_expire_minutes: defaults::ACCESS_TOKEN_EXPIRE_MINUTES,
            refresh_token_expire_days: defaults::REFRESH_TOKEN_EXPIRE_DAYS,
            media_access_ticket_expire_hours: defaults::MEDIA_ACCESS_TICKET_EXPIRE_HOURS,
            share_session_expire_hours: defaults::SHARE_SESSION_EXPIRE_HOURS,
            password_attempt_window_seconds: defaults::PASSWORD_ATTEMPT_WINDOW_SECONDS,
            password_attempts_per_identity: defaults::PASSWORD_ATTEMPTS_PER_IDENTITY,
            password_attempts_per_source: defaults::PASSWORD_ATTEMPTS_PER_SOURCE,
            password_lockout_seconds: defaults::PASSWORD_LOCKOUT_SECONDS,
            trusted_proxy_ip_addresses: defaults::trusted_proxy_ip_addresses(),
            refresh_token_cleanup_interval_seconds:
                defaults::REFRESH_TOKEN_CLEANUP_INTERVAL_SECONDS,
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
        if self.password_attempt_window_seconds == 0
            || self.password_attempts_per_identity == 0
            || self.password_attempts_per_source == 0
            || self.password_lockout_seconds == 0
            || self.refresh_token_cleanup_interval_seconds == 0
        {
            return Err(std::io::Error::other(
                "security password limits and refresh-token cleanup interval must be positive",
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
    #[serde(default = "defaults::webdav_stable_file_age_seconds")]
    pub stable_file_age_seconds: u64,
}

impl Default for WebDAVConfig {
    fn default() -> Self {
        Self {
            mount_path: defaults::webdav_mount_path(),
            realm: defaults::webdav_realm(),
            max_upload_bytes: defaults::WEBDAV_MAX_UPLOAD_BYTES,
            stable_file_age_seconds: defaults::WEBDAV_STABLE_FILE_AGE_SECONDS,
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
    #[serde(default = "defaults::backup_session_expiry_hours")]
    pub session_expiry_hours: u64,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            max_upload_bytes: defaults::BACKUP_MAX_UPLOAD_BYTES,
            max_chunk_bytes: defaults::BACKUP_MAX_CHUNK_BYTES,
            session_expiry_hours: defaults::BACKUP_SESSION_EXPIRY_HOURS,
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
        if self.session_expiry_hours == 0 {
            return Err(std::io::Error::other(
                "backup session expiry must be positive",
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
}

impl Default for MetadataConfig {
    fn default() -> Self {
        Self {
            thumbnails_max_size: defaults::fallback::THUMBNAILS_MAX_SIZE,
            thumbnails_tiny_size: defaults::fallback::THUMBNAILS_TINY_SIZE,
            thumbnails_quality: defaults::fallback::THUMBNAILS_QUALITY,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaProcessConfig {
    #[serde(default = "defaults::media_process_maximum_stderr_bytes")]
    pub maximum_stderr_bytes: usize,
    #[serde(default = "defaults::media_process_maximum_metadata_output_bytes")]
    pub maximum_metadata_output_bytes: usize,
    #[serde(default = "defaults::media_process_maximum_normalized_image_output_bytes")]
    pub maximum_normalized_image_output_bytes: usize,
    #[serde(default = "defaults::media_process_maximum_decoded_image_pixels")]
    pub maximum_decoded_image_pixels: u64,
}

impl Default for MediaProcessConfig {
    fn default() -> Self {
        Self {
            maximum_stderr_bytes: defaults::MEDIA_PROCESS_MAXIMUM_STDERR_BYTES,
            maximum_metadata_output_bytes: defaults::MEDIA_PROCESS_MAXIMUM_METADATA_OUTPUT_BYTES,
            maximum_normalized_image_output_bytes:
                defaults::MEDIA_PROCESS_MAXIMUM_NORMALIZED_IMAGE_OUTPUT_BYTES,
            maximum_decoded_image_pixels: defaults::MEDIA_PROCESS_MAXIMUM_DECODED_IMAGE_PIXELS,
        }
    }
}

impl MediaProcessConfig {
    fn validate(&self) -> std::io::Result<()> {
        if self.maximum_stderr_bytes == 0
            || self.maximum_metadata_output_bytes == 0
            || self.maximum_normalized_image_output_bytes == 0
            || self.maximum_decoded_image_pixels == 0
        {
            return Err(std::io::Error::other(
                "media_process limits must all be positive",
            ));
        }
        if self.maximum_metadata_output_bytes > 4 * 1024 * 1024 {
            return Err(std::io::Error::other(
                "media_process maximum_metadata_output_bytes must not exceed 4194304",
            ));
        }
        Ok(())
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

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            enabled: defaults::fallback::LLM_ENABLED,
            server_address: defaults::llm_server_address(),
            client_id: defaults::fallback::LLM_CLIENT_ID.to_string(),
            api_key: defaults::fallback::LLM_API_KEY.to_string(),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreadPoolConfig {
    pub cpu_workers: usize,
    pub io_workers: usize,
    pub sqlite_workers: usize,
}

impl Default for ThreadPoolConfig {
    fn default() -> Self {
        Self {
            cpu_workers: defaults::THREAD_POOL_CPU_WORKERS,
            io_workers: defaults::THREAD_POOL_IO_WORKERS,
            sqlite_workers: defaults::THREAD_POOL_SQLITE_WORKERS,
        }
    }
}

impl ThreadPoolConfig {
    fn validate(&self) -> std::io::Result<()> {
        crate::runtime::RuntimeSizing::validate_worker_counts(self)
            .map(|_| ())
            .map_err(std::io::Error::other)
    }
}

impl LlmConfig {
    fn validate(&self) -> std::io::Result<()> {
        for (name, expression) in [
            ("ocr", &self.ocr_cron),
            ("image_tagging", &self.image_tagging_cron),
            ("deduplicate", &self.deduplicate_cron),
            ("face_detection", &self.face_detection_cron),
            ("image_aesthetics", &self.image_aesthetics_cron),
            ("screenshot_detection", &self.screenshot_detection_cron),
            ("document_detection", &self.document_detection_cron),
        ] {
            validate_ai_cron_expression(name, expression)?;
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
pub struct FaceGroupConfig {
    #[serde(default = "defaults::face_group_similarity_threshold")]
    pub similarity_threshold: f32,
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

impl Default for FaceGroupConfig {
    fn default() -> Self {
        Self {
            similarity_threshold: defaults::FACE_GROUP_SIMILARITY_THRESHOLD,
            confidence_weight: defaults::FACE_REPRESENTATIVE_CONFIDENCE_WEIGHT,
            face_size_weight: defaults::FACE_REPRESENTATIVE_FACE_SIZE_WEIGHT,
            center_proximity_weight: defaults::FACE_REPRESENTATIVE_CENTER_PROXIMITY_WEIGHT,
            frontality_weight: defaults::FACE_REPRESENTATIVE_FRONTALITY_WEIGHT,
            visibility_weight: defaults::FACE_REPRESENTATIVE_VISIBILITY_WEIGHT,
            feature_clarity_weight: defaults::FACE_REPRESENTATIVE_FEATURE_CLARITY_WEIGHT,
        }
    }
}

impl FaceGroupConfig {
    fn validate(&self) -> std::io::Result<()> {
        if !self.similarity_threshold.is_finite()
            || !(0.0..=1.0).contains(&self.similarity_threshold)
        {
            return Err(std::io::Error::other(
                "face_group similarity_threshold must be within [0, 1]",
            ));
        }
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
                    "face_group {name} must be finite and non-negative"
                )));
            }
        }
        let total_weight = weights.iter().map(|(_, weight)| weight).sum::<f64>();
        if (total_weight - 1.0).abs() > 1e-6 {
            return Err(std::io::Error::other("face_group weights must sum to 1"));
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
    pub media_process: MediaProcessConfig,
    #[serde(default)]
    pub thread_pool: ThreadPoolConfig,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub face_group: FaceGroupConfig,
}

#[derive(Clone)]
pub struct ConfigManager {
    config_sender: watch::Sender<Arc<Config>>,
    update_lock: Arc<Mutex<()>>,
    config_identity: Arc<Mutex<crate::runtime::ConfigFileIdentity>>,
    cpu: crate::executor::CpuExecutorHandle,
    file_io: crate::executor::FileIoExecutorHandle,
    scheduler: crate::runtime::SchedulerHandle,
}

#[derive(Debug)]
pub struct LoadedConfig {
    pub config: Config,
    pub identity: crate::runtime::ConfigFileIdentity,
}

impl ConfigManager {
    pub fn new(loaded: LoadedConfig, executors: &crate::runtime::ExecutorHandles) -> Self {
        let (config_sender, _) = watch::channel(Arc::new(loaded.config));
        Self {
            config_sender,
            update_lock: Arc::new(Mutex::new(())),
            config_identity: Arc::new(Mutex::new(loaded.identity)),
            cpu: executors.cpu.clone(),
            file_io: executors.file_io.clone(),
            scheduler: executors.scheduler.clone(),
        }
    }

    pub fn current(&self) -> Arc<Config> {
        Arc::clone(&self.config_sender.borrow())
    }

    pub fn subscribe(&self) -> watch::Receiver<Arc<Config>> {
        self.config_sender.subscribe()
    }

    pub async fn update_llm_cron_expression(
        &self,
        config_field_name: &'static str,
        feature_name: &'static str,
        cron_expression: String,
    ) -> std::io::Result<String> {
        let cron_expression = cron_expression
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        validate_ai_cron_expression(feature_name, &cron_expression)?;
        let _update_guard = self.update_lock.lock().await;
        let identity = self.config_identity.lock().await.clone();
        let contents = self
            .file_io
            .read_config_durable(identity.clone())
            .await
            .map_err(executor_io_error)?;
        let (updated_contents, updated_config) = self
            .cpu
            .prepare_llm_cron_update_durable(
                identity.clone(),
                contents,
                config_field_name,
                feature_name,
                cron_expression.clone(),
            )
            .await
            .map_err(executor_io_error)?;
        let updated_identity = self
            .file_io
            .replace_config_durable(identity, updated_contents)
            .await
            .map_err(executor_io_error)?;
        *self.config_identity.lock().await = updated_identity;
        self.config_sender.send_replace(Arc::new(updated_config));
        self.scheduler
            .signal_control(crate::runtime::SchedulerControlSource::ConfigChanged);
        Ok(cron_expression)
    }

    pub async fn consume_admin_password_reset(&self) -> std::io::Result<bool> {
        if !self.current().server.reset_admin_password {
            return Ok(false);
        }
        let _update_guard = self.update_lock.lock().await;
        if !self.current().server.reset_admin_password {
            return Ok(false);
        }
        let identity = self.config_identity.lock().await.clone();
        let contents = self
            .file_io
            .read_config_durable(identity.clone())
            .await
            .map_err(executor_io_error)?;
        let (updated_contents, updated_config) = self
            .cpu
            .prepare_admin_password_reset_update_durable(identity.clone(), contents)
            .await
            .map_err(executor_io_error)?;
        let updated_identity = self
            .file_io
            .replace_config_durable(identity, updated_contents)
            .await
            .map_err(executor_io_error)?;
        *self.config_identity.lock().await = updated_identity;
        self.config_sender.send_replace(Arc::new(updated_config));
        self.scheduler
            .signal_control(crate::runtime::SchedulerControlSource::ConfigChanged);
        Ok(true)
    }
}

fn executor_io_error(error: crate::executor::ExecutorError) -> std::io::Error {
    std::io::Error::other(error.to_string())
}

/// Reads and parses the config file. A missing or malformed file is an error: silently
/// falling back to defaults would start the server against the wrong data directory.
pub fn load_config(config_path: &Path) -> std::io::Result<Config> {
    load_config_with_identity(config_path).map(|loaded| loaded.config)
}

pub fn load_config_with_identity(config_path: &Path) -> std::io::Result<LoadedConfig> {
    let config_file = crate::runtime::config_bootstrap::read_existing_config(config_path)?;
    let config =
        parse_config_contents(&config_file.identity.canonical_path, &config_file.contents)?;
    Ok(LoadedConfig {
        config,
        identity: config_file.identity,
    })
}

fn parse_config_contents(config_path: &Path, content: &str) -> std::io::Result<Config> {
    let llm_service_address = read_environment_variable("LLM_SERVICE_ADDRESS")?;
    let reset_admin_password = read_environment_variable("RESET_ADMIN_PASSWORD")?;
    let secret_key = read_environment_variable("SECRET_KEY")?;
    let api_key = read_environment_variable("LLM_SERVICE_API_KEY")?;
    let content = resolve_config_environment(
        content,
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
    config.media_process.validate()?;
    config.thread_pool.validate()?;
    config.llm.validate()?;
    config.face_group.validate()?;
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

pub fn validate_ai_cron_expression(
    feature_name: &str,
    cron_expression: &str,
) -> std::io::Result<()> {
    let normalized_cron = format!("0 {cron_expression} *");
    normalized_cron
        .parse::<cron::Schedule>()
        .map(|_| ())
        .map_err(|error| std::io::Error::other(format!("invalid {feature_name} cronjob: {error}")))
}

pub(crate) fn prepare_llm_cron_update(
    config_path: &Path,
    contents: &str,
    config_field_name: &str,
    feature_name: &str,
    cron_expression: &str,
) -> std::io::Result<(String, Config)> {
    validate_ai_cron_expression(feature_name, cron_expression)?;
    let mut document = contents.parse::<DocumentMut>().map_err(|error| {
        std::io::Error::other(format!(
            "invalid config at {}: {error}",
            config_path.display()
        ))
    })?;
    if !document.contains_key("llm") {
        document["llm"] = toml_edit::table();
    }
    let llm = document
        .get_mut("llm")
        .and_then(toml_edit::Item::as_table_mut)
        .ok_or_else(|| std::io::Error::other("config llm section must be a table"))?;
    llm[config_field_name] = value(cron_expression);
    let updated_contents = document.to_string();
    let updated_config = parse_config_contents(config_path, &updated_contents)?;
    Ok((updated_contents, updated_config))
}

pub(crate) fn prepare_admin_password_reset_update(
    config_path: &Path,
    contents: &str,
) -> std::io::Result<(String, Config)> {
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
    let updated_contents = document.to_string();
    let updated_config = parse_config_contents(config_path, &updated_contents)?;
    Ok((updated_contents, updated_config))
}
