use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::error::ServiceError;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub service: Vec<ServiceConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_file_path")]
    pub file_path: PathBuf,
}

fn default_log_file_path() -> PathBuf {
    PathBuf::from("/data/logs/llm-service.log")
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            file_path: default_log_file_path(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct GeneralConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_max_request_bytes")]
    pub max_request_bytes: usize,
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    8100
}

fn default_max_request_bytes() -> usize {
    50 * 1024 * 1024
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            api_key: String::new(),
            max_request_bytes: default_max_request_bytes(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Baidu,
    #[default]
    Local,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServiceConfig {
    #[serde(default)]
    pub enabled: bool,
    pub model_type: String,
    pub model_version: String,
    #[serde(default)]
    pub provider: ProviderKind,
    #[serde(default)]
    pub docker_command: Vec<String>,
    #[serde(default = "default_device")]
    pub device: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub script_path: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub secret_key: String,
    #[serde(default = "default_baidu_token_url")]
    pub token_url: String,
    #[serde(default = "default_baidu_ocr_url")]
    pub ocr_url: String,
    #[serde(default = "default_max_image_width")]
    pub max_image_width: u32,
    #[serde(default = "default_max_image_height")]
    pub max_image_height: u32,
    #[serde(default = "default_startup_timeout_seconds")]
    pub startup_timeout_seconds: u64,
    #[serde(default = "default_request_timeout_seconds")]
    pub request_timeout_seconds: u64,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
}

fn default_baidu_token_url() -> String {
    "https://aip.baidubce.com/oauth/2.0/token".to_string()
}

fn default_baidu_ocr_url() -> String {
    "https://aip.baidubce.com/rest/2.0/ocr/v1/general".to_string()
}

fn default_max_image_width() -> u32 {
    4096
}

fn default_device() -> String {
    "auto".to_string()
}

fn default_max_image_height() -> u32 {
    16384
}

fn default_startup_timeout_seconds() -> u64 {
    300
}

fn default_request_timeout_seconds() -> u64 {
    180
}

fn default_max_tokens() -> u32 {
    8192
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ServiceError> {
        let content = std::fs::read_to_string(path).map_err(|error| {
            ServiceError::Configuration(format!("failed to read {}: {error}", path.display()))
        })?;
        let config = toml::from_str::<Self>(&content).map_err(|error| {
            ServiceError::Configuration(format!("failed to parse {}: {error}", path.display()))
        })?;
        config.validate()?;
        Ok(config)
    }

    pub fn service_for(&self, model_type: &str) -> Option<&ServiceConfig> {
        self.service
            .iter()
            .find(|service| service.enabled && service.model_type == model_type)
    }

    pub fn validate(&self) -> Result<(), ServiceError> {
        if self.general.host.trim().is_empty() {
            return Err(ServiceError::Configuration(
                "general.host must not be empty".to_string(),
            ));
        }
        if self.general.port == 0 {
            return Err(ServiceError::Configuration(
                "general.port must be greater than zero".to_string(),
            ));
        }
        if self.general.max_request_bytes == 0 {
            return Err(ServiceError::Configuration(
                "general.max_request_bytes must be greater than zero".to_string(),
            ));
        }
        if self.service.is_empty() {
            return Err(ServiceError::Configuration(
                "at least one service must be configured".to_string(),
            ));
        }

        for service in &self.service {
            self.validate_service(service)?;
        }
        if self
            .service
            .iter()
            .filter(|service| service.enabled)
            .all(|service| service.model_type != "ocr")
        {
            return Err(ServiceError::Configuration(
                "an enabled ocr service is required".to_string(),
            ));
        }
        Ok(())
    }

    fn validate_service(&self, service: &ServiceConfig) -> Result<(), ServiceError> {
        if service.model_type.trim().is_empty() || service.model_version.trim().is_empty() {
            return Err(ServiceError::Configuration(
                "service model_type and model_version are required".to_string(),
            ));
        }
        if !service.enabled {
            return Ok(());
        }

        match service.model_type.as_str() {
            "ocr" => self.validate_ocr_service(service),
            "image_tagging" => self.validate_image_tagging_service(service),
            model_type => Err(ServiceError::Configuration(format!(
                "unsupported service model_type: {model_type}"
            ))),
        }
    }

    fn validate_ocr_service(&self, service: &ServiceConfig) -> Result<(), ServiceError> {
        match service.provider {
            ProviderKind::Baidu => {
                if service.api_key.trim().is_empty() || service.secret_key.trim().is_empty() {
                    return Err(ServiceError::Configuration(
                        "baidu service api_key and secret_key are required".to_string(),
                    ));
                }
            }
            ProviderKind::Local => {
                if service.docker_command.is_empty()
                    || service.base_url.trim().is_empty()
                    || service.model.trim().is_empty()
                {
                    return Err(ServiceError::Configuration(
                        "local OCR docker_command, base_url, and model are required".to_string(),
                    ));
                }
                if service.max_image_width == 0
                    || service.max_image_height == 0
                    || service.startup_timeout_seconds == 0
                    || service.request_timeout_seconds == 0
                    || service.max_tokens == 0
                {
                    return Err(ServiceError::Configuration(
                        "local OCR image limits, timeouts, and max_tokens must be positive"
                            .to_string(),
                    ));
                }
            }
        }
        Ok(())
    }

    fn validate_image_tagging_service(&self, service: &ServiceConfig) -> Result<(), ServiceError> {
        if service.docker_command.is_empty()
            || service.base_url.trim().is_empty()
            || service.script_path.trim().is_empty()
        {
            return Err(ServiceError::Configuration(
                "image tagging docker_command, base_url, and script_path are required".to_string(),
            ));
        }
        if service.startup_timeout_seconds == 0 || service.request_timeout_seconds == 0 {
            return Err(ServiceError::Configuration(
                "image tagging timeouts must be positive".to_string(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_service() -> ServiceConfig {
        ServiceConfig {
            enabled: true,
            model_type: "ocr".to_string(),
            model_version: "unlimited_ocr".to_string(),
            provider: ProviderKind::Local,
            docker_command: vec!["vllm".to_string()],
            device: default_device(),
            base_url: "http://127.0.0.1:8000/v1".to_string(),
            model: "baidu/Unlimited-OCR".to_string(),
            script_path: String::new(),
            api_key: String::new(),
            secret_key: String::new(),
            token_url: default_baidu_token_url(),
            ocr_url: default_baidu_ocr_url(),
            max_image_width: 4096,
            max_image_height: 16384,
            startup_timeout_seconds: 10,
            request_timeout_seconds: 10,
            max_tokens: 100,
        }
    }

    fn local_config() -> Config {
        Config {
            general: GeneralConfig::default(),
            logging: LoggingConfig::default(),
            service: vec![local_service()],
        }
    }

    #[test]
    fn local_provider_configuration_is_valid() {
        assert!(local_config().validate().is_ok());
    }

    #[test]
    fn baidu_provider_requires_credentials() {
        let mut config = local_config();
        config.service[0].provider = ProviderKind::Baidu;
        assert!(config.validate().is_err());
    }

    #[test]
    fn image_tagging_service_is_valid() {
        let mut config = local_config();
        config.service.push(ServiceConfig {
            enabled: true,
            model_type: "image_tagging".to_string(),
            model_version: "ram++".to_string(),
            provider: ProviderKind::Local,
            docker_command: vec!["docker".to_string()],
            device: default_device(),
            base_url: "http://127.0.0.1:8200".to_string(),
            model: String::new(),
            script_path: "ram_server.py".to_string(),
            api_key: String::new(),
            secret_key: String::new(),
            token_url: default_baidu_token_url(),
            ocr_url: default_baidu_ocr_url(),
            max_image_width: 0,
            max_image_height: 0,
            startup_timeout_seconds: 10,
            request_timeout_seconds: 10,
            max_tokens: 0,
        });
        assert!(config.validate().is_ok());
    }
}
