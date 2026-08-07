use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use reqwest::multipart::{Form, Part};
use serde::Deserialize;
use std::collections::VecDeque;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot, Mutex, OwnedMutexGuard};
use tracing::{info, warn};

use crate::config::{Config, LlmConfig};
use crate::constants::{media_text_model_name, paths, IMAGE_TAGGING_MODEL_TYPE, OCR_MODEL_TYPE};
use crate::database::{fetch_all, queries, DbPool};
use crate::processor::regenerator::{record_media_text_job_completed, record_regeneration_error};

const INFERENCE_ENDPOINT: &str = "/v1/infer";

#[derive(Debug, Clone)]
pub struct LlmClient {
    client: reqwest::Client,
    config: LlmConfig,
}

#[derive(Debug, Deserialize)]
struct InferenceResponse {
    task: String,
    text: String,
    markdown: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(rename = "modelType")]
    model_type: String,
    #[serde(rename = "modelVersion")]
    model_version: String,
    #[serde(default)]
    embedding: Option<String>,
    #[serde(default)]
    #[serde(rename = "embeddingEncoding")]
    embedding_encoding: Option<String>,
    #[serde(default)]
    #[serde(rename = "embeddingDimensions")]
    embedding_dimensions: Option<usize>,
    #[serde(default)]
    #[serde(rename = "perceptualHash")]
    perceptual_hash: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ImageClusteringResult {
    pub embedding: Vec<f32>,
    pub perceptual_hash: u64,
    pub model_version: String,
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
    #[error("LLM service rejected an unreadable image: {0}")]
    InvalidImage(String),
    #[error("LLM service returned an invalid response: {0}")]
    Response(String),
    #[error("failed to update LLM metadata: {0}")]
    Database(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InferenceTask {
    Ocr,
    ImageTagging,
    ImageClustering,
}

impl InferenceTask {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ocr => "ocr",
            Self::ImageTagging => "image_tagging",
            Self::ImageClustering => "image_clustering",
        }
    }
}

enum InferenceOutput {
    Stored(bool),
    ImageClustering(ImageClusteringResult),
}

type InferenceJob = Pin<Box<dyn Future<Output = Result<InferenceOutput, LlmClientError>> + Send>>;

struct QueuedInference {
    task: InferenceTask,
    operation: InferenceJob,
    response: oneshot::Sender<Result<InferenceOutput, LlmClientError>>,
}

#[derive(Clone)]
struct LlmScheduler {
    sender: mpsc::Sender<QueuedInference>,
}

impl LlmScheduler {
    fn new() -> Arc<Self> {
        let (sender, receiver) = mpsc::channel(64);
        let scheduler = Arc::new(Self { sender });
        std::thread::Builder::new()
            .name("llm-scheduler".to_string())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("LLM scheduler runtime should build");
                runtime.block_on(run_inference_batches(receiver));
            })
            .expect("LLM scheduler thread should start");
        scheduler
    }

    async fn submit(
        &self,
        task: InferenceTask,
        operation: InferenceJob,
    ) -> Result<InferenceOutput, LlmClientError> {
        let (response, result) = oneshot::channel();
        self.sender
            .send(QueuedInference {
                task,
                operation,
                response,
            })
            .await
            .map_err(|_| LlmClientError::Request("LLM scheduler stopped".to_string()))?;
        result
            .await
            .map_err(|_| LlmClientError::Request("LLM scheduler dropped the result".to_string()))?
    }
}

async fn run_inference_batches(mut receiver: mpsc::Receiver<QueuedInference>) {
    let Some(mut current) = receiver.recv().await else {
        return;
    };
    let mut pending = VecDeque::new();

    loop {
        let current_task = current.task;
        let result = current.operation.await;
        let _ = current.response.send(result);

        while let Ok(job) = receiver.try_recv() {
            pending.push_back(job);
        }
        if let Some(index) = pending.iter().position(|job| job.task == current_task) {
            current = pending
                .remove(index)
                .expect("queued job index should remain valid");
            continue;
        }
        if let Some(job) = pending.pop_front() {
            current = job;
            continue;
        }
        let Some(job) = receiver.recv().await else {
            return;
        };
        current = job;
    }
}

