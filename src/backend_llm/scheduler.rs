use std::collections::{HashSet, VecDeque};
use std::convert::Infallible;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use momento_common::llm::{
    CancelJobsRequest, CancelJobsResponse, JobInputDescriptor, JobInputResult, JobResult,
};
use momento_common::rolling::{run_rolling_window, RollingWindowControl};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;
use tracing::warn;

use crate::config::SchedulerConfig;
use crate::error::ServiceError;
use crate::provider::{
    InferenceDispatcher, InferenceInput, InputInferenceResponse, ServiceManager, ServiceType,
};
use crate::transport::ResultDeliveryTransport;

const MAX_INPUT_BYTES: u64 = 50 * 1024 * 1024;
const MAX_JOB_BYTES: u64 = 512 * 1024 * 1024;
const MAX_INPUTS_PER_JOB: usize = 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueueManifest {
    pub client_id: String,
    pub job_id: String,
    pub media_id: i64,
    pub task: String,
    pub attempt: u32,
    pub inputs: Vec<JobInputDescriptor>,
}

pub type QueueInputDescriptor = JobInputDescriptor;

pub struct Scheduler {
    queue_dir: PathBuf,
    configuration: SchedulerConfig,
    manager: Arc<Mutex<ServiceManager>>,
    result_delivery: Arc<dyn ResultDeliveryTransport>,
}

struct ClaimedJob {
    path: PathBuf,
    manifest: QueueManifest,
}

enum JobExecution {
    Inferred {
        job: ClaimedJob,
        result: Result<Vec<InputInferenceResponse>, ServiceError>,
    },
    Invalid {
        job: ClaimedJob,
        error: String,
    },
}

impl Scheduler {
    pub fn new(
        queue_dir: PathBuf,
        configuration: SchedulerConfig,
        manager: Arc<Mutex<ServiceManager>>,
        result_delivery: Arc<dyn ResultDeliveryTransport>,
    ) -> Result<Self, ServiceError> {
        std::fs::create_dir_all(queue_dir.join("queuing")).map_err(io_error)?;
        std::fs::create_dir_all(queue_dir.join("processing")).map_err(io_error)?;
        std::fs::create_dir_all(queue_dir.join("callback_pending")).map_err(io_error)?;
        std::fs::create_dir_all(queue_dir.join("failed")).map_err(io_error)?;
        std::fs::create_dir_all(queue_dir.join("cancelled")).map_err(io_error)?;
        std::fs::create_dir_all(queue_dir.join(".tmp")).map_err(io_error)?;
        std::fs::create_dir_all(queue_dir.join(".deleting")).map_err(io_error)?;
        recover_queue(&queue_dir)?;
        Ok(Self {
            queue_dir,
            configuration,
            manager,
            result_delivery,
        })
    }

