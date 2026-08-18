use base64::{engine::general_purpose::STANDARD, Engine};
use reqwest::Client;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::info;

use crate::adapters::{normalize_unlimited_ocr_text, UNLIMITED_OCR_MODEL};
use crate::config::{Config, ServerConfig, ServiceConfig};
use crate::error::ServiceError;

const RUNTIME_HOST: &str = "127.0.0.1";
const RUNTIME_INPUT_PLACEHOLDER: &str = "{input_root}";
const RUNTIME_CACHE_PLACEHOLDER: &str = "{cache_dir}";
const RUNTIME_CONCURRENCY_PLACEHOLDER: &str = "{max_concurrent_jobs}";
const FACE_LIKELIHOOD_PLACEHOLDER: &str = "{minimum_face_likelihood}";
const FACE_RESOLUTION_PLACEHOLDER: &str = "{minimum_face_resolution_pixels}";
const SYSTEM_RUNTIME_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
const CLUSTERING_EMBEDDING_DIMENSIONS: usize = 384;
const FACE_EMBEDDING_DIMENSIONS: usize = 512;
const RUNTIME_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);
const RUNTIME_PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(50);
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceResponse {
    pub task: String,
    pub text: String,
    pub markdown: String,
    pub provider: String,
    pub model_type: String,
    pub model_version: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding_encoding: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding_dimensions: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub perceptual_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality_score: Option<f32>,
    #[serde(default)]
    pub faces: Vec<FaceDetection>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FaceDetection {
    pub index: usize,
    pub bounding_box: NormalizedBoundingBox,
    pub eye_center: NormalizedPoint,
    pub confidence: f32,
    pub quality_score: f32,
    pub frontality_score: f32,
    pub embedding: String,
    pub embedding_encoding: String,
    pub embedding_dimensions: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedBoundingBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedPoint {
    pub x: f32,
    pub y: f32,
}

pub struct InferenceInput {
    pub job_id: String,
    pub sequence: u32,
    pub frame_timestamp_ms: Option<i64>,
    pub path: PathBuf,
    pub byte_size: u64,
    pub content_hash: String,
    pub mime_type: String,
    pub filename: String,
}

pub struct InputInferenceResponse {
    pub sequence: u32,
    pub frame_timestamp_ms: Option<i64>,
    pub response: InferenceResponse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceType {
    Ocr,
    ImageTagging,
    ImageClustering,
    FaceDetection,
}

#[derive(Debug, Clone)]
pub struct RuntimeSpec {
    pub service_type: ServiceType,
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub base_url: String,
    pub model: String,
    pub model_version: String,
    pub embedding_dimensions: usize,
}

#[derive(Debug, Clone)]
pub struct RuntimeCatalog {
    specs: Vec<RuntimeSpec>,
}

impl RuntimeCatalog {
    pub fn new(specs: Vec<RuntimeSpec>) -> Self {
        Self { specs }
    }

    pub fn production() -> Self {
        Self::new(vec![
            RuntimeSpec {
                service_type: ServiceType::Ocr,
                executable: PathBuf::from("/opt/venvs/ocr/bin/python"),
                arguments: vec![
                    "-m",
                    "vllm.entrypoints.openai.api_server",
                    "--model",
                    "/opt/models/unlimited-ocr",
                    "--served-model-name",
                    UNLIMITED_OCR_MODEL,
                    "--trust-remote-code",
                    "--logits-processors",
                    "vllm.model_executor.models.unlimited_ocr:NGramPerReqLogitsProcessor",
                    "--host",
                    RUNTIME_HOST,
                    "--port",
                    "8400",
                    "--no-enable-prefix-caching",
                    "--mm-processor-cache-gb",
                    "0",
                    "--max-num-batched-tokens",
                    "8192",
                    "--max-num-seqs",
                    RUNTIME_CONCURRENCY_PLACEHOLDER,
                    "--allowed-local-media-path",
                    RUNTIME_INPUT_PLACEHOLDER,
                    "--gpu-memory-utilization",
                    "0.65",
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
                environment: vec![("VLLM_USE_FLASHINFER_SAMPLER".to_string(), "0".to_string())],
                base_url: "http://127.0.0.1:8400/v1".to_string(),
                model: UNLIMITED_OCR_MODEL.to_string(),
                model_version: "unlimited_ocr".to_string(),
                embedding_dimensions: 0,
            },
            RuntimeSpec {
                service_type: ServiceType::ImageTagging,
                executable: PathBuf::from("/opt/venvs/ram/bin/python"),
                arguments: vec![
                    "/app/runtimes/ram_server.py",
                    "--checkpoint",
                    "/opt/models/ram/ram_plus_swin_large_14m.pth",
                    "--host",
                    RUNTIME_HOST,
                    "--port",
                    "8200",
                    "--device",
                    "cuda:0",
                    "--max-concurrent-jobs",
                    RUNTIME_CONCURRENCY_PLACEHOLDER,
                    "--input-root",
                    RUNTIME_INPUT_PLACEHOLDER,
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
                environment: Vec::new(),
                base_url: "http://127.0.0.1:8200".to_string(),
                model: String::new(),
                model_version: "ram++".to_string(),
                embedding_dimensions: 0,
            },
            RuntimeSpec {
                service_type: ServiceType::ImageClustering,
                executable: PathBuf::from("/opt/venvs/dinov2/bin/python"),
                arguments: vec![
                    "/app/runtimes/image_clustering_server.py",
                    "--model",
                    "/opt/models/dinov2-small",
                    "--cache-dir",
                    "{cache_dir}/huggingface",
                    "--device",
                    "cuda:0",
                    "--host",
                    RUNTIME_HOST,
                    "--port",
                    "8300",
                    "--max-concurrent-jobs",
                    RUNTIME_CONCURRENCY_PLACEHOLDER,
                    "--input-root",
                    RUNTIME_INPUT_PLACEHOLDER,
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
                environment: Vec::new(),
                base_url: "http://127.0.0.1:8300".to_string(),
                model: "facebook/dinov2-small".to_string(),
                model_version: "dinov2-small".to_string(),
                embedding_dimensions: CLUSTERING_EMBEDDING_DIMENSIONS,
            },
            RuntimeSpec {
                service_type: ServiceType::FaceDetection,
                executable: PathBuf::from("/opt/venvs/insightface/bin/python"),
                arguments: vec![
                    "/app/runtimes/face_detection_server.py",
                    "--model",
                    "buffalo_l",
                    "--cache-dir",
                    "/opt/models/insightface",
                    "--host",
                    RUNTIME_HOST,
                    "--port",
                    "8500",
                    "--max-concurrent-jobs",
                    RUNTIME_CONCURRENCY_PLACEHOLDER,
                    "--input-root",
                    RUNTIME_INPUT_PLACEHOLDER,
                    "--minimum-face-likelihood",
                    FACE_LIKELIHOOD_PLACEHOLDER,
                    "--minimum-face-resolution-pixels",
                    FACE_RESOLUTION_PLACEHOLDER,
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
                environment: Vec::new(),
                base_url: "http://127.0.0.1:8500".to_string(),
                model: "buffalo_l".to_string(),
                model_version: "buffalo_l".to_string(),
                embedding_dimensions: FACE_EMBEDDING_DIMENSIONS,
            },
        ])
    }

    fn get(&self, service_type: ServiceType) -> Result<&RuntimeSpec, ServiceError> {
        self.specs
            .iter()
            .find(|spec| spec.service_type == service_type)
            .ok_or_else(|| {
                ServiceError::Configuration(format!(
                    "no runtime specification exists for {}",
                    service_type.as_str()
                ))
            })
    }
}

impl ServiceType {
    pub fn from_task(task: &str) -> Result<Self, ServiceError> {
        match task {
            "ocr" => Ok(Self::Ocr),
            "image_tagging" => Ok(Self::ImageTagging),
            "image_clustering" => Ok(Self::ImageClustering),
            "face_detection" => Ok(Self::FaceDetection),
            _ => Err(ServiceError::NotImplemented(format!(
                "inference task `{task}` has no configured managed runtime"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ocr => "ocr",
            Self::ImageTagging => "image_tagging",
            Self::ImageClustering => "image_clustering",
            Self::FaceDetection => "face_detection",
        }
    }

    fn config_key(self) -> &'static str {
        match self {
            Self::Ocr => "ocr",
            Self::ImageTagging => "image_tagging",
            Self::ImageClustering => "image_clustering",
            Self::FaceDetection => "face_detection",
        }
    }
}

#[derive(Clone)]
enum ActiveService {
    Ocr(Arc<LocalProvider>),
    ImageTagging(Arc<RamProvider>),
    ImageClustering(Arc<ImageClusteringProvider>),
    FaceDetection(Arc<FaceDetectionProvider>),
}

impl ActiveService {
    fn service_type(&self) -> ServiceType {
        match self {
            Self::Ocr(_) => ServiceType::Ocr,
            Self::ImageTagging(_) => ServiceType::ImageTagging,
            Self::ImageClustering(_) => ServiceType::ImageClustering,
            Self::FaceDetection(_) => ServiceType::FaceDetection,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Ocr(provider) => provider.name(),
            Self::ImageTagging(_) => "ram++",
            Self::ImageClustering(_) => "dinov2",
            Self::FaceDetection(_) => "insightface",
        }
    }
    async fn shutdown(&self) -> Result<(), ServiceError> {
        match self {
            Self::Ocr(provider) => provider.shutdown().await,
            Self::ImageTagging(provider) => provider.shutdown().await,
            Self::ImageClustering(provider) => provider.shutdown().await,
            Self::FaceDetection(provider) => provider.shutdown().await,
        }
    }

    async fn is_alive(&self) -> Result<bool, ServiceError> {
        match self {
            Self::Ocr(provider) => provider.is_alive().await,
            Self::ImageTagging(provider) => provider.runtime.is_alive().await,
            Self::ImageClustering(provider) => provider.runtime.is_alive().await,
            Self::FaceDetection(provider) => provider.runtime.is_alive().await,
        }
    }

    async fn infer_inputs(
        &self,
        inputs: Vec<InferenceInput>,
    ) -> Result<Vec<InputInferenceResponse>, ServiceError> {
        match self {
            Self::Ocr(provider) => provider.infer_inputs(inputs).await,
            Self::ImageTagging(provider) => provider.infer_inputs(inputs).await,
            Self::ImageClustering(provider) => provider.infer_inputs(inputs).await,
            Self::FaceDetection(provider) => provider.infer_inputs(inputs).await,
        }
    }
}

#[derive(Clone)]
pub struct InferenceDispatcher {
    active: ActiveService,
}

impl InferenceDispatcher {
    pub async fn infer_inputs(
        &self,
        inputs: Vec<InferenceInput>,
    ) -> Result<Vec<InputInferenceResponse>, ServiceError> {
        self.active.infer_inputs(inputs).await
    }
}

pub struct ServiceManager {
    config: Arc<Config>,
    runtimes: RuntimeCatalog,
    active: Option<ActiveService>,
}

impl ServiceManager {
    pub fn new(config: Arc<Config>) -> Self {
        Self::with_runtime_catalog(config, RuntimeCatalog::production())
    }

    pub fn with_runtime_catalog(config: Arc<Config>, runtimes: RuntimeCatalog) -> Self {
        Self {
            config,
            runtimes,
            active: None,
        }
    }

    pub fn active_name(&self) -> &'static str {
        self.active
            .as_ref()
            .map(ActiveService::name)
            .unwrap_or("on-demand")
    }

    pub fn active_task(&self) -> Option<&'static str> {
        self.active
            .as_ref()
            .map(|active| active.service_type().as_str())
    }

    pub async fn dispatcher(&mut self, task: &str) -> Result<InferenceDispatcher, ServiceError> {
        let service_type = ServiceType::from_task(task)?;
        self.activate(service_type).await?;
        let active = self
            .active
            .as_ref()
            .expect("active service set by activate")
            .clone();
        Ok(InferenceDispatcher { active })
    }

    async fn activate(&mut self, service_type: ServiceType) -> Result<(), ServiceError> {
        if let Some(active) = self.active.as_ref() {
            if active.service_type() == service_type && active.is_alive().await? {
                return Ok(());
            }
        }
        if let Some(active) = self.active.take() {
            active.shutdown().await?;
        }

        let service = self
            .config
            .service_for(service_type.config_key())
            .cloned()
            .ok_or_else(|| {
                ServiceError::NotImplemented(format!(
                    "inference task `{}` has no configured managed runtime",
                    service_type.config_key()
                ))
            })?;
        let runtime = self.runtimes.get(service_type)?.clone();
        let active = match service_type {
            ServiceType::Ocr => ActiveService::Ocr(Arc::new(
                LocalProvider::new(&service, &runtime, &self.config.server).await?,
            )),
            ServiceType::ImageTagging => ActiveService::ImageTagging(Arc::new(
                RamProvider::new(&service, &runtime, &self.config.server).await?,
            )),
            ServiceType::ImageClustering => ActiveService::ImageClustering(Arc::new(
                ImageClusteringProvider::new(&service, &runtime, &self.config.server).await?,
            )),
            ServiceType::FaceDetection => ActiveService::FaceDetection(Arc::new(
                FaceDetectionProvider::new(&service, &runtime, &self.config.server).await?,
            )),
        };
        self.active = Some(active);
        Ok(())
    }

    pub async fn shutdown(&mut self) -> Result<(), ServiceError> {
        let Some(active) = self.active.take() else {
            return Ok(());
        };
        active.shutdown().await
    }
}

pub struct LocalProvider {
    client: Client,
    config: ServiceConfig,
    runtime: RuntimeSpec,
    child: Arc<Mutex<Child>>,
    process_id: u32,
    input_root: PathBuf,
}

impl Drop for LocalProvider {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.try_lock() {
            kill_runtime_process_group(&mut child, self.process_id);
        }
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiMessage {
    content: String,
}

impl LocalProvider {
    async fn new(
        config: &ServiceConfig,
        runtime: &RuntimeSpec,
        server: &ServerConfig,
    ) -> Result<Self, ServiceError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.request_timeout_seconds))
            .build()
            .map_err(|error| {
                ServiceError::Internal(format!("failed to build HTTP client: {error}"))
            })?;
        let input_root = runtime_input_root(server)?;
        let child = spawn_service_command(config, runtime, &input_root, &server.cache_dir())?;
        let process_id = child.id().ok_or_else(|| {
            ServiceError::Internal("OCR runtime has no process ID after startup".to_string())
        })?;
        let runtime = Self {
            client,
            config: config.clone(),
            runtime: runtime.clone(),
            child: Arc::new(Mutex::new(child)),
            process_id,
            input_root,
        };
        if let Err(error) = runtime.wait_until_ready().await {
            if let Err(shutdown_error) = runtime.shutdown().await {
                tracing::error!(
                    "Failed to stop OCR runtime after startup failure: {shutdown_error}"
                );
            }
            return Err(error);
        }
        Ok(runtime)
    }

    async fn wait_until_ready(&self) -> Result<(), ServiceError> {
        let started = Instant::now();
        let models_url = format!("{}/models", self.runtime.base_url.trim_end_matches('/'));
        loop {
            if let Ok(response) = self.client.get(&models_url).send().await {
                if response.status().is_success() {
                    info!("local OCR runtime is ready at {}", self.runtime.base_url);
                    return Ok(());
                }
            }

            if let Ok(Some(status)) = self.child.lock().await.try_wait() {
                return Err(ServiceError::Internal(format!(
                    "local OCR runtime exited during startup with {status}"
                )));
            }

            if started.elapsed() >= Duration::from_secs(self.config.startup_timeout_seconds) {
                return Err(ServiceError::Internal(format!(
                    "local OCR runtime did not become ready within {} seconds",
                    self.config.startup_timeout_seconds
                )));
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    async fn shutdown(&self) -> Result<(), ServiceError> {
        stop_service_child(&self.child, self.process_id, "OCR").await
    }

    async fn is_alive(&self) -> Result<bool, ServiceError> {
        Ok(self
            .child
            .lock()
            .await
            .try_wait()
            .map_err(|error| {
                ServiceError::Internal(format!("failed to inspect OCR runtime: {error}"))
            })?
            .is_none())
    }
}

fn runtime_input_path(input_root: &Path, input: &InferenceInput) -> PathBuf {
    input_root
        .join(&input.job_id)
        .join(format!("input-{}", input.sequence))
}

fn runtime_input_descriptor(input: &InferenceInput) -> serde_json::Value {
    json!({
        "jobId": input.job_id,
        "sequence": input.sequence,
        "byteSize": input.byte_size,
        "contentHash": input.content_hash,
        "mimeType": input.mime_type,
    })
}

impl LocalProvider {
    async fn infer(&self, input: &InferenceInput) -> Result<InferenceResponse, ServiceError> {
        let input_path = runtime_input_path(&self.input_root, input);
        let image_url = reqwest::Url::from_file_path(&input_path).map_err(|_| {
            ServiceError::Configuration(format!(
                "OCR runtime input path is invalid: {}",
                input_path.display()
            ))
        })?;
        let request = json!({
            "model": self.runtime.model,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "<image>document parsing."},
                    {"type": "image_url", "image_url": {"url": image_url.to_string()}}
                ]
            }],
            "max_tokens": self.config.max_tokens,
            "temperature": 0.0,
            "extra_body": {
                "skip_special_tokens": false,
                "vllm_xargs": {"ngram_size": 35, "window_size": 128}
            }
        });
        let url = format!(
            "{}/chat/completions",
            self.runtime.base_url.trim_end_matches('/')
        );
        let response = self
            .client
            .post(url)
            .json(&request)
            .send()
            .await
            .map_err(|error| {
                ServiceError::RuntimeUnavailable(format!("local OCR request failed: {error}"))
            })?;
        let status = response.status();
        let body = response.text().await.map_err(|error| {
            ServiceError::Upstream(format!("failed to read local OCR response: {error}"))
        })?;
        if !status.is_success() {
            if status == StatusCode::BAD_REQUEST {
                return Err(ServiceError::BadRequest(format!(
                    "local OCR runtime rejected the image: {body}"
                )));
            }
            return Err(ServiceError::Upstream(format!(
                "local OCR runtime returned {status}: {body}"
            )));
        }

        let response: OpenAiResponse = serde_json::from_str(&body).map_err(|error| {
            ServiceError::Upstream(format!("invalid local OCR response: {error}"))
        })?;
        let raw_text = response
            .choices
            .first()
            .map(|choice| choice.message.content.as_str())
            .ok_or_else(|| {
                ServiceError::Upstream("local OCR response had no choices".to_string())
            })?;
        let text = if self.runtime.model == UNLIMITED_OCR_MODEL {
            normalize_unlimited_ocr_text(raw_text)
        } else {
            raw_text.trim().to_string()
        };
        Ok(InferenceResponse {
            task: "ocr".to_string(),
            text: text.clone(),
            markdown: text,
            provider: self.name().to_string(),
            model_type: "ocr".to_string(),
            model_version: self.runtime.model_version.clone(),
            tags: Vec::new(),
            embedding: None,
            embedding_encoding: None,
            embedding_dimensions: None,
            perceptual_hash: None,
            quality_score: None,
            faces: Vec::new(),
        })
    }

    fn name(&self) -> &'static str {
        "local"
    }

    async fn infer_inputs(
        &self,
        inputs: Vec<InferenceInput>,
    ) -> Result<Vec<InputInferenceResponse>, ServiceError> {
        let mut responses = Vec::with_capacity(inputs.len());
        for input in inputs {
            responses.push(InputInferenceResponse {
                sequence: input.sequence,
                frame_timestamp_ms: input.frame_timestamp_ms,
                response: self.infer(&input).await?,
            });
        }
        Ok(responses)
    }
}

struct ManagedRuntime {
    client: Client,
    config: ServiceConfig,
    spec: RuntimeSpec,
    child: Arc<Mutex<Child>>,
    process_id: u32,
    runtime_name: &'static str,
}

impl Drop for ManagedRuntime {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.try_lock() {
            kill_runtime_process_group(&mut child, self.process_id);
        }
    }
}

#[derive(Debug, Deserialize)]
struct TaggingResponse {
    tags: Vec<String>,
}

impl ManagedRuntime {
    async fn new(
        config: &ServiceConfig,
        spec: &RuntimeSpec,
        server: &ServerConfig,
        runtime_name: &'static str,
    ) -> Result<Self, ServiceError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.request_timeout_seconds))
            .build()
            .map_err(|error| {
                ServiceError::Internal(format!(
                    "failed to build {runtime_name} HTTP client: {error}"
                ))
            })?;
        let input_root = runtime_input_root(server)?;
        let child = spawn_service_command(config, spec, &input_root, &server.cache_dir())?;
        let process_id = child.id().ok_or_else(|| {
            ServiceError::Internal(format!(
                "{runtime_name} runtime has no process ID after startup"
            ))
        })?;
        let runtime = Self {
            client,
            config: config.clone(),
            spec: spec.clone(),
            child: Arc::new(Mutex::new(child)),
            process_id,
            runtime_name,
        };
        if let Err(error) = runtime.wait_until_ready().await {
            if let Err(shutdown_error) = runtime.shutdown().await {
                tracing::error!(
                    "Failed to stop {runtime_name} runtime after startup failure: {shutdown_error}"
                );
            }
            return Err(error);
        }
        Ok(runtime)
    }

    async fn wait_until_ready(&self) -> Result<(), ServiceError> {
        let started = Instant::now();
        let url = format!("{}/ready", self.spec.base_url.trim_end_matches('/'));
        loop {
            if let Ok(response) = self.client.get(&url).send().await {
                if response.status().is_success() {
                    info!(
                        "{} runtime is ready at {}",
                        self.runtime_name, self.spec.base_url
                    );
                    return Ok(());
                }
            }
            if let Ok(Some(status)) = self.child.lock().await.try_wait() {
                return Err(ServiceError::Internal(format!(
                    "{} runtime exited during startup with {status}",
                    self.runtime_name
                )));
            }
            if started.elapsed() >= Duration::from_secs(self.config.startup_timeout_seconds) {
                return Err(ServiceError::Internal(format!(
                    "{} runtime did not become ready within {} seconds",
                    self.runtime_name, self.config.startup_timeout_seconds
                )));
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    async fn shutdown(&self) -> Result<(), ServiceError> {
        stop_service_child(&self.child, self.process_id, self.runtime_name).await
    }

    async fn is_alive(&self) -> Result<bool, ServiceError> {
        Ok(self
            .child
            .lock()
            .await
            .try_wait()
            .map_err(|error| {
                ServiceError::Internal(format!(
                    "failed to inspect {} runtime: {error}",
                    self.runtime_name
                ))
            })?
            .is_none())
    }
}

pub struct RamProvider {
    runtime: ManagedRuntime,
}

impl RamProvider {
    pub async fn new(
        config: &ServiceConfig,
        runtime: &RuntimeSpec,
        server: &ServerConfig,
    ) -> Result<Self, ServiceError> {
        Ok(Self {
            runtime: ManagedRuntime::new(config, runtime, server, "RAM++").await?,
        })
    }

    pub async fn infer(&self, input: &InferenceInput) -> Result<InferenceResponse, ServiceError> {
        let url = format!("{}/infer", self.runtime.spec.base_url.trim_end_matches('/'));
        let response = self
            .runtime
            .client
            .post(url)
            .json(&runtime_input_descriptor(input))
            .send()
            .await
            .map_err(|error| {
                ServiceError::RuntimeUnavailable(format!("RAM++ request failed: {error}"))
            })?;
        let status = response.status();
        let body = response.text().await.map_err(|error| {
            ServiceError::Upstream(format!("failed to read RAM++ response: {error}"))
        })?;
        if !status.is_success() {
            return Err(ServiceError::BadRequest(format!(
                "RAM++ runtime returned {status}: {body}"
            )));
        }
        let result: TaggingResponse = serde_json::from_str(&body)
            .map_err(|error| ServiceError::Upstream(format!("invalid RAM++ response: {error}")))?;
        let text = result.tags.join("\n");
        Ok(InferenceResponse {
            task: "image_tagging".to_string(),
            text: text.clone(),
            markdown: text,
            provider: "ram++".to_string(),
            model_type: "image_tagging".to_string(),
            model_version: self.runtime.spec.model_version.clone(),
            tags: result.tags,
            embedding: None,
            embedding_encoding: None,
            embedding_dimensions: None,
            perceptual_hash: None,
            quality_score: None,
            faces: Vec::new(),
        })
    }

    async fn infer_inputs(
        &self,
        inputs: Vec<InferenceInput>,
    ) -> Result<Vec<InputInferenceResponse>, ServiceError> {
        let mut responses = Vec::with_capacity(inputs.len());
        for input in inputs {
            responses.push(InputInferenceResponse {
                sequence: input.sequence,
                frame_timestamp_ms: input.frame_timestamp_ms,
                response: self.infer(&input).await?,
            });
        }
        Ok(responses)
    }

    async fn shutdown(&self) -> Result<(), ServiceError> {
        self.runtime.shutdown().await
    }
}

pub struct ImageClusteringProvider {
    runtime: ManagedRuntime,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImageClusteringRuntimeResponse {
    embedding: String,
    embedding_encoding: String,
    embedding_dimensions: usize,
    perceptual_hash: String,
    quality_score: f32,
}

impl ImageClusteringProvider {
    pub async fn new(
        config: &ServiceConfig,
        runtime: &RuntimeSpec,
        server: &ServerConfig,
    ) -> Result<Self, ServiceError> {
        Ok(Self {
            runtime: ManagedRuntime::new(config, runtime, server, "DINOv2").await?,
        })
    }

    pub async fn infer(&self, input: &InferenceInput) -> Result<InferenceResponse, ServiceError> {
        let url = format!("{}/infer", self.runtime.spec.base_url.trim_end_matches('/'));
        let response = self
            .runtime
            .client
            .post(url)
            .json(&runtime_input_descriptor(input))
            .send()
            .await
            .map_err(|error| {
                ServiceError::RuntimeUnavailable(format!("DINOv2 request failed: {error}"))
            })?;
        let status = response.status();
        let body = response.text().await.map_err(|error| {
            ServiceError::Upstream(format!("failed to read DINOv2 response: {error}"))
        })?;
        if !status.is_success() {
            let message = format!("DINOv2 runtime returned {status}: {body}");
            return if status.is_client_error() {
                Err(ServiceError::BadRequest(message))
            } else {
                Err(ServiceError::Upstream(message))
            };
        }

        let clustering_response: ImageClusteringRuntimeResponse = serde_json::from_str(&body)
            .map_err(|error| ServiceError::Upstream(format!("invalid DINOv2 response: {error}")))?;
        self.validate_response(&clustering_response)?;

        Ok(InferenceResponse {
            task: "image_clustering".to_string(),
            text: String::new(),
            markdown: String::new(),
            provider: "dinov2".to_string(),
            model_type: "image_clustering".to_string(),
            model_version: self.runtime.spec.model_version.clone(),
            tags: Vec::new(),
            embedding: Some(clustering_response.embedding),
            embedding_encoding: Some(clustering_response.embedding_encoding),
            embedding_dimensions: Some(clustering_response.embedding_dimensions),
            perceptual_hash: Some(clustering_response.perceptual_hash),
            quality_score: Some(clustering_response.quality_score),
            faces: Vec::new(),
        })
    }

    async fn infer_inputs(
        &self,
        inputs: Vec<InferenceInput>,
    ) -> Result<Vec<InputInferenceResponse>, ServiceError> {
        let mut responses = Vec::with_capacity(inputs.len());
        for input in inputs {
            responses.push(InputInferenceResponse {
                sequence: input.sequence,
                frame_timestamp_ms: input.frame_timestamp_ms,
                response: self.infer(&input).await?,
            });
        }
        Ok(responses)
    }

    fn validate_response(
        &self,
        response: &ImageClusteringRuntimeResponse,
    ) -> Result<(), ServiceError> {
        if response.embedding_encoding != "float32_le" {
            return Err(ServiceError::Upstream(format!(
                "DINOv2 returned unsupported embedding encoding `{}`",
                response.embedding_encoding
            )));
        }
        if response.embedding_dimensions != self.runtime.spec.embedding_dimensions {
            return Err(ServiceError::Upstream(format!(
                "DINOv2 returned {} dimensions; expected {}",
                response.embedding_dimensions, self.runtime.spec.embedding_dimensions
            )));
        }
        validate_normalized_embedding(
            &response.embedding,
            response.embedding_dimensions,
            "DINOv2",
        )?;
        if response.perceptual_hash.len() != 16
            || !response
                .perceptual_hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(ServiceError::Upstream(
                "DINOv2 returned an invalid perceptual hash".to_string(),
            ));
        }
        if !response.quality_score.is_finite() || !(0.0..=1.0).contains(&response.quality_score) {
            return Err(ServiceError::Upstream(
                "DINOv2 returned an invalid quality score".to_string(),
            ));
        }
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), ServiceError> {
        self.runtime.shutdown().await
    }
}

pub struct FaceDetectionProvider {
    runtime: ManagedRuntime,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FaceDetectionRuntimeResponse {
    faces: Vec<FaceDetectionRuntimeFace>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FaceDetectionRuntimeFace {
    index: usize,
    bounding_box: FaceDetectionRuntimeBoundingBox,
    eye_center: FaceDetectionRuntimePoint,
    confidence: f32,
    quality_score: f32,
    frontality_score: f32,
    embedding: String,
    embedding_encoding: String,
    embedding_dimensions: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FaceDetectionRuntimeBoundingBox {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FaceDetectionRuntimePoint {
    x: f32,
    y: f32,
}

impl FaceDetectionProvider {
    pub async fn new(
        config: &ServiceConfig,
        runtime: &RuntimeSpec,
        server: &ServerConfig,
    ) -> Result<Self, ServiceError> {
        Ok(Self {
            runtime: ManagedRuntime::new(config, runtime, server, "InsightFace").await?,
        })
    }

    pub async fn infer(&self, input: &InferenceInput) -> Result<InferenceResponse, ServiceError> {
        let url = format!("{}/infer", self.runtime.spec.base_url.trim_end_matches('/'));
        let response = self
            .runtime
            .client
            .post(url)
            .json(&runtime_input_descriptor(input))
            .send()
            .await
            .map_err(|error| {
                ServiceError::RuntimeUnavailable(format!("InsightFace request failed: {error}"))
            })?;
        let status = response.status();
        let body = response.text().await.map_err(|error| {
            ServiceError::Upstream(format!("failed to read InsightFace response: {error}"))
        })?;
        if !status.is_success() {
            let message = format!("InsightFace runtime returned {status}: {body}");
            return if status.is_client_error() {
                Err(ServiceError::BadRequest(message))
            } else {
                Err(ServiceError::Upstream(message))
            };
        }
        let runtime_response: FaceDetectionRuntimeResponse =
            serde_json::from_str(&body).map_err(|error| {
                ServiceError::Upstream(format!("invalid InsightFace response: {error}"))
            })?;
        self.validate_response(&runtime_response)?;
        let faces = runtime_response
            .faces
            .into_iter()
            .map(|face| FaceDetection {
                index: face.index,
                bounding_box: NormalizedBoundingBox {
                    x: face.bounding_box.x,
                    y: face.bounding_box.y,
                    width: face.bounding_box.width,
                    height: face.bounding_box.height,
                },
                eye_center: NormalizedPoint {
                    x: face.eye_center.x,
                    y: face.eye_center.y,
                },
                confidence: face.confidence,
                quality_score: face.quality_score,
                frontality_score: face.frontality_score,
                embedding: face.embedding,
                embedding_encoding: face.embedding_encoding,
                embedding_dimensions: face.embedding_dimensions,
            })
            .collect();

        Ok(InferenceResponse {
            task: "face_detection".to_string(),
            text: String::new(),
            markdown: String::new(),
            provider: "insightface".to_string(),
            model_type: "face_detection".to_string(),
            model_version: self.runtime.spec.model_version.clone(),
            tags: Vec::new(),
            embedding: None,
            embedding_encoding: None,
            embedding_dimensions: None,
            perceptual_hash: None,
            quality_score: None,
            faces,
        })
    }

    async fn infer_inputs(
        &self,
        inputs: Vec<InferenceInput>,
    ) -> Result<Vec<InputInferenceResponse>, ServiceError> {
        let mut responses = Vec::with_capacity(inputs.len());
        for input in inputs {
            responses.push(InputInferenceResponse {
                sequence: input.sequence,
                frame_timestamp_ms: input.frame_timestamp_ms,
                response: self.infer(&input).await?,
            });
        }
        Ok(responses)
    }

    fn validate_response(
        &self,
        response: &FaceDetectionRuntimeResponse,
    ) -> Result<(), ServiceError> {
        for (index, face) in response.faces.iter().enumerate() {
            if face.index != index {
                return Err(ServiceError::Upstream(format!(
                    "InsightFace returned face index {}; expected {index}",
                    face.index
                )));
            }
            validate_normalized_face_box(&face.bounding_box)?;
            validate_normalized_point(&face.eye_center, "eye center")?;
            validate_unit_score(face.confidence, "confidence")?;
            validate_unit_score(face.quality_score, "quality score")?;
            validate_unit_score(face.frontality_score, "frontality score")?;
            if face.embedding_encoding != "float32_le" {
                return Err(ServiceError::Upstream(format!(
                    "InsightFace returned unsupported embedding encoding `{}`",
                    face.embedding_encoding
                )));
            }
            if face.embedding_dimensions != self.runtime.spec.embedding_dimensions {
                return Err(ServiceError::Upstream(format!(
                    "InsightFace returned {} dimensions; expected {}",
                    face.embedding_dimensions, self.runtime.spec.embedding_dimensions
                )));
            }
            validate_normalized_embedding(
                &face.embedding,
                face.embedding_dimensions,
                "InsightFace",
            )?;
        }
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), ServiceError> {
        self.runtime.shutdown().await
    }
}

fn validate_normalized_face_box(
    bounding_box: &FaceDetectionRuntimeBoundingBox,
) -> Result<(), ServiceError> {
    let values = [
        bounding_box.x,
        bounding_box.y,
        bounding_box.width,
        bounding_box.height,
    ];
    if values.iter().any(|value| !value.is_finite())
        || !(0.0..=1.0).contains(&bounding_box.x)
        || !(0.0..=1.0).contains(&bounding_box.y)
        || bounding_box.width <= 0.0
        || bounding_box.height <= 0.0
        || bounding_box.x + bounding_box.width > 1.0
        || bounding_box.y + bounding_box.height > 1.0
    {
        return Err(ServiceError::Upstream(
            "InsightFace returned an invalid normalized bounding box".to_string(),
        ));
    }
    Ok(())
}

fn validate_normalized_point(
    point: &FaceDetectionRuntimePoint,
    point_name: &str,
) -> Result<(), ServiceError> {
    if !point.x.is_finite()
        || !point.y.is_finite()
        || !(0.0..=1.0).contains(&point.x)
        || !(0.0..=1.0).contains(&point.y)
    {
        return Err(ServiceError::Upstream(format!(
            "InsightFace returned an invalid normalized {point_name}"
        )));
    }
    Ok(())
}

fn validate_unit_score(score: f32, name: &str) -> Result<(), ServiceError> {
    if !score.is_finite() || !(0.0..=1.0).contains(&score) {
        return Err(ServiceError::Upstream(format!(
            "InsightFace returned an invalid {name}"
        )));
    }
    Ok(())
}

fn validate_normalized_embedding(
    embedding: &str,
    dimensions: usize,
    provider_name: &str,
) -> Result<(), ServiceError> {
    let embedding_bytes = STANDARD.decode(embedding).map_err(|error| {
        ServiceError::Upstream(format!(
            "{provider_name} returned invalid base64 embedding: {error}"
        ))
    })?;
    let expected_bytes = dimensions * std::mem::size_of::<f32>();
    if embedding_bytes.len() != expected_bytes {
        return Err(ServiceError::Upstream(format!(
            "{provider_name} returned {} embedding bytes; expected {expected_bytes}",
            embedding_bytes.len()
        )));
    }
    let mut squared_norm = 0.0_f64;
    for encoded_value in embedding_bytes.chunks_exact(4) {
        let value =
            f32::from_le_bytes(encoded_value.try_into().map_err(|_| {
                ServiceError::Internal("failed to decode embedding value".to_string())
            })?);
        if !value.is_finite() {
            return Err(ServiceError::Upstream(format!(
                "{provider_name} returned a non-finite embedding"
            )));
        }
        squared_norm += f64::from(value) * f64::from(value);
    }
    let norm = squared_norm.sqrt();
    if (norm - 1.0).abs() > 0.01 {
        return Err(ServiceError::Upstream(format!(
            "{provider_name} embedding norm {norm:.6} is not normalized"
        )));
    }
    Ok(())
}

async fn stop_service_child(
    child: &Arc<Mutex<Child>>,
    process_id: u32,
    runtime_name: &str,
) -> Result<(), ServiceError> {
    let mut child = child.lock().await;
    let child_exited = child
        .try_wait()
        .map_err(|error| {
            ServiceError::Internal(format!("failed to inspect {runtime_name} runtime: {error}"))
        })?
        .is_some();
    if child_exited
        && !process_group_exists(process_id).map_err(|error| {
            ServiceError::Internal(format!(
                "failed to inspect {runtime_name} runtime process group: {error}"
            ))
        })?
    {
        return Ok(());
    }

    signal_process_group(process_id, libc::SIGTERM).map_err(|error| {
        ServiceError::Internal(format!(
            "failed to terminate {runtime_name} runtime: {error}"
        ))
    })?;
    match tokio::time::timeout(
        RUNTIME_SHUTDOWN_TIMEOUT,
        wait_for_process_group_exit(&mut child, process_id),
    )
    .await
    {
        Ok(result) => result.map_err(|error| {
            ServiceError::Internal(format!(
                "failed to wait for {runtime_name} runtime: {error}"
            ))
        })?,
        Err(_) => {
            signal_process_group(process_id, libc::SIGKILL).map_err(|error| {
                ServiceError::Internal(format!("failed to kill {runtime_name} runtime: {error}"))
            })?;
            tokio::time::timeout(
                RUNTIME_SHUTDOWN_TIMEOUT,
                wait_for_process_group_exit(&mut child, process_id),
            )
            .await
            .map_err(|_| {
                ServiceError::Internal(format!(
                    "{runtime_name} runtime process group survived forced shutdown"
                ))
            })?
            .map_err(|error| {
                ServiceError::Internal(format!(
                    "failed to reap {runtime_name} runtime after forced shutdown: {error}"
                ))
            })?;
        }
    }
    Ok(())
}

async fn wait_for_process_group_exit(child: &mut Child, process_id: u32) -> std::io::Result<()> {
    loop {
        let child_exited = child.try_wait()?.is_some();
        if child_exited && !process_group_exists(process_id)? {
            return Ok(());
        }
        tokio::time::sleep(RUNTIME_PROCESS_POLL_INTERVAL).await;
    }
}

fn runtime_input_root(server: &ServerConfig) -> Result<PathBuf, ServiceError> {
    std::fs::canonicalize(server.processing_dir()).map_err(|error| {
        ServiceError::Configuration(format!(
            "failed to resolve runtime queue input directory: {error}"
        ))
    })
}

fn signal_process_group(process_id: u32, signal: libc::c_int) -> std::io::Result<()> {
    let result = unsafe { libc::kill(-(process_id as libc::pid_t), signal) };
    if result == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        return Ok(());
    }
    Err(error)
}

fn process_group_exists(process_id: u32) -> std::io::Result<bool> {
    let result = unsafe { libc::kill(-(process_id as libc::pid_t), 0) };
    if result == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ESRCH) => Ok(false),
        Some(libc::EPERM) => Ok(true),
        _ => Err(error),
    }
}

fn kill_runtime_process_group(child: &mut Child, process_id: u32) {
    let _ = signal_process_group(process_id, libc::SIGKILL);
    let _ = child.start_kill();
}

fn spawn_service_command(
    config: &ServiceConfig,
    runtime: &RuntimeSpec,
    input_root: &Path,
    cache_dir: &Path,
) -> Result<Child, ServiceError> {
    let input_root = input_root.to_string_lossy();
    std::fs::create_dir_all(cache_dir).map_err(|error| {
        ServiceError::Configuration(format!(
            "failed to create runtime cache directory {}: {error}",
            cache_dir.display()
        ))
    })?;
    let cache_dir = cache_dir.to_string_lossy();
    let minimum_face_likelihood = config
        .minimum_face_likelihood
        .map(|likelihood| likelihood.to_string());
    let minimum_face_resolution_pixels = config
        .minimum_face_resolution_pixels
        .map(|resolution| resolution.to_string());
    let args = runtime
        .arguments
        .iter()
        .map(|arg| {
            let mut argument = arg.replace(RUNTIME_INPUT_PLACEHOLDER, &input_root).replace(
                RUNTIME_CONCURRENCY_PLACEHOLDER,
                &config.max_concurrent_jobs.to_string(),
            );
            argument = argument.replace(RUNTIME_CACHE_PLACEHOLDER, &cache_dir);
            if let Some(likelihood) = &minimum_face_likelihood {
                argument = argument.replace(FACE_LIKELIHOOD_PLACEHOLDER, likelihood);
            }
            if let Some(resolution) = &minimum_face_resolution_pixels {
                argument = argument.replace(FACE_RESOLUTION_PLACEHOLDER, resolution);
            }
            argument
        })
        .collect::<Vec<_>>();
    let mut command = Command::new(&runtime.executable);
    if let Some(executable_directory) = runtime
        .executable
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        command.env(
            "PATH",
            format!("{}:{SYSTEM_RUNTIME_PATH}", executable_directory.display()),
        );
    }
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .envs(runtime.environment.iter().cloned())
        .env("HOME", cache_dir.as_ref())
        .env("XDG_CACHE_HOME", cache_dir.as_ref())
        .env("HF_HUB_OFFLINE", "1")
        .env("TRANSFORMERS_OFFLINE", "1")
        .process_group(0);
    let mut child = command.spawn().map_err(|error| {
        ServiceError::Configuration(format!(
            "failed to start {} runtime `{}`: {error}",
            config.model_type,
            runtime.executable.display()
        ))
    })?;
    forward_child_output(&mut child, &config.model_type);
    Ok(child)
}

fn forward_child_output(child: &mut Child, service_type: &str) {
    let service_type = service_type.to_string();
    if let Some(stdout) = child.stdout.take() {
        forward_child_stream(stdout, service_type.clone(), "stdout");
    }
    if let Some(stderr) = child.stderr.take() {
        forward_child_stream(stderr, service_type, "stderr");
    }
}

fn forward_child_stream<R>(stream: R, service_type: String, stream_name: &'static str)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::info!(
                target: "llm_runtime",
                service = %service_type,
                stream = stream_name,
                "{}",
                redact_base64_text(&line)
            );
        }
    });
}