fn llm_scheduler() -> &'static Arc<LlmScheduler> {
    static SCHEDULER: std::sync::OnceLock<Arc<LlmScheduler>> = std::sync::OnceLock::new();
    SCHEDULER.get_or_init(LlmScheduler::new)
}

fn inference_batch_lock() -> &'static Arc<Mutex<()>> {
    static BATCH_LOCK: std::sync::OnceLock<Arc<Mutex<()>>> = std::sync::OnceLock::new();
    BATCH_LOCK.get_or_init(|| Arc::new(Mutex::new(())))
}

pub async fn begin_inference_batch() -> OwnedMutexGuard<()> {
    Arc::clone(inference_batch_lock()).lock_owned().await
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
        self.submit_inference(pool, media_id, image_path, InferenceTask::Ocr)
            .await
    }

    pub async fn image_tagging_and_store(
        &self,
        pool: &DbPool,
        media_id: i64,
        image_path: &Path,
    ) -> Result<bool, LlmClientError> {
        self.submit_inference(pool, media_id, image_path, InferenceTask::ImageTagging)
            .await
    }

    async fn submit_inference(
        &self,
        pool: &DbPool,
        media_id: i64,
        image_path: &Path,
        task: InferenceTask,
    ) -> Result<bool, LlmClientError> {
        let _batch_guard = begin_inference_batch().await;
        let client = self.clone();
        let pool = pool.clone();
        let image_path = image_path.to_path_buf();
        let output = llm_scheduler()
            .submit(
                task,
                Box::pin(async move {
                    client
                        .infer_and_store_direct(&pool, media_id, &image_path, task)
                        .await
                        .map(InferenceOutput::Stored)
                }),
            )
            .await?;
        match output {
            InferenceOutput::Stored(stored) => Ok(stored),
            InferenceOutput::ImageClustering(_) => Err(LlmClientError::Response(
                "LLM scheduler returned an embedding for a storage request".to_string(),
            )),
        }
    }

    pub async fn image_clustering(
        &self,
        image_path: &Path,
    ) -> Result<ImageClusteringResult, LlmClientError> {
        let _batch_guard = begin_inference_batch().await;
        self.image_clustering_in_batch(image_path).await
    }

    pub async fn image_clustering_in_batch(
        &self,
        image_path: &Path,
    ) -> Result<ImageClusteringResult, LlmClientError> {
        let client = self.clone();
        let image_path = image_path.to_path_buf();
        let output = llm_scheduler()
            .submit(
                InferenceTask::ImageClustering,
                Box::pin(async move {
                    let response = client
                        .infer_direct(&image_path, InferenceTask::ImageClustering)
                        .await?;
                    decode_image_clustering(response).map(InferenceOutput::ImageClustering)
                }),
            )
            .await?;
        match output {
            InferenceOutput::ImageClustering(result) => Ok(result),
            InferenceOutput::Stored(_) => Err(LlmClientError::Response(
                "LLM scheduler returned a storage result for image clustering".to_string(),
            )),
        }
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

    async fn infer_and_store_direct(
        &self,
        pool: &DbPool,
        media_id: i64,
        image_path: &Path,
        task: InferenceTask,
    ) -> Result<bool, LlmClientError> {
        if !self.config.enabled {
            return Ok(false);
        }
        let result = self.infer_direct(image_path, task).await?;
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
            let transaction = conn
                .unchecked_transaction()
                .map_err(|error| LlmClientError::Database(error.to_string()))?;
            transaction
                .execute(
                    queries::media_text::DELETE_BY_MEDIA_ID_AND_MODEL_TYPE,
                    rusqlite::params![media_id, stored_model_type],
                )
                .map_err(|error| LlmClientError::Database(error.to_string()))?;
            transaction
                .execute(
                    queries::media_text::INSERT,
                    rusqlite::params![media_id, stored_model_type, model_version, text],
                )
                .map_err(|error| LlmClientError::Database(error.to_string()))?;
            transaction
                .commit()
                .map_err(|error| LlmClientError::Database(error.to_string()))?;
            Ok::<(), LlmClientError>(())
        })
        .await
        .map_err(|error| LlmClientError::Database(error.to_string()))??;

        info!(
            "stored {} text for media {}",
            media_text_model_name(&model_type).unwrap_or("LLM"),
            media_id
        );
        Ok(true)
    }

    async fn infer_direct(
        &self,
        image_path: &Path,
        task: InferenceTask,
    ) -> Result<InferenceResponse, LlmClientError> {
        if !self.config.enabled {
            return Err(LlmClientError::Request(
                "LLM service is disabled".to_string(),
            ));
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
        let form = Form::new().text("task", task.as_str()).part("file", part);
        let endpoint = INFERENCE_ENDPOINT;
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
            if status == reqwest::StatusCode::BAD_REQUEST
                && (body.contains("could not decode image")
                    || body.contains("cannot identify image")
                    || body.contains("image must not be empty"))
            {
                return Err(LlmClientError::InvalidImage(body));
            }
            return Err(LlmClientError::Request(format!(
                "service returned {status}: {body}"
            )));
        }

        let response: InferenceResponse = serde_json::from_str(&body)
            .map_err(|error| LlmClientError::Response(error.to_string()))?;
        if response.task != task.as_str() || response.model_type != task.as_str() {
            return Err(LlmClientError::Response(format!(
                "service returned task `{}` and modelType `{}` for `{}` request",
                response.task,
                response.model_type,
                task.as_str()
            )));
        }
        Ok(response)
    }
}