    pub fn begin_admission(&self, manifest: QueueManifest) -> Result<QueueAdmission, ServiceError> {
        ServiceType::from_task(&manifest.task)?;
        if !is_valid_job_id(&manifest.job_id) {
            return Err(ServiceError::BadRequest(
                "jobId must be a non-empty hexadecimal identifier".to_string(),
            ));
        }
        if manifest.client_id.is_empty()
            || manifest.inputs.is_empty()
            || manifest.inputs.len() > MAX_INPUTS_PER_JOB
        {
            return Err(ServiceError::BadRequest(
                "clientId and between 1 and 1024 inputs are required".to_string(),
            ));
        }
        let mut sequences = HashSet::with_capacity(manifest.inputs.len());
        let mut declared_job_bytes = 0_u64;
        if manifest.inputs.iter().any(|descriptor| {
            declared_job_bytes = declared_job_bytes.saturating_add(descriptor.byte_size);
            !sequences.insert(descriptor.sequence)
                || descriptor.filename.is_empty()
                || !descriptor.mime_type.starts_with("image/")
                || descriptor.byte_size == 0
                || descriptor.byte_size > MAX_INPUT_BYTES
                || descriptor.content_hash.len() != 64
                || !descriptor
                    .content_hash
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
        }) || declared_job_bytes > MAX_JOB_BYTES
        {
            return Err(ServiceError::BadRequest(
                "input descriptors must have unique sequences, bounded job/image bytes, and SHA-256 hashes"
                    .to_string(),
            ));
        }
        if let Some(existing) = self.existing_manifest(&manifest.job_id) {
            if existing == manifest {
                return Ok(QueueAdmission::Duplicate);
            }
            if existing.client_id == manifest.client_id {
                return Err(ServiceError::Conflict(
                    "job ID is already associated with a different manifest".to_string(),
                ));
            }
            return Err(ServiceError::Conflict(
                "job ID is already owned by another client".to_string(),
            ));
        }
        let cancelled = self.cancellation_marker(&manifest.client_id, &manifest.job_id);
        if cancelled.exists() {
            return Ok(QueueAdmission::Cancelled);
        }
        let temporary = self.queue_dir.join(".tmp").join(&manifest.job_id);
        let queuing = self.queue_dir.join("queuing").join(&manifest.job_id);
        match std::fs::create_dir(&temporary) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(ServiceError::Conflict(format!(
                    "temporary job already exists: {}",
                    manifest.job_id
                )));
            }
            Err(error) => return Err(io_error(error)),
        }
        let staging = QueueStaging {
            manifest,
            temporary,
            queuing,
            cancelled,
            verified_sequences: HashSet::new(),
            committed: false,
        };
        sync_directory(staging.temporary.parent().ok_or_else(|| {
            ServiceError::Internal("temporary job path has no parent".to_string())
        })?)?;
        let manifest_bytes = serde_json::to_vec(&staging.manifest)
            .map_err(|error| ServiceError::BadRequest(error.to_string()))?;
        let mut manifest_file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(staging.temporary.join("manifest.json"))
            .map_err(io_error)?;
        std::io::Write::write_all(&mut manifest_file, &manifest_bytes).map_err(io_error)?;
        manifest_file.sync_all().map_err(io_error)?;
        Ok(QueueAdmission::Staging(Box::new(staging)))
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
        staging.commit().map(|_| ())
    }

    pub fn cancel_jobs(
        &self,
        client_id: &str,
        request: &CancelJobsRequest,
    ) -> Result<CancelJobsResponse, ServiceError> {
        if request.all == !request.tasks.is_empty() {
            return Err(ServiceError::BadRequest(
                "cancellation must select all tasks or at least one specific task".to_string(),
            ));
        }
        let mut tasks = HashSet::with_capacity(request.tasks.len());
        for task in &request.tasks {
            if ServiceType::from_task(task).is_err() || !tasks.insert(task.as_str()) {
                return Err(ServiceError::BadRequest(
                    "tasks must contain unique supported task identifiers".to_string(),
                ));
            }
        }
        let mut job_ids = HashSet::with_capacity(request.job_ids.len());
        for job_id in &request.job_ids {
            if !is_valid_job_id(job_id) || !job_ids.insert(job_id.clone()) {
                return Err(ServiceError::BadRequest(
                    "jobIds must contain unique hexadecimal identifiers".to_string(),
                ));
            }
        }
        for state in [
            ".tmp",
            "queuing",
            "processing",
            "callback_pending",
            "failed",
        ] {
            let entries = std::fs::read_dir(self.queue_dir.join(state)).map_err(io_error)?;
            for entry in entries {
                let entry = entry.map_err(io_error)?;
                if !entry.file_type().map_err(io_error)?.is_dir() {
                    continue;
                }
                let Some(job_id) = entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                let matches_client_and_scope =
                    self.read_manifest(&entry.path()).is_ok_and(|manifest| {
                        manifest.client_id == client_id
                            && (request.all || tasks.contains(manifest.task.as_str()))
                    });
                if matches_client_and_scope {
                    job_ids.insert(job_id);
                }
            }
        }
        let mut response = CancelJobsResponse {
            requested_jobs: request.job_ids.len(),
            cancelled_jobs: 0,
            running_jobs: 0,
            missing_jobs: 0,
        };
        for job_id in job_ids {
            let owns_existing_job = [
                ".tmp",
                "queuing",
                "processing",
                "callback_pending",
                "failed",
            ]
            .iter()
            .filter_map(|state| {
                self.read_manifest(&self.queue_dir.join(state).join(&job_id))
                    .ok()
            })
            .any(|manifest| manifest.client_id == client_id);
            if !owns_existing_job && !request.job_ids.contains(&job_id) {
                continue;
            }
            if !owns_existing_job
                && [
                    ".tmp",
                    "queuing",
                    "processing",
                    "callback_pending",
                    "failed",
                ]
                .iter()
                .any(|state| self.queue_dir.join(state).join(&job_id).exists())
            {
                response.missing_jobs += 1;
                continue;
            }
            let marker = self.cancellation_marker(client_id, &job_id);
            let existed = !create_cancellation_marker(&marker)?;
            if self.queue_dir.join("processing").join(&job_id).exists() {
                response.running_jobs += 1;
                continue;
            }
            let mut removed = false;
            let staging = self.queue_dir.join(".tmp").join(&job_id);
            if staging.exists() {
                match std::fs::remove_dir_all(&staging) {
                    Ok(()) => {
                        sync_directory(&self.queue_dir.join(".tmp"))?;
                        removed = true;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(io_error(error)),
                }
            }
            for state in ["queuing", "callback_pending", "failed"] {
                removed |= self.remove_cancelled_job(state, &job_id)?;
            }
            if removed || existed {
                response.cancelled_jobs += 1;
            } else {
                response.missing_jobs += 1;
            }
        }
        Ok(response)
    }

    fn remove_cancelled_job(&self, state: &str, job_id: &str) -> Result<bool, ServiceError> {
        let source = self.queue_dir.join(state).join(job_id);
        if !source.exists() {
            return Ok(false);
        }
        let deleting = self
            .queue_dir
            .join(".deleting")
            .join(format!("cancel-{state}-{job_id}"));
        match std::fs::rename(&source, &deleting) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(io_error(error)),
        }
        sync_directory(source.parent().ok_or_else(|| {
            ServiceError::Internal("cancelled queue path has no parent".to_string())
        })?)?;
        sync_directory(&self.queue_dir.join(".deleting"))?;
        std::fs::remove_dir_all(&deleting).map_err(io_error)?;
        sync_directory(&self.queue_dir.join(".deleting"))?;
        Ok(true)
    }

    fn cancellation_marker(&self, client_id: &str, job_id: &str) -> PathBuf {
        self.queue_dir
            .join("cancelled")
            .join(format!("{client_id}-{job_id}"))
    }
}

