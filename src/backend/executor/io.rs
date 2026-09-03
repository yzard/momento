use std::ffi::CString;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;

use crossbeam_channel::Receiver;
use tokio::sync::{oneshot, Notify};

use super::{
    ExecutorDomain, ExecutorError, ExecutorErrorKind, OperationSpec, MAX_PROBE_OUTPUT_BYTES,
};
use crate::io::file::{
    JournalMutationLease, JournalMutationTicket, MutationAuthorization, MutationGateRegistry,
    MutationLeaseError, MutationOperationGuard, NormalizedStoragePath, StorageRootId,
    StorageRootRegistry,
};
use crate::io::journal::{FileEntryAction, JournalMutationStage};
use crate::io::log::{LogEventConsumer, LogEventProducer, RuntimeLogWriter};
use crate::io::session::{
    rename_descriptor_entry, snapshot_regular_file, ChildDescriptorAccess, ChildDescriptorLease,
    FileHandleRegistry, RegisteredFile, StorageFileSession, StorageFileSnapshot,
};
use crate::io::space_budget::{
    DataDirSpaceBudget, ProvisionalSpaceToken, SpaceAdmission, SpaceBudgetError,
};
use crate::runtime::scheduler::{SchedulerIngress, SubmissionMode};
use crate::runtime::ConfigFileIdentity;

const STORAGE_DIRECTORY_CHUNK_BYTES: usize = 64 * 1024;
pub const FILE_IO_ENTRY_BATCH: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageDirectoryEntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageDirectoryEntry {
    pub name: String,
    pub kind: StorageDirectoryEntryKind,
    pub resume_offset: u64,
}

pub(crate) enum FileOperation {
    Probe {
        sequence: u64,
    },
    ReadConfig {
        expected: ConfigFileIdentity,
    },
    ReplaceConfig {
        expected: ConfigFileIdentity,
        contents: String,
    },
    PublishJournalEntry {
        authorization: MutationAuthorization,
        storage_root: StorageRootId,
        temporary_path: NormalizedStoragePath,
        destination_path: NormalizedStoragePath,
        expected_size: Option<u64>,
        expected_version: Option<String>,
    },
    RenameJournalEntry {
        authorization: MutationAuthorization,
        action: FileEntryAction,
        storage_root: StorageRootId,
        source_path: NormalizedStoragePath,
        destination_path: NormalizedStoragePath,
        expected_size: Option<u64>,
        expected_version: Option<String>,
    },
    CleanupJournalEntry {
        authorization: MutationAuthorization,
        storage_root: StorageRootId,
        source_path: NormalizedStoragePath,
        expected_size: Option<u64>,
    },
    OpenStorageWriteSession {
        storage_root: StorageRootId,
        path: NormalizedStoragePath,
        rollback_length: u64,
    },
    OpenStorageReadSession {
        storage_root: StorageRootId,
        path: NormalizedStoragePath,
    },
    OpenStorageDirectorySession {
        storage_root: StorageRootId,
        path: Option<NormalizedStoragePath>,
    },
    CreateStorageDirectory {
        storage_root: StorageRootId,
        path: NormalizedStoragePath,
    },
    SetStorageModifiedTime {
        storage_root: StorageRootId,
        path: NormalizedStoragePath,
        seconds: i64,
        nanoseconds: u32,
    },
    PinStorageSessionForChild {
        session: StorageFileSession,
        child_fd: std::os::fd::RawFd,
        access: ChildDescriptorAccess,
    },
    ReturnStorageSessionFromChild {
        lease: ChildDescriptorLease,
    },
    InspectStorageSession {
        session: StorageFileSession,
    },
    AtomicReplaceStorageFile {
        storage_root: StorageRootId,
        temporary_path: NormalizedStoragePath,
        destination_path: NormalizedStoragePath,
        contents: Vec<u8>,
    },
    WriteStorageSession {
        session: StorageFileSession,
        bytes: Vec<u8>,
    },
    ReadStorageSession {
        session: StorageFileSession,
        maximum_bytes: usize,
    },
    ReadStorageDirectorySession {
        session: StorageFileSession,
    },
    SeekStorageReadSession {
        session: StorageFileSession,
        offset: u64,
    },
    CommitStorageSession {
        session: StorageFileSession,
    },
    AbortStorageSession {
        session: StorageFileSession,
    },
    CloseStorageSession {
        session: StorageFileSession,
    },
}

impl FileOperation {
    fn name(&self) -> &'static str {
        match self {
            Self::Probe { .. } => "file_probe",
            Self::ReadConfig { .. } => "read_config",
            Self::ReplaceConfig { .. } => "replace_config",
            Self::PublishJournalEntry { .. } => "publish_journal_entry",
            Self::RenameJournalEntry { .. } => "rename_journal_entry",
            Self::CleanupJournalEntry { .. } => "cleanup_journal_entry",
            Self::OpenStorageWriteSession { .. } => "open_storage_write_session",
            Self::OpenStorageReadSession { .. } => "open_storage_read_session",
            Self::OpenStorageDirectorySession { .. } => "open_storage_directory_session",
            Self::CreateStorageDirectory { .. } => "create_storage_directory",
            Self::SetStorageModifiedTime { .. } => "set_storage_modified_time",
            Self::PinStorageSessionForChild { .. } => "pin_storage_session_for_child",
            Self::ReturnStorageSessionFromChild { .. } => "return_storage_session_from_child",
            Self::InspectStorageSession { .. } => "inspect_storage_session",
            Self::AtomicReplaceStorageFile { .. } => "atomic_replace_storage_file",
            Self::WriteStorageSession { .. } => "write_storage_session",
            Self::ReadStorageSession { .. } => "read_storage_session",
            Self::ReadStorageDirectorySession { .. } => "read_storage_directory_session",
            Self::SeekStorageReadSession { .. } => "seek_storage_read_session",
            Self::CommitStorageSession { .. } => "commit_storage_session",
            Self::AbortStorageSession { .. } => "abort_storage_session",
            Self::CloseStorageSession { .. } => "close_storage_session",
        }
    }

    pub(crate) fn spec(&self) -> OperationSpec {
        match self {
            Self::Probe { .. } => OperationSpec {
                domain: ExecutorDomain::FileIo,
                maximum_input_bytes: size_of::<u64>(),
                maximum_output_bytes: MAX_PROBE_OUTPUT_BYTES,
                maximum_temporary_bytes: 0,
            },
            Self::ReadConfig { .. } => OperationSpec {
                domain: ExecutorDomain::FileIo,
                maximum_input_bytes: crate::runtime::config_bootstrap::MAX_CONFIG_PATH_BYTES
                    + size_of::<ConfigFileIdentity>(),
                maximum_output_bytes: 0,
                maximum_temporary_bytes: crate::runtime::config_bootstrap::MAX_CONFIG_BYTES
                    as usize,
            },
            Self::ReplaceConfig { .. } => OperationSpec {
                domain: ExecutorDomain::FileIo,
                maximum_input_bytes: crate::runtime::config_bootstrap::MAX_CONFIG_PATH_BYTES
                    + crate::runtime::config_bootstrap::MAX_CONFIG_BYTES as usize
                    + size_of::<ConfigFileIdentity>(),
                maximum_output_bytes: size_of::<ConfigFileIdentity>(),
                maximum_temporary_bytes: crate::runtime::config_bootstrap::MAX_CONFIG_BYTES
                    as usize,
            },
            Self::PublishJournalEntry { .. } | Self::RenameJournalEntry { .. } => OperationSpec {
                domain: ExecutorDomain::FileIo,
                maximum_input_bytes: 2 * crate::io::file::MAX_STORAGE_PATH_BYTES
                    + crate::io::file::MAX_FILE_OPERATION_ID_BYTES
                    + 64,
                maximum_output_bytes: size_of::<PublishJournalOutcome>(),
                maximum_temporary_bytes: 4096,
            },
            Self::CleanupJournalEntry { .. } => OperationSpec {
                domain: ExecutorDomain::FileIo,
                maximum_input_bytes: crate::io::file::MAX_STORAGE_PATH_BYTES
                    + crate::io::file::MAX_FILE_OPERATION_ID_BYTES
                    + 64,
                maximum_output_bytes: size_of::<CleanupJournalOutcome>(),
                maximum_temporary_bytes: 4096,
            },
            Self::OpenStorageWriteSession { .. }
            | Self::OpenStorageReadSession { .. }
            | Self::OpenStorageDirectorySession { .. }
            | Self::CreateStorageDirectory { .. }
            | Self::SetStorageModifiedTime { .. } => OperationSpec {
                domain: ExecutorDomain::FileIo,
                maximum_input_bytes: crate::io::file::MAX_STORAGE_PATH_BYTES + 32,
                maximum_output_bytes: 64,
                maximum_temporary_bytes: 4096,
            },
            Self::PinStorageSessionForChild { .. }
            | Self::ReturnStorageSessionFromChild { .. }
            | Self::InspectStorageSession { .. } => OperationSpec {
                domain: ExecutorDomain::FileIo,
                maximum_input_bytes: 128,
                maximum_output_bytes: 128,
                maximum_temporary_bytes: 0,
            },
            Self::AtomicReplaceStorageFile { .. } => OperationSpec {
                domain: ExecutorDomain::FileIo,
                maximum_input_bytes: 1024 * 1024 + 2 * crate::io::file::MAX_STORAGE_PATH_BYTES + 32,
                maximum_output_bytes: 0,
                maximum_temporary_bytes: 4096,
            },
            Self::WriteStorageSession { .. } | Self::ReadStorageSession { .. } => OperationSpec {
                domain: ExecutorDomain::FileIo,
                maximum_input_bytes: crate::runtime::FILE_IO_CHUNK_BYTES as usize + 64,
                maximum_output_bytes: crate::runtime::FILE_IO_CHUNK_BYTES as usize + 64,
                maximum_temporary_bytes: 4096,
            },
            Self::ReadStorageDirectorySession { .. } => OperationSpec {
                domain: ExecutorDomain::FileIo,
                maximum_input_bytes: 64,
                maximum_output_bytes: 1024 * 1024,
                maximum_temporary_bytes: STORAGE_DIRECTORY_CHUNK_BYTES,
            },
            Self::SeekStorageReadSession { .. } => OperationSpec {
                domain: ExecutorDomain::FileIo,
                maximum_input_bytes: 72,
                maximum_output_bytes: 64,
                maximum_temporary_bytes: 0,
            },
            Self::CommitStorageSession { .. }
            | Self::AbortStorageSession { .. }
            | Self::CloseStorageSession { .. } => OperationSpec {
                domain: ExecutorDomain::FileIo,
                maximum_input_bytes: 64,
                maximum_output_bytes: 0,
                maximum_temporary_bytes: 4096,
            },
        }
    }
}

