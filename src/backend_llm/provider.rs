use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine};
use reqwest::Client;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::info;

use crate::adapters::{normalize_baidu_unlimited_ocr_text, BAIDU_UNLIMITED_OCR_MODEL};
use crate::config::{Config, ProviderKind, ServiceConfig};
use crate::error::ServiceError;

const UV_BOOTSTRAP_COMMAND: &str = "UV_VERSION=0.8.22; UV_MACHINE=$(uname -m); if [ \"$UV_MACHINE\" = \"x86_64\" ]; then UV_TARGET=x86_64-unknown-linux-gnu; elif [ \"$UV_MACHINE\" = \"aarch64\" ] || [ \"$UV_MACHINE\" = \"arm64\" ]; then UV_TARGET=aarch64-unknown-linux-gnu; else echo \"Unsupported uv architecture: $UV_MACHINE\" >&2; exit 1; fi; python -c 'import sys, urllib.request; urllib.request.urlretrieve(sys.argv[1], sys.argv[2])' \"https://github.com/astral-sh/uv/releases/download/$UV_VERSION/uv-$UV_TARGET.tar.gz\" /tmp/uv.tar.gz && tar -xzf /tmp/uv.tar.gz -C /tmp && install \"/tmp/uv-$UV_TARGET/uv\" /usr/local/bin/uv";

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
}

#[async_trait]
pub trait OcrProvider: Send + Sync {
    async fn infer(&self, image: &[u8], filename: &str) -> Result<InferenceResponse, ServiceError>;
    fn name(&self) -> &'static str;
}

pub enum Provider {
    Baidu(BaiduProvider),
    Local(LocalProvider),
}

impl Provider {
    pub async fn build(service: &ServiceConfig) -> Result<Self, ServiceError> {
        match service.provider {
            ProviderKind::Baidu => Ok(Self::Baidu(BaiduProvider::new(service)?)),
            ProviderKind::Local => Ok(Self::Local(LocalProvider::new(service).await?)),
        }
    }

    pub async fn infer(
        &self,
        image: &[u8],
        filename: &str,
    ) -> Result<InferenceResponse, ServiceError> {
        match self {
            Self::Baidu(provider) => provider.infer(image, filename).await,
            Self::Local(provider) => provider.infer(image, filename).await,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Baidu(provider) => provider.name(),
            Self::Local(provider) => provider.name(),
        }
    }

    async fn shutdown(self) -> Result<(), ServiceError> {
        if let Self::Local(provider) = self {
            provider.shutdown().await
        } else {
            Ok(())
        }
    }

    async fn is_alive(&self) -> Result<bool, ServiceError> {
        match self {
            Self::Baidu(_) => Ok(true),
            Self::Local(provider) => provider.is_alive().await,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceType {
    Ocr,
    ImageTagging,
    ImageClustering,
}

impl ServiceType {
    fn from_task(task: &str) -> Result<Self, ServiceError> {
        match task {
            "ocr" => Ok(Self::Ocr),
            "image_tagging" => Ok(Self::ImageTagging),
            "image_clustering" => Ok(Self::ImageClustering),
            _ => Err(ServiceError::NotImplemented(format!(
                "inference task `{task}` has no configured model provider"
            ))),
        }
    }

    fn config_key(self) -> &'static str {
        match self {
            Self::Ocr => "ocr",
            Self::ImageTagging => "image_tagging",
            Self::ImageClustering => "image_clustering",
        }
    }
}

enum ActiveService {
    Ocr(Provider),
    ImageTagging(RamProvider),
    ImageClustering(ImageClusteringProvider),
}

impl ActiveService {
    fn service_type(&self) -> ServiceType {
        match self {
            Self::Ocr(_) => ServiceType::Ocr,
            Self::ImageTagging(_) => ServiceType::ImageTagging,
            Self::ImageClustering(_) => ServiceType::ImageClustering,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Ocr(provider) => provider.name(),
            Self::ImageTagging(_) => "ram++",
            Self::ImageClustering(_) => "dinov2",
        }
    }

    async fn shutdown(self) -> Result<(), ServiceError> {
        match self {
            Self::Ocr(provider) => provider.shutdown().await,
            Self::ImageTagging(provider) => provider.shutdown().await,
            Self::ImageClustering(provider) => provider.shutdown().await,
        }
    }

    async fn is_alive(&self) -> Result<bool, ServiceError> {
        match self {
            Self::Ocr(provider) => provider.is_alive().await,
            Self::ImageTagging(provider) => provider.runtime.is_alive().await,
            Self::ImageClustering(provider) => provider.runtime.is_alive().await,
        }
    }
}

pub struct ServiceManager {
    config: Arc<Config>,
    active: Option<ActiveService>,
}

impl ServiceManager {
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            config,
            active: None,
        }
    }

