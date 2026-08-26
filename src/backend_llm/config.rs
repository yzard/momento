mod defaults;

use serde::{Deserialize, Serialize};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use momento_common::config_file::write_new_config;

use crate::error::ServiceError;

static DEFAULT_CONFIG_TEMPLATE: LazyLock<String> =
    LazyLock::new(|| defaults::render_template(include_str!("default.toml")));
const LLM_SERVICE_API_KEY_PLACEHOLDER: &str = "${LLM_SERVICE_API_KEY}";

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
    pub face_detection_size: Option<u32>,
    pub recognition_batch_size: Option<usize>,
    pub recognition_batch_wait_milliseconds: Option<u64>,
    pub model_batch_wait_milliseconds: Option<u64>,
    pub max_concurrent_jobs: Option<usize>,
    pub cpu_processing_concurrency: Option<usize>,
    pub model_concurrency: Option<usize>,
}

impl ServiceConfig {
    pub fn configured_model_concurrency(&self) -> Result<usize, ServiceError> {
        let concurrency = if matches!(
            self.model_type.as_str(),
            "image_clustering"
                | "image_aesthetics"
                | "face_detection"
                | "screenshot_detection"
                | "document_detection"
        ) {
            self.model_concurrency
        } else {
            self.max_concurrent_jobs
        };
        concurrency.ok_or_else(|| {
            ServiceError::Configuration(format!(
                "{} model concurrency is not configured",
                self.model_type
            ))
        })
    }
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
        let api_key = read_environment_variable("LLM_SERVICE_API_KEY")?;
        let content = resolve_config_environment(&content, api_key.as_deref())?;
        let mut config = toml::from_str::<Self>(&content).map_err(|error| {
            ServiceError::Configuration(format!("failed to parse {}: {error}", path.display()))
        })?;
        apply_config_environment(&mut config, api_key.as_deref())?;
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
        match service.model_type.as_str() {
            "ocr" => self.validate_standard_service(service, Self::validate_ocr_service),
            "image_tagging" => {
                self.validate_standard_service(service, Self::validate_image_tagging_service)
            }
            "image_clustering" => self.validate_image_clustering_service(service),
            "image_aesthetics" => self.validate_image_aesthetics_service(service),
            "face_detection" => self.validate_face_detection_service(service),
            "screenshot_detection" => {
                self.validate_classifier_service(service, "screenshot detection")
            }
            "document_detection" => self.validate_classifier_service(service, "document detection"),
            model_type => Err(ServiceError::Configuration(format!(
                "unsupported service model_type: {model_type}"
            ))),
        }
    }

    fn validate_standard_service(
        &self,
        service: &ServiceConfig,
        validate_service: fn(&Self, &ServiceConfig) -> Result<(), ServiceError>,
    ) -> Result<(), ServiceError> {
        if service
            .max_concurrent_jobs
            .is_none_or(|concurrency| concurrency == 0)
        {
            return Err(ServiceError::Configuration(
                "enabled service max_concurrent_jobs must be positive".to_string(),
            ));
        }
        if service.cpu_processing_concurrency.is_some()
            || service.model_concurrency.is_some()
            || service.model_batch_wait_milliseconds.is_some()
        {
            return Err(ServiceError::Configuration(format!(
                "{} does not accept staged concurrency fields",
                service.model_type
            )));
        }
        validate_service(self, service)
    }

    fn validate_classifier_service(
        &self,
        service: &ServiceConfig,
        service_name: &str,
    ) -> Result<(), ServiceError> {
        self.validate_staged_image_service(service, service_name)?;
        self.validate_image_service_timeouts(service, service_name)
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
        self.validate_image_service_timeouts(service, "image tagging")
    }

    fn validate_image_clustering_service(
        &self,
        service: &ServiceConfig,
    ) -> Result<(), ServiceError> {
        self.validate_dynamic_batched_image_service(service, "image clustering")?;
        self.validate_image_service_timeouts(service, "image clustering")
    }

    fn validate_image_aesthetics_service(
        &self,
        service: &ServiceConfig,
    ) -> Result<(), ServiceError> {
        self.validate_dynamic_batched_image_service(service, "image aesthetics")?;
        self.validate_image_service_timeouts(service, "image aesthetics")
    }

    fn validate_dynamic_batched_image_service(
        &self,
        service: &ServiceConfig,
        service_name: &str,
    ) -> Result<(), ServiceError> {
        self.validate_staged_image_service(service, service_name)?;
        if service.model_batch_wait_milliseconds.is_none() {
            return Err(ServiceError::Configuration(format!(
                "{service_name} model_batch_wait_milliseconds is required"
            )));
        }
        Ok(())
    }

    fn validate_image_service_timeouts(
        &self,
        service: &ServiceConfig,
        service_name: &str,
    ) -> Result<(), ServiceError> {
        if service.startup_timeout_seconds == 0 || service.request_timeout_seconds == 0 {
            return Err(ServiceError::Configuration(format!(
                "{service_name} timeouts must be positive"
            )));
        }
        Ok(())
    }

    fn validate_face_detection_service(&self, service: &ServiceConfig) -> Result<(), ServiceError> {
        self.validate_staged_image_service(service, "face detection")?;
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
        if !matches!(service.face_detection_size, Some(640 | 960 | 1280)) {
            return Err(ServiceError::Configuration(
                "face detection face_detection_size must be one of 640, 960, or 1280".to_string(),
            ));
        }
        if service
            .recognition_batch_size
            .is_none_or(|batch_size| batch_size == 0)
        {
            return Err(ServiceError::Configuration(
                "face detection recognition_batch_size must be positive".to_string(),
            ));
        }
        if service.recognition_batch_wait_milliseconds.is_none() {
            return Err(ServiceError::Configuration(
                "face detection recognition_batch_wait_milliseconds is required".to_string(),
            ));
        }
        Ok(())
    }

    fn validate_staged_image_service(
        &self,
        service: &ServiceConfig,
        service_name: &str,
    ) -> Result<(), ServiceError> {
        if service
            .cpu_processing_concurrency
            .is_none_or(|concurrency| concurrency == 0)
        {
            return Err(ServiceError::Configuration(format!(
                "enabled {service_name} service cpu_processing_concurrency must be positive"
            )));
        }
        if service
            .model_concurrency
            .is_none_or(|concurrency| concurrency == 0)
        {
            return Err(ServiceError::Configuration(format!(
                "enabled {service_name} service model_concurrency must be positive"
            )));
        }
        if service.max_concurrent_jobs.is_some() {
            return Err(ServiceError::Configuration(format!(
                "{service_name} uses cpu_processing_concurrency and model_concurrency, not max_concurrent_jobs"
            )));
        }
        Ok(())
    }
}