pub enum QueueAdmission {
    Cancelled,
    Duplicate,
    Staging(Box<QueueStaging>),
}

pub struct QueueStaging {
    manifest: QueueManifest,
    temporary: PathBuf,
    queuing: PathBuf,
    cancelled: PathBuf,
    verified_sequences: HashSet<u32>,
    committed: bool,
}

impl QueueStaging {
    pub fn write_input(
        &mut self,
        descriptor: &QueueInputDescriptor,
        bytes: &[u8],
    ) -> Result<(), ServiceError> {
        let byte_count = u64::try_from(bytes.len())
            .map_err(|_| ServiceError::BadRequest("input bytes are too large".to_string()))?;
        if byte_count > descriptor.byte_size {
            return Err(ServiceError::BadRequest(
                "input bytes exceed descriptor size".to_string(),
            ));
        }
        let input_path = self.input_path(descriptor)?;
        let mut input_file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(input_path)
            .map_err(io_error)?;
        std::io::Write::write_all(&mut input_file, bytes).map_err(io_error)?;
        input_file.sync_all().map_err(io_error)?;
        self.verify_input(descriptor, byte_count, Sha256::digest(bytes))
    }

    pub fn input_path(&self, descriptor: &QueueInputDescriptor) -> Result<PathBuf, ServiceError> {
        self.validate_descriptor(descriptor)?;
        Ok(self
            .temporary
            .join(format!("input-{}", descriptor.sequence)))
    }

    pub fn verify_input(
        &mut self,
        descriptor: &QueueInputDescriptor,
        byte_count: u64,
        content_hash: impl std::fmt::LowerHex,
    ) -> Result<(), ServiceError> {
        self.validate_descriptor(descriptor)?;
        if byte_count == 0
            || byte_count != descriptor.byte_size
            || format!("{content_hash:x}") != descriptor.content_hash
        {
            return Err(ServiceError::BadRequest(
                "input bytes do not match descriptor".to_string(),
            ));
        }
        if !self.verified_sequences.insert(descriptor.sequence) {
            return Err(ServiceError::BadRequest(
                "input sequence was supplied more than once".to_string(),
            ));
        }
        Ok(())
    }