pub(crate) enum FileOutput {
    Probe {
        sequence: u64,
        thread_name: String,
    },
    ConfigRead(String),
    ConfigReplaced(ConfigFileIdentity),
    JournalEntryPublished(PublishJournalOutcome),
    JournalEntryRenamed(RenameJournalOutcome),
    JournalEntryCleaned(CleanupJournalOutcome),
    StorageSessionOpened(StorageFileSession),
    StorageReadSessionOpened {
        session: StorageFileSession,
        snapshot: StorageFileSnapshot,
    },
    StorageDirectorySessionOpened(StorageFileSession),
    StorageDirectoryCreated,
    StorageSessionWritten {
        session: StorageFileSession,
        written: usize,
    },
    StorageSessionRead {
        session: StorageFileSession,
        bytes: Vec<u8>,
    },
    StorageDirectorySessionRead {
        session: StorageFileSession,
        entries: Vec<StorageDirectoryEntry>,
        finished: bool,
    },
    StorageReadSessionSeeked(StorageFileSession),
    StorageSessionClosed,
    StorageModifiedTimeSet,
    ChildDescriptorPinned(ChildDescriptorLease),
    StorageSessionReturned(StorageFileSession),
    StorageSessionInspected {
        session: StorageFileSession,
        snapshot: StorageFileSnapshot,
    },
    StorageFileAtomicallyReplaced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishJournalOutcome {
    Published,
    AlreadyPublished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameJournalOutcome {
    Renamed,
    AlreadyRenamed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupJournalOutcome {
    Removed,
    AlreadyAbsent,
    ProgressPending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppliedJournalEntry {
    pub sequence: u16,
    pub outcome: JournalFileMutationOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalFileMutationOutcome {
    Published(PublishJournalOutcome),
    Renamed(RenameJournalOutcome),
    Cleaned(CleanupJournalOutcome),
}

pub(crate) struct FileCommand {
    operation: FileOperation,
    reply: oneshot::Sender<Result<FileOutput, ExecutorError>>,
}

impl FileCommand {
    pub(crate) fn new(
        operation: FileOperation,
        reply: oneshot::Sender<Result<FileOutput, ExecutorError>>,
    ) -> Self {
        Self { operation, reply }
    }

    pub(crate) fn reject(self, error: ExecutorError) {
        let _ = self.reply.send(Err(error));
    }
}

#[derive(Clone)]
pub struct FileIoExecutorHandle {
    ingress: SchedulerIngress,
    mutation_gates: Arc<MutationGateRegistry>,
    space_budget: DataDirSpaceBudget,
    log_events: LogEventProducer,
}

impl FileIoExecutorHandle {
    pub(crate) fn new(
        ingress: SchedulerIngress,
        mutation_gates: Arc<MutationGateRegistry>,
        space_budget: DataDirSpaceBudget,
        log_events: LogEventProducer,
    ) -> Self {
        Self {
            ingress,
            mutation_gates,
            space_budget,
            log_events,
        }
    }

    pub fn log_event_producer(&self) -> LogEventProducer {
        self.log_events.clone()
    }

    pub fn reserve_journal_space(
        &self,
        reservation_id: String,
        peak_additional_bytes: u64,
    ) -> Result<SpaceAdmission<ProvisionalSpaceToken>, SpaceBudgetError> {
        self.space_budget
            .reserve_journal(reservation_id, peak_additional_bytes)
    }

    pub fn space_budget_snapshot(
        &self,
    ) -> Result<crate::io::space_budget::LedgerSnapshot, SpaceBudgetError> {
        self.space_budget.snapshot()
    }

    pub fn reserve_journal_mutation(
        &self,
        group_id: &str,
        group_version: i64,
    ) -> Result<JournalMutationTicket, MutationLeaseError> {
        self.mutation_gates.reserve(group_id, group_version)
    }

    pub async fn fence_journal_mutations(
        &self,
        group_id: &str,
        expected_version: i64,
    ) -> Result<(), MutationLeaseError> {
        let next_version = expected_version
            .checked_add(1)
            .ok_or(MutationLeaseError::GenerationExhausted)?;
        self.mutation_gates.fence(group_id, next_version).await
    }

    pub fn release_journal_mutation_fence(
        &self,
        group_id: &str,
        durable_version: i64,
    ) -> Result<(), MutationLeaseError> {
        self.mutation_gates.release_fence(group_id, durable_version)
    }

    pub async fn publish_journal_entry_durable(
        &self,
        lease: &mut JournalMutationLease,
        sequence: u16,
    ) -> Result<PublishJournalOutcome, ExecutorError> {
        require_mutation_stage(
            lease.stage(),
            &[JournalMutationStage::Publication],
            "publish_journal_entry",
        )?;
        let entry = lease
            .take_entry(sequence, &[FileEntryAction::Publish])
            .map_err(|error| mutation_lease_error("publish_journal_entry", error))?;
        let temporary_path = entry
            .temporary_path
            .ok_or_else(|| output_mismatch("publish_journal_entry_missing_temporary_path"))?;
        let destination_path = entry
            .destination_path
            .ok_or_else(|| output_mismatch("publish_journal_entry_missing_destination_path"))?;
        let outcome = match self
            .submit(
                FileOperation::PublishJournalEntry {
                    authorization: lease.authorization(),
                    storage_root: entry.storage_root,
                    temporary_path,
                    destination_path,
                    expected_size: entry.expected_size,
                    expected_version: entry.expected_version,
                },
                "publish_journal_entry",
            )
            .await?
        {
            FileOutput::JournalEntryPublished(outcome) => outcome,
            _ => return Err(output_mismatch("publish_journal_entry")),
        };
        lease
            .finish_operation(sequence)
            .map_err(|error| mutation_lease_error("publish_journal_entry", error))?;
        Ok(outcome)
    }

    pub async fn rename_journal_entry_durable(
        &self,
        lease: &mut JournalMutationLease,
        sequence: u16,
    ) -> Result<RenameJournalOutcome, ExecutorError> {
        require_mutation_stage(
            lease.stage(),
            &[JournalMutationStage::Publication],
            "rename_journal_entry",
        )?;
        let entry = lease
            .take_entry(
                sequence,
                &[FileEntryAction::Move, FileEntryAction::Tombstone],
            )
            .map_err(|error| mutation_lease_error("rename_journal_entry", error))?;
        let source_path = entry
            .source_path
            .ok_or_else(|| output_mismatch("rename_journal_entry_missing_source_path"))?;
        let destination_path = match entry.action {
            FileEntryAction::Move => entry.destination_path,
            FileEntryAction::Tombstone => entry.tombstone_path,
            _ => None,
        }
        .ok_or_else(|| output_mismatch("rename_journal_entry_missing_destination_path"))?;
        let outcome = match self
            .submit(
                FileOperation::RenameJournalEntry {
                    authorization: lease.authorization(),
                    action: entry.action,
                    storage_root: entry.storage_root,
                    source_path,
                    destination_path,
                    expected_size: entry.expected_size,
                    expected_version: entry.expected_version,
                },
                "rename_journal_entry",
            )
            .await?
        {
            FileOutput::JournalEntryRenamed(outcome) => outcome,
            _ => return Err(output_mismatch("rename_journal_entry")),
        };
        lease
            .finish_operation(sequence)
            .map_err(|error| mutation_lease_error("rename_journal_entry", error))?;
        Ok(outcome)
    }

    pub async fn cleanup_journal_entry_durable(
        &self,
        lease: &mut JournalMutationLease,
        sequence: u16,
    ) -> Result<CleanupJournalOutcome, ExecutorError> {
        require_mutation_stage(
            lease.stage(),
            &[
                JournalMutationStage::Cleanup,
                JournalMutationStage::Rollback,
            ],
            "cleanup_journal_entry",
        )?;
        let entry = lease
            .take_entry(sequence, &[FileEntryAction::Cleanup])
            .map_err(|error| mutation_lease_error("cleanup_journal_entry", error))?;
        let source_path = entry
            .source_path
            .ok_or_else(|| output_mismatch("cleanup_journal_entry_missing_source_path"))?;
        let outcome = match self
            .submit(
                FileOperation::CleanupJournalEntry {
                    authorization: lease.authorization(),
                    storage_root: entry.storage_root,
                    source_path,
                    expected_size: entry.expected_size,
                },
                "cleanup_journal_entry",
            )
            .await?
        {
            FileOutput::JournalEntryCleaned(outcome) => outcome,
            _ => return Err(output_mismatch("cleanup_journal_entry")),
        };
        lease
            .finish_operation(sequence)
            .map_err(|error| mutation_lease_error("cleanup_journal_entry", error))?;
        Ok(outcome)
    }

    pub async fn apply_next_journal_entry_durable(
        &self,
        lease: &mut JournalMutationLease,
    ) -> Result<AppliedJournalEntry, ExecutorError> {
        let entry = lease
            .take_next_entry()
            .map_err(|error| mutation_lease_error("apply_next_journal_entry", error))?;
        let sequence = entry.sequence;
        let outcome = match entry.action {
            FileEntryAction::Publish => {
                let temporary_path = entry.temporary_path.ok_or_else(|| {
                    output_mismatch("apply_next_journal_entry_missing_temporary_path")
                })?;
                if lease.stage() == JournalMutationStage::Rollback {
                    match self
                        .submit(
                            FileOperation::CleanupJournalEntry {
                                authorization: lease.authorization(),
                                storage_root: entry.storage_root,
                                source_path: temporary_path,
                                expected_size: entry.expected_size,
                            },
                            "rollback_journal_entry",
                        )
                        .await?
                    {
                        FileOutput::JournalEntryCleaned(outcome) => {
                            JournalFileMutationOutcome::Cleaned(outcome)
                        }
                        _ => return Err(output_mismatch("apply_next_journal_entry")),
                    }
                } else {
                    require_mutation_stage(
                        lease.stage(),
                        &[JournalMutationStage::Publication],
                        "apply_next_journal_entry",
                    )?;
                    let destination_path = entry.destination_path.ok_or_else(|| {
                        output_mismatch("apply_next_journal_entry_missing_destination_path")
                    })?;
                    match self
                        .submit(
                            FileOperation::PublishJournalEntry {
                                authorization: lease.authorization(),
                                storage_root: entry.storage_root,
                                temporary_path,
                                destination_path,
                                expected_size: entry.expected_size,
                                expected_version: entry.expected_version,
                            },
                            "publish_journal_entry",
                        )
                        .await?
                    {
                        FileOutput::JournalEntryPublished(outcome) => {
                            JournalFileMutationOutcome::Published(outcome)
                        }
                        _ => return Err(output_mismatch("apply_next_journal_entry")),
                    }
                }
            }
            FileEntryAction::Move | FileEntryAction::Tombstone => {
                require_mutation_stage(
                    lease.stage(),
                    &[JournalMutationStage::Publication],
                    "apply_next_journal_entry",
                )?;
                let source_path = entry.source_path.ok_or_else(|| {
                    output_mismatch("apply_next_journal_entry_missing_source_path")
                })?;
                let destination_path = match entry.action {
                    FileEntryAction::Move => entry.destination_path,
                    FileEntryAction::Tombstone => entry.tombstone_path,
                    _ => None,
                }
                .ok_or_else(|| {
                    output_mismatch("apply_next_journal_entry_missing_destination_path")
                })?;
                match self
                    .submit(
                        FileOperation::RenameJournalEntry {
                            authorization: lease.authorization(),
                            action: entry.action,
                            storage_root: entry.storage_root,
                            source_path,
                            destination_path,
                            expected_size: entry.expected_size,
                            expected_version: entry.expected_version,
                        },
                        "rename_journal_entry",
                    )
                    .await?
                {
                    FileOutput::JournalEntryRenamed(outcome) => {
                        JournalFileMutationOutcome::Renamed(outcome)
                    }
                    _ => return Err(output_mismatch("apply_next_journal_entry")),
                }
            }
            FileEntryAction::Cleanup => {
                require_mutation_stage(
                    lease.stage(),
                    &[
                        JournalMutationStage::Cleanup,
                        JournalMutationStage::Rollback,
                    ],
                    "apply_next_journal_entry",
                )?;
                let source_path = entry.source_path.ok_or_else(|| {
                    output_mismatch("apply_next_journal_entry_missing_source_path")
                })?;
                match self
                    .submit(
                        FileOperation::CleanupJournalEntry {
                            authorization: lease.authorization(),
                            storage_root: entry.storage_root,
                            source_path,
                            expected_size: entry.expected_size,
                        },
                        "cleanup_journal_entry",
                    )
                    .await?
                {
                    FileOutput::JournalEntryCleaned(outcome) => {
                        JournalFileMutationOutcome::Cleaned(outcome)
                    }
                    _ => return Err(output_mismatch("apply_next_journal_entry")),
                }
            }
        };
        lease
            .finish_operation(sequence)
            .map_err(|error| mutation_lease_error("apply_next_journal_entry", error))?;
        Ok(AppliedJournalEntry { sequence, outcome })
    }

    pub async fn probe_durable(&self, sequence: u64) -> Result<(u64, String), ExecutorError> {
        let operation = FileOperation::Probe { sequence };
        let operation_name = operation.name();
        let (reply, response) = oneshot::channel();
        self.ingress.submit_file(
            FileCommand::new(operation, reply),
            SubmissionMode::Durable,
            operation_name,
        )?;
        match response
            .await
            .map_err(|_| ExecutorError::shutting_down(operation_name))??
        {
            FileOutput::Probe {
                sequence,
                thread_name,
            } => Ok((sequence, thread_name)),
            FileOutput::ConfigRead(_)
            | FileOutput::ConfigReplaced(_)
            | FileOutput::JournalEntryPublished(_)
            | FileOutput::JournalEntryRenamed(_)
            | FileOutput::JournalEntryCleaned(_)
            | FileOutput::StorageSessionOpened(_)
            | FileOutput::StorageReadSessionOpened { .. }
            | FileOutput::StorageDirectorySessionOpened(_)
            | FileOutput::StorageDirectoryCreated
            | FileOutput::StorageSessionWritten { .. }
            | FileOutput::StorageSessionRead { .. }
            | FileOutput::StorageDirectorySessionRead { .. }
            | FileOutput::StorageReadSessionSeeked(_)
            | FileOutput::StorageSessionClosed
            | FileOutput::StorageModifiedTimeSet
            | FileOutput::ChildDescriptorPinned(_)
            | FileOutput::StorageSessionReturned(_)
            | FileOutput::StorageSessionInspected { .. }
            | FileOutput::StorageFileAtomicallyReplaced => Err(ExecutorError::new(
                ExecutorErrorKind::Internal,
                "file_probe",
                "file executor returned mismatched config operation output",
            )),
        }
    }

    pub(crate) async fn read_config_durable(
        &self,
        expected: ConfigFileIdentity,
    ) -> Result<String, ExecutorError> {
        match self
            .submit(FileOperation::ReadConfig { expected }, "read_config")
            .await?
        {
            FileOutput::ConfigRead(contents) => Ok(contents),
            _ => Err(output_mismatch("read_config")),
        }
    }

    pub(crate) async fn replace_config_durable(
        &self,
        expected: ConfigFileIdentity,
        contents: String,
    ) -> Result<ConfigFileIdentity, ExecutorError> {
        if contents.len() as u64 > crate::runtime::config_bootstrap::MAX_CONFIG_BYTES {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "replace_config",
                "updated config exceeds one mebibyte",
            ));
        }
        match self
            .submit(
                FileOperation::ReplaceConfig { expected, contents },
                "replace_config",
            )
            .await?
        {
            FileOutput::ConfigReplaced(identity) => Ok(identity),
            _ => Err(output_mismatch("replace_config")),
        }
    }

    pub async fn open_storage_write_session_request(
        &self,
        storage_root: StorageRootId,
        path: NormalizedStoragePath,
        rollback_length: u64,
    ) -> Result<StorageFileSession, ExecutorError> {
        self.open_storage_write_session(storage_root, path, rollback_length, SubmissionMode::Try)
            .await
    }

    pub async fn open_storage_write_session_durable(
        &self,
        storage_root: StorageRootId,
        path: NormalizedStoragePath,
        rollback_length: u64,
    ) -> Result<StorageFileSession, ExecutorError> {
        self.open_storage_write_session(
            storage_root,
            path,
            rollback_length,
            SubmissionMode::Durable,
        )
        .await
    }

    async fn open_storage_write_session(
        &self,
        storage_root: StorageRootId,
        path: NormalizedStoragePath,
        rollback_length: u64,
        mode: SubmissionMode,
    ) -> Result<StorageFileSession, ExecutorError> {
        match self
            .submit_with_mode(
                FileOperation::OpenStorageWriteSession {
                    storage_root,
                    path,
                    rollback_length,
                },
                "open_storage_write_session",
                mode,
            )
            .await?
        {
            FileOutput::StorageSessionOpened(session) => Ok(session),
            _ => Err(output_mismatch("open_storage_write_session")),
        }
    }

    pub async fn open_storage_read_session_request(
        &self,
        storage_root: StorageRootId,
        path: NormalizedStoragePath,
    ) -> Result<(StorageFileSession, StorageFileSnapshot), ExecutorError> {
        self.open_storage_read_session(storage_root, path, SubmissionMode::Try)
            .await
    }

    pub async fn open_storage_read_session_durable(
        &self,
        storage_root: StorageRootId,
        path: NormalizedStoragePath,
    ) -> Result<(StorageFileSession, StorageFileSnapshot), ExecutorError> {
        self.open_storage_read_session(storage_root, path, SubmissionMode::Durable)
            .await
    }

    async fn open_storage_read_session(
        &self,
        storage_root: StorageRootId,
        path: NormalizedStoragePath,
        mode: SubmissionMode,
    ) -> Result<(StorageFileSession, StorageFileSnapshot), ExecutorError> {
        match self
            .submit_with_mode(
                FileOperation::OpenStorageReadSession { storage_root, path },
                "open_storage_read_session",
                mode,
            )
            .await?
        {
            FileOutput::StorageReadSessionOpened { session, snapshot } => Ok((session, snapshot)),
            _ => Err(output_mismatch("open_storage_read_session")),
        }
    }

    pub async fn open_storage_directory_session_durable(
        &self,
        storage_root: StorageRootId,
        path: Option<NormalizedStoragePath>,
    ) -> Result<StorageFileSession, ExecutorError> {
        match self
            .submit_with_mode(
                FileOperation::OpenStorageDirectorySession { storage_root, path },
                "open_storage_directory_session",
                SubmissionMode::Durable,
            )
            .await?
        {
            FileOutput::StorageDirectorySessionOpened(session) => Ok(session),
            _ => Err(output_mismatch("open_storage_directory_session")),
        }
    }

    pub async fn open_storage_directory_session_request(
        &self,
        storage_root: StorageRootId,
        path: Option<NormalizedStoragePath>,
    ) -> Result<StorageFileSession, ExecutorError> {
        match self
            .submit_with_mode(
                FileOperation::OpenStorageDirectorySession { storage_root, path },
                "open_storage_directory_session",
                SubmissionMode::Try,
            )
            .await?
        {
            FileOutput::StorageDirectorySessionOpened(session) => Ok(session),
            _ => Err(output_mismatch("open_storage_directory_session")),
        }
    }

    pub async fn create_storage_directory_durable(
        &self,
        storage_root: StorageRootId,
        path: NormalizedStoragePath,
    ) -> Result<(), ExecutorError> {
        match self
            .submit_with_mode(
                FileOperation::CreateStorageDirectory { storage_root, path },
                "create_storage_directory",
                SubmissionMode::Durable,
            )
            .await?
        {
            FileOutput::StorageDirectoryCreated => Ok(()),
            _ => Err(output_mismatch("create_storage_directory")),
        }
    }

    pub async fn seek_storage_read_session_request(
        &self,
        session: StorageFileSession,
        offset: u64,
    ) -> Result<StorageFileSession, ExecutorError> {
        self.seek_storage_read_session(session, offset, SubmissionMode::Try)
            .await
    }

    pub async fn seek_storage_read_session_durable(
        &self,
        session: StorageFileSession,
        offset: u64,
    ) -> Result<StorageFileSession, ExecutorError> {
        self.seek_storage_read_session(session, offset, SubmissionMode::Durable)
            .await
    }

    async fn seek_storage_read_session(
        &self,
        session: StorageFileSession,
        offset: u64,
        mode: SubmissionMode,
    ) -> Result<StorageFileSession, ExecutorError> {
        match self
            .submit_with_mode(
                FileOperation::SeekStorageReadSession { session, offset },
                "seek_storage_read_session",
                mode,
            )
            .await?
        {
            FileOutput::StorageReadSessionSeeked(session) => Ok(session),
            _ => Err(output_mismatch("seek_storage_read_session")),
        }
    }

    pub async fn set_storage_modified_time_durable(
        &self,
        storage_root: StorageRootId,
        path: NormalizedStoragePath,
        seconds: i64,
        nanoseconds: u32,
    ) -> Result<(), ExecutorError> {
        if nanoseconds >= 1_000_000_000 {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "set_storage_modified_time",
                "modified-time nanoseconds must be below one billion",
            ));
        }
        match self
            .submit_with_mode(
                FileOperation::SetStorageModifiedTime {
                    storage_root,
                    path,
                    seconds,
                    nanoseconds,
                },
                "set_storage_modified_time",
                SubmissionMode::Durable,
            )
            .await?
        {
            FileOutput::StorageModifiedTimeSet => Ok(()),
            _ => Err(output_mismatch("set_storage_modified_time")),
        }
    }

    pub(crate) async fn pin_storage_session_for_child_durable(
        &self,
        session: StorageFileSession,
        child_fd: std::os::fd::RawFd,
        access: ChildDescriptorAccess,
    ) -> Result<ChildDescriptorLease, ExecutorError> {
        match self
            .submit_with_mode(
                FileOperation::PinStorageSessionForChild {
                    session,
                    child_fd,
                    access,
                },
                "pin_storage_session_for_child",
                SubmissionMode::Durable,
            )
            .await?
        {
            FileOutput::ChildDescriptorPinned(lease) => Ok(lease),
            _ => Err(output_mismatch("pin_storage_session_for_child")),
        }
    }

    pub(crate) async fn return_storage_session_from_child_durable(
        &self,
        lease: ChildDescriptorLease,
    ) -> Result<StorageFileSession, ExecutorError> {
        match self
            .submit_with_mode(
                FileOperation::ReturnStorageSessionFromChild { lease },
                "return_storage_session_from_child",
                SubmissionMode::Durable,
            )
            .await?
        {
            FileOutput::StorageSessionReturned(session) => Ok(session),
            _ => Err(output_mismatch("return_storage_session_from_child")),
        }
    }

    pub(crate) async fn inspect_storage_session_durable(
        &self,
        session: StorageFileSession,
    ) -> Result<(StorageFileSession, StorageFileSnapshot), ExecutorError> {
        match self
            .submit_with_mode(
                FileOperation::InspectStorageSession { session },
                "inspect_storage_session",
                SubmissionMode::Durable,
            )
            .await?
        {
            FileOutput::StorageSessionInspected { session, snapshot } => Ok((session, snapshot)),
            _ => Err(output_mismatch("inspect_storage_session")),
        }
    }

    pub async fn atomic_replace_storage_file_durable(
        &self,
        storage_root: StorageRootId,
        temporary_path: NormalizedStoragePath,
        destination_path: NormalizedStoragePath,
        contents: Vec<u8>,
    ) -> Result<(), ExecutorError> {
        if contents.is_empty() || contents.len() > 1024 * 1024 || temporary_path == destination_path
        {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "atomic_replace_storage_file",
                "atomic replacement requires distinct paths and 1..=1048576 bytes",
            ));
        }
        match self
            .submit_with_mode(
                FileOperation::AtomicReplaceStorageFile {
                    storage_root,
                    temporary_path,
                    destination_path,
                    contents,
                },
                "atomic_replace_storage_file",
                SubmissionMode::Durable,
            )
            .await?
        {
            FileOutput::StorageFileAtomicallyReplaced => Ok(()),
            _ => Err(output_mismatch("atomic_replace_storage_file")),
        }
    }

    pub async fn write_storage_session_request(
        &self,
        session: StorageFileSession,
        bytes: Vec<u8>,
    ) -> Result<(StorageFileSession, usize), ExecutorError> {
        self.write_storage_session(session, bytes, SubmissionMode::Try)
            .await
    }

    pub async fn write_storage_session_durable(
        &self,
        session: StorageFileSession,
        bytes: Vec<u8>,
    ) -> Result<(StorageFileSession, usize), ExecutorError> {
        self.write_storage_session(session, bytes, SubmissionMode::Durable)
            .await
    }

    async fn write_storage_session(
        &self,
        session: StorageFileSession,
        bytes: Vec<u8>,
        mode: SubmissionMode,
    ) -> Result<(StorageFileSession, usize), ExecutorError> {
        if bytes.is_empty() || bytes.len() > crate::runtime::FILE_IO_CHUNK_BYTES as usize {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "write_storage_session",
                "storage session write must contain at most one mebibyte",
            ));
        }
        match self
            .submit_with_mode(
                FileOperation::WriteStorageSession { session, bytes },
                "write_storage_session",
                mode,
            )
            .await?
        {
            FileOutput::StorageSessionWritten { session, written } => Ok((session, written)),
            _ => Err(output_mismatch("write_storage_session")),
        }
    }

    pub async fn read_storage_session_request(
        &self,
        session: StorageFileSession,
        maximum_bytes: usize,
    ) -> Result<(StorageFileSession, Vec<u8>), ExecutorError> {
        self.read_storage_session(session, maximum_bytes, SubmissionMode::Try)
            .await
    }

    pub async fn read_storage_session_durable(
        &self,
        session: StorageFileSession,
        maximum_bytes: usize,
    ) -> Result<(StorageFileSession, Vec<u8>), ExecutorError> {
        self.read_storage_session(session, maximum_bytes, SubmissionMode::Durable)
            .await
    }

    async fn read_storage_session(
        &self,
        session: StorageFileSession,
        maximum_bytes: usize,
        mode: SubmissionMode,
    ) -> Result<(StorageFileSession, Vec<u8>), ExecutorError> {
        if maximum_bytes == 0 || maximum_bytes > crate::runtime::FILE_IO_CHUNK_BYTES as usize {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "read_storage_session",
                "storage session read must request at most one mebibyte",
            ));
        }
        match self
            .submit_with_mode(
                FileOperation::ReadStorageSession {
                    session,
                    maximum_bytes,
                },
                "read_storage_session",
                mode,
            )
            .await?
        {
            FileOutput::StorageSessionRead { session, bytes } => Ok((session, bytes)),
            _ => Err(output_mismatch("read_storage_session")),
        }
    }

    pub async fn read_storage_directory_session_durable(
        &self,
        session: StorageFileSession,
    ) -> Result<(StorageFileSession, Vec<StorageDirectoryEntry>, bool), ExecutorError> {
        match self
            .submit_with_mode(
                FileOperation::ReadStorageDirectorySession { session },
                "read_storage_directory_session",
                SubmissionMode::Durable,
            )
            .await?
        {
            FileOutput::StorageDirectorySessionRead {
                session,
                entries,
                finished,
            } => Ok((session, entries, finished)),
            _ => Err(output_mismatch("read_storage_directory_session")),
        }
    }

    pub async fn read_storage_directory_session_request(
        &self,
        session: StorageFileSession,
    ) -> Result<(StorageFileSession, Vec<StorageDirectoryEntry>, bool), ExecutorError> {
        match self
            .submit_with_mode(
                FileOperation::ReadStorageDirectorySession { session },
                "read_storage_directory_session",
                SubmissionMode::Try,
            )
            .await?
        {
            FileOutput::StorageDirectorySessionRead {
                session,
                entries,
                finished,
            } => Ok((session, entries, finished)),
            _ => Err(output_mismatch("read_storage_directory_session")),
        }
    }

    pub async fn commit_storage_session_request(
        &self,
        session: StorageFileSession,
    ) -> Result<(), ExecutorError> {
        self.commit_storage_session(session, SubmissionMode::Try)
            .await
    }

    pub async fn commit_storage_session_durable(
        &self,
        session: StorageFileSession,
    ) -> Result<(), ExecutorError> {
        self.commit_storage_session(session, SubmissionMode::Durable)
            .await
    }

    async fn commit_storage_session(
        &self,
        session: StorageFileSession,
        mode: SubmissionMode,
    ) -> Result<(), ExecutorError> {
        match self
            .submit_with_mode(
                FileOperation::CommitStorageSession { session },
                "commit_storage_session",
                mode,
            )
            .await?
        {
            FileOutput::StorageSessionClosed => Ok(()),
            _ => Err(output_mismatch("commit_storage_session")),
        }
    }

    pub async fn abort_storage_session_request(
        &self,
        session: StorageFileSession,
    ) -> Result<(), ExecutorError> {
        match self
            .submit_with_mode(
                FileOperation::AbortStorageSession { session },
                "abort_storage_session",
                SubmissionMode::Try,
            )
            .await?
        {
            FileOutput::StorageSessionClosed => Ok(()),
            _ => Err(output_mismatch("abort_storage_session")),
        }
    }

    pub async fn abort_storage_session_durable(
        &self,
        session: StorageFileSession,
    ) -> Result<(), ExecutorError> {
        match self
            .submit_with_mode(
                FileOperation::AbortStorageSession { session },
                "abort_storage_session",
                SubmissionMode::Durable,
            )
            .await?
        {
            FileOutput::StorageSessionClosed => Ok(()),
            _ => Err(output_mismatch("abort_storage_session")),
        }
    }

    pub async fn close_storage_session_request(
        &self,
        session: StorageFileSession,
    ) -> Result<(), ExecutorError> {
        self.close_storage_session(session, SubmissionMode::Try)
            .await
    }

    pub async fn close_storage_session_durable(
        &self,
        session: StorageFileSession,
    ) -> Result<(), ExecutorError> {
        self.close_storage_session(session, SubmissionMode::Durable)
            .await
    }

    async fn close_storage_session(
        &self,
        session: StorageFileSession,
        mode: SubmissionMode,
    ) -> Result<(), ExecutorError> {
        match self
            .submit_with_mode(
                FileOperation::CloseStorageSession { session },
                "close_storage_session",
                mode,
            )
            .await?
        {
            FileOutput::StorageSessionClosed => Ok(()),
            _ => Err(output_mismatch("close_storage_session")),
        }
    }

    async fn submit(
        &self,
        operation: FileOperation,
        operation_name: &'static str,
    ) -> Result<FileOutput, ExecutorError> {
        self.submit_with_mode(operation, operation_name, SubmissionMode::Durable)
            .await
    }

    async fn submit_with_mode(
        &self,
        operation: FileOperation,
        operation_name: &'static str,
        mode: SubmissionMode,
    ) -> Result<FileOutput, ExecutorError> {
        let (reply, response) = oneshot::channel();
        self.ingress
            .submit_file(FileCommand::new(operation, reply), mode, operation_name)?;
        response
            .await
            .map_err(|_| ExecutorError::shutting_down(operation_name))?
    }
}

