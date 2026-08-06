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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceType {
    Ocr,
    ImageTagging,
}

impl ServiceType {
    fn from_task(task: &str) -> Result<Self, ServiceError> {
        match task {
            "ocr" => Ok(Self::Ocr),
            "image_tagging" => Ok(Self::ImageTagging),
            _ => Err(ServiceError::NotImplemented(format!(
                "inference task `{task}` has no configured model provider"
            ))),
        }
    }

    fn config_key(self) -> &'static str {
        match self {
            Self::Ocr => "ocr",
            Self::ImageTagging => "image_tagging",
        }
    }
}

enum ActiveService {
    Ocr(Provider),
    ImageTagging(RamProvider),
}

impl ActiveService {
    fn service_type(&self) -> ServiceType {
        match self {
            Self::Ocr(_) => ServiceType::Ocr,
            Self::ImageTagging(_) => ServiceType::ImageTagging,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Ocr(provider) => provider.name(),
            Self::ImageTagging(_) => "ram++",
        }
    }

    async fn shutdown(self) -> Result<(), ServiceError> {
        match self {
            Self::Ocr(provider) => provider.shutdown().await,
            Self::ImageTagging(provider) => provider.shutdown().await,
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
            None => Err(ServiceError::Internal(
                "LLM service was not activated".to_string(),
            )),
        }
    }

    async fn activate(&mut self, service_type: ServiceType) -> Result<(), ServiceError> {
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.service_type() == service_type)
        {
            return Ok(());
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
        };
        self.active = Some(active);
        Ok(())
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
        provider.wait_until_ready().await?;
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
        let mut child = self.child.lock().await;
        if child
            .try_wait()
            .map_err(|error| {
                ServiceError::Internal(format!("failed to inspect OCR runtime: {error}"))
            })?
            .is_some()
        {
            return Ok(());
        }
        child.start_kill().map_err(|error| {
            ServiceError::Internal(format!("failed to stop OCR runtime: {error}"))
        })?;
        child.wait().await.map_err(|error| {
            ServiceError::Internal(format!("failed to wait for OCR runtime: {error}"))
        })?;
        Ok(())
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
        })
    }

    fn name(&self) -> &'static str {
        "local"
    }
}

pub struct RamProvider {
    client: Client,
    config: ServiceConfig,
    child: Arc<Mutex<Child>>,
}

impl Drop for RamProvider {
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

impl RamProvider {
    pub async fn new(config: &ServiceConfig) -> Result<Self, ServiceError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.request_timeout_seconds))
            .build()
            .map_err(|error| {
                ServiceError::Internal(format!("failed to build RAM++ HTTP client: {error}"))
            })?;
        let child = spawn_service_command(config)?;
        let provider = Self {
            client,
            config: config.clone(),
            child: Arc::new(Mutex::new(child)),
        };
        provider.wait_until_ready().await?;
        Ok(provider)
    }

    async fn wait_until_ready(&self) -> Result<(), ServiceError> {
        let started = Instant::now();
        let url = format!("{}/ready", self.config.base_url.trim_end_matches('/'));
        loop {
            if let Ok(response) = self.client.get(&url).send().await {
                if response.status().is_success() {
                    info!("RAM++ runtime is ready at {}", self.config.base_url);
                    return Ok(());
                }
            }
            if let Ok(Some(status)) = self.child.lock().await.try_wait() {
                return Err(ServiceError::Internal(format!(
                    "RAM++ runtime exited during startup with {status}"
                )));
            }
            if started.elapsed() >= Duration::from_secs(self.config.startup_timeout_seconds) {
                return Err(ServiceError::Internal(format!(
                    "RAM++ runtime did not become ready within {} seconds",
                    self.config.startup_timeout_seconds
                )));
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    async fn shutdown(self) -> Result<(), ServiceError> {
        let mut child = self.child.lock().await;
        if child
            .try_wait()
            .map_err(|error| {
                ServiceError::Internal(format!("failed to inspect image tagging runtime: {error}"))
            })?
            .is_some()
        {
            return Ok(());
        }
        child.start_kill().map_err(|error| {
            ServiceError::Internal(format!("failed to stop image tagging runtime: {error}"))
        })?;
        child.wait().await.map_err(|error| {
            ServiceError::Internal(format!("failed to wait for image tagging runtime: {error}"))
        })?;
        Ok(())
    }

    pub async fn infer(&self, image: &[u8]) -> Result<InferenceResponse, ServiceError> {
        let encoded = STANDARD.encode(image);
        let url = format!("{}/infer", self.config.base_url.trim_end_matches('/'));
        let response = self
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
            model_version: self.config.model_version.clone(),
            tags: result.tags,
        })
    }
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
