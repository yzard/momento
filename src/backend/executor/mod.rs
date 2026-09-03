mod bounded_json;
mod control_json;
mod cpu;
mod error;
mod io;
pub mod process;
mod sqlite;

pub use control_json::{
    BackupCapabilitiesResponse, CapabilitiesResponse, ControlRequest, ControlRequestDto,
    ControlRequestKind, ControlRequestParseError, ControlResponse, ErrorResponse,
    FaceGroupMergeResponse, FeatureFlagsResponse, HealthcheckResponse, MessageResponse,
    ParsedControlRequest, PublicAlbumContentResponse, PublicAlbumSummaryResponse,
    PublicMediaContentResponse,
};
pub use cpu::{
    CpuExecutorHandle, CronCatchUpPage, ParsedExifMetadata, ParsedFfprobeMetadata,
    ParsedSupplementalMetadata, PlaceIdentityDto, Sha256Session,
};
pub use error::{ExecutorError, ExecutorErrorKind};
pub use io::{
    AppliedJournalEntry, CleanupJournalOutcome, FileIoExecutorHandle, JournalFileMutationOutcome,
    PublishJournalOutcome, RenameJournalOutcome, StorageDirectoryEntry, StorageDirectoryEntryKind,
    FILE_IO_ENTRY_BATCH,
};
pub use sqlite::SqliteExecutorHandle;

pub(crate) use cpu::{spawn_cpu_workers, CpuCommand};
pub(crate) use io::{
    bootstrap_file_executor, complete_file_executor_bootstrap, recover_log_capacity,
    spawn_file_workers, BootstrapDatabaseState, FileCommand, FileWorkerContext,
};
pub(crate) use sqlite::{spawn_sqlite_workers, SqliteCommand};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutorDomain {
    Cpu,
    FileIo,
    Sqlite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationSpec {
    pub domain: ExecutorDomain,
    pub maximum_input_bytes: usize,
    pub maximum_output_bytes: usize,
    pub maximum_temporary_bytes: usize,
}

pub(crate) const MAX_PROBE_OUTPUT_BYTES: usize = 256;