fn output_mismatch(operation: &'static str) -> ExecutorError {
    ExecutorError::new(
        ExecutorErrorKind::Internal,
        operation,
        "file executor returned mismatched output",
    )
}

fn mutation_lease_error(operation: &'static str, error: MutationLeaseError) -> ExecutorError {
    ExecutorError::new(
        ExecutorErrorKind::InvalidInput,
        operation,
        error.to_string(),
    )
}

pub(crate) struct FileBootstrapCommand {
    config_identity: ConfigFileIdentity,
    data_dir: PathBuf,
}

pub(crate) struct FileBootstrapOutput {
    pub data_directory_lock: std::fs::File,
    pub space_budget: DataDirSpaceBudget,
    pub log_allocated_bytes: u64,
    pub database_state: BootstrapDatabaseState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BootstrapDatabaseState {
    Fresh,
    Existing,
}

pub(crate) fn bootstrap_file_executor(
    config_identity: ConfigFileIdentity,
    data_dir: PathBuf,
) -> Result<FileBootstrapOutput, String> {
    let (command_sender, command_receiver) = std::sync::mpsc::sync_channel(1);
    let (result_sender, result_receiver) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::Builder::new()
        .name("momento-io-file-bootstrap".to_string())
        .stack_size(crate::runtime::WORKER_STACK_BYTES as usize)
        .spawn(move || {
            let result = command_receiver
                .recv()
                .map_err(|_| "private file bootstrap command was not delivered".to_string())
                .and_then(run_file_bootstrap);
            let _ = result_sender.send(result);
        })
        .map_err(|error| error.to_string())?;
    command_sender
        .send(FileBootstrapCommand {
            config_identity,
            data_dir,
        })
        .map_err(|_| "private file bootstrap worker stopped before its command".to_string())?;
    drop(command_sender);
    let result = result_receiver
        .recv()
        .map_err(|_| "private file bootstrap result was dropped".to_string())?;
    if worker.join().is_err() {
        return Err("private file bootstrap worker panicked".to_string());
    }
    result
}

fn run_file_bootstrap(command: FileBootstrapCommand) -> Result<FileBootstrapOutput, String> {
    validate_bootstrap_config_identity(&command.config_identity)
        .map_err(|error| error.to_string())?;
    let data_directory_lock =
        acquire_data_directory_lock(&command.data_dir).map_err(|error| error.to_string())?;
    let budget_directory = open_existing_child_directory(&data_directory_lock, ".")
        .map_err(|error| format!("could not open data-directory capacity handle: {error}"))?
        .ok_or_else(|| "data-directory capacity handle disappeared".to_string())?;
    let space_budget =
        DataDirSpaceBudget::from_directory(budget_directory).map_err(|error| error.to_string())?;
    let log_allocated_bytes = match open_existing_child_directory(
        &data_directory_lock,
        StorageRootId::Logs.directory_name(),
    )
    .map_err(|error| format!("could not open retained Logs inventory: {error}"))?
    {
        Some(logs) => crate::io::log::measure_retained_log_allocation(&logs)
            .map_err(|error| format!("could not measure retained logs: {error}"))?,
        None => 0,
    };
    let database_state = inspect_database_state(&data_directory_lock)
        .map_err(|error| format!("could not inspect SQLite bootstrap state: {error}"))?;
    Ok(FileBootstrapOutput {
        data_directory_lock,
        space_budget,
        log_allocated_bytes,
        database_state,
    })
}

pub(crate) fn complete_file_executor_bootstrap(
    data_dir: PathBuf,
    static_dir: Option<PathBuf>,
    space_budget: DataDirSpaceBudget,
) -> Result<Arc<StorageRootRegistry>, String> {
    let worker = std::thread::Builder::new()
        .name("momento-io-file-bootstrap-mutations".to_string())
        .stack_size(crate::runtime::WORKER_STACK_BYTES as usize)
        .spawn(move || {
            recover_log_capacity(&data_dir, &space_budget)?;
            StorageRootRegistry::bootstrap(&data_dir, static_dir.as_deref(), &space_budget)
                .map(Arc::new)
                .map_err(|error| file_error("bootstrap_storage_roots", error).to_string())
        })
        .map_err(|error| error.to_string())?;
    worker
        .join()
        .map_err(|_| "private file bootstrap mutation worker panicked".to_string())?
}

pub(crate) fn recover_log_capacity(
    data_dir: &std::path::Path,
    space_budget: &DataDirSpaceBudget,
) -> Result<(), String> {
    loop {
        let snapshot = space_budget.snapshot().map_err(|error| error.to_string())?;
        match snapshot.health {
            crate::io::space_budget::SpaceBudgetHealth::Healthy => return Ok(()),
            crate::io::space_budget::SpaceBudgetHealth::ExternalDeficit => {
                return Err(
                    "data-directory capacity has an external deficit after reconstruction"
                        .to_string(),
                )
            }
            crate::io::space_budget::SpaceBudgetHealth::LogOverQuota => {}
        }
        let logs_path = data_dir.join(StorageRootId::Logs.directory_name());
        let logs = File::open(&logs_path)
            .map_err(|error| format!("could not open Logs for quota recovery: {error}"))?;
        let removed = crate::io::log::prune_oldest_closed_rotations_batch(
            &logs,
            snapshot.log_allocated_bytes,
            snapshot.log_quota_bytes,
        )
        .map_err(|error| format!("could not prune retained Logs: {error}"))?;
        if !removed {
            return Err(format!(
                "retained Momento logs use {} bytes, above the {} byte quota, and no closed rotation can be pruned",
                snapshot.log_allocated_bytes, snapshot.log_quota_bytes
            ));
        }
        let allocated = crate::io::log::measure_retained_log_allocation(&logs)
            .map_err(|error| format!("could not remeasure retained Logs: {error}"))?;
        space_budget
            .publish_log_cleanup_allocation(allocated)
            .map_err(|error| error.to_string())?;
    }
}

fn open_existing_child_directory(
    parent: &std::fs::File,
    name: &str,
) -> std::io::Result<Option<std::fs::File>> {
    let name =
        CString::new(name).map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor >= 0 {
        return Ok(Some(unsafe { std::fs::File::from_raw_fd(descriptor) }));
    }
    let error = std::io::Error::last_os_error();
    if error.kind() == std::io::ErrorKind::NotFound {
        Ok(None)
    } else {
        Err(error)
    }
}

fn inspect_database_state(
    data_directory: &std::fs::File,
) -> std::io::Result<BootstrapDatabaseState> {
    let main = child_regular_file_size(data_directory, "database.sqlite")?;
    let wal = child_regular_file_size(data_directory, "database.sqlite-wal")?;
    let shm = child_regular_file_size(data_directory, "database.sqlite-shm")?;
    match (main, wal, shm) {
        (None, None, None) => Ok(BootstrapDatabaseState::Fresh),
        (Some(size), _, _) if size > 0 => Ok(BootstrapDatabaseState::Existing),
        (None, Some(_), _) | (None, _, Some(_)) => Err(std::io::Error::other(
            "SQLite side file exists without database.sqlite",
        )),
        (Some(_), _, _) => Err(std::io::Error::other(
            "database.sqlite exists but is empty or truncated",
        )),
    }
}

fn child_regular_file_size(parent: &std::fs::File, name: &str) -> std::io::Result<Option<u64>> {
    let name =
        CString::new(name).map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let mut status = std::mem::MaybeUninit::<libc::stat>::uninit();
    let result = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            status.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result != 0 {
        let error = std::io::Error::last_os_error();
        return if error.kind() == std::io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(error)
        };
    }
    let status = unsafe { status.assume_init() };
    if status.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err(std::io::Error::other(format!(
            "{name:?} is not a regular file"
        )));
    }
    u64::try_from(status.st_size)
        .map(Some)
        .map_err(|_| std::io::Error::other("SQLite file size is negative"))
}

