use serde::Deserialize;
use std::path::Path;

use crate::error::ServiceError;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    pub provider: ProviderKind,
    #[serde(default)]
    pub baidu: BaiduConfig,
    #[serde(default)]
    pub local: LocalConfig,
    #[serde(default)]
    pub limits: LimitsConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub api_key: String,
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    8100
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            api_key: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Baidu,
    Local,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BaiduConfig {
    #[serde(default = "default_baidu_token_url")]
    pub token_url: String,
    #[serde(default = "default_baidu_ocr_url")]
    pub ocr_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub secret_key: String,
    #[serde(default = "default_request_timeout_seconds")]
    pub request_timeout_seconds: u64,
}

fn default_baidu_token_url() -> String {
    "https://aip.baidubce.com/oauth/2.0/token".to_string()
}

fn default_baidu_ocr_url() -> String {
    "https://aip.baidubce.com/rest/2.0/ocr/v1/general".to_string()
}

fn default_request_timeout_seconds() -> u64 {
    180
}

impl Default for BaiduConfig {
    fn default() -> Self {
        Self {
            token_url: default_baidu_token_url(),
            ocr_url: default_baidu_ocr_url(),
            api_key: String::new(),
            secret_key: String::new(),
            request_timeout_seconds: default_request_timeout_seconds(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct LocalConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_local_base_url")]
    pub base_url: String,
    pub model: String,
    #[serde(default = "default_startup_timeout_seconds")]
    pub startup_timeout_seconds: u64,
    #[serde(default = "default_request_timeout_seconds")]
    pub request_timeout_seconds: u64,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
}

fn default_local_base_url() -> String {
    "http://127.0.0.1:8000/v1".to_string()
}

fn default_startup_timeout_seconds() -> u64 {
    300
}

fn default_max_tokens() -> u32 {
    8192
}

impl Default for LocalConfig {
    fn default() -> Self {
        Self {
            command: "vllm".to_string(),
            args: Vec::new(),
            base_url: default_local_base_url(),
            model: String::new(),
            startup_timeout_seconds: default_startup_timeout_seconds(),
            request_timeout_seconds: default_request_timeout_seconds(),
            max_tokens: default_max_tokens(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct LimitsConfig {
    #[serde(default = "default_max_request_bytes")]
    pub max_request_bytes: usize,
}

fn default_max_request_bytes() -> usize {
    50 * 1024 * 1024
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            max_request_bytes: default_max_request_bytes(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ServiceError> {
        let content = std::fs::read_to_string(path).map_err(|error| {
            ServiceError::Configuration(format!("failed to read {}: {error}", path.display()))
        })?;
        let config = serde_yaml::from_str::<Self>(&content).map_err(|error| {
            ServiceError::Configuration(format!("failed to parse {}: {error}", path.display()))
        })?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ServiceError> {
        if self.server.host.trim().is_empty() {
            return Err(ServiceError::Configuration(
                "server.host must not be empty".to_string(),
            ));
        }
        if self.server.port == 0 {
            return Err(ServiceError::Configuration(
                "server.port must be greater than zero".to_string(),
            ));
        }
        if self.limits.max_request_bytes == 0 {
            return Err(ServiceError::Configuration(
                "limits.max_request_bytes must be greater than zero".to_string(),
            ));
        }

        match &self.provider {
            ProviderKind::Baidu => {
                if self.baidu.api_key.trim().is_empty() || self.baidu.secret_key.trim().is_empty() {
                    return Err(ServiceError::Configuration(
                        "baidu.api_key and baidu.secret_key are required for the baidu provider"
                            .to_string(),
                    ));
                }
                if self.baidu.request_timeout_seconds == 0 {
                    return Err(ServiceError::Configuration(
                        "baidu.request_timeout_seconds must be greater than zero".to_string(),
                    ));
                }
            }
            ProviderKind::Local => {
                if self.local.command.trim().is_empty() {
                    return Err(ServiceError::Configuration(
                        "local.command is required for the local provider".to_string(),
                    ));
                }
                if self.local.base_url.trim().is_empty() || self.local.model.trim().is_empty() {
                    return Err(ServiceError::Configuration(
                        "local.base_url and local.model are required for the local provider"
                            .to_string(),
                    ));
                }
                if self.local.startup_timeout_seconds == 0
                    || self.local.request_timeout_seconds == 0
                    || self.local.max_tokens == 0
                {
                    return Err(ServiceError::Configuration(
                        "local timeouts and max_tokens must be greater than zero".to_string(),
                    ));
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_config() -> Config {
        Config {
            server: ServerConfig::default(),
            provider: ProviderKind::Local,
            baidu: BaiduConfig::default(),
            local: LocalConfig {
                command: "vllm".to_string(),
                args: Vec::new(),
                base_url: default_local_base_url(),
                model: "baidu/Unlimited-OCR".to_string(),
                startup_timeout_seconds: 10,
                request_timeout_seconds: 10,
                max_tokens: 100,
            },
            limits: LimitsConfig::default(),
        }
    }

    #[test]
    fn local_provider_configuration_is_valid() {
        assert!(local_config().validate().is_ok());
    }

    #[test]
    fn baidu_provider_requires_credentials() {
        let mut config = local_config();
        config.provider = ProviderKind::Baidu;
        assert!(config.validate().is_err());
    }
}