    pub fn commit(mut self) -> Result<bool, ServiceError> {
        if self.verified_sequences.len() != self.manifest.inputs.len()
            || self
                .manifest
                .inputs
                .iter()
                .any(|descriptor| !self.verified_sequences.contains(&descriptor.sequence))
        {
            return Err(ServiceError::BadRequest(
                "every manifest input must be supplied exactly once".to_string(),
            ));
        }
        if self.cancelled.exists() {
            match std::fs::remove_dir_all(&self.temporary) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(io_error(error)),
            }
            self.committed = true;
            sync_directory(self.temporary.parent().ok_or_else(|| {
                ServiceError::Internal("temporary job path has no parent".to_string())
            })?)?;
            return Ok(false);
        }
        let manifest_path = self.temporary.join("manifest.json");
        std::fs::File::open(&manifest_path)
            .map_err(io_error)?
            .sync_all()
            .map_err(io_error)?;
        sync_directory(&self.temporary)?;
        let queue_path = self.queuing.clone();
        std::fs::rename(&self.temporary, queue_path).map_err(io_error)?;
        self.committed = true;
        let queue_directory = self
            .queuing
            .parent()
            .ok_or_else(|| ServiceError::Internal("queue job path has no parent".to_string()))?;
        sync_directory(queue_directory)?;
        Ok(true)
    }

    fn validate_descriptor(&self, descriptor: &QueueInputDescriptor) -> Result<(), ServiceError> {
        let expected = self
            .manifest
            .inputs
            .iter()
            .find(|expected| expected.sequence == descriptor.sequence)
            .ok_or_else(|| {
                ServiceError::BadRequest("input has no manifest descriptor".to_string())
            })?;
        if expected != descriptor
            || descriptor.filename.is_empty()
            || !descriptor.mime_type.starts_with("image/")
            || descriptor.byte_size == 0
            || descriptor.byte_size > MAX_INPUT_BYTES
        {
            return Err(ServiceError::BadRequest(
                "input descriptor does not match manifest".to_string(),
            ));
        }
        Ok(())
    }
}

impl Drop for QueueStaging {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_dir_all(&self.temporary);
        }
    }
}

impl Scheduler {
    pub async fn run(self: Arc<Self>) {
        tokio::join!(
            Arc::clone(&self).run_inference_loop(),
            self.run_result_delivery_loop()
        );
    }

