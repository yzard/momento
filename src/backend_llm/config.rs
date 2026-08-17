use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::error::ServiceError;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub callback: CallbackConfig,
    #[serde(default)]
    pub service: Vec<ServiceConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    #[serde(default)]
    pub scheduler: SchedulerConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SchedulerConfig {
    #[serde(default = "default_poll_interval_seconds")]
    pub poll_interval_seconds: u64,
    #[serde(default = "default_idle_shutdown_seconds")]
    pub idle_shutdown_seconds: u64,
    #[serde(default = "default_max_in_flight_jobs")]
    pub max_in_flight_jobs: usize,
    #[serde(default = "default_runtime_max_attempts")]
    pub runtime_max_attempts: usize,
}

fn default_poll_interval_seconds() -> u64 {
    5
}

fn default_idle_shutdown_seconds() -> u64 {
    60
}

fn default_max_in_flight_jobs() -> usize {
    128
}

fn default_runtime_max_attempts() -> usize {
    3
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            poll_interval_seconds: default_poll_interval_seconds(),
            idle_shutdown_seconds: default_idle_shutdown_seconds(),
            max_in_flight_jobs: default_max_in_flight_jobs(),
            runtime_max_attempts: default_runtime_max_attempts(),
        }
    }
}

fn default_data_dir() -> PathBuf {
    PathBuf::from("/data")
}

#[derive(Debug, Clone, Deserialize)]
pub struct CallbackConfig {
    #[serde(default = "default_callback_timeout_seconds")]
    pub request_timeout_seconds: u64,
    #[serde(default = "default_callback_retry_delay_seconds")]
    pub retry_delay_seconds: u64,
    #[serde(default = "default_callback_max_attempts")]
    pub max_attempts: usize,
    #[serde(default = "default_callback_max_concurrent_deliveries")]
    pub max_concurrent_deliveries: usize,
    #[serde(default)]
    pub key: String,
}

fn default_callback_timeout_seconds() -> u64 {
    30
}
fn default_callback_retry_delay_seconds() -> u64 {
    30
}
fn default_callback_max_attempts() -> usize {
    10
}
fn default_callback_max_concurrent_deliveries() -> usize {
    16
}

impl Default for CallbackConfig {
    fn default() -> Self {
        Self {
            request_timeout_seconds: default_callback_timeout_seconds(),
            retry_delay_seconds: default_callback_retry_delay_seconds(),
            max_attempts: default_callback_max_attempts(),
            max_concurrent_deliveries: default_callback_max_concurrent_deliveries(),
            key: String::new(),
        }
    }
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
            data_dir: default_data_dir(),
            scheduler: SchedulerConfig::default(),
        }
    }
}

impl ServerConfig {
    pub fn queue_dir(&self) -> PathBuf {
        self.data_dir.join("queue")
    }

