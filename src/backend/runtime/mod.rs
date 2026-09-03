mod bootstrap;
pub(crate) mod config_bootstrap;
mod control;
mod http_server;
mod job;
pub(crate) mod scheduler;
mod sizing;
mod timezone;

pub use bootstrap::{ApplicationRuntime, RuntimeBuildError, RuntimeBuilder};
pub use config_bootstrap::ConfigFileIdentity;
pub use control::{
    schedule_client_request, ActiveDurableClaim, ConnectionAdmission, DurableAdmission,
    FileChunkAdmission, HttpRequestAdmission, OutboundStreamAdmission, RequestAdmission,
    SchedulerControlSource, SchedulerHandle, SchedulerState, StreamSessionAdmission,
};
pub use http_server::{serve_http1, HttpIdleTimeouts};
pub use job::{CronTaskId, DurableSourceId, SchedulerAdmissionKind};
pub use scheduler::{ExecutorHandles, ExecutorRuntime};
pub(crate) use sizing::FILE_IO_CHUNK_BYTES;
pub use sizing::{
    RuntimePreflightError, RuntimeSizing, RuntimeSizingBreakdown, ARGON2_WORKSPACE_BYTES,
    MAX_CPU_WORKERS, MAX_DERIVED_RUNTIME_BYTES, MAX_IO_WORKERS, MAX_SQLITE_WORKERS,
    WORKER_STACK_BYTES,
};
pub use timezone::SystemTimezoneSnapshot;
