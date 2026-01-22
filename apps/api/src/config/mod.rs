mod settings;

use crate::constants::{
    DEFAULT_THUMBNAIL_QUALITY, DEFAULT_THUMBNAIL_SIZE, DEFAULT_TINY_THUMBNAIL_SIZE,
    DEFAULT_VIDEO_FRAME_QUALITY,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

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
pub struct SecurityConfig {
    #[serde(default = "default_secret_key")]
    pub secret_key: String,
    #[serde(default = "default_algorithm")]
    pub algorithm: String,
    #[serde(default = "default_access_token_expire_minutes")]
    pub access_token_expire_minutes: i64,
    #[serde(default = "default_refresh_token_expire_days")]
    pub refresh_token_expire_days: i64,
}

fn default_secret_key() -> String {
    "change-me-in-production-use-openssl-rand-hex-32".to_string()
}

fn default_algorithm() -> String {
    "HS256".to_string()
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
            algorithm: default_algorithm(),
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
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_remote_path")]
    pub remote_path: String,
}

fn default_remote_path() -> String {
    "/".to_string()
}

impl Default for WebDAVConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            hostname: String::new(),
            username: String::new(),
            password: String::new(),
            remote_path: default_remote_path(),
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
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
}

pub fn load_config(config_path: &Path) -> Config {
    if !config_path.exists() {
        return Config::default();
    }

    match fs::read_to_string(config_path) {
        Ok(content) => serde_yaml::from_str(&content).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

pub fn save_default_config(config_path: &Path) -> std::io::Result<()> {
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let config = Config::default();
    let yaml = serde_yaml::to_string(&config).map_err(|e| std::io::Error::other(e.to_string()))?;
    fs::write(config_path, yaml)
}
