use std::collections::HashSet;
use std::future::Future;
use std::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use momento_common::work_signal::WorkSignal;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::response::{IntoResponse, Response};
use http_body_util::BodyExt;

use super::scheduler::SchedulerIngress;
use super::{DurableSourceId, RuntimeSizing, SchedulerAdmissionKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SchedulerState {
    Running = 0,
    Quiescing = 1,
    SecuringMutations = 2,
    Draining = 3,
    Stopped = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum SchedulerControlSource {
    CpuCapacity,
    FileIoCapacity,
    SqliteCapacity,
    AdmissionReleased,
    ConfigChanged,
    CancellationChanged,
    ShutdownRequested,
}

impl SchedulerControlSource {
    pub const ALL: [Self; 7] = [
        Self::CpuCapacity,
        Self::FileIoCapacity,
        Self::SqliteCapacity,
        Self::AdmissionReleased,
        Self::ConfigChanged,
        Self::CancellationChanged,
        Self::ShutdownRequested,
    ];
    pub const COUNT: usize = Self::ALL.len();

    const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone)]
pub struct SchedulerHandle {
    shared: Arc<SchedulerShared>,
}

struct SchedulerShared {
    state: AtomicU8,
    connections: AdmissionCounter,
    durable: AdmissionCounter,
    requests: AdmissionCounter,
    streams: AdmissionCounter,
    outbound_streams: AdmissionCounter,
    file_chunks: AdmissionCounter,
    durable_by_source: [AtomicUsize; DurableSourceId::COUNT],
    durable_by_kind: [AtomicUsize; SchedulerAdmissionKind::COUNT],
    active_claim_tokens: Mutex<HashSet<(DurableSourceId, String)>>,
    durable_claim_registry_capacity: usize,
    control_versions: [AtomicU64; SchedulerControlSource::COUNT],
    control_changed: Notify,
    backup_import_wake: Notify,
    webdav_import_wake: Notify,
    metadata_work: WorkSignal,
    llm_result_work: WorkSignal,
    ai_finalization_wake: Notify,
    journal_recovery_wake: Notify,
    scheduler_ingress: SchedulerIngress,
}

struct AdmissionCounter {
    active: AtomicUsize,
    maximum: usize,
    released: Notify,
}

pub struct DurableAdmission {
    counter: Arc<SchedulerShared>,
    pub source: DurableSourceId,
    pub kind: SchedulerAdmissionKind,
}

pub struct ActiveDurableClaim {
    counter: Arc<SchedulerShared>,
    source: DurableSourceId,
    token: Option<String>,
}

pub struct ConnectionAdmission {
    counter: Arc<SchedulerShared>,
}

pub struct RequestAdmission {
    counter: Option<Arc<SchedulerShared>>,
}

pub struct StreamSessionAdmission {
    counter: Arc<SchedulerShared>,
}

pub struct OutboundStreamAdmission {
    counter: Arc<SchedulerShared>,
}

pub struct FileChunkAdmission {
    counter: Arc<SchedulerShared>,
}

#[derive(Clone)]
pub struct HttpRequestAdmission {
    state: Arc<Mutex<Option<HttpAdmission>>>,
}

enum HttpAdmission {
    Request(RequestAdmission),
    Stream(StreamSessionAdmission),
}

impl SchedulerHandle {
    pub(crate) fn new(sizing: &RuntimeSizing, scheduler_ingress: SchedulerIngress) -> Self {
        Self {
            shared: Arc::new(SchedulerShared {
                state: AtomicU8::new(SchedulerState::Running as u8),
                connections: AdmissionCounter::new(sizing.active_connections),
                durable: AdmissionCounter::new(sizing.durable_orchestrations),
                requests: AdmissionCounter::new(sizing.active_requests),
                streams: AdmissionCounter::new(sizing.active_stream_sessions),
                outbound_streams: AdmissionCounter::new(sizing.active_outbound_stream_sessions),
                file_chunks: AdmissionCounter::new(sizing.active_file_chunks),
                durable_by_source: std::array::from_fn(|_| AtomicUsize::new(0)),
                durable_by_kind: std::array::from_fn(|_| AtomicUsize::new(0)),
                active_claim_tokens: Mutex::new(HashSet::new()),
                durable_claim_registry_capacity: sizing.durable_claim_registry_capacity,
                control_versions: std::array::from_fn(|_| AtomicU64::new(0)),
                control_changed: Notify::new(),
                backup_import_wake: Notify::new(),
                webdav_import_wake: Notify::new(),
                metadata_work: WorkSignal::default(),
                llm_result_work: WorkSignal::default(),
                ai_finalization_wake: Notify::new(),
                journal_recovery_wake: Notify::new(),
                scheduler_ingress,
            }),
        }
    }

    pub fn durable_capacity(&self) -> usize {
        self.shared.durable.maximum
    }

    pub fn outbound_stream_capacity(&self) -> usize {
        self.shared.outbound_streams.maximum
    }

    pub fn durable_claim_registry_capacity(&self) -> usize {
        self.shared.durable_claim_registry_capacity
    }

    pub async fn acquire_durable(
        &self,
        source: DurableSourceId,
        kind: SchedulerAdmissionKind,
    ) -> Result<DurableAdmission, String> {
        loop {
            let state = self.state();
            if state == SchedulerState::Stopped
                || (state != SchedulerState::Running && kind == SchedulerAdmissionKind::NewClaim)
            {
                return Err("scheduler is shutting down".to_string());
            }
            if self.shared.durable.try_acquire() {
                let current_state = self.state();
                if current_state == SchedulerState::Stopped
                    || (current_state != SchedulerState::Running
                        && kind == SchedulerAdmissionKind::NewClaim)
                {
                    self.shared.durable.release();
                    return Err("scheduler is shutting down".to_string());
                }
                self.shared.durable_by_source[source.index()].fetch_add(1, Ordering::AcqRel);
                self.shared.durable_by_kind[kind.index()].fetch_add(1, Ordering::AcqRel);
                return Ok(DurableAdmission {
                    counter: Arc::clone(&self.shared),
                    source,
                    kind,
                });
            }
            self.shared.durable.released.notified().await;
        }
    }

    pub async fn acquire_outbound_stream(&self) -> Result<OutboundStreamAdmission, String> {
        loop {
            if self.state() != SchedulerState::Running {
                return Err("scheduler is shutting down".to_string());
            }
            if self.shared.outbound_streams.try_acquire() {
                if self.state() != SchedulerState::Running {
                    self.shared.outbound_streams.release();
                    return Err("scheduler is shutting down".to_string());
                }
                return Ok(OutboundStreamAdmission {
                    counter: Arc::clone(&self.shared),
                });
            }
            self.shared.outbound_streams.released.notified().await;
        }
    }

    pub fn try_acquire_request(&self) -> Result<RequestAdmission, String> {
        if self.state() != SchedulerState::Running {
            return Err("scheduler is shutting down".to_string());
        }
        if !self.shared.requests.try_acquire() {
            return Err("request admission is at capacity".to_string());
        }
        Ok(RequestAdmission {
            counter: Some(Arc::clone(&self.shared)),
        })
    }

    pub fn try_acquire_connection(&self) -> Result<ConnectionAdmission, String> {
        if self.state() != SchedulerState::Running {
            return Err("scheduler is shutting down".to_string());
        }
        if !self.shared.connections.try_acquire() {
            return Err("connection admission is at capacity".to_string());
        }
        if self.state() != SchedulerState::Running {
            self.shared.connections.release();
            return Err("scheduler is shutting down".to_string());
        }
        Ok(ConnectionAdmission {
            counter: Arc::clone(&self.shared),
        })
    }

    pub fn connection_capacity(&self) -> usize {
        self.shared.connections.maximum
    }

    pub fn spawn_control<Task, Output>(&self, task: Task) -> JoinHandle<Output>
    where
        Task: Future<Output = Output> + Send + 'static,
        Output: Send + 'static,
    {
        tokio::spawn(task)
    }

    pub fn spawn_scheduler_control<Task>(&self, task: Task) -> Result<(), String>
    where
        Task: Future<Output = ()> + Send + 'static,
    {
        self.shared
            .scheduler_ingress
            .submit_scheduler_control(task)
            .map_err(|error| error.to_string())
    }

    pub fn spawn_durable<Task>(
        &self,
        source: DurableSourceId,
        kind: SchedulerAdmissionKind,
        task_name: &'static str,
        task: Task,
    ) -> JoinHandle<()>
    where
        Task: Future<Output = ()> + Send + 'static,
    {
        let scheduler = self.clone();
        self.spawn_control(async move {
            let admission = match scheduler.acquire_durable(source, kind).await {
                Ok(admission) => admission,
                Err(error) => {
                    tracing::error!(task = task_name, error, "durable work was rejected");
                    return;
                }
            };
            task.await;
            drop(admission);
        })
    }

    pub async fn execute_durable<Task, Output>(
        &self,
        source: DurableSourceId,
        kind: SchedulerAdmissionKind,
        task_name: &'static str,
        task: Task,
    ) -> Result<Output, String>
    where
        Task: Future<Output = Output> + Send + 'static,
        Output: Send + 'static,
    {
        let scheduler = self.clone();
        tokio::spawn(async move {
            let admission = scheduler.acquire_durable(source, kind).await?;
            let output = task.await;
            drop(admission);
            Ok::<Output, String>(output)
        })
        .await
        .map_err(|error| format!("durable task {task_name} panicked: {error}"))?
    }

    pub fn state(&self) -> SchedulerState {
        match self.shared.state.load(Ordering::Acquire) {
            0 => SchedulerState::Running,
            1 => SchedulerState::Quiescing,
            2 => SchedulerState::SecuringMutations,
            3 => SchedulerState::Draining,
            _ => SchedulerState::Stopped,
        }
    }

    pub fn active_durable_for(&self, source: DurableSourceId) -> usize {
        self.shared.durable_by_source[source.index()].load(Ordering::Acquire)
    }

    pub fn active_durable_kind(&self, kind: SchedulerAdmissionKind) -> usize {
        self.shared.durable_by_kind[kind.index()].load(Ordering::Acquire)
    }

    pub fn active_durable_total(&self) -> usize {
        self.shared.durable.active.load(Ordering::Acquire)
    }

    pub fn register_durable_claim(
        &self,
        admission: &DurableAdmission,
        token: String,
    ) -> Result<ActiveDurableClaim, String> {
        if !Arc::ptr_eq(&self.shared, &admission.counter) {
            return Err("durable admission belongs to another scheduler".to_string());
        }
        if token.is_empty() || token.len() > 128 {
            return Err("durable claim token must contain 1 to 128 bytes".to_string());
        }
        let mut active = self
            .shared
            .active_claim_tokens
            .lock()
            .map_err(|_| "durable claim registry is poisoned".to_string())?;
        if active.len() >= self.shared.durable_claim_registry_capacity {
            return Err("durable claim registry is at capacity".to_string());
        }
        if !active.insert((admission.source, token.clone())) {
            return Err("durable claim token is already active".to_string());
        }
        drop(active);
        Ok(ActiveDurableClaim {
            counter: Arc::clone(&self.shared),
            source: admission.source,
            token: Some(token),
        })
    }

    pub fn active_request_total(&self) -> usize {
        self.shared.requests.active.load(Ordering::Acquire)
    }

    pub fn active_outbound_stream_total(&self) -> usize {
        self.shared.outbound_streams.active.load(Ordering::Acquire)
    }

    pub fn active_connection_total(&self) -> usize {
        self.shared.connections.active.load(Ordering::Acquire)
    }

    pub fn active_stream_total(&self) -> usize {
        self.shared.streams.active.load(Ordering::Acquire)
    }

    pub fn stream_capacity(&self) -> usize {
        self.shared.streams.maximum
    }

    pub fn active_file_chunk_total(&self) -> usize {
        self.shared.file_chunks.active.load(Ordering::Acquire)
    }

    pub async fn acquire_file_chunk(&self) -> Result<FileChunkAdmission, String> {
        loop {
            if self.state() == SchedulerState::Stopped {
                return Err("scheduler is stopped".to_string());
            }
            if self.shared.file_chunks.try_acquire() {
                return Ok(FileChunkAdmission {
                    counter: Arc::clone(&self.shared),
                });
            }
            self.shared.file_chunks.released.notified().await;
        }
    }

    pub fn control_version(&self, source: SchedulerControlSource) -> u64 {
        self.shared.control_versions[source.index()].load(Ordering::Acquire)
    }

    pub fn signal_control(&self, source: SchedulerControlSource) -> u64 {
        let version = self.shared.control_versions[source.index()]
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1);
        self.shared.control_changed.notify_waiters();
        version
    }

    pub async fn wait_for_control_change(
        &self,
        source: SchedulerControlSource,
        observed_version: u64,
    ) -> u64 {
        loop {
            let notified = self.shared.control_changed.notified();
            let current_version = self.control_version(source);
            if current_version != observed_version {
                return current_version;
            }
            notified.await;
        }
    }

    pub fn transition_to(&self, next: SchedulerState) -> Result<(), String> {
        let current = self.state();
        let expected_next = match current {
            SchedulerState::Running => SchedulerState::Quiescing,
            SchedulerState::Quiescing => SchedulerState::SecuringMutations,
            SchedulerState::SecuringMutations => SchedulerState::Draining,
            SchedulerState::Draining => SchedulerState::Stopped,
            SchedulerState::Stopped => return Err("scheduler is already stopped".to_string()),
        };
        if next != expected_next {
            return Err(format!(
                "invalid scheduler transition from {current:?} to {next:?}"
            ));
        }
        self.shared
            .state
            .compare_exchange(
                current as u8,
                next as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map_err(|_| "scheduler state changed concurrently".to_string())?;
        self.signal_control(SchedulerControlSource::ShutdownRequested);
        self.wake_all();
        Ok(())
    }

    pub fn begin_quiescing(&self) -> Result<(), String> {
        match self.state() {
            SchedulerState::Running => self.transition_to(SchedulerState::Quiescing),
            SchedulerState::Quiescing => Ok(()),
            state => Err(format!("cannot begin quiescing from {state:?}")),
        }
    }

    pub async fn finish_shutdown(&self, grace: std::time::Duration) -> Result<(), String> {
        self.begin_quiescing()?;
        self.transition_to(SchedulerState::SecuringMutations)?;
        let deadline = tokio::time::Instant::now() + grace;
        self.wait_until_idle(deadline).await?;
        self.transition_to(SchedulerState::Draining)?;
        self.wait_until_idle(deadline).await?;
        self.transition_to(SchedulerState::Stopped)
    }

    async fn wait_until_idle(&self, deadline: tokio::time::Instant) -> Result<(), String> {
        loop {
            if self.active_connection_total() == 0
                && self.active_durable_total() == 0
                && self.active_request_total() == 0
                && self.active_stream_total() == 0
                && self.active_outbound_stream_total() == 0
                && self.active_file_chunk_total() == 0
            {
                return Ok(());
            }
            let observed = self.control_version(SchedulerControlSource::AdmissionReleased);
            if self.active_connection_total() == 0
                && self.active_durable_total() == 0
                && self.active_request_total() == 0
                && self.active_stream_total() == 0
                && self.active_outbound_stream_total() == 0
                && self.active_file_chunk_total() == 0
            {
                return Ok(());
            }
            if tokio::time::timeout_at(
                deadline,
                self.wait_for_control_change(SchedulerControlSource::AdmissionReleased, observed),
            )
            .await
            .is_err()
            {
                return Err(self.unresolved_admission_report());
            }
        }
    }

    fn unresolved_admission_report(&self) -> String {
        let mut sources = Vec::new();
        for source in DurableSourceId::ALL {
            let active = self.active_durable_for(source);
            if active > 0 {
                sources.push(format!("{source:?}={active}"));
            }
        }
        let mut claims = self
            .shared
            .active_claim_tokens
            .lock()
            .map(|claims| {
                claims
                    .iter()
                    .map(|(source, token)| format!("{source:?}:{token}"))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|_| vec!["registry-poisoned".to_string()]);
        claims.sort_unstable();
        format!(
            "shutdown grace expired with connections={} requests={} streams={} outbound_streams={} file_chunks={} durable={} sources=[{}] claims=[{}]",
            self.active_connection_total(),
            self.active_request_total(),
            self.active_stream_total(),
            self.active_outbound_stream_total(),
            self.active_file_chunk_total(),
            self.active_durable_total(),
            sources.join(", "),
            claims.join(", ")
        )
    }

    fn wake_all(&self) {
        self.shared.durable.released.notify_waiters();
        self.shared.connections.released.notify_waiters();
        self.shared.requests.released.notify_waiters();
        self.shared.streams.released.notify_waiters();
        self.shared.outbound_streams.released.notify_waiters();
        self.shared.file_chunks.released.notify_waiters();
        self.shared.backup_import_wake.notify_waiters();
        self.shared.webdav_import_wake.notify_waiters();
        self.shared.metadata_work.notify();
        self.shared.llm_result_work.notify();
        self.shared.ai_finalization_wake.notify_waiters();
        self.shared.journal_recovery_wake.notify_waiters();
    }

    pub fn wake_backup_import(&self) {
        self.shared.backup_import_wake.notify_one();
    }

    pub async fn backup_import_notified(&self) {
        self.shared.backup_import_wake.notified().await;
    }

    pub fn wake_webdav_import(&self) {
        self.shared.webdav_import_wake.notify_one();
    }

    pub async fn webdav_import_notified(&self) {
        self.shared.webdav_import_wake.notified().await;
    }

    pub fn wake_metadata(&self) {
        self.shared.metadata_work.notify();
    }

    pub fn metadata_work_version(&self) -> u64 {
        self.shared.metadata_work.version()
    }

    pub async fn wait_for_metadata_work(&self, observed_version: u64) -> u64 {
        self.shared
            .metadata_work
            .wait_for_change(observed_version)
            .await
    }

    pub fn wake_llm_results(&self) {
        self.shared.llm_result_work.notify();
    }

    pub fn llm_result_work_version(&self) -> u64 {
        self.shared.llm_result_work.version()
    }

    pub async fn wait_for_llm_result_work(&self, observed_version: u64) -> u64 {
        self.shared
            .llm_result_work
            .wait_for_change(observed_version)
            .await
    }

    pub fn wake_ai_finalization(&self) {
        self.shared.ai_finalization_wake.notify_waiters();
    }

    pub async fn ai_finalization_notified(&self) {
        self.shared.ai_finalization_wake.notified().await;
    }

    pub fn wake_journal_recovery(&self) {
        self.shared.journal_recovery_wake.notify_one();
    }

    pub async fn journal_recovery_notified(&self) {
        self.shared.journal_recovery_wake.notified().await;
    }
}

impl AdmissionCounter {
    fn new(maximum: usize) -> Self {
        Self {
            active: AtomicUsize::new(0),
            maximum,
            released: Notify::new(),
        }
    }

    fn try_acquire(&self) -> bool {
        self.active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < self.maximum).then_some(active + 1)
            })
            .is_ok()
    }

    fn release(&self) {
        let previous = self.active.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "admission counter underflow");
        self.released.notify_one();
    }
}

