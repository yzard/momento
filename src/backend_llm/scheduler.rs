use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, MutexGuard as StdMutexGuard};
use std::time::{Duration, Instant};

use momento_common::llm::result_stream::{
    ResultInputCorrelation, ResultManifest, ResultRecordChunkDecoder, ResultRecordStreamValidator,
};
use momento_common::llm::{
    is_valid_job_id, validate_job_manifest_fields, CancelJobsRequest, CancelJobsResponse,
    JobInputDescriptor, MAX_BINARY_CHUNK_BYTES, MAX_LLM_INPUT_BYTES,
};
use momento_common::rolling::{run_rolling_window, RollingWindowControl};
use momento_common::work_signal::WorkSignal;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;
use tracing::warn;

use crate::config::SchedulerConfig;
use crate::content_store::ContentStore;
use crate::error::ServiceError;
use crate::input_normalizer::{
    ensure_raw_normalized, requires_raw_normalization, runtime_input_path,
};
use crate::provider::{
    InferenceDispatcher, InferenceInput, InputInferenceResponse, ServiceManager, ServiceType,
};
use crate::queue_capacity::{
    QueueCapacityDecision, QueueCapacityInput, QueueCapacityManager, QueueCapacityReservation,
    QueueCapacityStatus,
};
use crate::result_output::{encode_completed_result, encode_failed_result, DurableResultOutput};
use crate::transport::{ResultDeliveryError, ResultDeliveryOutcome, ResultDeliveryTransport};

const RESULT_MANIFEST_FILE: &str = "result-manifest.json";
const RESULT_MANIFEST_TEMPORARY_FILE: &str = "result-manifest.tmp";
const RESULT_RECORDS_FILE: &str = "result-records.bin";
const RESULT_RECORDS_TEMPORARY_FILE: &str = "result-records.tmp";
const WORKER_ERROR_RETRY_DELAY: Duration = Duration::from_secs(1);

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
    content_store: Arc<StdMutex<ContentStore>>,
    queue_capacity: Arc<QueueCapacityManager>,
    configuration: SchedulerConfig,
    manager: Arc<Mutex<ServiceManager>>,
    result_delivery: Arc<dyn ResultDeliveryTransport>,
    inference_work: WorkSignal,
    result_delivery_work: WorkSignal,
    normalization_lock: Mutex<()>,
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