fn validate_bootstrap_config_identity(expected: &ConfigFileIdentity) -> Result<(), ExecutorError> {
    let observed = crate::runtime::config_bootstrap::read_existing_config(&expected.canonical_path)
        .map_err(|error| file_error("validate_config_identity", error))?
        .identity;
    if observed != *expected {
        return Err(ExecutorError::new(
            ExecutorErrorKind::InvalidInput,
            "validate_config_identity",
            "config identity or content changed before file bootstrap",
        ));
    }
    crate::runtime::config_bootstrap::recover_config_update_temporary(expected)
        .map_err(|error| file_error("validate_config_identity", error))
}

fn acquire_data_directory_lock(data_dir: &PathBuf) -> Result<std::fs::File, ExecutorError> {
    let metadata = std::fs::symlink_metadata(data_dir)
        .map_err(|error| file_error("acquire_data_directory_lock", error))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ExecutorError::new(
            ExecutorErrorKind::InvalidInput,
            "acquire_data_directory_lock",
            "configured data directory must be an existing non-symlink directory",
        ));
    }
    let directory = std::fs::File::open(data_dir)
        .map_err(|error| file_error("acquire_data_directory_lock", error))?;
    let result = unsafe { libc::flock(directory.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result != 0 {
        return Err(file_error(
            "acquire_data_directory_lock",
            std::io::Error::last_os_error(),
        ));
    }
    Ok(directory)
}

#[derive(Clone)]
pub(crate) struct FileWorkerContext {
    pub(crate) capacity_wake: Arc<Notify>,
    pub(crate) storage_roots: Arc<OnceLock<Arc<StorageRootRegistry>>>,
    pub(crate) mutation_gates: Arc<MutationGateRegistry>,
    pub(crate) file_handles: Arc<FileHandleRegistry>,
    pub(crate) close_receiver: Receiver<()>,
    pub(crate) log_consumer: Arc<LogEventConsumer>,
    pub(crate) log_writer: Arc<Mutex<RuntimeLogWriter>>,
    pub(crate) space_budget: DataDirSpaceBudget,
}

pub(crate) fn spawn_file_workers(
    worker_count: usize,
    receiver: Receiver<FileCommand>,
    context: FileWorkerContext,
) -> Result<Vec<JoinHandle<()>>, std::io::Error> {
    let mut workers = Vec::new();
    workers.try_reserve_exact(worker_count).map_err(|error| {
        std::io::Error::other(format!("failed to reserve file worker handles: {error}"))
    })?;
    for worker_index in 0..worker_count {
        let receiver = receiver.clone();
        let context = context.clone();
        workers.push(
            std::thread::Builder::new()
                .name(format!("momento-io-file-{worker_index}"))
                .stack_size(crate::runtime::WORKER_STACK_BYTES as usize)
                .spawn(move || run_worker(receiver, context))?,
        );
    }
    Ok(workers)
}

fn run_worker(receiver: Receiver<FileCommand>, context: FileWorkerContext) {
    loop {
        crossbeam_channel::select! {
            recv(receiver) -> command => {
                let Ok(command) = command else {
                    drain_and_flush_logs(
                        &context.storage_roots,
                        &context.log_consumer,
                        &context.log_writer,
                        &context.space_budget,
                    );
                    context.file_handles.sweep_close_requests();
                    return;
                };
                context.capacity_wake.notify_one();
                context.file_handles.sweep_close_requests();
                let operation_name = command.operation.name();
                let operation_result = catch_unwind(AssertUnwindSafe(|| {
                    execute(
                        command.operation,
                        &context.storage_roots,
                        &context.mutation_gates,
                        &context.file_handles,
                    )
                }))
                .unwrap_or_else(|_| {
                    Err(ExecutorError::new(
                        ExecutorErrorKind::WorkerPanic,
                        operation_name,
                        "file operation panicked",
                    ))
                });
                let _ = command.reply.send(operation_result);
                context.file_handles.sweep_close_requests();
            }
            recv(context.close_receiver) -> _ => context.file_handles.sweep_close_requests(),
            recv(context.log_consumer.receiver()) -> event => {
                let Ok(event) = event else {
                    continue;
                };
                let Some(roots) = context.storage_roots.get() else {
                    continue;
                };
                let Ok(logs) = roots.directory(StorageRootId::Logs) else {
                    continue;
                };
                let Ok(mut writer) = context.log_writer.lock() else {
                    continue;
                };
                writer.append_received(&context.log_consumer, logs, &context.space_budget, event);
            }
        }
    }
}

fn drain_and_flush_logs(
    storage_roots: &OnceLock<Arc<StorageRootRegistry>>,
    consumer: &LogEventConsumer,
    writer: &Mutex<RuntimeLogWriter>,
    space_budget: &DataDirSpaceBudget,
) {
    let Some(roots) = storage_roots.get() else {
        return;
    };
    let Ok(logs) = roots.directory(StorageRootId::Logs) else {
        return;
    };
    let Ok(mut writer) = writer.lock() else {
        return;
    };
    writer.drain_all(consumer, logs, space_budget);
    let _ = writer.flush();
}

fn execute(
    operation: FileOperation,
    storage_roots: &OnceLock<Arc<StorageRootRegistry>>,
    mutation_gates: &Arc<MutationGateRegistry>,
    file_handles: &Arc<FileHandleRegistry>,
) -> Result<FileOutput, ExecutorError> {
    let _operation_spec = operation.spec();
    match operation {
        FileOperation::Probe { sequence } => Ok(FileOutput::Probe {
            sequence,
            thread_name: std::thread::current()
                .name()
                .unwrap_or("unnamed")
                .to_string(),
        }),
        FileOperation::ReadConfig { expected } => {
            let observed =
                crate::runtime::config_bootstrap::read_existing_config(&expected.canonical_path)
                    .map_err(|error| file_error("read_config", error))?;
            if observed.identity != expected {
                return Err(ExecutorError::new(
                    ExecutorErrorKind::InvalidInput,
                    "read_config",
                    "config identity or content changed",
                ));
            }
            Ok(FileOutput::ConfigRead(observed.contents))
        }
        FileOperation::ReplaceConfig { expected, contents } => {
            crate::runtime::config_bootstrap::replace_config_if_unchanged(&expected, &contents)
                .map(FileOutput::ConfigReplaced)
                .map_err(|error| file_error("replace_config", error))
        }
        FileOperation::PublishJournalEntry {
            authorization,
            storage_root,
            temporary_path,
            destination_path,
            expected_size,
            expected_version,
        } => {
            require_mutation_stage(
                authorization.stage(),
                &[JournalMutationStage::Publication],
                "publish_journal_entry",
            )?;
            let (root, _mutation) = authorized_root(
                storage_roots,
                mutation_gates,
                &authorization,
                storage_root,
                "publish_journal_entry",
            )?;
            publish_journal_entry(
                root,
                &temporary_path,
                &destination_path,
                expected_size,
                expected_version.as_deref(),
            )
            .map(FileOutput::JournalEntryPublished)
            .map_err(|error| file_error("publish_journal_entry", error))
        }
        FileOperation::RenameJournalEntry {
            authorization,
            action,
            storage_root,
            source_path,
            destination_path,
            expected_size,
            expected_version,
        } => {
            if !matches!(action, FileEntryAction::Move | FileEntryAction::Tombstone) {
                return Err(ExecutorError::new(
                    ExecutorErrorKind::InvalidInput,
                    "rename_journal_entry",
                    "journal rename action must be move or tombstone",
                ));
            }
            require_mutation_stage(
                authorization.stage(),
                &[JournalMutationStage::Publication],
                "rename_journal_entry",
            )?;
            let (root, _mutation) = authorized_root(
                storage_roots,
                mutation_gates,
                &authorization,
                storage_root,
                "rename_journal_entry",
            )?;
            rename_journal_entry(
                root,
                &source_path,
                &destination_path,
                expected_size,
                expected_version.as_deref(),
            )
            .map(FileOutput::JournalEntryRenamed)
            .map_err(|error| file_error("rename_journal_entry", error))
        }
        FileOperation::CleanupJournalEntry {
            authorization,
            storage_root,
            source_path,
            expected_size,
        } => {
            require_mutation_stage(
                authorization.stage(),
                &[
                    JournalMutationStage::Cleanup,
                    JournalMutationStage::Rollback,
                ],
                "cleanup_journal_entry",
            )?;
            let (root, _mutation) = authorized_root(
                storage_roots,
                mutation_gates,
                &authorization,
                storage_root,
                "cleanup_journal_entry",
            )?;
            cleanup_journal_entry(root, &source_path, expected_size)
                .map(FileOutput::JournalEntryCleaned)
                .map_err(|error| file_error("cleanup_journal_entry", error))
        }
        FileOperation::OpenStorageWriteSession {
            storage_root,
            path,
            rollback_length,
        } => {
            let root =
                storage_root_directory(storage_roots, storage_root, "open_storage_write_session")?;
            let (parent, name) = create_parent_directories(root, &path)
                .map_err(|error| file_error("open_storage_write_session", error))?;
            let (mut file, created) = open_regular_file_for_write(&parent, &name)
                .map_err(|error| file_error("open_storage_write_session", error))?;
            if created {
                sync_directory(&parent)
                    .map_err(|error| file_error("open_storage_write_session", error))?;
            }
            file.set_len(rollback_length)
                .map_err(|error| file_error("open_storage_write_session", error))?;
            file.seek(SeekFrom::Start(rollback_length))
                .map_err(|error| file_error("open_storage_write_session", error))?;
            file_handles
                .register(RegisteredFile {
                    file,
                    rollback_length: Some(rollback_length),
                    child_access: Some(ChildDescriptorAccess::Write),
                })
                .map(FileOutput::StorageSessionOpened)
                .map_err(|error| file_error("open_storage_write_session", error))
        }
        FileOperation::OpenStorageReadSession { storage_root, path } => {
            let root =
                storage_root_directory(storage_roots, storage_root, "open_storage_read_session")?;
            let (parent, name) = open_parent(root, &path)
                .map_err(|error| file_error("open_storage_read_session", error))?;
            let mut file = open_regular_file_for_read(&parent, &name)
                .map_err(|error| file_error("open_storage_read_session", error))?;
            file.seek(SeekFrom::Start(0))
                .map_err(|error| file_error("open_storage_read_session", error))?;
            let snapshot = snapshot_regular_file(&file)
                .map_err(|error| file_error("open_storage_read_session", error))?;
            file_handles
                .register(RegisteredFile {
                    file,
                    rollback_length: None,
                    child_access: Some(ChildDescriptorAccess::Read),
                })
                .map(|session| FileOutput::StorageReadSessionOpened { session, snapshot })
                .map_err(|error| file_error("open_storage_read_session", error))
        }
        FileOperation::OpenStorageDirectorySession { storage_root, path } => {
            let root = storage_root_directory(
                storage_roots,
                storage_root,
                "open_storage_directory_session",
            )?;
            let directory = open_storage_directory(root, path.as_ref())
                .map_err(|error| file_error("open_storage_directory_session", error))?;
            file_handles
                .register(RegisteredFile {
                    file: directory,
                    rollback_length: None,
                    child_access: None,
                })
                .map(FileOutput::StorageDirectorySessionOpened)
                .map_err(|error| file_error("open_storage_directory_session", error))
        }
        FileOperation::CreateStorageDirectory { storage_root, path } => {
            let root =
                storage_root_directory(storage_roots, storage_root, "create_storage_directory")?;
            create_storage_directory(root, &path)
                .map(|()| FileOutput::StorageDirectoryCreated)
                .map_err(|error| file_error("create_storage_directory", error))
        }
        FileOperation::SetStorageModifiedTime {
            storage_root,
            path,
            seconds,
            nanoseconds,
        } => {
            let root =
                storage_root_directory(storage_roots, storage_root, "set_storage_modified_time")?;
            let (parent, name) = open_parent(root, &path)
                .map_err(|error| file_error("set_storage_modified_time", error))?;
            let file = open_regular_file_for_read(&parent, &name)
                .map_err(|error| file_error("set_storage_modified_time", error))?;
            let times = [
                libc::timespec {
                    tv_sec: 0,
                    tv_nsec: libc::UTIME_OMIT,
                },
                libc::timespec {
                    tv_sec: seconds,
                    tv_nsec: i64::from(nanoseconds),
                },
            ];
            if unsafe { libc::futimens(file.as_raw_fd(), times.as_ptr()) } != 0 {
                return Err(file_error(
                    "set_storage_modified_time",
                    std::io::Error::last_os_error(),
                ));
            }
            file.sync_all()
                .and_then(|()| sync_directory(&parent))
                .map(|()| FileOutput::StorageModifiedTimeSet)
                .map_err(|error| file_error("set_storage_modified_time", error))
        }
        FileOperation::PinStorageSessionForChild {
            session,
            child_fd,
            access,
        } => {
            if !session.belongs_to(file_handles) {
                return Err(ExecutorError::new(
                    ExecutorErrorKind::InvalidInput,
                    "pin_storage_session_for_child",
                    "file session belongs to another executor",
                ));
            }
            file_handles
                .pin_for_child(session, child_fd, access)
                .map(FileOutput::ChildDescriptorPinned)
                .map_err(|error| file_error("pin_storage_session_for_child", error))
        }
        FileOperation::ReturnStorageSessionFromChild { lease } => file_handles
            .return_from_child(lease)
            .map(FileOutput::StorageSessionReturned)
            .map_err(|error| file_error("return_storage_session_from_child", error)),
        FileOperation::InspectStorageSession { session } => {
            if !session.belongs_to(file_handles) {
                return Err(ExecutorError::new(
                    ExecutorErrorKind::InvalidInput,
                    "inspect_storage_session",
                    "file session belongs to another executor",
                ));
            }
            let mut handle = file_handles
                .begin(session)
                .map_err(|error| file_error("inspect_storage_session", error))?;
            let snapshot = snapshot_regular_file(
                handle
                    .file_mut()
                    .map_err(|error| file_error("inspect_storage_session", error))?,
            )
            .map_err(|error| file_error("inspect_storage_session", error))?;
            handle
                .finish()
                .map(|session| FileOutput::StorageSessionInspected { session, snapshot })
                .map_err(|error| file_error("inspect_storage_session", error))
        }
        FileOperation::AtomicReplaceStorageFile {
            storage_root,
            temporary_path,
            destination_path,
            contents,
        } => {
            let root =
                storage_root_directory(storage_roots, storage_root, "atomic_replace_storage_file")?;
            atomic_replace_storage_file(root, &temporary_path, &destination_path, &contents)
                .map(|()| FileOutput::StorageFileAtomicallyReplaced)
                .map_err(|error| file_error("atomic_replace_storage_file", error))
        }
        FileOperation::WriteStorageSession { session, bytes } => {
            if !session.belongs_to(file_handles) {
                return Err(ExecutorError::new(
                    ExecutorErrorKind::InvalidInput,
                    "write_storage_session",
                    "file session belongs to another executor",
                ));
            }
            let mut file = file_handles
                .begin(session)
                .map_err(|error| file_error("write_storage_session", error))?;
            file.file_mut()
                .and_then(|file| file.write_all(&bytes))
                .map_err(|error| file_error("write_storage_session", error))?;
            let written = bytes.len();
            file.finish()
                .map(|session| FileOutput::StorageSessionWritten { session, written })
                .map_err(|error| file_error("write_storage_session", error))
        }
        FileOperation::ReadStorageSession {
            session,
            maximum_bytes,
        } => {
            if !session.belongs_to(file_handles) {
                return Err(ExecutorError::new(
                    ExecutorErrorKind::InvalidInput,
                    "read_storage_session",
                    "file session belongs to another executor",
                ));
            }
            let mut file = file_handles
                .begin(session)
                .map_err(|error| file_error("read_storage_session", error))?;
            let mut bytes = vec![0_u8; maximum_bytes];
            let read = file
                .file_mut()
                .and_then(|file| file.read(&mut bytes))
                .map_err(|error| file_error("read_storage_session", error))?;
            bytes.truncate(read);
            file.finish()
                .map(|session| FileOutput::StorageSessionRead { session, bytes })
                .map_err(|error| file_error("read_storage_session", error))
        }
        FileOperation::ReadStorageDirectorySession { session } => {
            if !session.belongs_to(file_handles) {
                return Err(ExecutorError::new(
                    ExecutorErrorKind::InvalidInput,
                    "read_storage_directory_session",
                    "directory session belongs to another executor",
                ));
            }
            let mut directory = file_handles
                .begin(session)
                .map_err(|error| file_error("read_storage_directory_session", error))?;
            let (entries, finished) = read_storage_directory_chunk(
                directory
                    .file_mut()
                    .map_err(|error| file_error("read_storage_directory_session", error))?,
            )
            .map_err(|error| file_error("read_storage_directory_session", error))?;
            directory
                .finish()
                .map(|session| FileOutput::StorageDirectorySessionRead {
                    session,
                    entries,
                    finished,
                })
                .map_err(|error| file_error("read_storage_directory_session", error))
        }
        FileOperation::SeekStorageReadSession { session, offset } => {
            if !session.belongs_to(file_handles) {
                return Err(ExecutorError::new(
                    ExecutorErrorKind::InvalidInput,
                    "seek_storage_read_session",
                    "file session belongs to another executor",
                ));
            }
            let mut file = file_handles
                .begin(session)
                .map_err(|error| file_error("seek_storage_read_session", error))?;
            file.file_mut()
                .and_then(|file| file.seek(SeekFrom::Start(offset)))
                .map_err(|error| file_error("seek_storage_read_session", error))?;
            file.finish()
                .map(FileOutput::StorageReadSessionSeeked)
                .map_err(|error| file_error("seek_storage_read_session", error))
        }
        FileOperation::CommitStorageSession { session } => {
            if !session.belongs_to(file_handles) {
                return Err(ExecutorError::new(
                    ExecutorErrorKind::InvalidInput,
                    "commit_storage_session",
                    "file session belongs to another executor",
                ));
            }
            file_handles
                .begin(session)
                .and_then(|file| file.commit())
                .map(|()| FileOutput::StorageSessionClosed)
                .map_err(|error| file_error("commit_storage_session", error))
        }
        FileOperation::AbortStorageSession { session } => {
            if !session.belongs_to(file_handles) {
                return Err(ExecutorError::new(
                    ExecutorErrorKind::InvalidInput,
                    "abort_storage_session",
                    "file session belongs to another executor",
                ));
            }
            file_handles
                .begin(session)
                .and_then(|file| file.abort())
                .map(|()| FileOutput::StorageSessionClosed)
                .map_err(|error| file_error("abort_storage_session", error))
        }
        FileOperation::CloseStorageSession { session } => {
            if !session.belongs_to(file_handles) {
                return Err(ExecutorError::new(
                    ExecutorErrorKind::InvalidInput,
                    "close_storage_session",
                    "file session belongs to another executor",
                ));
            }
            file_handles
                .begin(session)
                .and_then(|file| file.close())
                .map(|()| FileOutput::StorageSessionClosed)
                .map_err(|error| file_error("close_storage_session", error))
        }
    }
}

fn storage_root_directory<'a>(
    storage_roots: &'a OnceLock<Arc<StorageRootRegistry>>,
    storage_root: StorageRootId,
    operation: &'static str,
) -> Result<&'a File, ExecutorError> {
    storage_roots
        .get()
        .ok_or_else(|| {
            ExecutorError::new(
                ExecutorErrorKind::Internal,
                operation,
                "storage-root registry is not ready",
            )
        })?
        .directory(storage_root)
        .map_err(|error| {
            ExecutorError::new(
                ExecutorErrorKind::FileNotFound,
                operation,
                error.to_string(),
            )
        })
}

