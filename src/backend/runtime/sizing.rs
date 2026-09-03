use std::fmt;

use crate::config::ThreadPoolConfig;

pub const MAX_CPU_WORKERS: usize = 256;
pub const MAX_IO_WORKERS: usize = 256;
pub const MAX_SQLITE_WORKERS: usize = 64;
pub const WORKER_STACK_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_DERIVED_RUNTIME_BYTES: u64 = 1024 * 1024 * 1024;

const EXECUTOR_QUEUE_OPERATIONS_PER_WORKER: u64 = 4;
const NETWORK_WORKERS: u64 = 2;
const R2D2_MAINTENANCE_THREADS: u64 = 1;
const DURABLE_SOURCE_COUNT: u64 = super::job::DurableSourceId::COUNT as u64;
const SCHEDULER_CONTROL_SOURCE_COUNT: u64 = super::control::SchedulerControlSource::COUNT as u64;
const CRON_TASK_COUNT: u64 = super::job::CronTaskId::COUNT as u64;
const EXECUTOR_COUNT: u64 = 3;
const LLM_TRANSPORT_INGRESS_CREDITS: u64 = 1;

const MEBIBYTE: u64 = 1024 * 1024;
const KIBIBYTE: u64 = 1024;
const HTTP1_READ_BUFFER_BYTES: u64 = 128 * KIBIBYTE;
const MAX_BUFFERED_HTTP_BODY_BYTES: u64 = MEBIBYTE;
const MAX_NORMALIZED_CONTROL_DTO_BYTES: u64 = MEBIBYTE;
const MAX_REQUEST_LOG_CAPTURE_BYTES: u64 = 48 * KIBIBYTE;
const MAX_BUFFERED_HTTP_RESPONSE_BYTES: u64 = 4 * MEBIBYTE;
const MAX_DB_OPERATION_OUTPUT_BYTES: u64 = MEBIBYTE;
const CONTROL_PARSE_SCRATCH_BYTES: u64 = MEBIBYTE;
pub(crate) const FILE_IO_CHUNK_BYTES: u64 = MEBIBYTE;
const IMAGE_TILE_BYTES: u64 = 16 * MEBIBYTE;
pub const ARGON2_WORKSPACE_BYTES: u64 = 19_456 * KIBIBYTE;
const MAX_LLM_RESULT_RECORD_BYTES: u64 = MEBIBYTE;
const MAX_NORMALIZED_RESULT_RECORD_BYTES: u64 = 2 * MEBIBYTE;
const MAX_LLM_RESULT_PERSIST_BATCH_BYTES: u64 = 4 * MEBIBYTE;
const RESULT_EXECUTOR_ENVELOPE_BYTES: u64 = 64 * KIBIBYTE;
const MAX_METADATA_SOURCE_BYTES: u64 = 4 * MEBIBYTE;
const MAX_NORMALIZED_MEDIA_METADATA_BYTES: u64 = MEBIBYTE;
const DURABLE_EXECUTOR_ENVELOPE_BYTES: u64 = 64 * KIBIBYTE;
const MAX_GEONAMES_COMPRESSED_BYTES: u64 = 4 * MEBIBYTE;
const MAX_GEONAMES_RUNTIME_BYTES: u64 = 48 * MEBIBYTE;
const MAX_GEONAMES_INIT_TEMP_BYTES: u64 = 4 * MEBIBYTE;
const SQLITE_PAGE_CACHE_BYTES: u64 = 8 * MEBIBYTE;
const SQLITE_STATEMENT_CACHE_BYTES: u64 = 2 * MEBIBYTE;
const MAX_SQLITE_OPERATION_TEMP_HEAP_BYTES: u64 = 8 * MEBIBYTE;
const LLM_STREAM_FRAME_BYTES: u64 = 64 * KIBIBYTE;
const MAX_MOMENTO_INBOUND_LLM_CONTROL_BYTES: u64 = 64 * KIBIBYTE;
const EXECUTOR_ENVELOPE_BYTES: u64 = 4 * KIBIBYTE;
const LOG_EVENT_BYTES: u64 = 64 * KIBIBYTE;
const LOG_EVENT_QUEUE_MULTIPLIER: u64 = 32;
const FILE_REGISTRY_ENTRY_BYTES: u64 = 256;
const JOURNAL_MUTATION_REGISTRY_ENTRY_BYTES: u64 = 512;
const DNS_OPERATION_STATE_BYTES: u64 = 64 * KIBIBYTE;
const FIXED_RUNTIME_INFRASTRUCTURE_BYTES: u64 = 8 * MEBIBYTE;