impl Drop for DurableAdmission {
    fn drop(&mut self) {
        let source_count =
            self.counter.durable_by_source[self.source.index()].fetch_sub(1, Ordering::AcqRel);
        let kind_count =
            self.counter.durable_by_kind[self.kind.index()].fetch_sub(1, Ordering::AcqRel);
        debug_assert!(source_count > 0, "durable source counter underflow");
        debug_assert!(kind_count > 0, "durable kind counter underflow");
        self.counter.durable.release();
        self.counter.control_versions[SchedulerControlSource::AdmissionReleased.index()]
            .fetch_add(1, Ordering::AcqRel);
        self.counter.control_changed.notify_waiters();
    }
}

impl Drop for ActiveDurableClaim {
    fn drop(&mut self) {
        let Some(token) = self.token.take() else {
            return;
        };
        let removed = self
            .counter
            .active_claim_tokens
            .lock()
            .expect("durable claim registry must not be poisoned")
            .remove(&(self.source, token));
        debug_assert!(removed, "durable claim token was not registered");
    }
}

impl Drop for ConnectionAdmission {
    fn drop(&mut self) {
        self.counter.connections.release();
        self.counter.control_versions[SchedulerControlSource::AdmissionReleased.index()]
            .fetch_add(1, Ordering::AcqRel);
        self.counter.control_changed.notify_waiters();
    }
}