fn require_mutation_stage(
    actual: JournalMutationStage,
    allowed: &[JournalMutationStage],
    operation: &'static str,
) -> Result<(), ExecutorError> {
    if allowed.contains(&actual) {
        Ok(())
    } else {
        Err(ExecutorError::new(
            ExecutorErrorKind::InvalidInput,
            operation,
            "journal mutation grant does not authorize this stage",
        ))
    }
}

fn authorized_root<'a>(
    storage_roots: &'a OnceLock<Arc<StorageRootRegistry>>,
    mutation_gates: &Arc<MutationGateRegistry>,
    authorization: &MutationAuthorization,
    storage_root: StorageRootId,
    operation: &'static str,
) -> Result<(&'a File, MutationOperationGuard), ExecutorError> {
    let mutation = mutation_gates
        .begin_operation(authorization)
        .map_err(|error| mutation_lease_error(operation, error))?;
    let root = storage_roots
        .get()
        .ok_or_else(|| {
            ExecutorError::new(
                ExecutorErrorKind::Internal,
                operation,
                "storage-root registry is not ready",
            )
        })?
        .directory(storage_root)
        .map_err(|error| {
            ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                operation,
                error.to_string(),
            )
        })?;
    Ok((root, mutation))
}

fn file_error(operation: &'static str, error: std::io::Error) -> ExecutorError {
    let kind = match error.kind() {
        std::io::ErrorKind::NotFound => ExecutorErrorKind::FileNotFound,
        std::io::ErrorKind::PermissionDenied => ExecutorErrorKind::FilePermission,
        std::io::ErrorKind::AlreadyExists => ExecutorErrorKind::FileConflict,
        std::io::ErrorKind::InvalidData | std::io::ErrorKind::InvalidInput => {
            ExecutorErrorKind::FileInvalidData
        }
        std::io::ErrorKind::Interrupted
        | std::io::ErrorKind::WouldBlock
        | std::io::ErrorKind::TimedOut => ExecutorErrorKind::FileTransient,
        _ => ExecutorErrorKind::FileSystem,
    };
    ExecutorError::new(kind, operation, error.to_string())
}