    async fn run_inference_loop(self: Arc<Self>) {
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

    async fn run_result_delivery_loop(self: Arc<Self>) {
        let interval = Duration::from_secs(self.configuration.poll_interval_seconds);
        loop {
            if self.deliver_pending_results().await > 0 {
                continue;
            }
            tokio::time::sleep(interval).await;
        }
    }

    async fn process_cycle(&self) -> bool {
        let active_task = self.manager.lock().await.active_task();
        let Some(task) = self.select_queued_task(active_task) else {
            return false;
        };
        let max_in_flight = NonZeroUsize::new(self.configuration.max_in_flight_jobs)
            .expect("validated scheduler inference window");
        let initial = self.claim_queued_jobs(&task, max_in_flight.get());
        if initial.is_empty() {
            return false;
        }
        let dispatcher = match self.manager.lock().await.dispatcher(&task).await {
            Ok(dispatcher) => dispatcher,
            Err(error) => {
                let message = error.to_string();
                for job in initial {
                    self.persist_job_result(
                        job.path,
                        job.manifest,
                        Err(ServiceError::Internal(message.clone())),
                    )
                    .await;
                }
                return true;
            }
        };
        let runtime_unavailable = Arc::new(AtomicBool::new(false));
        let mut initial = Some(initial);
        run_rolling_window(
            max_in_flight,
            |capacity| {
                Ok::<_, Infallible>(
                    initial
                        .take()
                        .unwrap_or_else(|| self.claim_queued_jobs(&task, capacity)),
                )
            },
            |job| self.execute_claimed_job(dispatcher.clone(), job),
            {
                let runtime_unavailable = Arc::clone(&runtime_unavailable);
                move |execution| {
                    let runtime_unavailable = Arc::clone(&runtime_unavailable);
                    async move {
                        match execution {
                            JobExecution::Invalid { job, error } => self.fail(job.path, error),
                            JobExecution::Inferred { job, result } => {
                                let unavailable =
                                    matches!(result, Err(ServiceError::RuntimeUnavailable(_)));
                                self.persist_job_result(job.path, job.manifest, result)
                                    .await;
                                if unavailable {
                                    runtime_unavailable.store(true, Ordering::Relaxed);
                                    return RollingWindowControl::Stop;
                                }
                            }
                        }
                        RollingWindowControl::Continue
                    }
                }
            },
        )
        .await
        .expect("infallible queue claim");
        if runtime_unavailable.load(Ordering::Relaxed) {
            if let Err(error) = self.manager.lock().await.shutdown().await {
                warn!("failed to stop unavailable model runtime: {error}");
            }
        }
        true
    }

    pub fn select_queued_jobs(&self, active_task: Option<&str>) -> Vec<(PathBuf, QueueManifest)> {
        let Some(task) = self.select_queued_task(active_task) else {
            return Vec::new();
        };
        self.queued_jobs(&task, self.configuration.max_in_flight_jobs)
    }

    fn queued_jobs(&self, task: &str, limit: usize) -> Vec<(PathBuf, QueueManifest)> {
        let Ok(entries) = std::fs::read_dir(self.queue_dir.join("queuing")) else {
            return Vec::new();
        };
        let mut selected = Vec::with_capacity(limit);
        for entry in entries.flatten() {
            let queue_path = entry.path();
            let Ok(manifest) = self.read_manifest(&queue_path) else {
                continue;
            };
            if manifest.task != task || !is_valid_job_id(&manifest.job_id) {
                continue;
            }
            insert_queued_job(&mut selected, (queue_path, manifest), limit);
        }
        selected
    }

    fn claim_queued_jobs(&self, task: &str, limit: usize) -> Vec<ClaimedJob> {
        let mut claimed = Vec::with_capacity(limit);
        for (queue_path, manifest) in self.queued_jobs(task, limit) {
            if self
                .cancellation_marker(&manifest.client_id, &manifest.job_id)
                .exists()
            {
                let _ = self.remove_cancelled_job("queuing", &manifest.job_id);
                continue;
            }
            let processing_path = self.queue_dir.join("processing").join(&manifest.job_id);
            if std::fs::rename(&queue_path, &processing_path).is_ok() {
                claimed.push(ClaimedJob {
                    path: processing_path,
                    manifest,
                });
            }
        }
        claimed
    }

    async fn execute_claimed_job(
        &self,
        dispatcher: InferenceDispatcher,
        job: ClaimedJob,
    ) -> JobExecution {
        let started = Instant::now();
        let inputs = match self.load_inputs(&job.path, &job.manifest).await {
            Ok(inputs) => inputs,
            Err(error) => return JobExecution::Invalid { job, error },
        };
        let input_verification_ms = started.elapsed().as_secs_f64() * 1000.0;
        let inference_started = Instant::now();
        let result = dispatcher.infer_inputs(inputs).await;
        tracing::debug!(
            job_id = job.manifest.job_id,
            task = job.manifest.task,
            input_verification_ms,
            inference_ms = inference_started.elapsed().as_secs_f64() * 1000.0,
            total_ms = started.elapsed().as_secs_f64() * 1000.0,
            "LLM inference timing"
        );
        JobExecution::Inferred { job, result }
    }

    fn select_queued_task(&self, active_task: Option<&str>) -> Option<String> {
        let entries = std::fs::read_dir(self.queue_dir.join("queuing")).ok()?;
        let mut first_job: Option<QueueManifest> = None;
        for entry in entries.flatten() {
            let Ok(manifest) = self.read_manifest(&entry.path()) else {
                continue;
            };
            if !is_valid_job_id(&manifest.job_id) || ServiceType::from_task(&manifest.task).is_err()
            {
                continue;
            }
            if active_task.is_some_and(|task| task == manifest.task) {
                return Some(manifest.task);
            }
            if first_job
                .as_ref()
                .is_none_or(|first| manifest.job_id < first.job_id)
            {
                first_job = Some(manifest);
            }
        }
        first_job.map(|manifest| manifest.task)
    }

    async fn load_inputs(
        &self,
        job_path: &Path,
        manifest: &QueueManifest,
    ) -> Result<Vec<InferenceInput>, String> {
        let job_metadata = tokio::fs::symlink_metadata(job_path)
            .await
            .map_err(|error| error.to_string())?;
        if !job_metadata.file_type().is_dir() {
            return Err("processing job is not a directory".to_string());
        }
        let mut inputs = Vec::with_capacity(manifest.inputs.len());
        for descriptor in &manifest.inputs {
            let path = job_path.join(format!("input-{}", descriptor.sequence));
            let metadata = tokio::fs::symlink_metadata(&path)
                .await
                .map_err(|error| error.to_string())?;
            if !metadata.file_type().is_file() {
                return Err("queued input is not a regular file".to_string());
            }
            if metadata.len() != descriptor.byte_size {
                return Err("queued input size does not match manifest".to_string());
            }
            let mut file = tokio::fs::File::open(&path)
                .await
                .map_err(|error| error.to_string())?;
            let mut hasher = Sha256::new();
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let read = file
                    .read(&mut buffer)
                    .await
                    .map_err(|error| error.to_string())?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            if format!("{:x}", hasher.finalize()) != descriptor.content_hash {
                return Err("queued input hash does not match manifest".to_string());
            }
            inputs.push(InferenceInput {
                job_id: manifest.job_id.clone(),
                sequence: descriptor.sequence,
                frame_timestamp_ms: descriptor.frame_timestamp_ms,
                path,
                byte_size: descriptor.byte_size,
                content_hash: descriptor.content_hash.clone(),
                mime_type: descriptor.mime_type.clone(),
                filename: descriptor.filename.clone(),
            });
        }
        Ok(inputs)
    }

    async fn persist_job_result(
        &self,
        job_path: PathBuf,
        manifest: QueueManifest,
        inference: Result<Vec<InputInferenceResponse>, ServiceError>,
    ) {
        if self
            .cancellation_marker(&manifest.client_id, &manifest.job_id)
            .exists()
        {
            self.remove_finished_cancelled_job(&job_path);
            return;
        }
        let result = match inference {
            Ok(input_responses) => {
                let Some(first_response) = input_responses.first() else {
                    self.fail(job_path, "inference returned no input results".to_string());
                    return;
                };
                let model_type = first_response.response.model_type.clone();
                let model_version = first_response.response.model_version.clone();
                let first_result = serde_json::to_value(&first_response.response)
                    .expect("inference response must serialize");
                JobResult {
                    job_id: manifest.job_id.clone(),
                    media_id: manifest.media_id,
                    task: manifest.task.clone(),
                    attempt: manifest.attempt,
                    status: "completed".to_string(),
                    model_type: Some(model_type),
                    model_version: Some(model_version),
                    result: Some(first_result),
                    input_results: Some(
                        input_responses
                            .into_iter()
                            .map(|input| JobInputResult {
                                sequence: input.sequence,
                                frame_timestamp_ms: input.frame_timestamp_ms,
                                result: serde_json::to_value(input.response)
                                    .expect("inference response must serialize"),
                            })
                            .collect(),
                    ),
                    error: None,
                }
            }
            Err(ServiceError::RuntimeUnavailable(error)) => {
                match self.requeue_runtime_failure(&job_path, &manifest, &error) {
                    Ok(true) => return,
                    Ok(false) => failed_job_result(
                        &manifest,
                        format!(
                            "local model runtime remained unavailable after {} attempts: {error}",
                            self.configuration.runtime_max_attempts
                        ),
                    ),
                    Err(requeue_error) => failed_job_result(&manifest, requeue_error.to_string()),
                }
            }
            Err(error) => failed_job_result(&manifest, error.to_string()),
        };
        let result_bytes = serde_json::to_vec(&result).expect("job result must serialize");
        if let Err(error) = write_synced_file(&job_path.join("result.json"), &result_bytes) {
            self.fail(job_path, error.to_string());
            return;
        }
        let destination = self
            .queue_dir
            .join("callback_pending")
            .join(&manifest.job_id);
        match transition_directory(&job_path, &destination) {
            Ok(()) => {
                if self
                    .cancellation_marker(&manifest.client_id, &manifest.job_id)
                    .exists()
                {
                    let _ = self.remove_cancelled_job("callback_pending", &manifest.job_id);
                }
            }
            Err(error) => self.fail(
                job_path,
                format!("failed to transition callback pending: {error}"),
            ),
        }
    }

    fn remove_finished_cancelled_job(&self, job_path: &Path) {
        match std::fs::remove_dir_all(job_path) {
            Ok(()) => {
                if let Some(parent) = job_path.parent() {
                    let _ = sync_directory(parent);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => warn!(
                job_path = %job_path.display(),
                error = %error,
                "failed to remove a finished cancelled job"
            ),
        }
    }

    pub fn requeue_runtime_failure(
        &self,
        job_path: &Path,
        manifest: &QueueManifest,
        error: &str,
    ) -> Result<bool, ServiceError> {
        let state_path = job_path.join("runtime.json");
        let mut state = std::fs::read(&state_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<RuntimeRetryState>(&bytes).ok())
            .unwrap_or_default();
        state.attempts += 1;
        state.last_error = error.to_string();
        if state.attempts >= self.configuration.runtime_max_attempts {
            return Ok(false);
        }
        let state_bytes = serde_json::to_vec(&state).map_err(|serialization_error| {
            ServiceError::Internal(serialization_error.to_string())
        })?;
        write_synced_file(&state_path, &state_bytes)?;
        let destination = self.queue_dir.join("queuing").join(&manifest.job_id);
        transition_directory(job_path, &destination)?;
        Ok(true)
    }

    async fn deliver_pending_results(&self) -> usize {
        if self.configuration.result_delivery_max_concurrent_deliveries == 0 {
            return 0;
        }
        let selection_limit = self
            .configuration
            .result_delivery_max_concurrent_deliveries
            .saturating_mul(16);
        let mut pending = VecDeque::from(self.select_due_results(selection_limit));
        if pending.is_empty() {
            return 0;
        }
        run_rolling_window(
            NonZeroUsize::new(self.configuration.result_delivery_max_concurrent_deliveries)
                .expect("validated result delivery window"),
            |capacity| {
                Ok::<_, Infallible>((0..capacity).filter_map(|_| pending.pop_front()).collect())
            },
            |path| async move {
                self.deliver_result(path.clone()).await;
                path
            },
            |_path: PathBuf| async { RollingWindowControl::Continue },
        )
        .await
        .expect("infallible result selection")
    }

    fn select_due_results(&self, limit: usize) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(self.queue_dir.join("callback_pending")) else {
            return Vec::new();
        };
        let mut selected = Vec::with_capacity(limit);
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(priority) = self.result_delivery_priority(&path) else {
                continue;
            };
            insert_result_path(&mut selected, (priority, path), limit);
        }
        selected.into_iter().map(|(_, path)| path).collect()
    }

    async fn deliver_result(&self, path: PathBuf) {
        let Ok(manifest) = self.read_manifest(&path) else {
            self.fail(path, "invalid manifest".to_string());
            return;
        };
        let Ok(result) = tokio::fs::read(path.join("result.json")).await else {
            self.fail(path, "missing inference result".to_string());
            return;
        };
        let Ok(result) = serde_json::from_slice::<JobResult>(&result) else {
            self.fail(path, "invalid inference result".to_string());
            return;
        };
        match self
            .result_delivery
            .deliver_result(
                &manifest.client_id,
                &result,
                Duration::from_secs(
                    self.configuration
                        .result_delivery_acknowledgement_timeout_seconds,
                ),
            )
            .await
        {
            Ok(()) => {
                let deleting_path = self.queue_dir.join(".deleting").join(
                    path.file_name()
                        .unwrap_or_else(|| std::ffi::OsStr::new("invalid-job")),
                );
                if let Err(error) = transition_directory(&path, &deleting_path) {
                    self.fail(
                        path,
                        format!("result acknowledged but cleanup transition failed: {error}"),
                    );
                    return;
                }
                if let Err(error) = tokio::fs::remove_dir_all(&deleting_path).await {
                    warn!("result acknowledged but queue cleanup will resume at startup: {error}");
                }
            }
            Err(delivery_error) => {
                self.log_result_delivery_failure(&manifest, &delivery_error);
                if let Err(error) = self.record_result_delivery_failure(&path, &delivery_error) {
                    self.fail(path, error.to_string());
                }
            }
        }
    }

    fn result_delivery_priority(&self, job_path: &Path) -> Option<(u8, i64, String)> {
        let metadata_path = job_path.join("callback.json");
        let Ok(bytes) = std::fs::read(metadata_path) else {
            return Some((
                0,
                0,
                job_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            ));
        };
        let Ok(metadata) = serde_json::from_slice::<ResultDeliveryState>(&bytes) else {
            return Some((
                1,
                i64::MIN,
                job_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            ));
        };
        if metadata.next_attempt_at > chrono::Utc::now().timestamp() {
            return None;
        }
        Some((
            1,
            metadata.next_attempt_at,
            job_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        ))
    }

    fn record_result_delivery_failure(
        &self,
        job_path: &Path,
        error: &str,
    ) -> Result<(), ServiceError> {
        let metadata_path = job_path.join("callback.json");
        let mut state = std::fs::read(&metadata_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<ResultDeliveryState>(&bytes).ok())
            .unwrap_or_default();
        state.attempts += 1;
        state.last_error = Some(error.to_string());
        if state.attempts >= self.configuration.result_delivery_max_attempts {
            return Err(ServiceError::Upstream(format!(
                "result delivery retry attempts exhausted after {} attempts: {error}",
                state.attempts
            )));
        }
        state.next_attempt_at = chrono::Utc::now().timestamp()
            + self.configuration.result_delivery_retry_delay_seconds as i64;
        let bytes = serde_json::to_vec(&state)
            .map_err(|error| ServiceError::Internal(error.to_string()))?;
        write_synced_file(&metadata_path, &bytes)
    }

    fn log_result_delivery_failure(&self, manifest: &QueueManifest, error: &str) {
        tracing::warn!(
            job_id = %manifest.job_id,
            client_id = %manifest.client_id,
            error = %error,
            "LLM result delivery failed"
        );
    }

    fn read_manifest(&self, path: &Path) -> Result<QueueManifest, ServiceError> {
        serde_json::from_slice(&std::fs::read(path.join("manifest.json")).map_err(io_error)?)
            .map_err(|error| ServiceError::BadRequest(error.to_string()))
    }
    fn existing_manifest(&self, job_id: &str) -> Option<QueueManifest> {
        ["queuing", "processing", "callback_pending", "failed"]
            .iter()
            .find_map(|state| {
                self.read_manifest(&self.queue_dir.join(state).join(job_id))
                    .ok()
            })
    }
    fn fail(&self, path: PathBuf, error: String) {
        let cancelled = self.read_manifest(&path).is_ok_and(|manifest| {
            self.cancellation_marker(&manifest.client_id, &manifest.job_id)
                .exists()
        });
        if cancelled {
            if std::fs::remove_dir_all(&path).is_ok() {
                if let Some(parent) = path.parent() {
                    let _ = sync_directory(parent);
                }
            }
            return;
        }
        if write_synced_file(&path.join("failure.json"), error.as_bytes()).is_err() {
            return;
        }
        let name = path.file_name().unwrap_or_default().to_owned();
        let _ = transition_directory(&path, &self.queue_dir.join("failed").join(name));
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

fn failed_job_result(manifest: &QueueManifest, error: String) -> JobResult {
    JobResult {
        job_id: manifest.job_id.clone(),
        media_id: manifest.media_id,
        task: manifest.task.clone(),
        attempt: manifest.attempt,
        status: "failed".to_string(),
        model_type: None,
        model_version: None,
        result: None,
        input_results: None,
        error: Some(error),
    }
}

fn insert_queued_job(
    selected: &mut Vec<(PathBuf, QueueManifest)>,
    candidate: (PathBuf, QueueManifest),
    maximum_count: usize,
) {
    if maximum_count == 0 {
        return;
    }
    let position = selected
        .binary_search_by(|(_, manifest)| manifest.job_id.cmp(&candidate.1.job_id))
        .unwrap_or_else(|position| position);
    if position >= maximum_count {
        return;
    }
    selected.insert(position, candidate);
    if selected.len() > maximum_count {
        selected.pop();
    }
}

fn insert_result_path(
    selected: &mut Vec<((u8, i64, String), PathBuf)>,
    candidate: ((u8, i64, String), PathBuf),
    maximum_count: usize,
) {
    if maximum_count == 0 {
        return;
    }
    let position = selected
        .binary_search_by(|(priority, _)| priority.cmp(&candidate.0))
        .unwrap_or_else(|position| position);
    if position >= maximum_count {
        return;
    }
    selected.insert(position, candidate);
    if selected.len() > maximum_count {
        selected.pop();
    }
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct ResultDeliveryState {
    attempts: usize,
    next_attempt_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct RuntimeRetryState {
    attempts: usize,
    last_error: String,
}

fn write_synced_file(path: &Path, bytes: &[u8]) -> Result<(), ServiceError> {
    let temporary_path = path.with_extension("tmp");
    let mut file = std::fs::File::create(&temporary_path).map_err(io_error)?;
    std::io::Write::write_all(&mut file, bytes).map_err(io_error)?;
    file.sync_all().map_err(io_error)?;
    std::fs::rename(&temporary_path, path).map_err(io_error)?;
    let parent = path
        .parent()
        .ok_or_else(|| ServiceError::Internal("durable file has no parent".to_string()))?;
    sync_directory(parent)
}

fn create_cancellation_marker(path: &Path) -> Result<bool, ServiceError> {
    let file = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
        Err(error) => return Err(io_error(error)),
    };
    file.sync_all().map_err(io_error)?;
    sync_directory(
        path.parent().ok_or_else(|| {
            ServiceError::Internal("cancellation marker has no parent".to_string())
        })?,
    )?;
    Ok(true)
}

fn transition_directory(source: &Path, destination: &Path) -> Result<(), ServiceError> {
    let source_parent = source
        .parent()
        .ok_or_else(|| ServiceError::Internal("source queue path has no parent".to_string()))?;
    let destination_parent = destination.parent().ok_or_else(|| {
        ServiceError::Internal("destination queue path has no parent".to_string())
    })?;
    std::fs::rename(source, destination).map_err(io_error)?;
    sync_directory(source_parent)?;
    sync_directory(destination_parent)
}

fn io_error(error: std::io::Error) -> ServiceError {
    ServiceError::Internal(error.to_string())
}

fn is_valid_job_id(job_id: &str) -> bool {
    !job_id.is_empty()
        && job_id
            .bytes()
            .all(|character| character.is_ascii_hexdigit())
}

fn sync_directory(path: &Path) -> Result<(), ServiceError> {
    std::fs::File::open(path)
        .map_err(io_error)?
        .sync_all()
        .map_err(io_error)
}

fn recover_queue(queue_dir: &Path) -> Result<(), ServiceError> {
    let temporary_dir = queue_dir.join(".tmp");
    for entry in std::fs::read_dir(&temporary_dir).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        std::fs::remove_dir_all(entry.path()).map_err(io_error)?;
    }
    for entry in std::fs::read_dir(queue_dir.join(".deleting")).map_err(io_error)? {
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