fn decode_image_clustering(
    response: InferenceResponse,
) -> Result<ImageClusteringResult, LlmClientError> {
    if response.task != "image_clustering" || response.model_type != "image_clustering" {
        return Err(LlmClientError::Response(
            "image clustering response task and modelType must be image_clustering".to_string(),
        ));
    }
    let model_version = response.model_version.clone();
    if response.embedding_encoding.as_deref() != Some("float32_le") {
        return Err(LlmClientError::Response(
            "image clustering embedding encoding must be float32_le".to_string(),
        ));
    }
    let encoded = response.embedding.as_deref().ok_or_else(|| {
        LlmClientError::Response("image clustering response did not contain embedding".to_string())
    })?;
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|error| LlmClientError::Response(format!("invalid embedding base64: {error}")))?;
    if bytes.is_empty() || bytes.len() % 4 != 0 {
        return Err(LlmClientError::Response(
            "embedding must contain little-endian float32 values".to_string(),
        ));
    }
    let embedding = bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect::<Vec<_>>();
    if embedding.iter().any(|component| !component.is_finite()) {
        return Err(LlmClientError::Response(
            "embedding contains a non-finite float32 value".to_string(),
        ));
    }
    if response.embedding_dimensions != Some(embedding.len()) {
        return Err(LlmClientError::Response(
            "image clustering embedding dimensions do not match the payload".to_string(),
        ));
    }
    if embedding.len() != 384 {
        return Err(LlmClientError::Response(
            "DINOv2-small embedding must contain 384 dimensions".to_string(),
        ));
    }
    let norm = embedding
        .iter()
        .map(|component| component * component)
        .sum::<f32>()
        .sqrt();
    if (norm - 1.0).abs() > 0.02 {
        return Err(LlmClientError::Response(
            "image clustering embedding must be L2 normalized".to_string(),
        ));
    }
    let perceptual_hash_string = response.perceptual_hash.as_deref().ok_or_else(|| {
        LlmClientError::Response(
            "image clustering response did not contain perceptualHash".to_string(),
        )
    })?;
    if perceptual_hash_string.len() != 16 {
        return Err(LlmClientError::Response(
            "perceptualHash must contain exactly 16 hexadecimal characters".to_string(),
        ));
    }
    let perceptual_hash = u64::from_str_radix(perceptual_hash_string, 16)
        .map_err(|error| LlmClientError::Response(format!("invalid perceptualHash: {error}")))?;
    Ok(ImageClusteringResult {
        embedding,
        perceptual_hash,
        model_version,
    })
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