impl Drop for RequestAdmission {
    fn drop(&mut self) {
        if let Some(counter) = self.counter.take() {
            release_admission(&counter, &counter.requests);
        }
    }
}

impl RequestAdmission {
    fn try_into_stream(mut self) -> Result<StreamSessionAdmission, Self> {
        let counter = self
            .counter
            .as_ref()
            .expect("request admission owns its scheduler");
        if counter.state.load(Ordering::Acquire) != SchedulerState::Running as u8
            || !counter.streams.try_acquire()
        {
            return Err(self);
        }
        let counter = self
            .counter
            .take()
            .expect("request admission owns its scheduler");
        release_admission(&counter, &counter.requests);
        Ok(StreamSessionAdmission { counter })
    }
}

impl Drop for StreamSessionAdmission {
    fn drop(&mut self) {
        release_admission(&self.counter, &self.counter.streams);
    }
}

impl Drop for OutboundStreamAdmission {
    fn drop(&mut self) {
        release_admission(&self.counter, &self.counter.outbound_streams);
    }
}

impl Drop for FileChunkAdmission {
    fn drop(&mut self) {
        release_admission(&self.counter, &self.counter.file_chunks);
    }
}

impl HttpRequestAdmission {
    fn new(admission: RequestAdmission) -> Self {
        Self {
            state: Arc::new(Mutex::new(Some(HttpAdmission::Request(admission)))),
        }
    }