const STORAGE_ROOT_COUNT: u64 = 13;
const CONFIG_FILE_CAPABILITY_FDS: u64 = 1;
const CHILD_SUPERVISION_FD_PEAK: u64 = 8;
const PLATFORM_FIXED_FDS: u64 = 16;
const BOOTSTRAP_PEAK_FDS: u64 = 96;
const LOG_FILE_HANDLE_SLOTS: u64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeSizingBreakdown {
    pub thread_stacks: u64,
    pub connection_state: u64,
    pub request_state: u64,
    pub cpu_state: u64,
    pub durable_state: u64,
    pub stream_state: u64,
    pub sqlite_state: u64,
    pub logging_state: u64,
    pub llm_transport_state: u64,
    pub geocoder_state: u64,
    pub fixed_infrastructure: u64,
}

impl RuntimeSizingBreakdown {
    fn checked_total(self) -> Result<u64, RuntimeSizingError> {
        [
            self.thread_stacks,
            self.connection_state,
            self.request_state,
            self.cpu_state,
            self.durable_state,
            self.stream_state,
            self.sqlite_state,
            self.logging_state,
            self.llm_transport_state,
            self.geocoder_state,
            self.fixed_infrastructure,
        ]
        .into_iter()
        .try_fold(0_u64, checked_add)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSizing {
    pub cpu_workers: usize,
    pub io_workers: usize,
    pub sqlite_workers: usize,
    pub file_workers: usize,
    pub active_connections: usize,
    pub active_requests: usize,
    pub active_stream_sessions: usize,
    pub active_outbound_stream_sessions: usize,
    pub active_inbound_durable_streams: usize,
    pub active_file_chunks: usize,
    pub durable_orchestrations: usize,
    pub scheduler_ingress_capacity: usize,
    pub cpu_queue_capacity: usize,
    pub file_queue_capacity: usize,
    pub sqlite_queue_capacity: usize,
    pub log_event_capacity: usize,
    pub file_registry_capacity: usize,
    pub journal_mutation_registry_capacity: usize,
    pub required_open_files: u64,
    pub bootstrap_peak_bytes: u64,
    pub pre_listener_initialization_peak_bytes: u64,
    pub running_peak_bytes: u64,
    pub derived_runtime_bytes: u64,
    pub breakdown: RuntimeSizingBreakdown,
}

impl RuntimeSizing {
    pub fn validate_worker_counts(
        configuration: &ThreadPoolConfig,
    ) -> Result<Self, RuntimeSizingError> {
        validate_range("cpu_workers", configuration.cpu_workers, 1, MAX_CPU_WORKERS)?;
        validate_range("io_workers", configuration.io_workers, 4, MAX_IO_WORKERS)?;
        validate_range(
            "sqlite_workers",
            configuration.sqlite_workers,
            1,
            MAX_SQLITE_WORKERS,
        )?;
        Self::new(configuration)
    }

    pub fn new(configuration: &ThreadPoolConfig) -> Result<Self, RuntimeSizingError> {
        Self::calculate(configuration, true)
    }

    fn calculate(
        configuration: &ThreadPoolConfig,
        enforce_runtime_budget: bool,
    ) -> Result<Self, RuntimeSizingError> {
        let cpu_workers = widen(configuration.cpu_workers)?;
        let io_workers = widen(configuration.io_workers)?;
        let sqlite_workers = widen(configuration.sqlite_workers)?;
        let file_workers = checked_sub(io_workers, NETWORK_WORKERS)?;

        let active_connections = checked_mul(16, io_workers)?;
        let active_requests = checked_mul(8, io_workers)?;
        let active_stream_sessions = checked_mul(8, io_workers)?;
        let active_outbound_stream_sessions = checked_mul(2, file_workers)?;
        let active_inbound_durable_streams = checked_add(cpu_workers, sqlite_workers)?;
        let active_file_chunks = checked_mul(2, file_workers)?;
        let durable_orchestrations =
            checked_add(checked_add(cpu_workers, file_workers)?, sqlite_workers)?;

        let scheduler_ingress_capacity = [
            active_requests,
            active_stream_sessions,
            active_outbound_stream_sessions,
            active_inbound_durable_streams,
            checked_mul(EXECUTOR_COUNT, DURABLE_SOURCE_COUNT)?,
            SCHEDULER_CONTROL_SOURCE_COUNT,
            LLM_TRANSPORT_INGRESS_CREDITS,
            CRON_TASK_COUNT,
        ]
        .into_iter()
        .try_fold(0_u64, checked_add)?;

        let cpu_queue_capacity = checked_mul(cpu_workers, EXECUTOR_QUEUE_OPERATIONS_PER_WORKER)?;
        let file_queue_capacity = checked_mul(file_workers, EXECUTOR_QUEUE_OPERATIONS_PER_WORKER)?;
        let sqlite_queue_capacity =
            checked_mul(sqlite_workers, EXECUTOR_QUEUE_OPERATIONS_PER_WORKER)?;
        let log_event_capacity = checked_mul(LOG_EVENT_QUEUE_MULTIPLIER, io_workers)?;

        let file_registry_capacity = [
            active_stream_sessions,
            active_outbound_stream_sessions,
            active_inbound_durable_streams,
            checked_mul(2, cpu_workers)?,
            checked_mul(2, DURABLE_SOURCE_COUNT)?,
            file_workers,
            LOG_FILE_HANDLE_SLOTS,
        ]
        .into_iter()
        .try_fold(0_u64, checked_add)?;
        let journal_mutation_registry_capacity = checked_add(
            checked_add(
                durable_orchestrations,
                active_requests.max(active_stream_sessions),
            )?,
            active_inbound_durable_streams,
        )?;

        let runtime_thread_count = checked_add(
            checked_add(checked_add(1, io_workers)?, cpu_workers)?,
            checked_add(sqlite_workers, R2D2_MAINTENANCE_THREADS)?,
        )?;
        let thread_stacks = checked_mul(runtime_thread_count, WORKER_STACK_BYTES)?;

        let connection_state = checked_mul(active_connections, HTTP1_READ_BUFFER_BYTES)?;
        let request_parse_peak = [
            MAX_BUFFERED_HTTP_BODY_BYTES,
            CONTROL_PARSE_SCRATCH_BYTES,
            MAX_NORMALIZED_CONTROL_DTO_BYTES,
            MAX_REQUEST_LOG_CAPTURE_BYTES,
        ]
        .into_iter()
        .try_fold(0_u64, checked_add)?;
        let request_response_peak = [
            MAX_DB_OPERATION_OUTPUT_BYTES,
            MAX_BUFFERED_HTTP_RESPONSE_BYTES,
            MAX_REQUEST_LOG_CAPTURE_BYTES,
        ]
        .into_iter()
        .try_fold(0_u64, checked_add)?;
        let request_state = checked_mul(
            active_requests,
            request_parse_peak.max(request_response_peak),
        )?;

        let cpu_operation_peak = checked_mul(2, IMAGE_TILE_BYTES)?.max(ARGON2_WORKSPACE_BYTES);
        let cpu_state = checked_mul(cpu_workers, cpu_operation_peak)?;

        let result_peak = [
            MAX_LLM_RESULT_RECORD_BYTES,
            MAX_NORMALIZED_RESULT_RECORD_BYTES,
            MAX_LLM_RESULT_PERSIST_BATCH_BYTES,
            RESULT_EXECUTOR_ENVELOPE_BYTES,
        ]
        .into_iter()
        .try_fold(0_u64, checked_add)?;
        let metadata_peak = [
            MAX_METADATA_SOURCE_BYTES,
            MAX_NORMALIZED_MEDIA_METADATA_BYTES,
            DURABLE_EXECUTOR_ENVELOPE_BYTES,
        ]
        .into_iter()
        .try_fold(0_u64, checked_add)?;
        let durable_state = checked_mul(durable_orchestrations, result_peak.max(metadata_peak))?;

        let stream_buffers = checked_mul(
            checked_add(active_stream_sessions, active_outbound_stream_sessions)?,
            FILE_IO_CHUNK_BYTES,
        )?;
        let file_chunk_buffers = checked_mul(active_file_chunks, FILE_IO_CHUNK_BYTES)?;
        let registry_bytes = checked_add(
            checked_mul(file_registry_capacity, FILE_REGISTRY_ENTRY_BYTES)?,
            checked_mul(
                journal_mutation_registry_capacity,
                JOURNAL_MUTATION_REGISTRY_ENTRY_BYTES,
            )?,
        )?;
        let stream_state = [stream_buffers, file_chunk_buffers, registry_bytes]
            .into_iter()
            .try_fold(0_u64, checked_add)?;

        let sqlite_connection_bytes =
            checked_add(SQLITE_PAGE_CACHE_BYTES, SQLITE_STATEMENT_CACHE_BYTES)?;
        let sqlite_state = checked_add(
            checked_mul(checked_add(sqlite_workers, 1)?, sqlite_connection_bytes)?,
            checked_mul(sqlite_workers, MAX_SQLITE_OPERATION_TEMP_HEAP_BYTES)?,
        )?;

        let logging_state = checked_mul(log_event_capacity, LOG_EVENT_BYTES)?;
        let raw_control_slots = [
            active_outbound_stream_sessions,
            active_inbound_durable_streams,
            DURABLE_SOURCE_COUNT,
        ]
        .into_iter()
        .try_fold(0_u64, checked_add)?;
        let llm_transport_state = checked_add(
            checked_mul(raw_control_slots, MAX_MOMENTO_INBOUND_LLM_CONTROL_BYTES)?,
            checked_mul(active_inbound_durable_streams, LLM_STREAM_FRAME_BYTES)?,
        )?;
        let geocoder_state = [MAX_GEONAMES_COMPRESSED_BYTES, MAX_GEONAMES_RUNTIME_BYTES]
            .into_iter()
            .try_fold(0_u64, checked_add)?;
        let executor_envelopes = checked_mul(
            checked_add(
                checked_add(cpu_queue_capacity, file_queue_capacity)?,
                checked_add(sqlite_queue_capacity, scheduler_ingress_capacity)?,
            )?,
            EXECUTOR_ENVELOPE_BYTES,
        )?;
        let outbound_dns_state = checked_mul(
            checked_add(
                checked_add(active_requests, durable_orchestrations)?,
                active_outbound_stream_sessions,
            )?,
            DNS_OPERATION_STATE_BYTES,
        )?;
        let fixed_infrastructure = [
            FIXED_RUNTIME_INFRASTRUCTURE_BYTES,
            executor_envelopes,
            outbound_dns_state,
        ]
        .into_iter()
        .try_fold(0_u64, checked_add)?;

        let breakdown = RuntimeSizingBreakdown {
            thread_stacks,
            connection_state,
            request_state,
            cpu_state,
            durable_state,
            stream_state,
            sqlite_state,
            logging_state,
            llm_transport_state,
            geocoder_state,
            fixed_infrastructure,
        };
        let running_peak_bytes = breakdown.checked_total()?;
        let bootstrap_peak_bytes = checked_add(thread_stacks, FIXED_RUNTIME_INFRASTRUCTURE_BYTES)?;
        let pre_listener_initialization_peak_bytes = [
            thread_stacks,
            sqlite_state,
            cpu_operation_peak,
            geocoder_state,
            MAX_GEONAMES_INIT_TEMP_BYTES,
            fixed_infrastructure,
        ]
        .into_iter()
        .try_fold(0_u64, checked_add)?;
        let derived_runtime_bytes = bootstrap_peak_bytes
            .max(pre_listener_initialization_peak_bytes)
            .max(running_peak_bytes);
        if enforce_runtime_budget && derived_runtime_bytes > MAX_DERIVED_RUNTIME_BYTES {
            return Err(RuntimeSizingError::RuntimeBytesExceeded {
                required: derived_runtime_bytes,
                maximum: MAX_DERIVED_RUNTIME_BYTES,
                breakdown: Box::new(breakdown),
                feasible_cpu_workers: maximum_feasible_workers(configuration, WorkerField::Cpu)?,
                feasible_io_workers: maximum_feasible_workers(configuration, WorkerField::Io)?,
                feasible_sqlite_workers: maximum_feasible_workers(
                    configuration,
                    WorkerField::Sqlite,
                )?,
            });
        }

        let active_outbound_network_operations = checked_add(
            checked_add(active_requests, durable_orchestrations)?,
            active_outbound_stream_sessions,
        )?;
        let maximum_outbound_sockets =
            checked_add(checked_mul(2, active_outbound_network_operations)?, 1)?;
        let sqlite_descriptors = checked_mul(3, checked_add(sqlite_workers, 1)?)?;
        let child_descriptors = checked_mul(CHILD_SUPERVISION_FD_PEAK, cpu_workers)?;
        let runtime_required_open_files = [
            active_connections,
            maximum_outbound_sockets,
            1,
            file_registry_capacity,
            checked_add(STORAGE_ROOT_COUNT, 1)?,
            CONFIG_FILE_CAPABILITY_FDS,
            sqlite_descriptors,
            child_descriptors,
            PLATFORM_FIXED_FDS,
        ]
        .into_iter()
        .try_fold(0_u64, checked_add)?;
        let required_open_files = runtime_required_open_files.max(BOOTSTRAP_PEAK_FDS);

        Ok(Self {
            cpu_workers: narrow(cpu_workers)?,
            io_workers: narrow(io_workers)?,
            sqlite_workers: narrow(sqlite_workers)?,
            file_workers: narrow(file_workers)?,
            active_connections: narrow(active_connections)?,
            active_requests: narrow(active_requests)?,
            active_stream_sessions: narrow(active_stream_sessions)?,
            active_outbound_stream_sessions: narrow(active_outbound_stream_sessions)?,
            active_inbound_durable_streams: narrow(active_inbound_durable_streams)?,
            active_file_chunks: narrow(active_file_chunks)?,
            durable_orchestrations: narrow(durable_orchestrations)?,
            scheduler_ingress_capacity: narrow(scheduler_ingress_capacity)?,
            cpu_queue_capacity: narrow(cpu_queue_capacity)?,
            file_queue_capacity: narrow(file_queue_capacity)?,
            sqlite_queue_capacity: narrow(sqlite_queue_capacity)?,
            log_event_capacity: narrow(log_event_capacity)?,
            file_registry_capacity: narrow(file_registry_capacity)?,
            journal_mutation_registry_capacity: narrow(journal_mutation_registry_capacity)?,
            required_open_files,
            bootstrap_peak_bytes,
            pre_listener_initialization_peak_bytes,
            running_peak_bytes,
            derived_runtime_bytes,
            breakdown,
        })
    }

    pub fn validate_pre_spawn_environment(&self) -> Result<(), RuntimePreflightError> {
        validate_reservations(self)?;
        validate_open_file_limit(self.required_open_files)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeSizingError {
    WorkerCount {
        field: &'static str,
        actual: usize,
        minimum: usize,
        maximum: usize,
    },
    ArithmeticOverflow,
    PlatformCountOverflow(u64),
    RuntimeBytesExceeded {
        required: u64,
        maximum: u64,
        breakdown: Box<RuntimeSizingBreakdown>,
        feasible_cpu_workers: usize,
        feasible_io_workers: usize,
        feasible_sqlite_workers: usize,
    },
}

impl fmt::Display for RuntimeSizingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WorkerCount {
                field,
                actual,
                minimum,
                maximum,
            } => write!(
                formatter,
                "thread_pool {field} must be within {minimum}..={maximum}; got {actual}"
            ),
            Self::ArithmeticOverflow => write!(formatter, "runtime sizing arithmetic overflow"),
            Self::PlatformCountOverflow(value) => write!(
                formatter,
                "runtime sizing value {value} cannot be represented on this platform"
            ),
            Self::RuntimeBytesExceeded {
                required,
                maximum,
                breakdown,
                feasible_cpu_workers,
                feasible_io_workers,
                feasible_sqlite_workers,
            } => write!(
                formatter,
                "derived runtime reservation {required} exceeds {maximum} bytes: {breakdown:?}; maximum feasible workers with the other configured values fixed: cpu={feasible_cpu_workers}, io={feasible_io_workers}, sqlite={feasible_sqlite_workers}"
            ),
        }
    }
}

impl std::error::Error for RuntimeSizingError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimePreflightError {
    Allocation {
        resource: &'static str,
        capacity: usize,
    },
    OpenFileLimit {
        required: u64,
        available: u64,
    },
    OpenFileLimitUnavailable(String),
}

impl fmt::Display for RuntimePreflightError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Allocation { resource, capacity } => write!(
                formatter,
                "failed to reserve {capacity} entries for {resource} before worker startup"
            ),
            Self::OpenFileLimit {
                required,
                available,
            } => write!(
                formatter,
                "soft RLIMIT_NOFILE is {available}, but Momento requires {required} open files"
            ),
            Self::OpenFileLimitUnavailable(detail) => {
                write!(formatter, "failed to read soft RLIMIT_NOFILE: {detail}")
            }
        }
    }
}

