use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutorErrorKind {
    Overloaded,
    ShuttingDown,
    InvalidInput,
    BadRequest,
    Conflict,
    NotFound,
    WorkerPanic,
    DatabaseBusy,
    DatabaseTimeout,
    DatabasePermanent,
    Database,
    FileNotFound,
    FilePermission,
    FileConflict,
    FileInvalidData,
    FileTransient,
    FileSystem,
    Internal,
}

#[derive(Debug)]
pub struct ExecutorError {
    pub kind: ExecutorErrorKind,
    pub operation: &'static str,
    pub detail: String,
}

impl ExecutorError {
    pub(crate) fn new(
        kind: ExecutorErrorKind,
        operation: &'static str,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            operation,
            detail: detail.into(),
        }
    }

    pub(crate) fn overloaded(operation: &'static str) -> Self {
        Self::new(
            ExecutorErrorKind::Overloaded,
            operation,
            "executor FIFO has no immediately available slot",
        )
    }

    pub(crate) fn shutting_down(operation: &'static str) -> Self {
        Self::new(
            ExecutorErrorKind::ShuttingDown,
            operation,
            "runtime is shutting down",
        )
    }
}

impl fmt::Display for ExecutorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} {:?}: {}",
            self.operation, self.kind, self.detail
        )
    }
}

impl std::error::Error for ExecutorError {}