pub async fn generate_missing_batches(config: &Config, pool: &DbPool) {
    // Complete one service-type batch before starting the next one. The LLM
    // service owns only one model runtime, so interleaving these batches would
    // force an unnecessary shutdown/startup cycle for every request.
    generate_missing_model(config, pool, OCR_MODEL_TYPE, InferenceTask::Ocr, "OCR").await;

    if crate::processor::regenerator::is_cancel_requested() {
        return;
    }
    if !config.llm.image_tagging_enabled {
        return;
    }
    generate_missing_model(
        config,
        pool,
        IMAGE_TAGGING_MODEL_TYPE,
        InferenceTask::ImageTagging,
        "image tagging",
    )
    .await;
}

async fn generate_missing_model(
    config: &Config,
    pool: &DbPool,
    model_type: &str,
    task: InferenceTask,
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
            queries::media_text::SELECT_MISSING_FOR_MODEL_TYPE,
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
    let plugin_name = plugin_name.to_string();
    let mut workers = Vec::with_capacity(concurrency);
    for _ in 0..concurrency {
        let client = Arc::clone(&client);
        let pool = pool.clone();
        let plugin_name = plugin_name.clone();
        let receiver = Arc::clone(&receiver);
        workers.push(tokio::spawn(async move {
            loop {
                let job = receiver.lock().await.recv().await;
                let Some((media_id, file_path)) = job else {
                    break;
                };
                let path = paths().originals.join(file_path);
                match client.submit_inference(&pool, media_id, &path, task).await {
                    Ok(true) => {
                        record_media_text_job_completed();
                    }
                    Ok(false) => {
                        record_media_text_job_completed();
                    }
                    Err(error) => {
                        record_media_text_job_completed();
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
    use super::{is_heic_filename, InferenceOutput, InferenceTask, LlmScheduler};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::Notify;

    #[test]
    fn recognizes_heic_extensions_for_in_memory_conversion() {
        assert!(is_heic_filename("photo.HEIC"));
        assert!(is_heic_filename("photo.heif"));
        assert!(!is_heic_filename("photo.jpg"));
    }

    #[tokio::test]
    async fn scheduler_finishes_queued_same_type_before_switching() {
        let scheduler = LlmScheduler::new();
        let order = Arc::new(Mutex::new(Vec::new()));
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());

        let first_order = Arc::clone(&order);
        let first_started = Arc::clone(&started);
        let first_release = Arc::clone(&release);
        let first_scheduler = scheduler.clone();
        let first = tokio::spawn(async move {
            first_scheduler
                .submit(
                    InferenceTask::Ocr,
                    Box::pin(async move {
                        first_order.lock().unwrap().push(InferenceTask::Ocr);
                        first_started.notify_one();
                        first_release.notified().await;
                        Ok(InferenceOutput::Stored(true))
                    }),
                )
                .await
        });
        started.notified().await;

        let second_order = Arc::clone(&order);
        let second_scheduler = scheduler.clone();
        let second = tokio::spawn(async move {
            second_scheduler
                .submit(
                    InferenceTask::ImageTagging,
                    Box::pin(async move {
                        second_order
                            .lock()
                            .unwrap()
                            .push(InferenceTask::ImageTagging);
                        Ok(InferenceOutput::Stored(true))
                    }),
                )
                .await
        });
        let third_order = Arc::clone(&order);
        let third_scheduler = scheduler.clone();
        let third = tokio::spawn(async move {
            third_scheduler
                .submit(
                    InferenceTask::Ocr,
                    Box::pin(async move {
                        third_order.lock().unwrap().push(InferenceTask::Ocr);
                        Ok(InferenceOutput::Stored(true))
                    }),
                )
                .await
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        release.notify_one();

        assert!(matches!(
            first.await.unwrap().unwrap(),
            InferenceOutput::Stored(true)
        ));
        assert!(matches!(
            second.await.unwrap().unwrap(),
            InferenceOutput::Stored(true)
        ));
        assert!(matches!(
            third.await.unwrap().unwrap(),
            InferenceOutput::Stored(true)
        ));
        assert_eq!(
            *order.lock().unwrap(),
            vec![
                InferenceTask::Ocr,
                InferenceTask::Ocr,
                InferenceTask::ImageTagging
            ]
        );
    }
}