fn read_environment_variable(name: &str) -> Result<Option<String>, ServiceError> {
    match env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(ServiceError::Configuration(format!(
            "{name} must contain valid Unicode"
        ))),
    }
}

pub fn apply_config_environment(
    config: &mut Config,
    api_key: Option<&str>,
) -> Result<(), ServiceError> {
    let Some(api_key) = api_key else {
        return Ok(());
    };
    if api_key.trim().is_empty() {
        return Err(ServiceError::Configuration(
            "LLM_SERVICE_API_KEY must not be empty".to_string(),
        ));
    }
    config.server.api_key = api_key.to_string();
    Ok(())
}

pub fn resolve_config_environment(
    content: &str,
    api_key: Option<&str>,
) -> Result<String, ServiceError> {
    if !content.contains(LLM_SERVICE_API_KEY_PLACEHOLDER) {
        return Ok(content.to_string());
    }
    let api_key = api_key
        .filter(|api_key| !api_key.trim().is_empty())
        .ok_or_else(|| {
            ServiceError::Configuration("LLM_SERVICE_API_KEY must not be empty".to_string())
        })?;
    Ok(content.replace(
        &format!("\"{LLM_SERVICE_API_KEY_PLACEHOLDER}\""),
        &toml::Value::String(api_key.to_string()).to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(model_type: &str) -> ServiceConfig {
        let is_face_detection = model_type == "face_detection";
        let is_image_clustering = model_type == "image_clustering";
        let is_image_aesthetics = model_type == "image_aesthetics";
        let is_screenshot_detection = model_type == "screenshot_detection";
        let is_document_detection = model_type == "document_detection";
        let uses_dynamic_batching = is_image_clustering || is_image_aesthetics;
        let uses_staged_concurrency = uses_dynamic_batching
            || is_face_detection
            || is_screenshot_detection
            || is_document_detection;
        ServiceConfig {
            enabled: true,
            model_type: model_type.to_string(),
            startup_timeout_seconds: 10,
            request_timeout_seconds: 10,
            max_tokens: 100,
            minimum_face_likelihood: is_face_detection.then_some(0.8),
            minimum_face_resolution_pixels: is_face_detection.then_some(112),
            face_detection_size: is_face_detection.then_some(960),
            recognition_batch_size: is_face_detection.then_some(64),
            recognition_batch_wait_milliseconds: is_face_detection.then_some(5),
            model_batch_wait_milliseconds: uses_dynamic_batching.then_some(5),
            max_concurrent_jobs: (!uses_staged_concurrency).then_some(8),
            cpu_processing_concurrency: uses_staged_concurrency.then_some(8),
            model_concurrency: uses_staged_concurrency.then_some(8),
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
        config.service[0].max_concurrent_jobs = Some(0);

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
        config.service.push(service("image_aesthetics"));
        config.service.push(service("face_detection"));
        config.service.push(service("screenshot_detection"));
        config.service.push(service("document_detection"));

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
        config.service[1].minimum_face_resolution_pixels = Some(112);
        config.service[1].face_detection_size = Some(800);
        assert!(config.validate().is_err());
        config.service[1].face_detection_size = Some(960);
        config.service[1].recognition_batch_size = Some(0);
        assert!(config.validate().is_err());
        config.service[1].recognition_batch_size = Some(64);
        config.service[1].recognition_batch_wait_milliseconds = None;
        assert!(config.validate().is_err());
    }

    #[test]
    fn image_aesthetics_requires_staged_concurrency_and_batch_wait() {
        let mut config = local_config();
        config.service.push(service("image_aesthetics"));
        assert!(config.validate().is_ok());

        config.service[1].cpu_processing_concurrency = Some(0);
        assert!(config.validate().is_err());
        config.service[1].cpu_processing_concurrency = Some(8);
        config.service[1].model_concurrency = Some(0);
        assert!(config.validate().is_err());
        config.service[1].model_concurrency = Some(64);
        config.service[1].model_batch_wait_milliseconds = None;
        assert!(config.validate().is_err());
    }

    #[test]
    fn image_clustering_requires_staged_concurrency_and_batch_wait() {
        let mut config = local_config();
        config.service.push(service("image_clustering"));
        assert!(config.validate().is_ok());

        config.service[1].cpu_processing_concurrency = Some(0);
        assert!(config.validate().is_err());
        config.service[1].cpu_processing_concurrency = Some(16);
        config.service[1].model_concurrency = Some(0);
        assert!(config.validate().is_err());
        config.service[1].model_concurrency = Some(16);
        config.service[1].model_batch_wait_milliseconds = None;
        assert!(config.validate().is_err());
    }
}