    pub fn active_name(&self) -> &'static str {
        self.active
            .as_ref()
            .map(ActiveService::name)
            .unwrap_or("on-demand")
    }

    pub async fn infer(
        &mut self,
        task: &str,
        image: &[u8],
        filename: &str,
    ) -> Result<InferenceResponse, ServiceError> {
        let service_type = ServiceType::from_task(task)?;
        self.activate(service_type).await?;
        match self.active.as_ref() {
            Some(ActiveService::Ocr(provider)) => provider.infer(image, filename).await,
            Some(ActiveService::ImageTagging(provider)) => provider.infer(image).await,
            Some(ActiveService::ImageClustering(provider)) => provider.infer(image).await,
            None => Err(ServiceError::Internal(
                "LLM service was not activated".to_string(),
            )),
        }
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
                    "inference task `{}` has no configured model provider",
                    service_type.config_key()
                ))
            })?;
        let active = match service_type {
            ServiceType::Ocr => ActiveService::Ocr(Provider::build(&service).await?),
            ServiceType::ImageTagging => {
                ActiveService::ImageTagging(RamProvider::new(&service).await?)
            }
            ServiceType::ImageClustering => {
                ActiveService::ImageClustering(ImageClusteringProvider::new(&service).await?)
            }
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

pub struct BaiduProvider {
    client: Client,
    config: ServiceConfig,
    token: Mutex<Option<CachedToken>>,
}

#[derive(Debug, Clone)]
struct CachedToken {
    value: String,
    expires_at: Instant,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    expires_in: Option<u64>,
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BaiduResponse {
    words_result: Option<Vec<BaiduWord>>,
    error_code: Option<i64>,
    error_msg: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BaiduWord {
    words: String,
}

impl BaiduProvider {
    fn new(config: &ServiceConfig) -> Result<Self, ServiceError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.request_timeout_seconds))
            .build()
            .map_err(|error| {
                ServiceError::Internal(format!("failed to build HTTP client: {error}"))
            })?;

        Ok(Self {
            client,
            config: config.clone(),
            token: Mutex::new(None),
        })
    }

    async fn access_token(&self) -> Result<String, ServiceError> {
        let cached_token = self.token.lock().await.clone();
        if let Some(token) = cached_token {
            if token.expires_at > Instant::now() {
                return Ok(token.value);
            }
        }

        let response = self
            .client
            .post(&self.config.token_url)
            .query(&[
                ("grant_type", "client_credentials"),
                ("client_id", self.config.api_key.as_str()),
                ("client_secret", self.config.secret_key.as_str()),
            ])
            .send()
            .await
            .map_err(|error| ServiceError::Upstream(format!("token request failed: {error}")))?;

        let status = response.status();
        let body = response.text().await.map_err(|error| {
            ServiceError::Upstream(format!("failed to read token response: {error}"))
        })?;
        if !status.is_success() {
            if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
                return Err(ServiceError::Configuration(format!(
                    "Baidu credentials were rejected by the token endpoint: {body}"
                )));
            }
            return Err(ServiceError::Upstream(format!(
                "token endpoint returned {status}: {body}"
            )));
        }

        let token_response: TokenResponse = serde_json::from_str(&body)
            .map_err(|error| ServiceError::Upstream(format!("invalid token response: {error}")))?;
        let token = token_response.access_token.ok_or_else(|| {
            ServiceError::Upstream(
                token_response
                    .error_description
                    .unwrap_or_else(|| "token response did not contain access_token".to_string()),
            )
        })?;
        let expires_in = token_response.expires_in.unwrap_or(1800).saturating_sub(60);
        *self.token.lock().await = Some(CachedToken {
            value: token.clone(),
            expires_at: Instant::now() + Duration::from_secs(expires_in),
        });
        Ok(token)
    }
}