fn publish_journal_entry(
    root: &File,
    temporary_path: &NormalizedStoragePath,
    destination_path: &NormalizedStoragePath,
    expected_size: Option<u64>,
    expected_version: Option<&str>,
) -> std::io::Result<PublishJournalOutcome> {
    match rename_entry(
        root,
        temporary_path,
        destination_path,
        expected_size,
        expected_version,
    )? {
        RenameJournalOutcome::Renamed => Ok(PublishJournalOutcome::Published),
        RenameJournalOutcome::AlreadyRenamed => Ok(PublishJournalOutcome::AlreadyPublished),
    }
}

fn rename_journal_entry(
    root: &File,
    source_path: &NormalizedStoragePath,
    destination_path: &NormalizedStoragePath,
    expected_size: Option<u64>,
    expected_version: Option<&str>,
) -> std::io::Result<RenameJournalOutcome> {
    rename_entry(
        root,
        source_path,
        destination_path,
        expected_size,
        expected_version,
    )
}

fn rename_entry(
    root: &File,
    source_path: &NormalizedStoragePath,
    destination_path: &NormalizedStoragePath,
    expected_size: Option<u64>,
    expected_version: Option<&str>,
) -> std::io::Result<RenameJournalOutcome> {
    let (source_parent, source_name) = open_parent(root, source_path)?;
    let (destination_parent, destination_name) = open_parent(root, destination_path)?;
    match open_entry_at(&source_parent, &source_name) {
        Ok(source) => {
            verify_entry(&source, expected_size, expected_version)?;
            rename_descriptor_entry(
                source_parent.as_raw_fd(),
                &source_name,
                destination_parent.as_raw_fd(),
                &destination_name,
                libc::RENAME_NOREPLACE,
            )?;
            sync_directory(&source_parent)?;
            sync_directory(&destination_parent)?;
            Ok(RenameJournalOutcome::Renamed)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let destination = open_entry_at(&destination_parent, &destination_name)?;
            verify_entry(&destination, expected_size, expected_version)?;
            sync_directory(&source_parent)?;
            sync_directory(&destination_parent)?;
            Ok(RenameJournalOutcome::AlreadyRenamed)
        }
        Err(error) => Err(error),
    }
}

