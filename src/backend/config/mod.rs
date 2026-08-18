mod settings;

use crate::constants::{
    DEFAULT_THUMBNAIL_QUALITY, DEFAULT_THUMBNAIL_SIZE, DEFAULT_TINY_THUMBNAIL_SIZE,
    DEFAULT_VIDEO_FRAME_QUALITY,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use momento_common::config_file::{replace_config, write_new_config};
use toml_edit::{value, DocumentMut};

pub const DEFAULT_CONFIG_TEMPLATE: &str = include_str!("default.toml");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub debug: bool,
    #[serde(default)]
    pub reset_admin_password: bool,
    /// Root for the database and every generated media directory.
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    /// Built frontend served as the HTTP fallback.
    #[serde(default = "default_static_dir")]
    pub static_dir: PathBuf,
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
            reset_admin_password: false,
            data_dir: default_data_dir(),
            static_dir: default_static_dir(),
        }
    }
}

fn default_data_dir() -> PathBuf {
    PathBuf::from("/data")
}

fn default_static_dir() -> PathBuf {
    PathBuf::from("/app/static")
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
#[serde(deny_unknown_fields)]
pub struct WebDAVConfig {
    #[serde(default = "default_webdav_mount_path")]
    pub mount_path: String,
    #[serde(default = "default_webdav_realm")]
    pub realm: String,
    #[serde(default = "default_max_upload_bytes")]
    pub max_upload_bytes: u64,
    #[serde(default = "default_max_concurrent_requests")]
    pub max_concurrent_requests: usize,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_seconds: u64,
    #[serde(default = "default_stable_file_age")]
    pub stable_file_age_seconds: u64,
    #[serde(default = "default_max_concurrent_processing")]
    pub max_concurrent_processing: usize,
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
            mount_path: default_webdav_mount_path(),
            realm: default_webdav_realm(),
            max_upload_bytes: default_max_upload_bytes(),
            max_concurrent_requests: default_max_concurrent_requests(),
            poll_interval_seconds: default_poll_interval(),
            stable_file_age_seconds: default_stable_file_age(),
            max_concurrent_processing: default_max_concurrent_processing(),
        }
    }
}

fn default_max_upload_bytes() -> u64 {
    10 * 1024 * 1024 * 1024
}

fn default_max_concurrent_requests() -> usize {
    16
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
pub struct MetadataConfig {
    #[serde(default = "default_thumbnails_max_size")]
    pub thumbnails_max_size: u32,
    #[serde(default = "default_thumbnails_tiny_size")]
    pub thumbnails_tiny_size: u32,
    #[serde(default = "default_thumbnails_quality")]
    pub thumbnails_quality: u8,
    #[serde(default = "default_thumbnails_video_frame_quality")]
    pub thumbnails_video_frame_quality: u8,
}

fn default_thumbnails_max_size() -> u32 {
    DEFAULT_THUMBNAIL_SIZE
}

fn default_thumbnails_tiny_size() -> u32 {
    DEFAULT_TINY_THUMBNAIL_SIZE
}

fn default_thumbnails_quality() -> u8 {
    DEFAULT_THUMBNAIL_QUALITY
}

fn default_thumbnails_video_frame_quality() -> u8 {
    DEFAULT_VIDEO_FRAME_QUALITY
}

impl Default for MetadataConfig {
    fn default() -> Self {
        Self {
            thumbnails_max_size: default_thumbnails_max_size(),
            thumbnails_tiny_size: default_thumbnails_tiny_size(),
            thumbnails_quality: default_thumbnails_quality(),
            thumbnails_video_frame_quality: default_thumbnails_video_frame_quality(),
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
    pub client_id: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub image_tagging_enabled: bool,
    #[serde(default)]
    pub deduplicate_enabled: bool,
    #[serde(default)]
    pub face_detection_enabled: bool,
    #[serde(default = "default_face_group_similarity_threshold")]
    pub face_group_similarity_threshold: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CronjobConfig {
    #[serde(default = "default_cronjob_timezone")]
    pub timezone: String,
    #[serde(default = "default_ocr_cron")]
    pub ocr_cron: String,
    #[serde(default = "default_image_tagging_cron")]
    pub image_tagging_cron: String,
    #[serde(default = "default_deduplicate_cron")]
    pub deduplicate_cron: String,
    #[serde(default = "default_face_detection_cron")]
    pub face_detection_cron: String,
}

fn default_cronjob_timezone() -> String {
    "Etc/UTC".to_string()
}

fn default_ocr_cron() -> String {
    "0 1 * * *".to_string()
}

fn default_image_tagging_cron() -> String {
    "0 2 * * *".to_string()
}

fn default_deduplicate_cron() -> String {
    "0 3 * * *".to_string()
}

fn default_face_detection_cron() -> String {
    "0 4 * * *".to_string()
}

impl Default for CronjobConfig {
    fn default() -> Self {
        Self {
            timezone: default_cronjob_timezone(),
            ocr_cron: default_ocr_cron(),
            image_tagging_cron: default_image_tagging_cron(),
            deduplicate_cron: default_deduplicate_cron(),
            face_detection_cron: default_face_detection_cron(),
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
        ] {
            let normalized_cron = format!("0 {expression} *");
            normalized_cron.parse::<cron::Schedule>().map_err(|error| {
                std::io::Error::other(format!("invalid {name} cronjob: {error}"))
            })?;
        }
        Ok(())
    }
}

fn default_llm_service_url() -> String {
    "ws://127.0.0.1:8100/api/v1/llm/connect".to_string()
}

fn default_face_group_similarity_threshold() -> f32 {
    0.55
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            service_url: default_llm_service_url(),
            client_id: String::new(),
            api_key: String::new(),
            image_tagging_enabled: false,
            deduplicate_enabled: false,
            face_detection_enabled: false,
            face_group_similarity_threshold: default_face_group_similarity_threshold(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlmSubmissionWorkerConfig {
    #[serde(default = "default_llm_submission_poll_interval_seconds")]
    pub poll_interval_seconds: u64,
    #[serde(default = "default_llm_submission_max_in_flight")]
    pub max_in_flight: usize,
}

fn default_llm_submission_poll_interval_seconds() -> u64 {
    5
}
fn default_llm_submission_max_in_flight() -> usize {
    128
}
impl Default for LlmSubmissionWorkerConfig {
    fn default() -> Self {
        Self {
            poll_interval_seconds: default_llm_submission_poll_interval_seconds(),
            max_in_flight: default_llm_submission_max_in_flight(),
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
        if self.service_url.trim().is_empty() {
            return Err(std::io::Error::other(
                "llm service_url is required when LLM is enabled",
            ));
        }
        if !self.service_url.starts_with("ws://") && !self.service_url.starts_with("wss://") {
            return Err(std::io::Error::other(
                "llm service_url must use ws:// or wss:// when LLM is enabled",
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
    pub metadata: MetadataConfig,
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
    config.webdav.validate()?;
    config.llm.validate()?;
    config.llm_submission_worker.validate()?;
    config.cronjob.validate()?;
    Ok(config)
}

pub fn save_default_config(config_path: &Path) -> std::io::Result<()> {
    write_new_config(config_path, DEFAULT_CONFIG_TEMPLATE)
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
