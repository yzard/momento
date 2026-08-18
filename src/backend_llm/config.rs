mod defaults;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use momento_common::config_file::write_new_config;

use crate::error::ServiceError;

static DEFAULT_CONFIG_TEMPLATE: LazyLock<String> =
    LazyLock::new(|| defaults::render_template(include_str!("default.toml")));

pub fn default_config_template() -> &'static str {
    DEFAULT_CONFIG_TEMPLATE.as_str()
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub scheduler: SchedulerConfig,
    #[serde(default)]
    pub service: Vec<ServiceConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    #[serde(default = "defaults::server_host")]
    pub host: String,
    #[serde(default = "defaults::server_port")]
    pub port: u16,
    #[serde(default = "defaults::server_api_key")]
    pub api_key: String,
    #[serde(default = "defaults::server_data_dir")]
    pub data_dir: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SchedulerConfig {
    #[serde(default = "defaults::scheduler_poll_interval_seconds")]
    pub poll_interval_seconds: u64,
    #[serde(default = "defaults::scheduler_idle_shutdown_seconds")]
    pub idle_shutdown_seconds: u64,
    #[serde(default = "defaults::scheduler_max_in_flight_jobs")]
    pub max_in_flight_jobs: usize,
    #[serde(default = "defaults::scheduler_runtime_max_attempts")]
    pub runtime_max_attempts: usize,
    #[serde(default = "defaults::result_delivery_acknowledgement_timeout_seconds")]
    pub result_delivery_acknowledgement_timeout_seconds: u64,
    #[serde(default = "defaults::result_delivery_retry_delay_seconds")]
    pub result_delivery_retry_delay_seconds: u64,
    #[serde(default = "defaults::result_delivery_max_attempts")]
    pub result_delivery_max_attempts: usize,
    #[serde(default = "defaults::result_delivery_max_concurrent_deliveries")]
    pub result_delivery_max_concurrent_deliveries: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            poll_interval_seconds: defaults::SCHEDULER_POLL_INTERVAL_SECONDS,
            idle_shutdown_seconds: defaults::SCHEDULER_IDLE_SHUTDOWN_SECONDS,
            max_in_flight_jobs: defaults::SCHEDULER_MAX_IN_FLIGHT_JOBS,
            runtime_max_attempts: defaults::SCHEDULER_RUNTIME_MAX_ATTEMPTS,
            result_delivery_acknowledgement_timeout_seconds:
                defaults::RESULT_DELIVERY_ACKNOWLEDGEMENT_TIMEOUT_SECONDS,
            result_delivery_retry_delay_seconds: defaults::RESULT_DELIVERY_RETRY_DELAY_SECONDS,
            result_delivery_max_attempts: defaults::RESULT_DELIVERY_MAX_ATTEMPTS,
            result_delivery_max_concurrent_deliveries:
                defaults::RESULT_DELIVERY_MAX_CONCURRENT_DELIVERIES,
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: defaults::server_host(),
            port: defaults::SERVER_PORT,
            api_key: defaults::fallback::SERVER_API_KEY.to_string(),
            data_dir: defaults::server_data_dir(),
        }
    }
}

impl ServerConfig {
    pub fn llm_dir(&self) -> PathBuf {
        self.data_dir.join("llm")
    }

    pub fn queue_dir(&self) -> PathBuf {
        self.llm_dir().join("queue")
    }

    pub fn processing_dir(&self) -> PathBuf {
        self.queue_dir().join("processing")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.llm_dir().join("cache")
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceConfig {
    #[serde(default = "defaults::service_enabled")]
    pub enabled: bool,
    pub model_type: String,
    #[serde(default = "defaults::service_startup_timeout_seconds")]
    pub startup_timeout_seconds: u64,
    #[serde(default = "defaults::service_request_timeout_seconds")]
    pub request_timeout_seconds: u64,
    #[serde(default = "defaults::service_max_tokens")]
    pub max_tokens: u32,
    pub minimum_face_likelihood: Option<f64>,
    pub minimum_face_resolution_pixels: Option<u32>,
    pub max_concurrent_jobs: usize,
}

impl Config {
    pub fn save_default(path: &Path) -> Result<(), ServiceError> {
        write_new_config(path, default_config_template()).map_err(|error| {
            ServiceError::Configuration(format!("failed to write {}: {error}", path.display()))
        })
    }

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
        if self.server.api_key.trim().is_empty() {
            return Err(ServiceError::Configuration(
                "server.api_key must not be empty".to_string(),
            ));
        }
        if self.scheduler.poll_interval_seconds == 0
            || self.scheduler.idle_shutdown_seconds == 0
            || self.scheduler.max_in_flight_jobs == 0
            || self.scheduler.runtime_max_attempts == 0
        {
            return Err(ServiceError::Configuration(
                "scheduler poll interval, idle shutdown timeout, max in-flight jobs, and runtime attempts must be positive"
                    .to_string(),
            ));
        }
        if self
            .scheduler
            .result_delivery_acknowledgement_timeout_seconds
            == 0
            || self.scheduler.result_delivery_retry_delay_seconds == 0
            || self.scheduler.result_delivery_max_attempts == 0
            || self.scheduler.result_delivery_max_concurrent_deliveries == 0
        {
            return Err(ServiceError::Configuration(
                "result delivery acknowledgement timeout, retry delay, attempts, and concurrency must be positive".to_string(),
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
            server: ServerConfig {
                api_key: "test-key".to_string(),
                ..ServerConfig::default()
            },
            scheduler: SchedulerConfig::default(),
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