fn atomic_replace_storage_file(
    root: &File,
    temporary_path: &NormalizedStoragePath,
    destination_path: &NormalizedStoragePath,
    contents: &[u8],
) -> std::io::Result<()> {
    let (temporary_parent, temporary_name) = create_parent_directories(root, temporary_path)?;
    let (destination_parent, destination_name) = create_parent_directories(root, destination_path)?;
    let result = (|| {
        let (mut temporary_file, created) =
            open_regular_file_for_write(&temporary_parent, &temporary_name)?;
        if created {
            sync_directory(&temporary_parent)?;
        }
        temporary_file.set_len(0)?;
        temporary_file.seek(SeekFrom::Start(0))?;
        temporary_file.write_all(contents)?;
        temporary_file.sync_all()?;
        drop(temporary_file);
        rename_descriptor_entry(
            temporary_parent.as_raw_fd(),
            &temporary_name,
            destination_parent.as_raw_fd(),
            &destination_name,
            0,
        )?;
        sync_directory(&temporary_parent)?;
        sync_directory(&destination_parent)
    })();
    if result.is_err() {
        let removed =
            unsafe { libc::unlinkat(temporary_parent.as_raw_fd(), temporary_name.as_ptr(), 0) };
        if removed == 0 {
            let _ = sync_directory(&temporary_parent);
        }
    }
    result
}

fn cleanup_journal_entry(
    root: &File,
    source_path: &NormalizedStoragePath,
    expected_size: Option<u64>,
) -> std::io::Result<CleanupJournalOutcome> {
    let (parent, name) = match open_parent(root, source_path) {
        Ok(parent) => parent,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            root.sync_all()?;
            return Ok(CleanupJournalOutcome::AlreadyAbsent);
        }
        Err(error) => return Err(error),
    };
    let entry = match open_entry_at(&parent, &name) {
        Ok(entry) => entry,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            sync_directory(&parent)?;
            return Ok(CleanupJournalOutcome::AlreadyAbsent);
        }
        Err(error) => return Err(error),
    };
    verify_entry(&entry, expected_size, None)?;
    if entry.metadata()?.is_dir() {
        let mut remaining = 256_usize;
        if !cleanup_directory_tree(&parent, &name, &mut remaining, 0)? {
            return Ok(CleanupJournalOutcome::ProgressPending);
        }
        return Ok(CleanupJournalOutcome::Removed);
    }
    let flags = 0;
    let result = unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }
    sync_directory(&parent)?;
    Ok(CleanupJournalOutcome::Removed)
}

