use reqwest::multipart::{Form, Part};
use serde::Deserialize;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::{mpsc, Mutex};
use tracing::{info, warn};

use crate::config::{Config, LlmConfig};
use crate::constants::{image_text_model_name, paths, IMAGE_TAGGING_MODEL_TYPE, OCR_MODEL_TYPE};
use crate::database::{execute_query, fetch_all, queries, DbPool};
use crate::processor::regenerator::{record_image_text_job_completed, record_regeneration_error};

#[derive(Debug, Clone)]
pub struct LlmClient {
    client: reqwest::Client,
    config: LlmConfig,
}

#[derive(Debug, Deserialize)]
struct InferenceResponse {
    text: String,
    markdown: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(rename = "modelType")]
    model_type: String,
    #[serde(rename = "modelVersion")]
    model_version: String,
}

#[derive(Debug, thiserror::Error)]
pub enum LlmClientError {
    #[error("failed to build LLM client: {0}")]
    Client(String),
    #[error("failed to read image: {0}")]
    ReadImage(String),
    #[error("failed to convert image: {0}")]
    ConvertImage(String),
    #[error("LLM service request failed: {0}")]
    Request(String),
    #[error("LLM service returned an invalid response: {0}")]
    Response(String),
    #[error("failed to update LLM metadata: {0}")]
    Database(String),
}

impl LlmClient {
    pub fn new(config: &LlmConfig) -> Result<Self, LlmClientError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds))
            .build()
            .map_err(|error| LlmClientError::Client(error.to_string()))?;
        Ok(Self {
            client,
            config: config.clone(),
        })
    }

    pub async fn ocr_and_store(
        &self,
        pool: &DbPool,
        media_id: i64,
        image_path: &Path,
    ) -> Result<bool, LlmClientError> {
        self.infer_and_store(pool, media_id, image_path, "ocr", "/v1/infer")
            .await
    }

    pub async fn image_tagging_and_store(
        &self,
        pool: &DbPool,
        media_id: i64,
        image_path: &Path,
    ) -> Result<bool, LlmClientError> {
        self.infer_and_store(
            pool,
            media_id,
            image_path,
            "image_tagging",
            &self.config.image_tagging_endpoint,
        )
        .await
    }

    pub async fn wait_until_ready(&self) -> Result<(), LlmClientError> {
        let url = format!("{}/ready", self.config.service_url.trim_end_matches('/'));
        let started = std::time::Instant::now();
        loop {
            match self
                .client
                .get(&url)
                .timeout(Duration::from_secs(
                    self.config.ready_connection_timeout_seconds.max(1),
                ))
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => {
                    return Ok(());
                }
                Ok(response) => {
                    warn!(
                        "LLM service readiness check returned {} for {}",
                        response.status(),
                        url
                    );
                }
                Err(error) => {
                    warn!("LLM service readiness check failed for {}: {}", url, error);
                }
            }
            if started.elapsed() >= Duration::from_secs(self.config.startup_timeout_seconds) {
                return Err(LlmClientError::Request(format!(
                    "LLM service did not become ready within {} seconds",
                    self.config.startup_timeout_seconds
                )));
            }
            tokio::time::sleep(Duration::from_secs(
                self.config.ready_poll_interval_seconds.max(1),
            ))
            .await;
        }
    }

    async fn infer_and_store(
        &self,
        pool: &DbPool,
        media_id: i64,
        image_path: &Path,
        task: &str,
        endpoint: &str,
    ) -> Result<bool, LlmClientError> {
        if !self.config.enabled {
            return Ok(false);
        }

        let source_image = tokio::fs::read(image_path)
            .await
            .map_err(|error| LlmClientError::ReadImage(error.to_string()))?;
        let source_filename = image_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("image.jpg")
            .to_string();
        let (image, filename) = prepare_image(source_image, source_filename).await?;
        let part = Part::bytes(image)
            .file_name(filename.clone())
            .mime_str(
                mime_guess::from_path(&filename)
                    .first_raw()
                    .unwrap_or("image/jpeg"),
            )
            .map_err(|error| LlmClientError::Client(error.to_string()))?;
        let form = Form::new()
            .text("task", task.to_string())
            .part("file", part);
        let url = if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
            endpoint.to_string()
        } else {
            format!(
                "{}/{}",
                self.config.service_url.trim_end_matches('/'),
                endpoint.trim_start_matches('/')
            )
        };
        let mut request = self.client.post(url).multipart(form);
        if !self.config.api_key.is_empty() {
            request = request.header("x-api-key", &self.config.api_key);
        }

        let response = request
            .send()
            .await
            .map_err(|error| LlmClientError::Request(error.to_string()))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| LlmClientError::Request(error.to_string()))?;
        if !status.is_success() {
            return Err(LlmClientError::Request(format!(
                "service returned {status}: {body}"
            )));
        }

        let result: InferenceResponse = serde_json::from_str(&body)
            .map_err(|error| LlmClientError::Response(error.to_string()))?;
        let text = if !result.text.trim().is_empty() {
            result.text.trim().to_string()
        } else if !result.tags.is_empty() {
            result.tags.join("\n")
        } else {
            result.markdown.trim().to_string()
        };
        let model_type = result.model_type.clone();
        let model_version = result.model_version.clone();
        let stored_model_type = model_type.clone();
        let pool = pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool
                .get()
                .map_err(|error| LlmClientError::Database(error.to_string()))?;
            execute_query(
                &conn,
                queries::image_text::DELETE_BY_IMAGE_ID_AND_MODEL_TYPE,
                &[&media_id, &stored_model_type],
            )
            .map_err(|error| LlmClientError::Database(error.to_string()))?;
            execute_query(
                &conn,
                queries::image_text::INSERT,
                &[&media_id, &stored_model_type, &model_version, &text],
            )
            .map_err(|error| LlmClientError::Database(error.to_string()))?;
            Ok::<(), LlmClientError>(())
        })
        .await
        .map_err(|error| LlmClientError::Database(error.to_string()))??;

        info!(
            "stored {} text for media {}",
            image_text_model_name(&model_type).unwrap_or("LLM"),
            media_id
        );
        Ok(true)
    }
}