impl std::error::Error for RuntimePreflightError {}

#[derive(Clone, Copy)]
enum WorkerField {
    Cpu,
    Io,
    Sqlite,
}

fn maximum_feasible_workers(
    configuration: &ThreadPoolConfig,
    field: WorkerField,
) -> Result<usize, RuntimeSizingError> {
    let (minimum, maximum): (usize, usize) = match field {
        WorkerField::Cpu => (1, MAX_CPU_WORKERS),
        WorkerField::Io => (4, MAX_IO_WORKERS),
        WorkerField::Sqlite => (1, MAX_SQLITE_WORKERS),
    };
    let mut feasible = minimum.saturating_sub(1);
    for candidate in minimum..=maximum {
        let mut adjusted = configuration.clone();
        match field {
            WorkerField::Cpu => adjusted.cpu_workers = candidate,
            WorkerField::Io => adjusted.io_workers = candidate,
            WorkerField::Sqlite => adjusted.sqlite_workers = candidate,
        }
        let sizing = RuntimeSizing::calculate(&adjusted, false)?;
        if sizing.derived_runtime_bytes > MAX_DERIVED_RUNTIME_BYTES {
            break;
        }
        feasible = candidate;
    }
    Ok(feasible)
}

fn validate_reservations(sizing: &RuntimeSizing) -> Result<(), RuntimePreflightError> {
    for (resource, capacity) in [
        ("scheduler ingress", sizing.scheduler_ingress_capacity),
        ("CPU executor queue", sizing.cpu_queue_capacity),
        ("file executor queue", sizing.file_queue_capacity),
        ("SQLite executor queue", sizing.sqlite_queue_capacity),
        ("log event ring", sizing.log_event_capacity),
        ("file handle registry", sizing.file_registry_capacity),
        (
            "Journal mutation registry",
            sizing.journal_mutation_registry_capacity,
        ),
    ] {
        let mut reservation = Vec::<u8>::new();
        reservation
            .try_reserve_exact(capacity)
            .map_err(|_| RuntimePreflightError::Allocation { resource, capacity })?;
    }
    Ok(())
}