#[async_trait]
impl OcrProvider for BaiduProvider {
    async fn infer(
        &self,
        image: &[u8],
        _filename: &str,
    ) -> Result<InferenceResponse, ServiceError> {
        let token = self.access_token().await?;
        let encoded = STANDARD.encode(image);
        let response = self
            .client
            .post(&self.config.ocr_url)
            .query(&[("access_token", token)])
            .form(&[("image", encoded.as_str())])
            .send()
            .await
            .map_err(|error| ServiceError::Upstream(format!("OCR request failed: {error}")))?;

        let status = response.status();
        let body = response.text().await.map_err(|error| {
            ServiceError::Upstream(format!("failed to read OCR response: {error}"))
        })?;
        if !status.is_success() {
            if status == StatusCode::BAD_REQUEST {
                return Err(ServiceError::BadRequest(format!(
                    "Baidu OCR rejected the image: {body}"
                )));
            }
            return Err(ServiceError::Upstream(format!(
                "OCR endpoint returned {status}: {body}"
            )));
        }

        let result: BaiduResponse = serde_json::from_str(&body)
            .map_err(|error| ServiceError::Upstream(format!("invalid OCR response: {error}")))?;
        if let Some(error_code) = result.error_code {
            return Err(ServiceError::Upstream(format!(
                "Baidu OCR error {error_code}: {}",
                result
                    .error_msg
                    .unwrap_or_else(|| "unknown error".to_string())
            )));
        }

        let text = result
            .words_result
            .unwrap_or_default()
            .into_iter()
            .map(|word| word.words)
            .collect::<Vec<_>>()
            .join("\n");
        let text = normalize_baidu_unlimited_ocr_text(&text);
        Ok(InferenceResponse {
            task: "ocr".to_string(),
            text: text.clone(),
            markdown: text,
            provider: self.name().to_string(),
            model_type: "ocr".to_string(),
            model_version: self.config.model_version.clone(),
            tags: Vec::new(),
            embedding: None,
            embedding_encoding: None,
            embedding_dimensions: None,
            perceptual_hash: None,
            quality_score: None,
        })
    }

    fn name(&self) -> &'static str {
        "baidu"
    }
}

pub struct LocalProvider {
    client: Client,
    config: ServiceConfig,
    child: Arc<Mutex<Child>>,
}

