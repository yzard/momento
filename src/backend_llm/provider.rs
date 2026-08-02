use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine};
use reqwest::Client;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::info;

use crate::adapters::{normalize_baidu_unlimited_ocr_text, BAIDU_UNLIMITED_OCR_MODEL};
use crate::config::{BaiduConfig, Config, LocalConfig, ProviderKind};
use crate::error::ServiceError;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceResponse {
    pub task: String,
    pub text: String,
    pub markdown: String,
    pub provider: String,
    pub model: String,
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
    pub async fn build(config: &Config) -> Result<Self, ServiceError> {
        match &config.provider {
            ProviderKind::Baidu => Ok(Self::Baidu(BaiduProvider::new(&config.baidu)?)),
            ProviderKind::Local => Ok(Self::Local(LocalProvider::new(&config.local).await?)),
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
}

pub struct BaiduProvider {
    client: Client,
    config: BaiduConfig,
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
    fn new(config: &BaiduConfig) -> Result<Self, ServiceError> {
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
            model: BAIDU_UNLIMITED_OCR_MODEL.to_string(),
        })
    }

    fn name(&self) -> &'static str {
        "baidu"
    }
}

pub struct LocalProvider {
    client: Client,
    config: LocalConfig,
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
    async fn new(config: &LocalConfig) -> Result<Self, ServiceError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.request_timeout_seconds))
            .build()
            .map_err(|error| {
                ServiceError::Internal(format!("failed to build HTTP client: {error}"))
            })?;
        let mut command = Command::new(&config.command);
        command
            .args(&config.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        let child = command.spawn().map_err(|error| {
            ServiceError::Configuration(format!(
                "failed to start local OCR command `{}`: {error}",
                config.command
            ))
        })?;
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
}

#[async_trait]
impl OcrProvider for LocalProvider {
    async fn infer(&self, image: &[u8], filename: &str) -> Result<InferenceResponse, ServiceError> {
        let mime_type = mime_guess::from_path(filename)
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
            model: self.config.model.clone(),
        })
    }

    fn name(&self) -> &'static str {
        "local"
    }
}

#[cfg(test)]
mod tests {
    use super::{BaiduProvider, OcrProvider};
    use crate::config::BaiduConfig;
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

        let provider = BaiduProvider::new(&BaiduConfig {
            token_url: format!("{}/token", server.uri()),
            ocr_url: format!("{}/ocr", server.uri()),
            api_key: "test-key".to_string(),
            secret_key: "test-secret".to_string(),
            request_timeout_seconds: 10,
        })
        .expect("Failed to create Baidu provider");
        let response = provider
            .infer(b"image", "image.jpg")
            .await
            .expect("Baidu OCR should succeed");

        assert_eq!(response.text, "recognized text");
    }
}