fn cleanup_directory_tree(
    parent: &OwnedFd,
    name: &CString,
    remaining: &mut usize,
    depth: usize,
) -> std::io::Result<bool> {
    if depth >= crate::io::file::MAX_STORAGE_PATH_COMPONENTS {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "cleanup directory tree exceeds the bounded depth",
        ));
    }
    if *remaining == 0 {
        return Ok(false);
    }
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let directory = unsafe { libc::fdopendir(descriptor) };
    if directory.is_null() {
        let error = std::io::Error::last_os_error();
        unsafe {
            libc::close(descriptor);
        }
        return Err(error);
    }
    let directory = DirectoryStream(directory);
    loop {
        unsafe {
            *libc::__errno_location() = 0;
        }
        let entry = unsafe { libc::readdir(directory.0) };
        if entry.is_null() {
            let error_code = unsafe { *libc::__errno_location() };
            if error_code != 0 {
                return Err(std::io::Error::from_raw_os_error(error_code));
            }
            break;
        }
        let entry_name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) };
        if entry_name.to_bytes() == b"." || entry_name.to_bytes() == b".." {
            continue;
        }
        let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe {
            libc::fstatat(
                libc::dirfd(directory.0),
                entry_name.as_ptr(),
                metadata.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::NotFound {
                continue;
            }
            return Err(error);
        }
        let metadata = unsafe { metadata.assume_init() };
        let file_type = metadata.st_mode & libc::S_IFMT;
        if file_type == libc::S_IFDIR {
            let child_name = CString::new(entry_name.to_bytes())
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
            let child_parent_descriptor =
                unsafe { libc::fcntl(libc::dirfd(directory.0), libc::F_DUPFD_CLOEXEC, 0) };
            if child_parent_descriptor < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let child_parent = unsafe { OwnedFd::from_raw_fd(child_parent_descriptor) };
            if !cleanup_directory_tree(&child_parent, &child_name, remaining, depth + 1)? {
                return Ok(false);
            }
        } else if file_type == libc::S_IFREG {
            if *remaining == 0 {
                return Ok(false);
            }
            if unsafe { libc::unlinkat(libc::dirfd(directory.0), entry_name.as_ptr(), 0) } != 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::NotFound {
                    return Err(error);
                }
            }
            *remaining -= 1;
            let directory_descriptor = unsafe { libc::dirfd(directory.0) };
            sync_raw_directory(directory_descriptor)?;
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "cleanup directory contains a symlink or unsupported file type",
            ));
        }
        if *remaining == 0 {
            return Ok(false);
        }
    }
    drop(directory);
    if *remaining == 0 {
        return Ok(false);
    }
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::NotFound {
            return Err(error);
        }
    }
    *remaining -= 1;
    sync_directory(parent)?;
    Ok(true)
}

struct DirectoryStream(*mut libc::DIR);

impl Drop for DirectoryStream {
    fn drop(&mut self) {
        unsafe {
            libc::closedir(self.0);
        }
    }
}

fn sync_raw_directory(descriptor: libc::c_int) -> std::io::Result<()> {
    if unsafe { libc::fsync(descriptor) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

fn open_storage_directory(
    root: &File,
    path: Option<&NormalizedStoragePath>,
) -> std::io::Result<File> {
    let descriptor = if let Some(path) = path {
        let path = CString::new(path.relative_path())
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        let how = crate::io::file::directory_open_how();
        unsafe {
            libc::syscall(
                libc::SYS_openat2,
                root.as_raw_fd(),
                path.as_ptr(),
                &how,
                size_of::<libc::open_how>(),
            ) as libc::c_int
        }
    } else {
        unsafe { libc::fcntl(root.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) }
    };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let directory = unsafe { File::from_raw_fd(descriptor) };
    if !directory.metadata()?.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "opened storage descriptor is not a directory",
        ));
    }
    Ok(directory)
}

fn read_storage_directory_chunk(
    directory: &File,
) -> std::io::Result<(Vec<StorageDirectoryEntry>, bool)> {
    let mut buffer = vec![0_u8; STORAGE_DIRECTORY_CHUNK_BYTES];
    let read = unsafe {
        libc::syscall(
            libc::SYS_getdents64,
            directory.as_raw_fd(),
            buffer.as_mut_ptr(),
            buffer.len(),
        ) as libc::ssize_t
    };
    if read < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if read == 0 {
        return Ok((Vec::new(), true));
    }
    let used = usize::try_from(read)
        .map_err(|_| std::io::Error::other("directory read size is invalid"))?;
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(FILE_IO_ENTRY_BATCH)
        .map_err(|error| {
            std::io::Error::other(format!("could not reserve directory entry batch: {error}"))
        })?;
    let mut offset = 0usize;
    while offset < used {
        if used - offset < 19 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "directory entry header is truncated",
            ));
        }
        let record_length = usize::from(u16::from_ne_bytes([
            buffer[offset + 16],
            buffer[offset + 17],
        ]));
        if record_length < 20 || record_length > used - offset {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "directory entry length is invalid",
            ));
        }
        let name_bytes = &buffer[offset + 19..offset + record_length];
        let name_length = name_bytes
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "directory entry name is unterminated",
                )
            })?;
        let name_bytes = &name_bytes[..name_length];
        if name_bytes != b"." && name_bytes != b".." {
            let name = std::str::from_utf8(name_bytes).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "directory entry name is not UTF-8",
                )
            })?;
            NormalizedStoragePath::parse(name).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "directory entry name is not a normalized component",
                )
            })?;
            let name_c = CString::new(name_bytes)
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidData))?;
            let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
            if unsafe {
                libc::fstatat(
                    directory.as_raw_fd(),
                    name_c.as_ptr(),
                    metadata.as_mut_ptr(),
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            } != 0
            {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::NotFound {
                    offset += record_length;
                    continue;
                }
                return Err(error);
            }
            let metadata = unsafe { metadata.assume_init() };
            let kind = match metadata.st_mode & libc::S_IFMT {
                libc::S_IFREG => StorageDirectoryEntryKind::File,
                libc::S_IFDIR => StorageDirectoryEntryKind::Directory,
                _ => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "storage directory contains a symlink or unsupported entry",
                    ));
                }
            };
            entries.push(StorageDirectoryEntry {
                name: name.to_string(),
                kind,
                resume_offset: u64::try_from(i64::from_ne_bytes(
                    buffer[offset + 8..offset + 16]
                        .try_into()
                        .expect("validated directory entry offset bytes"),
                ))
                .map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "directory entry resume offset is negative",
                    )
                })?,
            });
            if entries.len() == FILE_IO_ENTRY_BATCH {
                let next_offset = i64::from_ne_bytes(
                    buffer[offset + 8..offset + 16]
                        .try_into()
                        .expect("validated directory entry offset bytes"),
                );
                if unsafe { libc::lseek(directory.as_raw_fd(), next_offset, libc::SEEK_SET) } < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                return Ok((entries, false));
            }
        }
        offset += record_length;
    }
    Ok((entries, false))
}

fn open_parent(root: &File, path: &NormalizedStoragePath) -> std::io::Result<(OwnedFd, CString)> {
    let (parent_path, filename) = path
        .relative_path()
        .rsplit_once('/')
        .map_or((None, path.relative_path()), |(parent, filename)| {
            (Some(parent), filename)
        });
    let filename = CString::new(filename)
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let descriptor = if let Some(parent_path) = parent_path {
        let parent_path = CString::new(parent_path)
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        let how = crate::io::file::directory_open_how();
        unsafe {
            libc::syscall(
                libc::SYS_openat2,
                root.as_raw_fd(),
                parent_path.as_ptr(),
                &how,
                size_of::<libc::open_how>(),
            ) as libc::c_int
        }
    } else {
        unsafe { libc::fcntl(root.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) }
    };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok((unsafe { OwnedFd::from_raw_fd(descriptor) }, filename))
}

fn create_storage_directory(root: &File, path: &NormalizedStoragePath) -> std::io::Result<()> {
    let (parent, name) = open_parent(root, path)?;
    if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    sync_directory(&parent)
}

fn open_entry_at(parent: &OwnedFd, name: &CString) -> std::io::Result<File> {
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let file = unsafe { File::from_raw_fd(descriptor) };
    let metadata = file.metadata()?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "journal entry is neither a regular file nor a directory",
        ));
    }
    Ok(file)
}

fn verify_entry(
    file: &File,
    expected_size: Option<u64>,
    expected_version: Option<&str>,
) -> std::io::Result<()> {
    let metadata = file.metadata()?;
    if let Some(expected_size) = expected_size {
        if !metadata.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "journal entry with expected size is not a regular file",
            ));
        }
        let observed_size = metadata.len();
        if observed_size != expected_size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "journal entry size mismatch: expected {expected_size}, observed {observed_size}"
                ),
            ));
        }
    }
    if let Some(expected_version) = expected_version {
        let observed = snapshot_regular_file(file)?.identity_version();
        if observed != expected_version {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "journal entry identity changed before mutation",
            ));
        }
    }
    Ok(())
}

fn sync_directory(directory: &OwnedFd) -> std::io::Result<()> {
    let result = unsafe { libc::fsync(directory.as_raw_fd()) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

fn create_parent_directories(
    root: &File,
    path: &NormalizedStoragePath,
) -> std::io::Result<(OwnedFd, CString)> {
    let mut components = path.relative_path().split('/').peekable();
    let filename = components
        .next_back()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let root_descriptor = unsafe { libc::fcntl(root.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) };
    if root_descriptor < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut parent = unsafe { OwnedFd::from_raw_fd(root_descriptor) };
    for component in components {
        let component = CString::new(component)
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        let created = unsafe { libc::mkdirat(parent.as_raw_fd(), component.as_ptr(), 0o700) } == 0;
        if !created {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(std::io::Error::new(
                    error.kind(),
                    format!(
                        "could not create storage directory {}: {error}",
                        component.to_string_lossy()
                    ),
                ));
            }
        }
        if created {
            sync_directory(&parent)?;
        }
        let descriptor = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                component.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            let error = std::io::Error::last_os_error();
            return Err(std::io::Error::new(
                error.kind(),
                format!(
                    "could not open storage directory {}: {error}",
                    component.to_string_lossy()
                ),
            ));
        }
        parent = unsafe { OwnedFd::from_raw_fd(descriptor) };
    }
    let filename = CString::new(filename)
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    Ok((parent, filename))
}

fn open_regular_file_for_write(parent: &OwnedFd, name: &CString) -> std::io::Result<(File, bool)> {
    let mut descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDWR | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    let mut created = false;
    if descriptor < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::NotFound {
            return Err(error);
        }
        descriptor = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_RDWR | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if descriptor < 0 {
            return Err(std::io::Error::last_os_error());
        }
        created = true;
    }
    let file = unsafe { File::from_raw_fd(descriptor) };
    require_regular_file(&file)?;
    Ok((file, created))
}

fn open_regular_file_for_read(parent: &OwnedFd, name: &CString) -> std::io::Result<File> {
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let file = unsafe { File::from_raw_fd(descriptor) };
    require_regular_file(&file)?;
    Ok(file)
}

fn require_regular_file(file: &File) -> std::io::Result<()> {
    if file.metadata()?.is_file() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "storage path is not a regular file",
        ))
    }
}