    pub fn processing_dir(&self) -> PathBuf {
        self.queue_dir().join("processing")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.data_dir.join("cache")
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceConfig {
    #[serde(default)]
    pub enabled: bool,
    pub model_type: String,
    #[serde(default = "default_startup_timeout_seconds")]
    pub startup_timeout_seconds: u64,
    #[serde(default = "default_request_timeout_seconds")]
    pub request_timeout_seconds: u64,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    pub minimum_face_likelihood: Option<f64>,
    pub minimum_face_resolution_pixels: Option<u32>,
    pub max_concurrent_jobs: usize,
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
        if self.server.data_dir.as_os_str().is_empty() {
            return Err(ServiceError::Configuration(
                "server.data_dir must not be empty".to_string(),
            ));
        }
        if self.server.scheduler.poll_interval_seconds == 0
            || self.server.scheduler.idle_shutdown_seconds == 0
            || self.server.scheduler.max_in_flight_jobs == 0
            || self.server.scheduler.runtime_max_attempts == 0
        {
            return Err(ServiceError::Configuration(
                "scheduler poll interval, idle shutdown timeout, max in-flight jobs, and runtime attempts must be positive"
                    .to_string(),
            ));
        }
        if self.callback.request_timeout_seconds == 0
            || self.callback.retry_delay_seconds == 0
            || self.callback.max_attempts == 0
            || self.callback.max_concurrent_deliveries == 0
        {
            return Err(ServiceError::Configuration(
                "callback timeout, retry delay, attempts, and concurrency must be positive"
                    .to_string(),
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
        if service.model_type.trim().is_empty() {
            return Err(ServiceError::Configuration(
                "service model_type is required".to_string(),
            ));
        }
        if !service.enabled {
            return Ok(());
        }
        if service.max_concurrent_jobs == 0 {
            return Err(ServiceError::Configuration(
                "enabled service max_concurrent_jobs must be positive".to_string(),
            ));
        }
        match service.model_type.as_str() {
            "ocr" => self.validate_ocr_service(service),
            "image_tagging" => self.validate_image_tagging_service(service),
            "image_clustering" => self.validate_image_clustering_service(service),
            "face_detection" => self.validate_face_detection_service(service),
            model_type => Err(ServiceError::Configuration(format!(
                "unsupported service model_type: {model_type}"
            ))),
        }
    }

    fn validate_ocr_service(&self, service: &ServiceConfig) -> Result<(), ServiceError> {
        if service.startup_timeout_seconds == 0
            || service.request_timeout_seconds == 0
            || service.max_tokens == 0
        {
            return Err(ServiceError::Configuration(
                "local OCR timeouts and max_tokens must be positive".to_string(),
            ));
        }
        Ok(())
    }

    fn validate_image_tagging_service(&self, service: &ServiceConfig) -> Result<(), ServiceError> {
        if service.startup_timeout_seconds == 0 || service.request_timeout_seconds == 0 {
            return Err(ServiceError::Configuration(
                "image tagging timeouts must be positive".to_string(),
            ));
        }
        Ok(())
    }

    fn validate_image_clustering_service(
        &self,
        service: &ServiceConfig,
    ) -> Result<(), ServiceError> {
        if service.startup_timeout_seconds == 0 || service.request_timeout_seconds == 0 {
            return Err(ServiceError::Configuration(
                "image clustering timeouts must be positive".to_string(),
            ));
        }
        Ok(())
    }

    fn validate_face_detection_service(&self, service: &ServiceConfig) -> Result<(), ServiceError> {
        if service.startup_timeout_seconds == 0 || service.request_timeout_seconds == 0 {
            return Err(ServiceError::Configuration(
                "face detection timeouts must be positive".to_string(),
            ));
        }
        let Some(minimum_face_likelihood) = service.minimum_face_likelihood else {
            return Err(ServiceError::Configuration(
                "face detection minimum_face_likelihood is required".to_string(),
            ));
        };
        if !minimum_face_likelihood.is_finite()
            || !(0.0 < minimum_face_likelihood && minimum_face_likelihood <= 1.0)
        {
            return Err(ServiceError::Configuration(
                "face detection minimum_face_likelihood must be within (0, 1]".to_string(),
            ));
        }
        if service
            .minimum_face_resolution_pixels
            .is_none_or(|resolution| resolution == 0)
        {
            return Err(ServiceError::Configuration(
                "face detection minimum_face_resolution_pixels must be positive".to_string(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(model_type: &str) -> ServiceConfig {
        ServiceConfig {
            enabled: true,
            model_type: model_type.to_string(),
            startup_timeout_seconds: 10,
            request_timeout_seconds: 10,
            max_tokens: 100,
            minimum_face_likelihood: (model_type == "face_detection").then_some(0.8),
            minimum_face_resolution_pixels: (model_type == "face_detection").then_some(112),
            max_concurrent_jobs: 8,
        }
    }

    fn local_config() -> Config {
        Config {
            server: ServerConfig::default(),
            callback: CallbackConfig::default(),
            service: vec![service("ocr")],
        }
    }

    #[test]
    fn operational_service_configuration_is_valid() {
        assert!(local_config().validate().is_ok());
    }

    #[test]
    fn enabled_service_requires_positive_concurrency() {
        let mut config = local_config();
        config.service[0].max_concurrent_jobs = 0;

        let error = config
            .validate()
            .expect_err("zero runtime concurrency must fail");

        assert!(error.to_string().contains("max_concurrent_jobs"));
    }

    #[test]
    fn all_runtime_types_are_valid() {
        let mut config = local_config();
        config.service.push(service("image_tagging"));
        config.service.push(service("image_clustering"));
        config.service.push(service("face_detection"));

        assert!(config.validate().is_ok());
    }

    #[test]
    fn face_detection_requires_valid_thresholds() {
        let mut config = local_config();
        config.service.push(service("face_detection"));
        assert!(config.validate().is_ok());

        config.service[1].minimum_face_likelihood = Some(0.0);
        assert!(config.validate().is_err());
        config.service[1].minimum_face_likelihood = Some(0.8);
        config.service[1].minimum_face_resolution_pixels = Some(0);
        assert!(config.validate().is_err());
    }
}
