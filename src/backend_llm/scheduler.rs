use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use tracing::warn;

use crate::config::{CallbackConfig, SchedulerConfig};
use crate::error::ServiceError;
use crate::provider::{
    InferenceInput, InferenceJob, InputInferenceResponse, ServiceManager, ServiceType,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueManifest {
    pub job_id: String,
    pub media_id: i64,
    pub task: String,
    pub attempt: u32,
    pub inputs: Vec<QueueInputDescriptor>,
    pub callback_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueueInputDescriptor {
    pub sequence: u32,
    pub filename: String,
    pub mime_type: String,
    pub byte_size: u64,
    pub content_hash: String,
    pub input_kind: String,
    pub frame_timestamp_ms: Option<i64>,
}

pub struct Scheduler {
    queue_dir: PathBuf,
    configuration: SchedulerConfig,
    callback: CallbackConfig,
    manager: Arc<Mutex<ServiceManager>>,
    client: reqwest::Client,
}

impl Scheduler {
    pub fn new(
        queue_dir: PathBuf,
        configuration: SchedulerConfig,
        callback: CallbackConfig,
        manager: Arc<Mutex<ServiceManager>>,
    ) -> Result<Self, ServiceError> {
        std::fs::create_dir_all(queue_dir.join("queuing")).map_err(io_error)?;
        std::fs::create_dir_all(queue_dir.join("processing")).map_err(io_error)?;
        std::fs::create_dir_all(queue_dir.join("callback_pending")).map_err(io_error)?;
        std::fs::create_dir_all(queue_dir.join("failed")).map_err(io_error)?;
        std::fs::create_dir_all(queue_dir.join(".tmp")).map_err(io_error)?;
        recover_queue(&queue_dir)?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(
                callback.request_timeout_seconds,
            ))
            .build()
            .map_err(|error| ServiceError::Internal(error.to_string()))?;
        Ok(Self {
            queue_dir,
            configuration,
            callback,
            manager,
            client,
        })
    }

    pub fn begin_admission(&self, manifest: QueueManifest) -> Result<QueueAdmission, ServiceError> {
        ServiceType::from_task(&manifest.task)?;
        if manifest.job_id.is_empty()
            || !manifest
                .job_id
                .bytes()
                .all(|character| character.is_ascii_hexdigit())
        {
            return Err(ServiceError::BadRequest(
                "jobId must be a non-empty hexadecimal identifier".to_string(),
            ));
        }
        if manifest.inputs.is_empty() || manifest.callback_url.is_empty() {
            return Err(ServiceError::BadRequest(
                "at least one input and callbackUrl are required".to_string(),
            ));
        }
        if self.job_exists(&manifest.job_id) {
            return Ok(QueueAdmission::Duplicate);
        }
        let temporary = self.queue_dir.join(".tmp").join(&manifest.job_id);
        let queuing = self.queue_dir.join("queuing").join(&manifest.job_id);
        if temporary.exists() {
            return Err(ServiceError::Conflict(format!(
                "temporary job already exists: {}",
                manifest.job_id
            )));
        }
        std::fs::create_dir_all(&temporary).map_err(io_error)?;
        std::fs::write(
            temporary.join("manifest.json"),
            serde_json::to_vec(&manifest)
                .map_err(|error| ServiceError::BadRequest(error.to_string()))?,
        )
        .map_err(io_error)?;
        Ok(QueueAdmission::Staging(QueueStaging {
            manifest,
            temporary,
            queuing,
        }))
    }

    pub fn accept(
        &self,
        manifest: QueueManifest,
        inputs: Vec<(QueueInputDescriptor, Vec<u8>)>,
    ) -> Result<(), ServiceError> {
        let QueueAdmission::Staging(mut staging) = self.begin_admission(manifest)? else {
            return Ok(());
        };
        for (descriptor, bytes) in inputs {
            staging.write_input(&descriptor, &bytes)?;
        }
        staging.commit()
    }
}

pub enum QueueAdmission {
    Duplicate,
    Staging(QueueStaging),
}

pub struct QueueStaging {
    manifest: QueueManifest,
    temporary: PathBuf,
    queuing: PathBuf,
}

impl QueueStaging {
    pub fn write_input(
        &mut self,
        descriptor: &QueueInputDescriptor,
        bytes: &[u8],
    ) -> Result<(), ServiceError> {
        let input_path = self.input_path(descriptor)?;
        let mut input_file = std::fs::File::create(input_path).map_err(io_error)?;
        std::io::Write::write_all(&mut input_file, bytes).map_err(io_error)?;
        input_file.sync_all().map_err(io_error)?;
        self.verify_input(descriptor)
    }

    pub fn input_path(&self, descriptor: &QueueInputDescriptor) -> Result<PathBuf, ServiceError> {
        self.validate_descriptor(descriptor)?;
        Ok(self
            .temporary
            .join(format!("input-{}", descriptor.sequence)))
    }

    pub fn verify_input(&self, descriptor: &QueueInputDescriptor) -> Result<(), ServiceError> {
        self.validate_descriptor(descriptor)?;
        let input_path = self
            .temporary
            .join(format!("input-{}", descriptor.sequence));
        let bytes = std::fs::read(input_path).map_err(io_error)?;
        if bytes.is_empty()
            || bytes.len() as u64 != descriptor.byte_size
            || format!("{:x}", Sha256::digest(bytes)) != descriptor.content_hash
        {
            return Err(ServiceError::BadRequest(
                "input bytes do not match descriptor".to_string(),
            ));
        }
        Ok(())
    }

    pub fn commit(self) -> Result<(), ServiceError> {
        for descriptor in &self.manifest.inputs {
            self.verify_input(descriptor)?;
        }
        let manifest_path = self.temporary.join("manifest.json");
        std::fs::File::open(&manifest_path)
            .map_err(io_error)?
            .sync_all()
            .map_err(io_error)?;
        std::fs::File::open(&self.temporary)
            .map_err(io_error)?
            .sync_all()
            .map_err(io_error)?;
        let queue_path = self.queuing.clone();
        std::fs::rename(self.temporary, queue_path).map_err(io_error)?;
        let queue_directory = self
            .queuing
            .parent()
            .ok_or_else(|| ServiceError::Internal("queue job path has no parent".to_string()))?;
        std::fs::File::open(queue_directory)
            .map_err(io_error)?
            .sync_all()
            .map_err(io_error)
    }

    fn validate_descriptor(&self, descriptor: &QueueInputDescriptor) -> Result<(), ServiceError> {
        let expected = self
            .manifest
            .inputs
            .iter()
            .find(|expected| expected.sequence == descriptor.sequence)
            .ok_or_else(|| {
                ServiceError::BadRequest("multipart input has no manifest descriptor".to_string())
            })?;
        if expected != descriptor
            || descriptor.filename.is_empty()
            || !descriptor.mime_type.starts_with("image/")
        {
            return Err(ServiceError::BadRequest(
                "multipart input descriptor does not match manifest".to_string(),
            ));
        }
        Ok(())
    }
}

impl Scheduler {
    pub async fn run(self: Arc<Self>) {
        let interval = Duration::from_secs(self.configuration.poll_interval_seconds);
        let idle_shutdown = Duration::from_secs(self.configuration.idle_shutdown_seconds);
        let mut idle_since = None;
        loop {
            if self.process_cycle().await {
                idle_since = None;
                continue;
            }
            let idle_since = *idle_since.get_or_insert_with(Instant::now);
            if idle_since.elapsed() >= idle_shutdown {
                if let Err(error) = self.manager.lock().await.shutdown().await {
                    warn!("failed to stop idle LLM runtime: {error}");
                }
            }
            tokio::time::sleep(interval).await;
        }
    }

    async fn process_cycle(&self) -> bool {
        self.retry_callbacks().await;
        let Ok(entries) = std::fs::read_dir(self.queue_dir.join("queuing")) else {
            return false;
        };
        let mut queued = entries
            .flatten()
            .filter_map(|entry| {
                self.read_manifest(&entry.path())
                    .ok()
                    .map(|manifest| (entry.path(), manifest))
            })
            .collect::<Vec<_>>();
        queued.sort_by(|left, right| left.1.job_id.cmp(&right.1.job_id));
        let Some((_, first)) = queued.first() else {
            return false;
        };
        let active_task = self.manager.lock().await.active_task();
        let task = select_task(&queued, active_task)
            .unwrap_or(&first.task)
            .to_string();
        let max_concurrent_jobs = match self.manager.lock().await.max_concurrent_jobs(&task) {
            Ok(max_concurrent_jobs) => max_concurrent_jobs,
            Err(_) => return false,
        };
        let mut claimed = Vec::new();
        for (queue_path, manifest) in queued
            .into_iter()
            .filter(|(_, manifest)| manifest.task == task)
            .take(max_concurrent_jobs)
        {
            let processing_path = self.queue_dir.join("processing").join(&manifest.job_id);
            if std::fs::rename(&queue_path, &processing_path).is_err() {
                continue;
            }
            claimed.push((processing_path, manifest));
        }
        let mut jobs = Vec::new();
        let mut ready = Vec::new();
        let processed = !claimed.is_empty();
        for (job_path, manifest) in claimed {
            match self.load_inputs(&job_path, &manifest).await {
                Ok(inputs) => {
                    jobs.push(InferenceJob {
                        task: manifest.task.clone(),
                        inputs,
                    });
                    ready.push((job_path, manifest));
                }
                Err(error) => self.fail(job_path, error),
            }
        }
        let responses = self
            .manager
            .lock()
            .await
            .infer_batch(jobs, max_concurrent_jobs)
            .await;
        for ((job_path, manifest), inference) in ready.into_iter().zip(responses) {
            self.finish_job(job_path, manifest, inference).await;
        }
        processed
    }

    async fn load_inputs(
        &self,
        job_path: &Path,
        manifest: &QueueManifest,
    ) -> Result<Vec<InferenceInput>, String> {
        let mut inputs = Vec::with_capacity(manifest.inputs.len());
        for descriptor in &manifest.inputs {
            let bytes = tokio::fs::read(job_path.join(format!("input-{}", descriptor.sequence)))
                .await
                .map_err(|error| error.to_string())?;
            inputs.push(InferenceInput {
                sequence: descriptor.sequence,
                frame_timestamp_ms: descriptor.frame_timestamp_ms,
                bytes,
                filename: descriptor.filename.clone(),
            });
        }
        Ok(inputs)
    }

    async fn finish_job(
        &self,
        job_path: PathBuf,
        manifest: QueueManifest,
        inference: Result<Vec<InputInferenceResponse>, ServiceError>,
    ) {
        let callback = match inference {
            Ok(input_responses) => {
                let Some(first_response) = input_responses.first() else {
                    self.fail(job_path, "inference returned no input results".to_string());
                    return;
                };
                serde_json::json!({"jobId": manifest.job_id, "mediaId": manifest.media_id, "task": manifest.task, "attempt": manifest.attempt, "status": "completed", "modelType": first_response.response.model_type, "modelVersion": first_response.response.model_version, "result": first_response.response, "inputResults": input_responses.into_iter().map(|input| serde_json::json!({"sequence": input.sequence, "frameTimestampMs": input.frame_timestamp_ms, "result": input.response})).collect::<Vec<_>>()})
            }
            Err(error) => {
                serde_json::json!({"jobId": manifest.job_id, "mediaId": manifest.media_id, "task": manifest.task, "attempt": manifest.attempt, "status": "failed", "retryable": false, "error": error.to_string()})
            }
        };
        if let Err(error) =
            tokio::fs::write(job_path.join("result.json"), callback.to_string()).await
        {
            self.fail(job_path, error.to_string());
            return;
        }
        let Err(callback_error) = self
            .deliver_callback(&manifest.callback_url, &callback)
            .await
        else {
            if let Err(error) = tokio::fs::remove_dir_all(&job_path).await {
                self.fail(
                    job_path,
                    format!("callback acknowledged but queue cleanup failed: {error}"),
                );
            }
            return;
        };
        self.log_callback_failure(&manifest, &callback_error);
        let destination = self
            .queue_dir
            .join("callback_pending")
            .join(&manifest.job_id);
        match tokio::fs::rename(&job_path, &destination).await {
            Ok(()) => {
                if let Err(error) = self.record_callback_failure(&destination, &callback_error) {
                    self.fail(destination, error.to_string());
                }
            }
            Err(error) => self.fail(
                job_path,
                format!("failed to transition callback pending: {error}"),
            ),
        }
    }

    async fn retry_callbacks(&self) {
        let Ok(entries) = std::fs::read_dir(self.queue_dir.join("callback_pending")) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !self.callback_is_due(&path) {
                continue;
            }
            let Ok(manifest) = self.read_manifest(&path) else {
                self.fail(path, "invalid manifest".to_string());
                continue;
            };
            let Ok(result) = tokio::fs::read(path.join("result.json")).await else {
                self.fail(path, "missing inference result".to_string());
                continue;
            };
            let Ok(callback) = serde_json::from_slice(&result) else {
                self.fail(path, "invalid inference result".to_string());
                continue;
            };
            match self
                .deliver_callback(&manifest.callback_url, &callback)
                .await
            {
                Ok(()) => {
                    if let Err(error) = tokio::fs::remove_dir_all(&path).await {
                        self.fail(
                            path,
                            format!("callback acknowledged but queue cleanup failed: {error}"),
                        );
                    }
                }
                Err(callback_error) => {
                    self.log_callback_failure(&manifest, &callback_error);
                    if let Err(error) = self.record_callback_failure(&path, &callback_error) {
                        self.fail(path, error.to_string());
                    }
                }
            }
        }
    }

    fn callback_is_due(&self, job_path: &Path) -> bool {
        let metadata_path = job_path.join("callback.json");
        let Ok(bytes) = std::fs::read(metadata_path) else {
            return true;
        };
        let Ok(metadata) = serde_json::from_slice::<CallbackState>(&bytes) else {
            return true;
        };
        metadata.next_attempt_at <= chrono::Utc::now().timestamp()
    }

    fn record_callback_failure(&self, job_path: &Path, error: &str) -> Result<(), ServiceError> {
        let metadata_path = job_path.join("callback.json");
        let mut state = std::fs::read(&metadata_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<CallbackState>(&bytes).ok())
            .unwrap_or_default();
        state.attempts += 1;
        state.last_error = Some(error.to_string());
        if state.attempts >= self.callback.max_attempts {
            return Err(ServiceError::Upstream(format!(
                "callback retry attempts exhausted after {} attempts: {error}",
                state.attempts
            )));
        }
        state.next_attempt_at =
            chrono::Utc::now().timestamp() + self.callback.retry_delay_seconds as i64;
        let bytes = serde_json::to_vec(&state)
            .map_err(|error| ServiceError::Internal(error.to_string()))?;
        let mut file = std::fs::File::create(metadata_path).map_err(io_error)?;
        std::io::Write::write_all(&mut file, &bytes).map_err(io_error)?;
        file.sync_all().map_err(io_error)
    }

    async fn deliver_callback(
        &self,
        callback_url: &str,
        callback: &serde_json::Value,
    ) -> Result<(), String> {
        let response = self
            .client
            .post(callback_url)
            .header("x-momento-callback-key", &self.callback.key)
            .json(callback)
            .send()
            .await
            .map_err(|error| format!("request failed: {error}"))?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        let response_body = response
            .text()
            .await
            .map_err(|error| format!("HTTP {status}; failed to read response body: {error}"))?;
        let response_body = response_body.chars().take(4096).collect::<String>();
        Err(format!("HTTP {status}: {response_body}"))
    }

    fn log_callback_failure(&self, manifest: &QueueManifest, error: &str) {
        tracing::warn!(
            job_id = %manifest.job_id,
            callback_url = %manifest.callback_url,
            error = %error,
            "LLM callback delivery failed"
        );
    }

    fn read_manifest(&self, path: &Path) -> Result<QueueManifest, ServiceError> {
        serde_json::from_slice(&std::fs::read(path.join("manifest.json")).map_err(io_error)?)
            .map_err(|error| ServiceError::BadRequest(error.to_string()))
    }
    fn job_exists(&self, job_id: &str) -> bool {
        ["queuing", "processing", "callback_pending", "failed"]
            .iter()
            .any(|state| self.queue_dir.join(state).join(job_id).exists())
    }
    fn fail(&self, path: PathBuf, error: String) {
        let _ = std::fs::write(path.join("failure.json"), error);
        let name = path.file_name().unwrap_or_default().to_owned();
        let _ = std::fs::rename(path, self.queue_dir.join("failed").join(name));
    }
}