impl Drop for LocalProvider {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.try_lock() {
            let _ = child.start_kill();
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
    async fn new(config: &ServiceConfig) -> Result<Self, ServiceError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.request_timeout_seconds))
            .build()
            .map_err(|error| {
                ServiceError::Internal(format!("failed to build HTTP client: {error}"))
            })?;
        let child = spawn_service_command(config)?;
        let provider = Self {
            client,
            config: config.clone(),
            child: Arc::new(Mutex::new(child)),
        };
        if let Err(error) = provider.wait_until_ready().await {
            if let Err(shutdown_error) = provider.shutdown().await {
                tracing::error!(
                    "Failed to stop OCR runtime after startup failure: {shutdown_error}"
                );
            }
            return Err(error);
        }
        Ok(provider)
    }

    async fn wait_until_ready(&self) -> Result<(), ServiceError> {
        let started = Instant::now();
        let models_url = format!("{}/models", self.config.base_url.trim_end_matches('/'));
        loop {
            if let Ok(response) = self.client.get(&models_url).send().await {
                if response.status().is_success() {
                    info!("local OCR runtime is ready at {}", self.config.base_url);
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

    async fn shutdown(self) -> Result<(), ServiceError> {
        stop_service_child(&self.child, &self.config, "OCR").await
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

#[async_trait]
impl OcrProvider for LocalProvider {
    async fn infer(&self, image: &[u8], filename: &str) -> Result<InferenceResponse, ServiceError> {
        let (image, filename) = normalize_local_image(
            image,
            filename,
            self.config.max_image_width,
            self.config.max_image_height,
        )
        .await?;
        let mime_type = mime_guess::from_path(&filename)
            .first_raw()
            .filter(|value| value.starts_with("image/"))
            .unwrap_or("image/jpeg");
        let encoded = STANDARD.encode(image);
        let request = json!({
            "model": self.config.model,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "<image>document parsing."},
                    {"type": "image_url", "image_url": {"url": format!("data:{mime_type};base64,{encoded}")}}
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
            self.config.base_url.trim_end_matches('/')
        );
        let response = self
            .client
            .post(url)
            .json(&request)
            .send()
            .await
            .map_err(|error| {
                ServiceError::Upstream(format!("local OCR request failed: {error}"))
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
        let text = if self.config.model == BAIDU_UNLIMITED_OCR_MODEL {
            normalize_baidu_unlimited_ocr_text(raw_text)
        } else {
            raw_text.trim().to_string()
        };
        Ok(InferenceResponse {
            task: "ocr".to_string(),
            text: text.clone(),
            markdown: text,
            provider: self.name().to_string(),
            model_type: "ocr".to_string(),
            model_version: self.config.model_version.clone(),
            tags: Vec::new(),
            embedding: None,
            embedding_encoding: None,
            embedding_dimensions: None,
            perceptual_hash: None,
            quality_score: None,
        })
    }

    fn name(&self) -> &'static str {
        "local"
    }
}

struct ManagedRuntime {
    client: Client,
    config: ServiceConfig,
    child: Arc<Mutex<Child>>,
    runtime_name: &'static str,
}

impl Drop for ManagedRuntime {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.try_lock() {
            let _ = child.start_kill();
        }
    }
}

#[derive(Debug, Deserialize)]
struct TaggingResponse {
    tags: Vec<String>,
}

impl ManagedRuntime {
    async fn new(config: &ServiceConfig, runtime_name: &'static str) -> Result<Self, ServiceError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.request_timeout_seconds))
            .build()
            .map_err(|error| {
                ServiceError::Internal(format!(
                    "failed to build {runtime_name} HTTP client: {error}"
                ))
            })?;
        let child = spawn_service_command(config)?;
        let runtime = Self {
            client,
            config: config.clone(),
            child: Arc::new(Mutex::new(child)),
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
        let url = format!("{}/ready", self.config.base_url.trim_end_matches('/'));
        loop {
            if let Ok(response) = self.client.get(&url).send().await {
                if response.status().is_success() {
                    info!(
                        "{} runtime is ready at {}",
                        self.runtime_name, self.config.base_url
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

    async fn shutdown(self) -> Result<(), ServiceError> {
        stop_service_child(&self.child, &self.config, self.runtime_name).await
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
    pub async fn new(config: &ServiceConfig) -> Result<Self, ServiceError> {
        Ok(Self {
            runtime: ManagedRuntime::new(config, "RAM++").await?,
        })
    }

    pub async fn infer(&self, image: &[u8]) -> Result<InferenceResponse, ServiceError> {
        let encoded = STANDARD.encode(image);
        let url = format!(
            "{}/infer",
            self.runtime.config.base_url.trim_end_matches('/')
        );
        let response = self
            .runtime
            .client
            .post(url)
            .json(&json!({ "image": encoded }))
            .send()
            .await
            .map_err(|error| ServiceError::Upstream(format!("RAM++ request failed: {error}")))?;
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
            model_version: self.runtime.config.model_version.clone(),
            tags: result.tags,
            embedding: None,
            embedding_encoding: None,
            embedding_dimensions: None,
            perceptual_hash: None,
            quality_score: None,
        })
    }

    async fn shutdown(self) -> Result<(), ServiceError> {
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
    pub async fn new(config: &ServiceConfig) -> Result<Self, ServiceError> {
        Ok(Self {
            runtime: ManagedRuntime::new(config, "DINOv2").await?,
        })
    }

    pub async fn infer(&self, image: &[u8]) -> Result<InferenceResponse, ServiceError> {
        let encoded = STANDARD.encode(image);
        let url = format!(
            "{}/infer",
            self.runtime.config.base_url.trim_end_matches('/')
        );
        let response = self
            .runtime
            .client
            .post(url)
            .json(&json!({ "image": encoded }))
            .send()
            .await
            .map_err(|error| ServiceError::Upstream(format!("DINOv2 request failed: {error}")))?;
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
            model_version: self.runtime.config.model_version.clone(),
            tags: Vec::new(),
            embedding: Some(clustering_response.embedding),
            embedding_encoding: Some(clustering_response.embedding_encoding),
            embedding_dimensions: Some(clustering_response.embedding_dimensions),
            perceptual_hash: Some(clustering_response.perceptual_hash),
            quality_score: Some(clustering_response.quality_score),
        })
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
        if response.embedding_dimensions != self.runtime.config.embedding_dimensions {
            return Err(ServiceError::Upstream(format!(
                "DINOv2 returned {} dimensions; expected {}",
                response.embedding_dimensions, self.runtime.config.embedding_dimensions
            )));
        }
        let embedding_bytes = STANDARD.decode(&response.embedding).map_err(|error| {
            ServiceError::Upstream(format!("DINOv2 returned invalid base64 embedding: {error}"))
        })?;
        let expected_bytes = response.embedding_dimensions * std::mem::size_of::<f32>();
        if embedding_bytes.len() != expected_bytes {
            return Err(ServiceError::Upstream(format!(
                "DINOv2 returned {} embedding bytes; expected {expected_bytes}",
                embedding_bytes.len()
            )));
        }

        let mut squared_norm = 0.0_f64;
        for encoded_value in embedding_bytes.chunks_exact(4) {
            let value = f32::from_le_bytes(encoded_value.try_into().map_err(|_| {
                ServiceError::Internal("failed to decode embedding value".to_string())
            })?);
            if !value.is_finite() {
                return Err(ServiceError::Upstream(
                    "DINOv2 returned a non-finite embedding".to_string(),
                ));
            }
            squared_norm += f64::from(value) * f64::from(value);
        }
        let norm = squared_norm.sqrt();
        if (norm - 1.0).abs() > 0.01 {
            return Err(ServiceError::Upstream(format!(
                "DINOv2 embedding norm {norm:.6} is not normalized"
            )));
        }
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

    async fn shutdown(self) -> Result<(), ServiceError> {
        self.runtime.shutdown().await
    }
}

fn configured_container_name(config: &ServiceConfig) -> Option<&str> {
    config
        .docker_command
        .windows(2)
        .find(|arguments| arguments[0] == "--name")
        .map(|arguments| arguments[1].as_str())
}

async fn stop_service_child(
    child: &Arc<Mutex<Child>>,
    config: &ServiceConfig,
    runtime_name: &str,
) -> Result<(), ServiceError> {
    let mut child = child.lock().await;
    if child
        .try_wait()
        .map_err(|error| {
            ServiceError::Internal(format!("failed to inspect {runtime_name} runtime: {error}"))
        })?
        .is_some()
    {
        return Ok(());
    }

    if let Some(container_name) = configured_container_name(config) {
        let stop_output = Command::new("docker")
            .args(["stop", "--time", "10", container_name])
            .output()
            .await
            .map_err(|error| {
                ServiceError::Internal(format!(
                    "failed to run docker stop for {runtime_name}: {error}"
                ))
            })?;
        if !stop_output.status.success() {
            let _ = Command::new("docker")
                .args(["rm", "-f", container_name])
                .output()
                .await;
        }
        child.wait().await.map_err(|error| {
            ServiceError::Internal(format!(
                "failed to wait for {runtime_name} docker client: {error}"
            ))
        })?;
        return Ok(());
    }

    child.start_kill().map_err(|error| {
        ServiceError::Internal(format!("failed to stop {runtime_name} runtime: {error}"))
    })?;
    child.wait().await.map_err(|error| {
        ServiceError::Internal(format!(
            "failed to wait for {runtime_name} runtime: {error}"
        ))
    })?;
    Ok(())
}

fn spawn_service_command(config: &ServiceConfig) -> Result<Child, ServiceError> {
    let executable = config.docker_command.first().ok_or_else(|| {
        ServiceError::Configuration(format!(
            "{} service docker_command must not be empty",
            config.model_type
        ))
    })?;
    let script_path = if config.script_path.is_empty() {
        String::new()
    } else {
        std::fs::canonicalize(&config.script_path)
            .map_err(|error| {
                ServiceError::Configuration(format!(
                    "failed to resolve {} service script {}: {error}",
                    config.model_type, config.script_path
                ))
            })?
            .to_string_lossy()
            .into_owned()
    };
    let args = config
        .docker_command
        .iter()
        .skip(1)
        .map(|arg| {
            arg.replace("{script_path}", &script_path)
                .replace("{device}", &config.device)
                .replace("{model}", &config.model)
                .replace("{uv_bootstrap}", UV_BOOTSTRAP_COMMAND)
        })
        .collect::<Vec<_>>();
    let mut command = Command::new(executable);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| {
        ServiceError::Configuration(format!(
            "failed to start {} service command `{executable}`: {error}",
            config.model_type
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
                line
            );
        }
    });
}

async fn normalize_local_image(
    image: &[u8],
    filename: &str,
    max_width: u32,
    max_height: u32,
) -> Result<(Vec<u8>, String), ServiceError> {
    let resize = format!("{max_width}x{max_height}>");
    let mut process = Command::new("magick")
        .args(["-", "-auto-orient", "-resize", &resize, "jpg:-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            ServiceError::Internal(format!(
                "failed to start ImageMagick for {filename}: {error}"
            ))
        })?;
    let mut stdin = process
        .stdin
        .take()
        .ok_or_else(|| ServiceError::Internal("ImageMagick stdin was not available".to_string()))?;
    stdin.write_all(image).await.map_err(|error| {
        ServiceError::Internal(format!("failed to send image to ImageMagick: {error}"))
    })?;
    drop(stdin);

    let output = process.wait_with_output().await.map_err(|error| {
        ServiceError::Internal(format!("ImageMagick failed to finish: {error}"))
    })?;
    if !output.status.success() {
        return Err(ServiceError::BadRequest(format!(
            "ImageMagick could not decode the image: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    if output.stdout.is_empty() {
        return Err(ServiceError::BadRequest(
            "ImageMagick returned an empty normalized image".to_string(),
        ));
    }

    Ok((output.stdout, "normalized.jpg".to_string()))
}

#[cfg(test)]
mod tests {
    use super::{BaiduProvider, OcrProvider, ServiceManager, ServiceType};
    use crate::config::{ProviderKind, ServiceConfig};
    use std::sync::Arc;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn baidu_provider_sends_oauth_credentials_as_query_parameters() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(query_param("grant_type", "client_credentials"))
            .and(query_param("client_id", "test-key"))
            .and(query_param("client_secret", "test-secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "test-token",
                "expires_in": 3600
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/ocr"))
            .and(query_param("access_token", "test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "words_result": [{"words": "recognized text"}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let provider = BaiduProvider::new(&ServiceConfig {
            enabled: true,
            model_type: "ocr".to_string(),
            model_version: "unlimited_ocr".to_string(),
            provider: ProviderKind::Baidu,
            docker_command: vec!["docker".to_string()],
            device: "auto".to_string(),
            base_url: String::new(),
            model: String::new(),
            script_path: String::new(),
            token_url: format!("{}/token", server.uri()),
            ocr_url: format!("{}/ocr", server.uri()),
            api_key: "test-key".to_string(),
            secret_key: "test-secret".to_string(),
            max_image_width: 4096,
            max_image_height: 16384,
            startup_timeout_seconds: 10,
            request_timeout_seconds: 10,
            max_tokens: 100,
            embedding_dimensions: 0,
        })
        .expect("Failed to create Baidu provider");
        let response = provider
            .infer(b"image", "image.jpg")
            .await
            .expect("Baidu OCR should succeed");

        assert_eq!(response.text, "recognized text");
    }

    #[test]
    fn service_manager_starts_without_an_active_runtime() {
        let manager = ServiceManager::new(Arc::new(crate::config::Config {
            general: Default::default(),
            logging: Default::default(),
            service: Vec::new(),
        }));
        assert_eq!(manager.active_name(), "on-demand");
    }

    #[test]
    fn service_type_rejects_unknown_tasks() {
        assert!(ServiceType::from_task("object_detection").is_err());
        assert_eq!(ServiceType::from_task("ocr").unwrap(), ServiceType::Ocr);
    }
}
