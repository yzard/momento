mod settings;

use crate::constants::{
    DEFAULT_THUMBNAIL_QUALITY, DEFAULT_THUMBNAIL_SIZE, DEFAULT_TINY_THUMBNAIL_SIZE,
    DEFAULT_VIDEO_FRAME_QUALITY,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub debug: bool,
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    8000
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            debug: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_file_path")]
    pub file_path: PathBuf,
}

fn default_log_file_path() -> PathBuf {
    PathBuf::from("/data/logs/momento-api.log")
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            file_path: default_log_file_path(),
        }
    }
}

/// Filesystem locations. `data_dir` is the root every media directory and the database
/// are derived from; `static_dir` holds the built frontend the server falls back to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    #[serde(default = "default_static_dir")]
    pub static_dir: PathBuf,
}

fn default_data_dir() -> PathBuf {
    PathBuf::from("/data")
}

fn default_static_dir() -> PathBuf {
    PathBuf::from("/app/static")
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: default_data_dir(),
            static_dir: default_static_dir(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityConfig {
    #[serde(default = "default_secret_key")]
    pub secret_key: String,
    #[serde(default = "default_access_token_expire_minutes")]
    pub access_token_expire_minutes: i64,
    #[serde(default = "default_refresh_token_expire_days")]
    pub refresh_token_expire_days: i64,
}

fn default_secret_key() -> String {
    "change-me-in-production-use-openssl-rand-hex-32".to_string()
}

fn default_access_token_expire_minutes() -> i64 {
    30
}

fn default_refresh_token_expire_days() -> i64 {
    7
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            secret_key: default_secret_key(),
            access_token_expire_minutes: default_access_token_expire_minutes(),
            refresh_token_expire_days: default_refresh_token_expire_days(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminConfig {
    #[serde(default = "default_admin_username")]
    pub username: String,
    #[serde(default = "default_admin_password")]
    pub password: String,
}

fn default_admin_username() -> String {
    "admin".to_string()
}

fn default_admin_password() -> String {
    "admin".to_string()
}

impl Default for AdminConfig {
    fn default() -> Self {
        Self {
            username: default_admin_username(),
            password: default_admin_password(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebDAVConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_webdav_mount_path")]
    pub mount_path: String,
    #[serde(default = "default_webdav_realm")]
    pub realm: String,
    #[serde(default)]
    pub limits: WebDAVLimits,
    #[serde(default)]
    pub processing: WebDAVProcessing,
}

fn default_webdav_mount_path() -> String {
    "/webdav".to_string()
}

fn default_webdav_realm() -> String {
    "Momento WebDAV".to_string()
}

impl Default for WebDAVConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mount_path: default_webdav_mount_path(),
            realm: default_webdav_realm(),
            limits: WebDAVLimits::default(),
            processing: WebDAVProcessing::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebDAVLimits {
    #[serde(default = "default_max_upload_bytes")]
    pub max_upload_bytes: u64,
    #[serde(default = "default_max_concurrent_requests")]
    pub max_concurrent_requests: usize,
}

fn default_max_upload_bytes() -> u64 {
    10 * 1024 * 1024 * 1024
}

fn default_max_concurrent_requests() -> usize {
    16
}

impl Default for WebDAVLimits {
    fn default() -> Self {
        Self {
            max_upload_bytes: default_max_upload_bytes(),
            max_concurrent_requests: default_max_concurrent_requests(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebDAVProcessing {
    #[serde(default = "default_poll_interval")]
    pub poll_interval_seconds: u64,
    #[serde(default = "default_stable_file_age")]
    pub stable_file_age_seconds: u64,
    #[serde(default = "default_max_concurrent_processing")]
    pub max_concurrent_processing: usize,
}

fn default_poll_interval() -> u64 {
    5
}

fn default_stable_file_age() -> u64 {
    10
}

fn default_max_concurrent_processing() -> usize {
    2
}

impl Default for WebDAVProcessing {
    fn default() -> Self {
        Self {
            poll_interval_seconds: default_poll_interval(),
            stable_file_age_seconds: default_stable_file_age(),
            max_concurrent_processing: default_max_concurrent_processing(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThumbnailConfig {
    #[serde(default = "default_max_size")]
    pub max_size: u32,
    #[serde(default = "default_tiny_size")]
    pub tiny_size: u32,
    #[serde(default = "default_quality")]
    pub quality: u8,
    #[serde(default = "default_video_frame_quality")]
    pub video_frame_quality: u8,
}

fn default_max_size() -> u32 {
    DEFAULT_THUMBNAIL_SIZE
}

fn default_tiny_size() -> u32 {
    DEFAULT_TINY_THUMBNAIL_SIZE
}

fn default_quality() -> u8 {
    DEFAULT_THUMBNAIL_QUALITY
}

fn default_video_frame_quality() -> u8 {
    DEFAULT_VIDEO_FRAME_QUALITY
}

impl Default for ThumbnailConfig {
    fn default() -> Self {
        Self {
            max_size: default_max_size(),
            tiny_size: default_tiny_size(),
            quality: default_quality(),
            video_frame_quality: default_video_frame_quality(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReverseGeocodingConfig {
    #[serde(default = "default_geo_enabled")]
    pub enabled: bool,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default = "default_user_agent")]
    pub user_agent: String,
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u64,
    #[serde(default = "default_rate_limit_seconds")]
    pub rate_limit_seconds: f64,
}

fn default_geo_enabled() -> bool {
    true
}

fn default_base_url() -> String {
    "https://nominatim.openstreetmap.org/reverse".to_string()
}

fn default_user_agent() -> String {
    "Momento/1.0 (self-hosted)".to_string()
}

fn default_timeout_seconds() -> u64 {
    10
}

fn default_rate_limit_seconds() -> f64 {
    1.0
}

impl Default for ReverseGeocodingConfig {
    fn default() -> Self {
        Self {
            enabled: default_geo_enabled(),
            base_url: default_base_url(),
            user_agent: default_user_agent(),
            timeout_seconds: default_timeout_seconds(),
            rate_limit_seconds: default_rate_limit_seconds(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegenerateConfig {
    #[serde(default = "default_regenerate_num_cpus")]
    pub num_cpus: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetadataWorkerConfig {
    #[serde(default = "default_metadata_poll_interval_seconds")]
    pub poll_interval_seconds: u64,
    #[serde(default = "default_metadata_concurrency")]
    pub concurrency: usize,
    #[serde(default = "default_metadata_lease_seconds")]
    pub lease_seconds: u64,
    #[serde(default = "default_metadata_max_attempts")]
    pub max_attempts: u32,
}

fn default_metadata_poll_interval_seconds() -> u64 {
    10
}

fn default_metadata_concurrency() -> usize {
    num_cpus::get()
}
fn default_metadata_lease_seconds() -> u64 {
    300
}
fn default_metadata_max_attempts() -> u32 {
    5
}

impl Default for MetadataWorkerConfig {
    fn default() -> Self {
        Self {
            poll_interval_seconds: default_metadata_poll_interval_seconds(),
            concurrency: default_metadata_concurrency(),
            lease_seconds: default_metadata_lease_seconds(),
            max_attempts: default_metadata_max_attempts(),
        }
    }
}

fn default_regenerate_num_cpus() -> usize {
    num_cpus::get()
}

impl Default for RegenerateConfig {
    fn default() -> Self {
        Self {
            num_cpus: default_regenerate_num_cpus(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlmConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_llm_service_url")]
    pub service_url: String,
    #[serde(default)]
    pub callback_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub callback_key: String,
    #[serde(default)]
    pub image_tagging_enabled: bool,
    #[serde(default)]
    pub deduplicate_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CronjobConfig {
    pub timezone: String,
    pub deduplicate_cron: String,
}

impl Default for CronjobConfig {
    fn default() -> Self {
        Self {
            timezone: "Etc/UTC".to_string(),
            deduplicate_cron: "0 3 * * *".to_string(),
        }
    }
}

impl CronjobConfig {
    fn validate(&self) -> std::io::Result<()> {
        self.timezone
            .parse::<chrono_tz::Tz>()
            .map_err(|error| std::io::Error::other(format!("invalid cronjob timezone: {error}")))?;
        let normalized_cron = format!("0 {} *", self.deduplicate_cron);
        normalized_cron.parse::<cron::Schedule>().map_err(|error| {
            std::io::Error::other(format!("invalid deduplicate cronjob: {error}"))
        })?;
        Ok(())
    }
}

fn default_llm_service_url() -> String {
    "http://127.0.0.1:8100".to_string()
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            service_url: default_llm_service_url(),
            callback_url: String::new(),
            api_key: String::new(),
            callback_key: String::new(),
            image_tagging_enabled: false,
            deduplicate_enabled: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmSubmissionWorkerConfig {
    #[serde(default = "default_llm_submission_poll_interval_seconds")]
    pub poll_interval_seconds: u64,
    #[serde(default = "default_llm_submission_batch_size")]
    pub batch_size: usize,
}

fn default_llm_submission_poll_interval_seconds() -> u64 {
    5
}
fn default_llm_submission_batch_size() -> usize {
    64
}

impl Default for LlmSubmissionWorkerConfig {
    fn default() -> Self {
        Self {
            poll_interval_seconds: default_llm_submission_poll_interval_seconds(),
            batch_size: default_llm_submission_batch_size(),
        }
    }
}

impl LlmConfig {
    fn validate(&self) -> std::io::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        if self.service_url.trim().is_empty() {
            return Err(std::io::Error::other(
                "llm service_url is required when LLM is enabled",
            ));
        }
        if self.callback_url.trim().is_empty() {
            return Err(std::io::Error::other(
                "llm callback_url is required when LLM is enabled",
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
    pub logging: LoggingConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub admin: AdminConfig,
    #[serde(default)]
    pub webdav: WebDAVConfig,
    #[serde(default)]
    pub thumbnails: ThumbnailConfig,
    #[serde(default)]
    pub reverse_geocoding: ReverseGeocodingConfig,
    #[serde(default)]
    pub regenerate: RegenerateConfig,
    #[serde(default)]
    pub metadata_worker: MetadataWorkerConfig,
    #[serde(default)]
    pub llm_submission_worker: LlmSubmissionWorkerConfig,
    #[serde(default)]
    pub llm: LlmConfig,
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

    let config: Config = toml::from_str(&content).map_err(|e| {
        std::io::Error::other(format!("invalid config at {}: {e}", config_path.display()))
    })?;
    config.llm.validate()?;
    config.cronjob.validate()?;
    Ok(config)
}

pub fn save_default_config(config_path: &Path) -> std::io::Result<()> {
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let config = Config::default();
    let toml = toml::to_string_pretty(&config).map_err(|e| std::io::Error::other(e.to_string()))?;
    fs::write(config_path, toml)
}
