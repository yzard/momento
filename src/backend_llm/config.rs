use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::error::ServiceError;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub callback: CallbackConfig,
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
    #[serde(default)]
    pub scheduler: SchedulerConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SchedulerConfig {
    #[serde(default = "default_poll_interval_seconds")]
    pub poll_interval_seconds: u64,
    #[serde(default = "default_idle_shutdown_seconds")]
    pub idle_shutdown_seconds: u64,
    #[serde(default = "default_dispatch_batch_size")]
    pub dispatch_batch_size: usize,
}

fn default_poll_interval_seconds() -> u64 {
    5
}

fn default_idle_shutdown_seconds() -> u64 {
    60
}

fn default_dispatch_batch_size() -> usize {
    64
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            poll_interval_seconds: default_poll_interval_seconds(),
            idle_shutdown_seconds: default_idle_shutdown_seconds(),
            dispatch_batch_size: default_dispatch_batch_size(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    #[serde(default = "default_queue_dir")]
    pub queue_dir: PathBuf,
}

fn default_data_dir() -> PathBuf {
    PathBuf::from("/data")
}
fn default_queue_dir() -> PathBuf {
    PathBuf::from("/data/queue")
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: default_data_dir(),
            queue_dir: default_queue_dir(),
        }
    }
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

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            api_key: String::new(),
            scheduler: SchedulerConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceConfig {
    #[serde(default)]
    pub enabled: bool,
    pub model_type: String,
    pub model_version: String,
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
    #[serde(default = "default_startup_timeout_seconds")]
    pub startup_timeout_seconds: u64,
    #[serde(default = "default_request_timeout_seconds")]
    pub request_timeout_seconds: u64,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default)]
    pub embedding_dimensions: usize,
    pub max_concurrent_jobs: usize,
}