#[cfg(unix)]
fn validate_open_file_limit(required: u64) -> Result<(), RuntimePreflightError> {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `limit` points to writable storage for the duration of the libc call.
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0 {
        return Err(RuntimePreflightError::OpenFileLimitUnavailable(
            std::io::Error::last_os_error().to_string(),
        ));
    }
    let available = if limit.rlim_cur == libc::RLIM_INFINITY {
        u64::MAX
    } else {
        limit.rlim_cur
    };
    if available < required {
        return Err(RuntimePreflightError::OpenFileLimit {
            required,
            available,
        });
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_open_file_limit(_required: u64) -> Result<(), RuntimePreflightError> {
    Ok(())
}

fn validate_range(
    field: &'static str,
    actual: usize,
    minimum: usize,
    maximum: usize,
) -> Result<(), RuntimeSizingError> {
    if (minimum..=maximum).contains(&actual) {
        return Ok(());
    }
    Err(RuntimeSizingError::WorkerCount {
        field,
        actual,
        minimum,
        maximum,
    })
}

fn widen(value: usize) -> Result<u64, RuntimeSizingError> {
    u64::try_from(value).map_err(|_| RuntimeSizingError::ArithmeticOverflow)
}

fn narrow(value: u64) -> Result<usize, RuntimeSizingError> {
    usize::try_from(value).map_err(|_| RuntimeSizingError::PlatformCountOverflow(value))
}

fn checked_add(left: u64, right: u64) -> Result<u64, RuntimeSizingError> {
    left.checked_add(right)
        .ok_or(RuntimeSizingError::ArithmeticOverflow)
}

fn checked_sub(left: u64, right: u64) -> Result<u64, RuntimeSizingError> {
    left.checked_sub(right)
        .ok_or(RuntimeSizingError::ArithmeticOverflow)
}

fn checked_mul(left: u64, right: u64) -> Result<u64, RuntimeSizingError> {
    left.checked_mul(right)
        .ok_or(RuntimeSizingError::ArithmeticOverflow)
}