    pub fn acquire(scheduler: &SchedulerHandle) -> Result<Self, String> {
        scheduler.try_acquire_request().map(Self::new)
    }

    pub fn scheduler(&self) -> Result<SchedulerHandle, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "HTTP admission state is poisoned".to_string())?;
        let counter = match state.as_ref() {
            Some(HttpAdmission::Request(admission)) => admission
                .counter
                .as_ref()
                .expect("request admission owns its scheduler"),
            Some(HttpAdmission::Stream(admission)) => &admission.counter,
            None => return Err("HTTP admission state is unavailable".to_string()),
        };
        Ok(SchedulerHandle {
            shared: Arc::clone(counter),
        })
    }

    pub fn convert_to_stream(&self) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "HTTP admission state is poisoned".to_string())?;
        let Some(admission) = state.take() else {
            return Err("HTTP admission state is unavailable".to_string());
        };
        match admission {
            HttpAdmission::Stream(admission) => {
                *state = Some(HttpAdmission::Stream(admission));
                Ok(())
            }
            HttpAdmission::Request(admission) => match admission.try_into_stream() {
                Ok(admission) => {
                    *state = Some(HttpAdmission::Stream(admission));
                    Ok(())
                }
                Err(admission) => {
                    *state = Some(HttpAdmission::Request(admission));
                    Err("stream-session admission is at capacity".to_string())
                }
            },
        }
    }
}

fn release_admission(counter: &SchedulerShared, admission: &AdmissionCounter) {
    admission.release();
    counter.control_versions[SchedulerControlSource::AdmissionReleased.index()]
        .fetch_add(1, Ordering::AcqRel);
    counter.control_changed.notify_waiters();
}

pub async fn schedule_client_request(
    State(scheduler): State<SchedulerHandle>,
    request: Request,
    next: axum::middleware::Next,
) -> Response {
    let admission = match HttpRequestAdmission::acquire(&scheduler) {
        Ok(admission) => admission,
        Err(_) => {
            let mut response = (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "Momento request capacity is unavailable",
            )
                .into_response();
            response.headers_mut().insert(
                axum::http::header::CONNECTION,
                axum::http::HeaderValue::from_static("close"),
            );
            return response;
        }
    };
    let mut request = request;
    request.extensions_mut().insert(admission.clone());
    guard_response_body(next.run(request).await, admission)
}

fn guard_response_body(response: Response, admission: HttpRequestAdmission) -> Response {
    let (parts, body) = response.into_parts();
    let guarded_body = body.map_frame(move |frame| {
        let _admission = &admission;
        frame
    });
    Response::from_parts(parts, Body::new(guarded_body))
}