pub fn redact_base64_text(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut token_start = None;
    for (index, character) in text.char_indices() {
        if character.is_ascii_alphanumeric() || matches!(character, '+' | '/' | '=') {
            token_start.get_or_insert(index);
            continue;
        }
        redact_base64_token(&mut output, text, token_start.take(), index);
        output.push(character);
    }
    redact_base64_token(&mut output, text, token_start, text.len());
    output
}

fn redact_base64_token(output: &mut String, text: &str, start: Option<usize>, end: usize) {
    let Some(start) = start else {
        return;
    };
    let token = &text[start..end];
    if token.len() >= 64 {
        output.push_str("[base64 omitted]");
        return;
    }
    output.push_str(token);
}

#[cfg(test)]
mod tests {
    use super::{RuntimeCatalog, ServiceManager, ServiceType};
    use std::sync::Arc;

    #[test]
    fn service_manager_starts_without_an_active_runtime() {
        let manager = ServiceManager::new(Arc::new(crate::config::Config {
            server: Default::default(),
            scheduler: Default::default(),
            service: Vec::new(),
        }));
        assert_eq!(manager.active_name(), "on-demand");
    }

    #[test]
    fn service_type_rejects_unknown_tasks() {
        assert!(ServiceType::from_task("object_detection").is_err());
        assert_eq!(ServiceType::from_task("ocr").unwrap(), ServiceType::Ocr);
    }

    #[test]
    fn production_runtimes_are_image_owned_direct_commands() {
        let runtimes = RuntimeCatalog::production();

        for service_type in [
            ServiceType::Ocr,
            ServiceType::ImageTagging,
            ServiceType::ImageClustering,
            ServiceType::FaceDetection,
        ] {
            let runtime = runtimes.get(service_type).expect("production runtime");
            assert!(runtime.executable.starts_with("/opt/venvs"));
            assert!(!runtime.arguments.iter().any(|argument| {
                argument == "sh" || argument == "bash" || argument.contains("pip install")
            }));
        }

        let ocr = runtimes.get(ServiceType::Ocr).expect("OCR runtime");
        assert!(ocr
            .environment
            .iter()
            .any(|(name, value)| { name == "VLLM_USE_FLASHINFER_SAMPLER" && value == "0" }));
    }
}