enum LoadInputsError {
    InvalidQueue(String),
    Inference(ServiceError),
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
        let content_store = Arc::new(StdMutex::new(ContentStore::new(&queue_dir)?));
        let queue_capacity = QueueCapacityManager::new(
            queue_dir.join("content"),
            queue_dir.clone(),
            configuration.max_queue_bytes,
            configuration.working_space_reserve_bytes,
        )?;
        Ok(Self {
            queue_dir,
            content_store,
            queue_capacity,
            configuration,
            manager,
            result_delivery,
            inference_work: WorkSignal::default(),
            result_delivery_work: WorkSignal::default(),
            normalization_lock: Mutex::new(()),
        })
    }

    pub fn begin_admission(&self, manifest: QueueManifest) -> Result<QueueAdmission, ServiceError> {
        ServiceType::from_task(&manifest.task)?;
        if manifest.client_id.is_empty() {
            return Err(ServiceError::BadRequest("clientId is required".to_string()));
        }
        validate_job_manifest_fields(
            &manifest.job_id,
            manifest.media_id,
            &manifest.task,
            manifest.attempt,
            &manifest.inputs,
        )
        .map_err(ServiceError::BadRequest)?;
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
        let store = lock_content_store(&self.content_store)?;
        let content = manifest
            .inputs
            .iter()
            .map(|descriptor| {
                Ok(QueueCapacityInput {
                    content_hash: descriptor.content_hash.clone(),
                    byte_size: descriptor.byte_size,
                    is_cached: store.input_is_cached(descriptor)?,
                })
            })
            .collect::<Result<Vec<_>, ServiceError>>()?;
        let capacity_reservation = match self
            .queue_capacity
            .try_reserve(&manifest.job_id, &content)?
        {
            Ok(reservation) => reservation,
            Err(QueueCapacityDecision::Deferred(status)) => {
                return Ok(QueueAdmission::Deferred(status));
            }
            Err(QueueCapacityDecision::JobTooLarge(status)) => {
                return Ok(QueueAdmission::JobTooLarge(status));
            }
        };
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
        let mut staging = QueueStaging {
            manifest,
            temporary,
            queuing,
            cancelled,
            verified_sequences: HashSet::new(),
            committed: false,
            inference_work: self.inference_work.clone(),
            content_store: Arc::clone(&self.content_store),
            queue_capacity: Arc::clone(&self.queue_capacity),
            capacity_reservation: Some(capacity_reservation),
            newly_published_bytes: 0,
            capacity_committed: false,
        };
        for descriptor in &staging.manifest.inputs {
            let input_path = staging
                .temporary
                .join(format!("input-{}", descriptor.sequence));
            let linked = match store.link_cached_input(descriptor, &input_path) {
                Ok(linked) => linked,
                Err(error) => {
                    drop(store);
                    return Err(error);
                }
            };
            if linked {
                staging.verified_sequences.insert(descriptor.sequence);
            }
        }
        drop(store);
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
        let mut staging = match self.begin_admission(manifest)? {
            QueueAdmission::Staging(staging) => staging,
            QueueAdmission::Cancelled | QueueAdmission::Duplicate => return Ok(()),
            QueueAdmission::Deferred(status) => {
                return Err(ServiceError::Internal(format!(
                    "LLM queue capacity is temporarily unavailable: required={}, available={}",
                    status.required_bytes, status.available_bytes
                )));
            }
            QueueAdmission::JobTooLarge(status) => {
                return Err(ServiceError::BadRequest(format!(
                    "LLM job requires {} bytes but queue capacity is {} bytes",
                    status.required_bytes, status.max_queue_bytes
                )));
            }
        };
        let required_sequences = staging
            .required_sequences()
            .into_iter()
            .collect::<HashSet<_>>();
        for (descriptor, bytes) in inputs {
            if required_sequences.contains(&descriptor.sequence) {
                staging.write_input(&descriptor, &bytes)?;
            }
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
                let manifest = self.read_manifest(&staging)?;
                lock_content_store(&self.content_store)?
                    .remove_job_directory(&staging, &manifest.inputs)?;
                if !staging.exists() {
                    removed = true;
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
        let manifest = self.read_manifest(&source)?;
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
        self.remove_committed_job_directory(&deleting, &manifest.inputs)?;
        Ok(true)
    }

    fn remove_committed_job_directory(
        &self,
        job_directory: &Path,
        inputs: &[JobInputDescriptor],
    ) -> Result<(), ServiceError> {
        let released_bytes =
            lock_content_store(&self.content_store)?.remove_job_directory(job_directory, inputs)?;
        if let Err(error) = self.queue_capacity.release_content(released_bytes) {
            self.queue_capacity.reconcile()?;
            return Err(error);
        }
        Ok(())
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
    Deferred(QueueCapacityStatus),
    JobTooLarge(QueueCapacityStatus),
    Staging(Box<QueueStaging>),
}

pub struct QueueStaging {
    manifest: QueueManifest,
    temporary: PathBuf,
    queuing: PathBuf,
    cancelled: PathBuf,
    verified_sequences: HashSet<u32>,
    committed: bool,
    inference_work: WorkSignal,
    content_store: Arc<StdMutex<ContentStore>>,
    queue_capacity: Arc<QueueCapacityManager>,
    capacity_reservation: Option<QueueCapacityReservation>,
    newly_published_bytes: u64,
    capacity_committed: bool,
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
        let input_path = self.input_path(descriptor)?;
        let published =
            lock_content_store(&self.content_store)?.publish_input(descriptor, &input_path)?;
        if published {
            self.newly_published_bytes = self
                .newly_published_bytes
                .checked_add(descriptor.byte_size)
                .ok_or_else(|| {
                    ServiceError::Internal("published input byte count overflowed".to_string())
                })?;
        }
        Ok(())
    }

    pub fn required_sequences(&self) -> Vec<u32> {
        self.manifest
            .inputs
            .iter()
            .filter(|descriptor| !self.verified_sequences.contains(&descriptor.sequence))
            .map(|descriptor| descriptor.sequence)
            .collect()
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
            lock_content_store(&self.content_store)?
                .remove_job_directory(&self.temporary, &self.manifest.inputs)?;
            self.committed = true;
            return Ok(false);
        }
        let manifest_path = self.temporary.join("manifest.json");
        std::fs::File::open(&manifest_path)
            .map_err(io_error)?
            .sync_all()
            .map_err(io_error)?;
        sync_directory(&self.temporary)?;
        self.capacity_reservation
            .take()
            .ok_or_else(|| {
                ServiceError::Internal("queue capacity reservation is unavailable".to_string())
            })?
            .commit(self.newly_published_bytes)?;
        self.capacity_committed = true;
        let queue_path = self.queuing.clone();
        std::fs::rename(&self.temporary, queue_path).map_err(io_error)?;
        self.committed = true;
        let queue_directory = self
            .queuing
            .parent()
            .ok_or_else(|| ServiceError::Internal("queue job path has no parent".to_string()))?;
        sync_directory(queue_directory)?;
        self.inference_work.notify();
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
            || descriptor.byte_size > MAX_LLM_INPUT_BYTES
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
            if let Ok(store) = self.content_store.lock() {
                if let Ok(released_bytes) =
                    store.remove_job_directory(&self.temporary, &self.manifest.inputs)
                {
                    if self.capacity_committed {
                        let _ = self.queue_capacity.release_content(released_bytes);
                    }
                }
            }
        }
    }
}