pub fn select_task<'a>(
    queued: &'a [(PathBuf, QueueManifest)],
    active_task: Option<&str>,
) -> Option<&'a str> {
    let active_task = active_task?;
    queued
        .iter()
        .find(|(_, manifest)| manifest.task == active_task)
        .map(|(_, manifest)| manifest.task.as_str())
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct CallbackState {
    attempts: usize,
    next_attempt_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
}

fn io_error(error: std::io::Error) -> ServiceError {
    ServiceError::Internal(error.to_string())
}

fn recover_queue(queue_dir: &Path) -> Result<(), ServiceError> {
    let temporary_dir = queue_dir.join(".tmp");
    for entry in std::fs::read_dir(&temporary_dir).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        std::fs::remove_dir_all(entry.path()).map_err(io_error)?;
    }
    for entry in std::fs::read_dir(queue_dir.join("processing")).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let destination = queue_dir.join("queuing").join(entry.file_name());
        if !destination.exists() {
            std::fs::rename(entry.path(), destination).map_err(io_error)?;
        }
    }
    for entry in std::fs::read_dir(queue_dir.join("callback_pending")).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let result_path = entry.path().join("result.json");
        if result_path.is_file() {
            continue;
        }
        let destination = queue_dir.join("queuing").join(entry.file_name());
        if !destination.exists() {
            std::fs::rename(entry.path(), destination).map_err(io_error)?;
        }
    }
    Ok(())
}