fn default_device() -> String {
    "auto".to_string()
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
        if self.general.scheduler.poll_interval_seconds == 0
            || self.general.scheduler.idle_shutdown_seconds == 0
            || self.general.scheduler.dispatch_batch_size == 0
        {
            return Err(ServiceError::Configuration(
                "scheduler poll interval, idle shutdown timeout, and dispatch batch size must be positive"
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
        if service.model_type.trim().is_empty() || service.model_version.trim().is_empty() {
            return Err(ServiceError::Configuration(
                "service model_type and model_version are required".to_string(),
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
        if !service
            .docker_command
            .iter()
            .any(|argument| argument.contains("{max_concurrent_jobs}"))
        {
            return Err(ServiceError::Configuration(
                "enabled service docker_command must pass {max_concurrent_jobs} to the local model subservice"
                    .to_string(),
            ));
        }
        validate_local_base_url(service)?;

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
        if service.docker_command.is_empty()
            || service.base_url.trim().is_empty()
            || service.model.trim().is_empty()
        {
            return Err(ServiceError::Configuration(
                "local OCR docker_command, base_url, and model are required".to_string(),
            ));
        }
        if service.startup_timeout_seconds == 0
            || service.request_timeout_seconds == 0
            || service.max_tokens == 0
        {
            return Err(ServiceError::Configuration(
                "local OCR timeouts and max_tokens must be positive".to_string(),
            ));
        }
        validate_cuda_service(service, "local OCR")?;
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
        validate_uv_package_installation(service, "image tagging")?;
        validate_cuda_service(service, "image tagging")?;
        Ok(())
    }

    fn validate_image_clustering_service(
        &self,
        service: &ServiceConfig,
    ) -> Result<(), ServiceError> {
        if service.docker_command.is_empty()
            || service.base_url.trim().is_empty()
            || service.model.trim().is_empty()
            || service.script_path.trim().is_empty()
        {
            return Err(ServiceError::Configuration(
                "image clustering docker_command, base_url, model, and script_path are required"
                    .to_string(),
            ));
        }
        if service.startup_timeout_seconds == 0 || service.request_timeout_seconds == 0 {
            return Err(ServiceError::Configuration(
                "image clustering timeouts must be positive".to_string(),
            ));
        }
        if service.embedding_dimensions != 384 {
            return Err(ServiceError::Configuration(
                "image clustering embedding_dimensions must be 384 for DINOv2-small".to_string(),
            ));
        }
        if service.model != "facebook/dinov2-small" {
            return Err(ServiceError::Configuration(
                "image clustering model must be facebook/dinov2-small".to_string(),
            ));
        }
        validate_uv_package_installation(service, "image clustering")?;
        validate_cuda_service(service, "image clustering")?;
        Ok(())
    }

    fn validate_face_detection_service(&self, service: &ServiceConfig) -> Result<(), ServiceError> {
        if service.docker_command.is_empty()
            || service.base_url.trim().is_empty()
            || service.model.trim().is_empty()
            || service.script_path.trim().is_empty()
        {
            return Err(ServiceError::Configuration(
                "face detection docker_command, base_url, model, and script_path are required"
                    .to_string(),
            ));
        }
        if service.startup_timeout_seconds == 0 || service.request_timeout_seconds == 0 {
            return Err(ServiceError::Configuration(
                "face detection timeouts must be positive".to_string(),
            ));
        }
        if service.embedding_dimensions != 512 {
            return Err(ServiceError::Configuration(
                "face detection embedding_dimensions must be 512 for InsightFace buffalo_l"
                    .to_string(),
            ));
        }
        if service.model != "buffalo_l" {
            return Err(ServiceError::Configuration(
                "face detection model must be buffalo_l".to_string(),
            ));
        }
        validate_uv_package_installation(service, "face detection")?;
        validate_cuda_service(service, "face detection")?;
        Ok(())
    }
}

fn validate_uv_package_installation(
    service: &ServiceConfig,
    service_name: &str,
) -> Result<(), ServiceError> {
    let command = service.docker_command.join(" ");
    if !command.contains("{uv_bootstrap}") || !command.contains("uv pip install --system") {
        return Err(ServiceError::Configuration(format!(
            "{service_name} docker_command must install Python packages with {{uv_bootstrap}} and uv pip install --system"
        )));
    }
    Ok(())
}

fn validate_local_base_url(service: &ServiceConfig) -> Result<(), ServiceError> {
    let base_url = reqwest::Url::parse(&service.base_url).map_err(|error| {
        ServiceError::Configuration(format!(
            "{} base_url must be a valid local HTTP URL: {error}",
            service.model_type
        ))
    })?;
    let local_host = matches!(base_url.host_str(), Some("127.0.0.1" | "localhost" | "::1"));
    if base_url.scheme() != "http" || !local_host {
        return Err(ServiceError::Configuration(format!(
            "{} base_url must point to a loopback HTTP model subservice",
            service.model_type
        )));
    }
    Ok(())
}

fn validate_cuda_service(service: &ServiceConfig, service_name: &str) -> Result<(), ServiceError> {
    if !service.device.starts_with("cuda") {
        return Err(ServiceError::Configuration(format!(
            "{service_name} device must select a CUDA GPU"
        )));
    }
    if !service
        .docker_command
        .iter()
        .any(|argument| argument == "--gpus")
    {
        return Err(ServiceError::Configuration(format!(
            "{service_name} docker_command must expose an NVIDIA GPU with --gpus"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_service() -> ServiceConfig {
        ServiceConfig {
            enabled: true,
            model_type: "ocr".to_string(),
            model_version: "unlimited_ocr".to_string(),
            docker_command: vec![
                "docker".to_string(),
                "--gpus".to_string(),
                "all".to_string(),
                "{max_concurrent_jobs}".to_string(),
            ],
            device: "cuda".to_string(),
            base_url: "http://127.0.0.1:8000/v1".to_string(),
            model: "baidu/Unlimited-OCR".to_string(),
            script_path: String::new(),
            startup_timeout_seconds: 10,
            request_timeout_seconds: 10,
            max_tokens: 100,
            embedding_dimensions: 0,
            max_concurrent_jobs: 8,
        }
    }

    fn local_config() -> Config {
        Config {
            general: GeneralConfig::default(),
            logging: LoggingConfig::default(),
            storage: StorageConfig::default(),
            callback: CallbackConfig::default(),
            service: vec![local_service()],
        }
    }

    #[test]
    fn local_provider_configuration_is_valid() {
        assert!(local_config().validate().is_ok());
    }

    #[test]
    fn model_subservice_rejects_remote_base_url() {
        let mut config = Config::default();
        let mut service = local_service();
        service.base_url = "https://models.example.com/v1".to_string();
        config.service = vec![service];

        let error = config.validate().expect_err("remote model URL must fail");

        assert!(error.to_string().contains("loopback"));
    }

    #[test]
    fn model_subservice_requires_local_concurrency_argument() {
        let mut config = local_config();
        config.service[0]
            .docker_command
            .retain(|argument| !argument.contains("{max_concurrent_jobs}"));

        let error = config
            .validate()
            .expect_err("model command without concurrency must fail");

        assert!(error.to_string().contains("local model subservice"));
    }

    #[test]
    fn image_tagging_service_is_valid() {
        let mut config = local_config();
        config.service.push(ServiceConfig {
            enabled: true,
            model_type: "image_tagging".to_string(),
            model_version: "ram++".to_string(),
            docker_command: vec![
                "docker".to_string(),
                "--gpus".to_string(),
                "all".to_string(),
                "{uv_bootstrap} && uv pip install --system Pillow".to_string(),
                "{max_concurrent_jobs}".to_string(),
            ],
            device: "cuda".to_string(),
            base_url: "http://127.0.0.1:8200".to_string(),
            model: String::new(),
            script_path: "ram_server.py".to_string(),
            startup_timeout_seconds: 10,
            request_timeout_seconds: 10,
            max_tokens: 0,
            embedding_dimensions: 0,
            max_concurrent_jobs: 8,
        });
        assert!(config.validate().is_ok());
    }

    #[test]
    fn image_clustering_service_requires_dinov2_small_model_and_dimensions() {
        let mut config = local_config();
        config.service.push(ServiceConfig {
            enabled: true,
            model_type: "image_clustering".to_string(),
            model_version: "dinov2-small".to_string(),
            docker_command: vec![
                "python3".to_string(),
                "--gpus".to_string(),
                "all".to_string(),
                "{uv_bootstrap} && uv pip install --system Pillow".to_string(),
                "{max_concurrent_jobs}".to_string(),
            ],
            device: "cuda".to_string(),
            base_url: "http://127.0.0.1:8300".to_string(),
            model: "facebook/dinov2-small".to_string(),
            script_path: "image_clustering_server.py".to_string(),
            startup_timeout_seconds: 10,
            request_timeout_seconds: 10,
            max_tokens: 0,
            embedding_dimensions: 384,
            max_concurrent_jobs: 8,
        });
        assert!(config.validate().is_ok());

        config.service[1].model = "facebook/dinov2-base".to_string();
        assert!(config.validate().is_err());
        config.service[1].model = "facebook/dinov2-small".to_string();
        config.service[1].embedding_dimensions = 768;
        assert!(config.validate().is_err());
    }

    #[test]
    fn face_detection_service_requires_buffalo_l_and_arcface_dimensions() {
        let mut config = local_config();
        config.service.push(ServiceConfig {
            enabled: true,
            model_type: "face_detection".to_string(),
            model_version: "buffalo_l".to_string(),
            docker_command: vec![
                "python3".to_string(),
                "--gpus".to_string(),
                "all".to_string(),
                "{uv_bootstrap} && uv pip install --system insightface".to_string(),
                "{max_concurrent_jobs}".to_string(),
            ],
            device: "cuda".to_string(),
            base_url: "http://127.0.0.1:8500".to_string(),
            model: "buffalo_l".to_string(),
            script_path: "face_detection_server.py".to_string(),
            startup_timeout_seconds: 10,
            request_timeout_seconds: 10,
            max_tokens: 0,
            embedding_dimensions: 512,
            max_concurrent_jobs: 8,
        });
        assert!(config.validate().is_ok());

        config.service[1].model = "antelopev2".to_string();
        assert!(config.validate().is_err());
        config.service[1].model = "buffalo_l".to_string();
        config.service[1].embedding_dimensions = 384;
        assert!(config.validate().is_err());
    }

    #[test]
    fn python_service_rejects_pip_package_installation() {
        let mut config = local_config();
        config.service.push(ServiceConfig {
            enabled: true,
            model_type: "image_tagging".to_string(),
            model_version: "ram++".to_string(),
            docker_command: vec![
                "python -m pip install Pillow".to_string(),
                "{max_concurrent_jobs}".to_string(),
            ],
            device: default_device(),
            base_url: "http://127.0.0.1:8200".to_string(),
            model: String::new(),
            script_path: "ram_server.py".to_string(),
            startup_timeout_seconds: 10,
            request_timeout_seconds: 10,
            max_tokens: 0,
            embedding_dimensions: 0,
            max_concurrent_jobs: 8,
        });

        let error = config
            .validate()
            .expect_err("pip package installation must be rejected");

        assert!(error.to_string().contains("uv pip install --system"));
    }
}