async fn prepare_image(
    image: Vec<u8>,
    filename: String,
) -> Result<(Vec<u8>, String), LlmClientError> {
    if !is_heic_filename(&filename) {
        return Ok((image, filename));
    }

    let mut process = Command::new("magick")
        .args(["-", "jpg:-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            LlmClientError::ConvertImage(format!("failed to start ImageMagick: {error}"))
        })?;
    let mut stdin = process.stdin.take().ok_or_else(|| {
        LlmClientError::ConvertImage("ImageMagick stdin was not available".to_string())
    })?;
    stdin
        .write_all(&image)
        .await
        .map_err(|error| LlmClientError::ConvertImage(error.to_string()))?;
    drop(stdin);

    let output = process
        .wait_with_output()
        .await
        .map_err(|error| LlmClientError::ConvertImage(error.to_string()))?;
    if !output.status.success() {
        return Err(LlmClientError::ConvertImage(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    if output.stdout.is_empty() {
        return Err(LlmClientError::ConvertImage(
            "ImageMagick returned an empty JPEG".to_string(),
        ));
    }

    Ok((output.stdout, format!("{filename}.jpg")))
}

fn is_heic_filename(filename: &str) -> bool {
    matches!(
        filename
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase())
            .as_deref(),
        Some("heic" | "heif")
    )
}

pub async fn generate_missing_ocr(config: &Config, pool: &DbPool) {
    generate_missing_model(config, pool, OCR_MODEL_TYPE, "ocr", "/v1/infer", "OCR").await;
}

pub async fn generate_missing_image_tagging(config: &Config, pool: &DbPool) {
    if !config.llm.image_tagging_enabled {
        return;
    }
    let endpoint = config.llm.image_tagging_endpoint.clone();
    generate_missing_model(
        config,
        pool,
        IMAGE_TAGGING_MODEL_TYPE,
        "image_tagging",
        &endpoint,
        "image tagging",
    )
    .await;
}

async fn generate_missing_model(
    config: &Config,
    pool: &DbPool,
    model_type: &str,
    task: &str,
    endpoint: &str,
    plugin_name: &str,
) {
    if !config.llm.enabled {
        warn!(
            "{} generation skipped because llm.enabled is false",
            plugin_name
        );
        return;
    }

    let rows = match pool.get() {
        Ok(conn) => fetch_all(
            &conn,
            queries::image_text::SELECT_MISSING_FOR_MODEL_TYPE,
            &[&model_type],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        ),
        Err(error) => {
            warn!(
                "failed to get database connection for {} backfill: {}",
                plugin_name, error
            );
            return;
        }
    };
    let rows = match rows {
        Ok(rows) => rows,
        Err(error) => {
            warn!("failed to load images missing {}: {}", plugin_name, error);
            return;
        }
    };
    if rows.is_empty() {
        return;
    }

    let client = match LlmClient::new(&config.llm) {
        Ok(client) => Arc::new(client),
        Err(error) => {
            warn!("failed to initialize {} client: {}", plugin_name, error);
            return;
        }
    };
    if let Err(error) = client.wait_until_ready().await {
        record_regeneration_error(&format!(
            "{} generation could not start: {}",
            plugin_name, error
        ));
        warn!("{} service readiness failed: {}", plugin_name, error);
        return;
    }
    let concurrency = config.llm.max_concurrent_requests.max(1);
    let (sender, receiver) = mpsc::channel(rows.len());
    for row in rows {
        sender
            .send(row)
            .await
            .expect("LLM worker queue should accept jobs");
    }
    drop(sender);

    let receiver = Arc::new(Mutex::new(receiver));
    let endpoint = endpoint.to_string();
    let task = task.to_string();
    let plugin_name = plugin_name.to_string();
    let mut workers = Vec::with_capacity(concurrency);
    for _ in 0..concurrency {
        let client = Arc::clone(&client);
        let pool = pool.clone();
        let endpoint = endpoint.clone();
        let task = task.clone();
        let plugin_name = plugin_name.clone();
        let receiver = Arc::clone(&receiver);
        workers.push(tokio::spawn(async move {
            loop {
                let job = receiver.lock().await.recv().await;
                let Some((media_id, file_path)) = job else {
                    break;
                };
                let path = paths().originals.join(file_path);
                match client
                    .infer_and_store(&pool, media_id, &path, &task, &endpoint)
                    .await
                {
                    Ok(true) => {
                        record_image_text_job_completed();
                    }
                    Ok(false) => {
                        record_image_text_job_completed();
                    }
                    Err(error) => {
                        record_image_text_job_completed();
                        record_regeneration_error(&format!(
                            "{} generation failed for media {}: {}",
                            plugin_name, media_id, error
                        ));
                        warn!(
                            "{} backfill failed for media {}: {}",
                            plugin_name, media_id, error
                        );
                    }
                }
            }
        }));
    }
    for worker in workers {
        if let Err(error) = worker.await {
            record_regeneration_error(&format!("{} LLM worker failed: {}", plugin_name, error));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_heic_filename;

    #[test]
    fn recognizes_heic_extensions_for_in_memory_conversion() {
        assert!(is_heic_filename("photo.HEIC"));
        assert!(is_heic_filename("photo.heif"));
        assert!(!is_heic_filename("photo.jpg"));
    }
}