fn lock_content_store(
    content_store: &StdMutex<ContentStore>,
) -> Result<StdMutexGuard<'_, ContentStore>, ServiceError> {
    content_store
        .lock()
        .map_err(|_| ServiceError::Internal("content store lock is poisoned".to_string()))
}

impl Scheduler {
    pub async fn run(self: Arc<Self>) {
        tokio::join!(
            Arc::clone(&self).run_inference_loop(),
            self.run_result_delivery_loop()
        );
    }

    async fn run_inference_loop(self: Arc<Self>) {
        let idle_shutdown = Duration::from_secs(self.configuration.idle_shutdown_seconds);
        let mut observed_version = self.inference_work.version();
        let mut idle_deadline = None;
        loop {
            match self.process_cycle().await {
                Ok(true) => {
                    idle_deadline = None;
                    continue;
                }
                Ok(false) => {}
                Err(error) => {
                    warn!("LLM inference queue read failed: {error}");
                    tokio::select! {
                        version = self.inference_work.wait_for_change(observed_version) => {
                            observed_version = version;
                        }
                        () = tokio::time::sleep(WORKER_ERROR_RETRY_DELAY) => {}
                    }
                    idle_deadline = None;
                    continue;
                }
            }
            let current_version = self.inference_work.version();
            if current_version != observed_version {
                observed_version = current_version;
                continue;
            }
            match idle_deadline {
                Some(deadline) => {
                    tokio::select! {
                        version = self.inference_work.wait_for_change(observed_version) => {
                            observed_version = version;
                            idle_deadline = None;
                        }
                        () = tokio::time::sleep_until(deadline) => {
                            match self.manager.lock().await.shutdown().await {
                                Ok(()) => {
                                    observed_version = self
                                        .inference_work
                                        .wait_for_change(observed_version)
                                        .await;
                                    idle_deadline = None;
                                }
                                Err(error) => {
                                    warn!("failed to stop idle LLM runtime: {error}");
                                    tokio::select! {
                                        version = self.inference_work.wait_for_change(observed_version) => {
                                            observed_version = version;
                                            idle_deadline = None;
                                        }
                                        () = tokio::time::sleep(WORKER_ERROR_RETRY_DELAY) => {
                                            idle_deadline = Some(tokio::time::Instant::now());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                None => {
                    idle_deadline = Some(tokio::time::Instant::now() + idle_shutdown);
                }
            }
        }
    }

    async fn run_result_delivery_loop(self: Arc<Self>) {
        let mut observed_version = self.result_delivery_work.version();
        loop {
            match self.deliver_pending_results().await {
                Ok(delivered) if delivered > 0 => continue,
                Ok(_) => {}
                Err(error) => {
                    warn!("LLM result delivery queue read failed: {error}");
                    tokio::select! {
                        version = self.result_delivery_work.wait_for_change(observed_version) => {
                            observed_version = version;
                        }
                        () = tokio::time::sleep(WORKER_ERROR_RETRY_DELAY) => {}
                    }
                    continue;
                }
            }
            let retry_delay = match self.next_result_delivery_delay() {
                Ok(delay) => delay,
                Err(error) => {
                    warn!("failed to read the next LLM result delivery deadline: {error}");
                    Some(WORKER_ERROR_RETRY_DELAY)
                }
            };
            let current_version = self.result_delivery_work.version();
            if current_version != observed_version {
                observed_version = current_version;
                continue;
            }
            match retry_delay {
                Some(delay) => tokio::select! {
                    version = self.result_delivery_work.wait_for_change(observed_version) => {
                        observed_version = version;
                    }
                    () = tokio::time::sleep(delay) => {}
                },
                None => {
                    observed_version = self
                        .result_delivery_work
                        .wait_for_change(observed_version)
                        .await;
                }
            }
        }
    }

    pub fn wake_result_delivery(&self) {
        self.result_delivery_work.notify();
    }

    async fn process_cycle(&self) -> Result<bool, ServiceError> {
        let active_task = self.manager.lock().await.active_task();
        let Some(task) = self.try_select_queued_task(active_task)? else {
            return Ok(false);
        };
        let max_in_flight = NonZeroUsize::new(self.configuration.max_in_flight_jobs)
            .expect("validated scheduler inference window");
        let initial = self.claim_queued_jobs(&task, max_in_flight.get())?;
        if initial.is_empty() {
            return Ok(false);
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
                return Ok(true);
            }
        };
        let runtime_unavailable = Arc::new(AtomicBool::new(false));
        let mut initial = Some(initial);
        run_rolling_window(
            max_in_flight,
            |capacity| {
                initial
                    .take()
                    .map_or_else(|| self.claim_queued_jobs(&task, capacity), Ok)
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
        .await?;
        if runtime_unavailable.load(Ordering::Relaxed) {
            if let Err(error) = self.manager.lock().await.shutdown().await {
                warn!("failed to stop unavailable model runtime: {error}");
            }
        }
        Ok(true)
    }

    pub fn select_queued_jobs(&self, active_task: Option<&str>) -> Vec<(PathBuf, QueueManifest)> {
        let Ok(Some(task)) = self.try_select_queued_task(active_task) else {
            return Vec::new();
        };
        self.queued_jobs(&task, self.configuration.max_in_flight_jobs)
            .unwrap_or_default()
    }

    fn queued_jobs(
        &self,
        task: &str,
        limit: usize,
    ) -> Result<Vec<(PathBuf, QueueManifest)>, ServiceError> {
        let entries = std::fs::read_dir(self.queue_dir.join("queuing")).map_err(io_error)?;
        let mut selected = Vec::with_capacity(limit);
        for entry in entries {
            let entry = entry.map_err(io_error)?;
            let queue_path = entry.path();
            let manifest = match self.read_manifest(&queue_path) {
                Ok(manifest) => manifest,
                Err(error) => {
                    self.fail(queue_path, format!("invalid queued manifest: {error}"));
                    continue;
                }
            };
            if manifest.task != task || !is_valid_job_id(&manifest.job_id) {
                continue;
            }
            insert_queued_job(&mut selected, (queue_path, manifest), limit);
        }
        Ok(selected)
    }

    fn claim_queued_jobs(&self, task: &str, limit: usize) -> Result<Vec<ClaimedJob>, ServiceError> {
        let mut claimed = Vec::with_capacity(limit);
        for (queue_path, manifest) in self.queued_jobs(task, limit)? {
            if self
                .cancellation_marker(&manifest.client_id, &manifest.job_id)
                .exists()
            {
                self.remove_cancelled_job("queuing", &manifest.job_id)?;
                continue;
            }
            let processing_path = self.queue_dir.join("processing").join(&manifest.job_id);
            match std::fs::rename(&queue_path, &processing_path) {
                Ok(()) => claimed.push(ClaimedJob {
                    path: processing_path,
                    manifest,
                }),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(io_error(error)),
            }
        }
        Ok(claimed)
    }

    async fn execute_claimed_job(
        &self,
        dispatcher: InferenceDispatcher,
        job: ClaimedJob,
    ) -> JobExecution {
        let started = Instant::now();
        let inputs = match self.load_inputs(&job.path, &job.manifest).await {
            Ok(inputs) => inputs,
            Err(LoadInputsError::InvalidQueue(error)) => {
                return JobExecution::Invalid { job, error };
            }
            Err(LoadInputsError::Inference(error)) => {
                return JobExecution::Inferred {
                    job,
                    result: Err(error),
                };
            }
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

    fn try_select_queued_task(
        &self,
        active_task: Option<&str>,
    ) -> Result<Option<String>, ServiceError> {
        let entries = std::fs::read_dir(self.queue_dir.join("queuing")).map_err(io_error)?;
        let mut first_job: Option<QueueManifest> = None;
        for entry in entries {
            let entry = entry.map_err(io_error)?;
            let queue_path = entry.path();
            let manifest = match self.read_manifest(&queue_path) {
                Ok(manifest) => manifest,
                Err(error) => {
                    self.fail(queue_path, format!("invalid queued manifest: {error}"));
                    continue;
                }
            };
            if !is_valid_job_id(&manifest.job_id) || ServiceType::from_task(&manifest.task).is_err()
            {
                self.fail(
                    queue_path,
                    "queued manifest has an invalid job ID or task".to_string(),
                );
                continue;
            }
            if active_task.is_some_and(|task| task == manifest.task) {
                return Ok(Some(manifest.task));
            }
            if first_job
                .as_ref()
                .is_none_or(|first| manifest.job_id < first.job_id)
            {
                first_job = Some(manifest);
            }
        }
        Ok(first_job.map(|manifest| manifest.task))
    }

    async fn load_inputs(
        &self,
        job_path: &Path,
        manifest: &QueueManifest,
    ) -> Result<Vec<InferenceInput>, LoadInputsError> {
        let job_metadata = tokio::fs::symlink_metadata(job_path)
            .await
            .map_err(|error| LoadInputsError::InvalidQueue(error.to_string()))?;
        if !job_metadata.file_type().is_dir() {
            return Err(LoadInputsError::InvalidQueue(
                "processing job is not a directory".to_string(),
            ));
        }
        let mut inputs = Vec::with_capacity(manifest.inputs.len());
        for descriptor in &manifest.inputs {
            let path = job_path.join(format!("input-{}", descriptor.sequence));
            let metadata = tokio::fs::symlink_metadata(&path)
                .await
                .map_err(|error| LoadInputsError::InvalidQueue(error.to_string()))?;
            if !metadata.file_type().is_file() {
                return Err(LoadInputsError::InvalidQueue(
                    "queued input is not a regular file".to_string(),
                ));
            }
            if metadata.len() != descriptor.byte_size {
                return Err(LoadInputsError::InvalidQueue(
                    "queued input size does not match manifest".to_string(),
                ));
            }
            let mut file = tokio::fs::File::open(&path)
                .await
                .map_err(|error| LoadInputsError::InvalidQueue(error.to_string()))?;
            let mut hasher = Sha256::new();
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let read = file
                    .read(&mut buffer)
                    .await
                    .map_err(|error| LoadInputsError::InvalidQueue(error.to_string()))?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            if format!("{:x}", hasher.finalize()) != descriptor.content_hash {
                return Err(LoadInputsError::InvalidQueue(
                    "queued input hash does not match manifest".to_string(),
                ));
            }
            let (runtime_path, runtime_byte_size, runtime_content_hash, runtime_mime_type) =
                if requires_raw_normalization(&descriptor.mime_type) {
                    let normalized = self
                        .prepare_normalized_input(job_path, descriptor)
                        .await
                        .map_err(LoadInputsError::Inference)?;
                    (
                        runtime_input_path(job_path, descriptor.sequence, true),
                        normalized.byte_size,
                        normalized.content_hash,
                        "image/tiff".to_string(),
                    )
                } else {
                    (
                        path,
                        descriptor.byte_size,
                        descriptor.content_hash.clone(),
                        descriptor.mime_type.clone(),
                    )
                };
            inputs.push(InferenceInput {
                job_id: manifest.job_id.clone(),
                sequence: descriptor.sequence,
                frame_timestamp_ms: descriptor.frame_timestamp_ms,
                path: runtime_path,
                byte_size: runtime_byte_size,
                content_hash: runtime_content_hash,
                mime_type: runtime_mime_type,
                filename: descriptor.filename.clone(),
            });
        }
        Ok(inputs)
    }

    async fn prepare_normalized_input(
        &self,
        job_path: &Path,
        descriptor: &JobInputDescriptor,
    ) -> Result<crate::input_normalizer::NormalizedInputDescriptor, ServiceError> {
        let _normalization_guard = self.normalization_lock.lock().await;
        let normalized_path =
            lock_content_store(&self.content_store)?.normalized_path(&descriptor.content_hash);
        let source_path = job_path.join(format!("input-{}", descriptor.sequence));
        let job_id = job_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                ServiceError::Internal("processing job path has no valid job ID".to_string())
            })?;
        let normalized = match ensure_raw_normalized(
            &source_path,
            &normalized_path,
            job_id,
            descriptor.sequence,
        )
        .await
        {
            Ok(normalized) => normalized,
            Err(error) => {
                warn!(
                    job_id,
                    sequence = descriptor.sequence,
                    mime_type = descriptor.mime_type,
                    error = %error,
                    "RAW input normalization failed"
                );
                return Err(ServiceError::Upstream(error));
            }
        };
        let runtime_path = runtime_input_path(job_path, descriptor.sequence, true);
        lock_content_store(&self.content_store)?
            .link_normalized_input(&descriptor.content_hash, &runtime_path)?;
        Ok(normalized)
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
            self.remove_finished_cancelled_job(&job_path, &manifest);
            return;
        }
        let output = match inference {
            Ok(input_responses) => match encode_completed_result(
                &manifest.job_id,
                manifest.media_id,
                &manifest.task,
                manifest.attempt,
                &manifest.inputs,
                input_responses,
            ) {
                Ok(output) => output,
                Err(error) => match encode_failed_result(
                    &manifest.job_id,
                    manifest.media_id,
                    &manifest.task,
                    manifest.attempt,
                    &manifest.inputs,
                    format!("inference result was invalid: {error}"),
                ) {
                    Ok(output) => output,
                    Err(error) => {
                        self.fail(job_path, error);
                        return;
                    }
                },
            },
            Err(ServiceError::RuntimeUnavailable(error)) => {
                match self.requeue_runtime_failure(&job_path, &manifest, &error) {
                    Ok(true) => return,
                    Ok(false) => match encode_failed_result(
                        &manifest.job_id,
                        manifest.media_id,
                        &manifest.task,
                        manifest.attempt,
                        &manifest.inputs,
                        format!(
                            "local model runtime remained unavailable after {} attempts: {error}",
                            self.configuration.runtime_max_attempts
                        ),
                    ) {
                        Ok(output) => output,
                        Err(error) => {
                            self.fail(job_path, error);
                            return;
                        }
                    },
                    Err(requeue_error) => match encode_failed_result(
                        &manifest.job_id,
                        manifest.media_id,
                        &manifest.task,
                        manifest.attempt,
                        &manifest.inputs,
                        requeue_error.to_string(),
                    ) {
                        Ok(output) => output,
                        Err(error) => {
                            self.fail(job_path, error);
                            return;
                        }
                    },
                }
            }
            Err(error) => match encode_failed_result(
                &manifest.job_id,
                manifest.media_id,
                &manifest.task,
                manifest.attempt,
                &manifest.inputs,
                error.to_string(),
            ) {
                Ok(output) => output,
                Err(error) => {
                    self.fail(job_path, error);
                    return;
                }
            },
        };
        if let Err(error) = publish_durable_result(&job_path, &output) {
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
                } else {
                    self.result_delivery_work.notify();
                }
            }
            Err(error) => self.fail(
                job_path,
                format!("failed to transition callback pending: {error}"),
            ),
        }
    }

    fn remove_finished_cancelled_job(&self, job_path: &Path, manifest: &QueueManifest) {
        if let Err(error) = self.remove_committed_job_directory(job_path, &manifest.inputs) {
            warn!(
                job_path = %job_path.display(),
                error = %error,
                "failed to remove a finished cancelled job"
            );
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
        self.inference_work.notify();
        Ok(true)
    }

    async fn deliver_pending_results(&self) -> Result<usize, ServiceError> {
        if self.configuration.result_delivery_max_concurrent_deliveries == 0 {
            return Ok(0);
        }
        let selection_limit = self
            .configuration
            .result_delivery_max_concurrent_deliveries
            .saturating_mul(16);
        let mut pending = VecDeque::from(self.select_due_results(selection_limit).await?);
        if pending.is_empty() {
            return Ok(0);
        }
        run_rolling_window(
            NonZeroUsize::new(self.configuration.result_delivery_max_concurrent_deliveries)
                .expect("validated result delivery window"),
            |capacity| {
                Ok::<_, ServiceError>((0..capacity).filter_map(|_| pending.pop_front()).collect())
            },
            |path| async move {
                self.deliver_result(path.clone()).await;
                path
            },
            |_path: PathBuf| async { RollingWindowControl::Continue },
        )
        .await
    }

    async fn select_due_results(&self, limit: usize) -> Result<Vec<PathBuf>, ServiceError> {
        let entries =
            std::fs::read_dir(self.queue_dir.join("callback_pending")).map_err(io_error)?;
        let mut candidates = Vec::new();
        for entry in entries {
            let entry = entry.map_err(io_error)?;
            let path = entry.path();
            let Some(priority) = self.result_delivery_priority(&path) else {
                continue;
            };
            candidates.push((priority, path));
        }
        candidates.sort_unstable_by(|left, right| left.0.cmp(&right.0));

        let mut availability = HashMap::new();
        let mut selected = Vec::with_capacity(limit);
        for (_, path) in candidates {
            if selected.len() == limit {
                break;
            }
            if let Ok(manifest) = self.read_manifest(&path) {
                let connected = if let Some(connected) = availability.get(&manifest.client_id) {
                    *connected
                } else {
                    let connected = self
                        .result_delivery
                        .client_is_connected(&manifest.client_id)
                        .await;
                    availability.insert(manifest.client_id, connected);
                    connected
                };
                if !connected {
                    continue;
                }
            }
            selected.push(path);
        }
        Ok(selected)
    }

    fn next_result_delivery_delay(&self) -> Result<Option<Duration>, ServiceError> {
        let entries =
            std::fs::read_dir(self.queue_dir.join("callback_pending")).map_err(io_error)?;
        let now = chrono::Utc::now().timestamp();
        let mut next_attempt_at = None;
        for entry in entries {
            let entry = entry.map_err(io_error)?;
            let Ok(bytes) = std::fs::read(entry.path().join("callback.json")) else {
                continue;
            };
            let Ok(state) = serde_json::from_slice::<ResultDeliveryState>(&bytes) else {
                continue;
            };
            if state.next_attempt_at > now {
                next_attempt_at = Some(
                    next_attempt_at.map_or(state.next_attempt_at, |current: i64| {
                        current.min(state.next_attempt_at)
                    }),
                );
            }
        }
        let Some(next_attempt_at) = next_attempt_at else {
            return Ok(None);
        };
        Ok(Some(Duration::from_secs(
            u64::try_from(next_attempt_at - now).unwrap_or(1).max(1),
        )))
    }

    async fn deliver_result(&self, path: PathBuf) {
        let Ok(manifest) = self.read_manifest(&path) else {
            self.fail(path, "invalid manifest".to_string());
            return;
        };
        let Ok(result_manifest) = read_result_manifest(&path) else {
            self.fail(path, "missing inference result".to_string());
            return;
        };
        if result_manifest.job_id != manifest.job_id
            || result_manifest.media_id != manifest.media_id
            || result_manifest.task != manifest.task
            || result_manifest.attempt != manifest.attempt
        {
            self.fail(path, "inference result correlation is invalid".to_string());
            return;
        }
        match self
            .result_delivery
            .deliver_result(
                &manifest.client_id,
                &result_manifest,
                &path.join(RESULT_RECORDS_FILE),
                Duration::from_secs(
                    self.configuration
                        .result_delivery_acknowledgement_timeout_seconds,
                ),
            )
            .await
        {
            Ok(ResultDeliveryOutcome::Received) => {
                let deleting_path = self.queue_dir.join(".deleting").join(
                    path.file_name()
                        .unwrap_or_else(|| std::ffi::OsStr::new("invalid-job")),
                );
                if let Err(error) = transition_directory(&path, &deleting_path) {
                    self.fail(
                        path,
                        format!(
                            "result received by Momento but cleanup transition failed: {error}"
                        ),
                    );
                    return;
                }
                if let Err(error) =
                    self.remove_committed_job_directory(&deleting_path, &manifest.inputs)
                {
                    warn!("result received by Momento but queue cleanup will resume at startup: {error}");
                }
            }
            Ok(ResultDeliveryOutcome::Deferred { retry_after_ms }) => {
                if let Err(error) = self.defer_result_delivery(&path, retry_after_ms) {
                    self.fail(path, error.to_string());
                }
            }
            Ok(ResultDeliveryOutcome::Rejected { error }) => {
                self.fail(path, format!("Momento rejected inference result: {error}"));
            }
            Err(ResultDeliveryError::ClientUnavailable { .. }) => {}
            Err(delivery_error @ ResultDeliveryError::AttemptFailed { .. }) => {
                self.log_result_delivery_failure(&manifest, delivery_error.message());
                if let Err(error) =
                    self.record_result_delivery_failure(&path, delivery_error.message())
                {
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

    fn defer_result_delivery(
        &self,
        job_path: &Path,
        retry_after_ms: u64,
    ) -> Result<(), ServiceError> {
        let metadata_path = job_path.join("callback.json");
        let mut state = std::fs::read(&metadata_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<ResultDeliveryState>(&bytes).ok())
            .unwrap_or_default();
        let retry_seconds = retry_after_ms.div_ceil(1_000).max(1);
        let retry_seconds = i64::try_from(retry_seconds)
            .map_err(|_| ServiceError::BadRequest("result retry delay is too large".to_string()))?;
        state.next_attempt_at = chrono::Utc::now()
            .timestamp()
            .checked_add(retry_seconds)
            .ok_or_else(|| ServiceError::BadRequest("result retry time overflowed".to_string()))?;
        state.last_error = Some("Momento deferred durable result receipt".to_string());
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
            let manifest = self.read_manifest(&path).ok();
            if let Some(manifest) = manifest {
                let _ = self.remove_committed_job_directory(&path, &manifest.inputs);
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
        let source = entry.path();
        let destination = if durable_result_is_complete(&source) {
            remove_result_temporaries(&source)?;
            queue_dir.join("callback_pending").join(entry.file_name())
        } else {
            remove_partial_result(&source)?;
            queue_dir.join("queuing").join(entry.file_name())
        };
        if !destination.exists() {
            transition_directory(&source, &destination)?;
        }
    }
    for entry in std::fs::read_dir(queue_dir.join("callback_pending")).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        if durable_result_is_complete(&entry.path()) {
            remove_result_temporaries(&entry.path())?;
            continue;
        }
        remove_partial_result(&entry.path())?;
        let destination = queue_dir.join("queuing").join(entry.file_name());
        if !destination.exists() {
            transition_directory(&entry.path(), &destination)?;
        }
    }
    Ok(())
}

fn read_result_manifest(job_path: &Path) -> Result<ResultManifest, ServiceError> {
    let manifest = serde_json::from_slice::<ResultManifest>(
        &std::fs::read(job_path.join(RESULT_MANIFEST_FILE)).map_err(io_error)?,
    )
    .map_err(|error| ServiceError::BadRequest(error.to_string()))?;
    manifest.validate().map_err(ServiceError::BadRequest)?;
    Ok(manifest)
}

fn publish_durable_result(
    job_path: &Path,
    output: &DurableResultOutput,
) -> Result<(), ServiceError> {
    let records_temporary = job_path.join(RESULT_RECORDS_TEMPORARY_FILE);
    write_synced_file(&records_temporary, &output.records)?;
    std::fs::rename(&records_temporary, job_path.join(RESULT_RECORDS_FILE)).map_err(io_error)?;
    sync_directory(job_path)?;

    let manifest_bytes = serde_json::to_vec(&output.manifest)
        .map_err(|error| ServiceError::Internal(error.to_string()))?;
    let manifest_temporary = job_path.join(RESULT_MANIFEST_TEMPORARY_FILE);
    write_synced_file(&manifest_temporary, &manifest_bytes)?;
    std::fs::rename(&manifest_temporary, job_path.join(RESULT_MANIFEST_FILE)).map_err(io_error)?;
    sync_directory(job_path)
}

fn durable_result_is_complete(job_path: &Path) -> bool {
    let queue_manifest = std::fs::read(job_path.join("manifest.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<QueueManifest>(&bytes).ok());
    queue_manifest.is_some_and(|queue| validate_durable_result(job_path, &queue).is_ok())
}

fn validate_durable_result(job_path: &Path, queue: &QueueManifest) -> Result<(), ServiceError> {
    let manifest = read_result_manifest(job_path)?;
    if manifest.job_id != queue.job_id
        || manifest.media_id != queue.media_id
        || manifest.task != queue.task
        || manifest.attempt != queue.attempt
    {
        return Err(ServiceError::BadRequest(
            "durable result correlation is invalid".to_string(),
        ));
    }
    let correlations = queue
        .inputs
        .iter()
        .map(|input| ResultInputCorrelation {
            sequence: input.sequence,
            frame_timestamp_ms: input.frame_timestamp_ms,
        })
        .collect::<Vec<_>>();
    let mut validator = ResultRecordStreamValidator::new(
        &manifest.task,
        manifest.status,
        &correlations,
        manifest.record_count,
        manifest.byte_size,
    )
    .map_err(ServiceError::BadRequest)?;
    let mut decoder = ResultRecordChunkDecoder::new();
    let mut file = std::fs::File::open(job_path.join(RESULT_RECORDS_FILE)).map_err(io_error)?;
    if file.metadata().map_err(io_error)?.len() != manifest.byte_size {
        return Err(ServiceError::BadRequest(
            "durable result file size does not match its manifest".to_string(),
        ));
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; MAX_BINARY_CHUNK_BYTES];
    loop {
        let read = file.read(&mut buffer).map_err(io_error)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        decoder
            .push(&buffer[..read], |record| {
                validator.push(record.as_borrowed()).map(|_| ())
            })
            .map_err(ServiceError::BadRequest)?;
    }
    decoder.finish().map_err(ServiceError::BadRequest)?;
    validator.finish().map_err(ServiceError::BadRequest)?;
    if format!("{:x}", hasher.finalize()) != manifest.content_hash {
        return Err(ServiceError::BadRequest(
            "durable result file hash does not match its manifest".to_string(),
        ));
    }
    Ok(())
}

fn remove_partial_result(job_path: &Path) -> Result<(), ServiceError> {
    for name in [
        RESULT_MANIFEST_FILE,
        RESULT_MANIFEST_TEMPORARY_FILE,
        RESULT_RECORDS_FILE,
        RESULT_RECORDS_TEMPORARY_FILE,
        "callback.json",
    ] {
        match std::fs::remove_file(job_path.join(name)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(error)),
        }
    }
    sync_directory(job_path)
}

fn remove_result_temporaries(job_path: &Path) -> Result<(), ServiceError> {
    for name in [
        RESULT_MANIFEST_TEMPORARY_FILE,
        RESULT_RECORDS_TEMPORARY_FILE,
    ] {
        match std::fs::remove_file(job_path.join(name)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(error)),
        }
    }
    sync_directory(job_path)
}
