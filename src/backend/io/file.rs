use std::collections::HashMap;
use std::ffi::CString;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

use super::journal::{
    AuthorizedJournalEntry, FileEntryAction, JournalMutationGrant, JournalMutationStage,
};
use super::session::{descriptor_statx, rename_descriptor_entry};
use super::space_budget::DataDirSpaceBudget;

pub const MAX_STORAGE_PATH_BYTES: usize = 4096;
pub const MAX_STORAGE_PATH_COMPONENTS: usize = 256;
pub const MAX_STORAGE_PATH_COMPONENT_BYTES: usize = 255;
pub const MAX_STORAGE_PATH_KEY_BYTES: usize =
    MAX_STORAGE_PATH_BYTES + 4 * MAX_STORAGE_PATH_COMPONENTS;
pub const MAX_FILE_OPERATION_ID_BYTES: usize = 128;
pub const LLM_RESULT_INBOX_DIRECTORY: &str = "llm-results";
const RENAME_PROBE_PREFIX: &str = ".momento-rename-probe-";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StorageRootId {
    Originals,
    Thumbnails,
    TinyThumbnails,
    PlaceThumbnails,
    Previews,
    Imports,
    Albums,
    Trash,
    WebDav,
    Backups,
    Logs,
    Journal,
    Static,
}

#[derive(Debug)]
struct StorageRootCapability {
    id: StorageRootId,
    directory: Option<Arc<File>>,
}

#[derive(Debug)]
pub struct StorageRootRegistry {
    capabilities: Vec<StorageRootCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationAuthorization {
    group_id: String,
    group_version: i64,
    owner_generation: u64,
    stage: JournalMutationStage,
}

impl MutationAuthorization {
    pub fn group_id(&self) -> &str {
        &self.group_id
    }

    pub fn group_version(&self) -> i64 {
        self.group_version
    }

    pub(crate) fn stage(&self) -> JournalMutationStage {
        self.stage
    }
}

#[derive(Debug)]
struct MutationOwner {
    group_version: i64,
    owner_generation: u64,
    stage: JournalMutationStage,
    active_operation: bool,
    closed: bool,
}

#[derive(Debug)]
struct MutationGate {
    epoch: u64,
    minimum_version: i64,
    candidate_count: usize,
    owner: Option<MutationOwner>,
    retain_fence: bool,
}

#[derive(Debug)]
pub(crate) struct MutationGateRegistry {
    next_generation: AtomicU64,
    gates: Mutex<HashMap<String, MutationGate>>,
    changed: Notify,
    maximum_owners: usize,
}

impl MutationGateRegistry {
    pub(crate) fn new(maximum_owners: usize) -> Result<Self, MutationLeaseError> {
        if maximum_owners == 0 {
            return Err(MutationLeaseError::Capacity);
        }
        let mut gates = HashMap::new();
        gates
            .try_reserve(maximum_owners)
            .map_err(|_| MutationLeaseError::Capacity)?;
        Ok(Self {
            next_generation: AtomicU64::new(0),
            gates: Mutex::new(gates),
            changed: Notify::new(),
            maximum_owners,
        })
    }

    pub(crate) fn reserve(
        self: &Arc<Self>,
        group_id: &str,
        group_version: i64,
    ) -> Result<JournalMutationTicket, MutationLeaseError> {
        if group_id.is_empty() || group_id.len() > MAX_FILE_OPERATION_ID_BYTES || group_version < 1
        {
            return Err(MutationLeaseError::InvalidIdentity);
        }
        let mut gates = self
            .gates
            .lock()
            .map_err(|_| MutationLeaseError::Poisoned)?;
        let epoch = match gates.get_mut(group_id) {
            Some(gate) => {
                if group_version < gate.minimum_version {
                    return Err(MutationLeaseError::Fenced);
                }
                if gate.retain_fence {
                    gate.epoch = self.next_generation()?;
                    gate.retain_fence = false;
                }
                gate.candidate_count = gate
                    .candidate_count
                    .checked_add(1)
                    .ok_or(MutationLeaseError::Capacity)?;
                gate.epoch
            }
            None => {
                if gates.len() >= self.maximum_owners {
                    return Err(MutationLeaseError::Capacity);
                }
                let epoch = self.next_generation()?;
                gates.insert(
                    group_id.to_string(),
                    MutationGate {
                        epoch,
                        minimum_version: group_version,
                        candidate_count: 1,
                        owner: None,
                        retain_fence: false,
                    },
                );
                epoch
            }
        };
        Ok(JournalMutationTicket {
            group_id: group_id.to_string(),
            group_version,
            epoch,
            gates: Arc::clone(self),
            active: true,
        })
    }

    fn next_generation(&self) -> Result<u64, MutationLeaseError> {
        self.next_generation
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |generation| {
                generation.checked_add(1)
            })
            .map(|generation| generation + 1)
            .map_err(|_| MutationLeaseError::GenerationExhausted)
    }

    fn acquire(
        self: &Arc<Self>,
        ticket: &mut JournalMutationTicket,
        grant: JournalMutationGrant,
    ) -> Result<JournalMutationLease, MutationLeaseError> {
        let (group_id, group_version, stage, entries) = grant.into_parts();
        if group_id != ticket.group_id || group_version != ticket.group_version {
            return Err(MutationLeaseError::InvalidIdentity);
        }
        let mut gates = self
            .gates
            .lock()
            .map_err(|_| MutationLeaseError::Poisoned)?;
        let gate = gates.get_mut(&group_id).ok_or(MutationLeaseError::Fenced)?;
        if gate.epoch != ticket.epoch || group_version < gate.minimum_version || gate.retain_fence {
            return Err(MutationLeaseError::Fenced);
        }
        if gate.owner.is_some() {
            return Err(MutationLeaseError::AlreadyOwned);
        }
        gate.candidate_count = gate
            .candidate_count
            .checked_sub(1)
            .ok_or(MutationLeaseError::Poisoned)?;
        gate.owner = Some(MutationOwner {
            group_version,
            owner_generation: ticket.epoch,
            stage,
            active_operation: false,
            closed: false,
        });
        ticket.active = false;
        Ok(JournalMutationLease {
            authorization: MutationAuthorization {
                group_id,
                group_version,
                owner_generation: ticket.epoch,
                stage,
            },
            gates: Arc::clone(self),
            entries,
            operation_started: false,
        })
    }

    pub(crate) fn begin_operation(
        self: &Arc<Self>,
        authorization: &MutationAuthorization,
    ) -> Result<MutationOperationGuard, MutationLeaseError> {
        let mut gates = self
            .gates
            .lock()
            .map_err(|_| MutationLeaseError::Poisoned)?;
        let gate = gates
            .get_mut(&authorization.group_id)
            .ok_or(MutationLeaseError::Fenced)?;
        if gate.epoch != authorization.owner_generation
            || authorization.group_version < gate.minimum_version
            || gate.retain_fence
        {
            return Err(MutationLeaseError::Fenced);
        }
        let owner = gate.owner.as_mut().ok_or(MutationLeaseError::Fenced)?;
        if owner.group_version != authorization.group_version
            || owner.owner_generation != authorization.owner_generation
            || owner.stage != authorization.stage
            || owner.closed
        {
            return Err(MutationLeaseError::Fenced);
        }
        if owner.active_operation {
            return Err(MutationLeaseError::OperationAlreadyStarted);
        }
        owner.active_operation = true;
        Ok(MutationOperationGuard {
            authorization: authorization.clone(),
            gates: Arc::clone(self),
        })
    }

    pub(crate) async fn fence(
        &self,
        group_id: &str,
        next_version: i64,
    ) -> Result<(), MutationLeaseError> {
        if group_id.is_empty() || group_id.len() > MAX_FILE_OPERATION_ID_BYTES || next_version < 2 {
            return Err(MutationLeaseError::InvalidIdentity);
        }
        {
            let mut gates = self
                .gates
                .lock()
                .map_err(|_| MutationLeaseError::Poisoned)?;
            let epoch = self.next_generation()?;
            match gates.get_mut(group_id) {
                Some(gate) => {
                    gate.epoch = epoch;
                    gate.minimum_version = gate.minimum_version.max(next_version);
                    gate.retain_fence = true;
                    if let Some(owner) = gate.owner.as_mut() {
                        owner.closed = true;
                    }
                }
                None => {
                    if gates.len() >= self.maximum_owners {
                        return Err(MutationLeaseError::Capacity);
                    }
                    gates.insert(
                        group_id.to_string(),
                        MutationGate {
                            epoch,
                            minimum_version: next_version,
                            candidate_count: 0,
                            owner: None,
                            retain_fence: true,
                        },
                    );
                }
            }
        }
        loop {
            let notified = self.changed.notified();
            let drained = {
                let mut gates = self
                    .gates
                    .lock()
                    .map_err(|_| MutationLeaseError::Poisoned)?;
                match gates.get_mut(group_id).and_then(|gate| gate.owner.as_mut()) {
                    Some(owner) if owner.active_operation => false,
                    Some(_) => {
                        if let Some(gate) = gates.get_mut(group_id) {
                            gate.owner = None;
                        }
                        true
                    }
                    None => true,
                }
            };
            if drained {
                return Ok(());
            }
            notified.await;
        }
    }

    pub(crate) fn release_fence(
        &self,
        group_id: &str,
        durable_version: i64,
    ) -> Result<(), MutationLeaseError> {
        let mut gates = self
            .gates
            .lock()
            .map_err(|_| MutationLeaseError::Poisoned)?;
        let Some(gate) = gates.get_mut(group_id) else {
            return Ok(());
        };
        if durable_version < gate.minimum_version || gate.owner.is_some() {
            return Err(MutationLeaseError::Fenced);
        }
        gate.epoch = self.next_generation()?;
        gate.retain_fence = false;
        if gate.candidate_count == 0 {
            gates.remove(group_id);
        }
        Ok(())
    }

    fn release_candidate(&self, group_id: &str) {
        if let Ok(mut gates) = self.gates.lock() {
            if let Some(gate) = gates.get_mut(group_id) {
                gate.candidate_count = gate.candidate_count.saturating_sub(1);
                if gate.candidate_count == 0 && gate.owner.is_none() && !gate.retain_fence {
                    gates.remove(group_id);
                }
            }
        }
    }

    fn release(&self, authorization: &MutationAuthorization) {
        if let Ok(mut gates) = self.gates.lock() {
            let is_owner = gates
                .get(&authorization.group_id)
                .and_then(|gate| gate.owner.as_ref())
                .map(|owner| {
                    owner.group_version == authorization.group_version
                        && owner.owner_generation == authorization.owner_generation
                        && owner.stage == authorization.stage
                })
                .unwrap_or(false);
            if is_owner {
                if gates
                    .get(&authorization.group_id)
                    .and_then(|gate| gate.owner.as_ref())
                    .is_some_and(|owner| owner.active_operation)
                {
                    if let Some(owner) = gates
                        .get_mut(&authorization.group_id)
                        .and_then(|gate| gate.owner.as_mut())
                    {
                        owner.closed = true;
                    }
                } else if let Some(gate) = gates.get_mut(&authorization.group_id) {
                    gate.owner = None;
                    if gate.candidate_count == 0 && !gate.retain_fence {
                        gates.remove(&authorization.group_id);
                    }
                }
            }
        }
        self.changed.notify_waiters();
    }

    fn finish_operation(&self, authorization: &MutationAuthorization) {
        if let Ok(mut gates) = self.gates.lock() {
            let remove = gates
                .get_mut(&authorization.group_id)
                .and_then(|gate| gate.owner.as_mut())
                .map(|owner| {
                    if owner.group_version == authorization.group_version
                        && owner.owner_generation == authorization.owner_generation
                        && owner.stage == authorization.stage
                    {
                        owner.active_operation = false;
                        owner.closed
                    } else {
                        false
                    }
                })
                .unwrap_or(false);
            if remove {
                if let Some(gate) = gates.get_mut(&authorization.group_id) {
                    gate.owner = None;
                    if gate.candidate_count == 0 && !gate.retain_fence {
                        gates.remove(&authorization.group_id);
                    }
                }
            }
        }
        self.changed.notify_waiters();
    }
}

#[derive(Debug)]
pub struct JournalMutationTicket {
    group_id: String,
    group_version: i64,
    epoch: u64,
    gates: Arc<MutationGateRegistry>,
    active: bool,
}

impl JournalMutationTicket {
    pub(crate) fn group_id(&self) -> &str {
        &self.group_id
    }

    pub(crate) fn group_version(&self) -> i64 {
        self.group_version
    }

    pub fn acquire(
        mut self,
        grant: JournalMutationGrant,
    ) -> Result<JournalMutationLease, MutationLeaseError> {
        let gates = Arc::clone(&self.gates);
        gates.acquire(&mut self, grant)
    }
}

impl Drop for JournalMutationTicket {
    fn drop(&mut self) {
        if self.active {
            self.gates.release_candidate(&self.group_id);
        }
    }
}

pub(crate) struct MutationOperationGuard {
    authorization: MutationAuthorization,
    gates: Arc<MutationGateRegistry>,
}

impl Drop for MutationOperationGuard {
    fn drop(&mut self) {
        self.gates.finish_operation(&self.authorization);
    }
}

#[derive(Debug)]
pub struct JournalMutationLease {
    authorization: MutationAuthorization,
    gates: Arc<MutationGateRegistry>,
    entries: Vec<AuthorizedJournalEntry>,
    operation_started: bool,
}

impl JournalMutationLease {
    pub(crate) fn authorization(&self) -> MutationAuthorization {
        self.authorization.clone()
    }

    pub fn group_id(&self) -> &str {
        self.authorization.group_id()
    }

    pub fn group_version(&self) -> i64 {
        self.authorization.group_version()
    }

    pub(crate) fn stage(&self) -> JournalMutationStage {
        self.authorization.stage()
    }

    pub(crate) fn take_entry(
        &mut self,
        sequence: u16,
        allowed_actions: &[FileEntryAction],
    ) -> Result<AuthorizedJournalEntry, MutationLeaseError> {
        if self.operation_started {
            return Err(MutationLeaseError::OperationAlreadyStarted);
        }
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.sequence == sequence && allowed_actions.contains(&entry.action))
            .cloned()
            .ok_or(MutationLeaseError::EntryNotAuthorized)?;
        self.operation_started = true;
        Ok(entry)
    }

    pub(crate) fn take_next_entry(&mut self) -> Result<AuthorizedJournalEntry, MutationLeaseError> {
        if self.operation_started {
            return Err(MutationLeaseError::OperationAlreadyStarted);
        }
        let entry = self
            .entries
            .first()
            .cloned()
            .ok_or(MutationLeaseError::EntryNotAuthorized)?;
        self.operation_started = true;
        Ok(entry)
    }

    pub(crate) fn next_sequence(&self) -> Result<u16, MutationLeaseError> {
        self.entries
            .first()
            .map(|entry| entry.sequence)
            .ok_or(MutationLeaseError::EntryNotAuthorized)
    }

    pub(crate) fn finish_operation(&mut self, sequence: u16) -> Result<(), MutationLeaseError> {
        if !self.operation_started {
            return Err(MutationLeaseError::EntryNotAuthorized);
        }
        let position = self
            .entries
            .iter()
            .position(|entry| entry.sequence == sequence)
            .ok_or(MutationLeaseError::EntryNotAuthorized)?;
        self.entries.remove(position);
        self.operation_started = false;
        Ok(())
    }
}

impl Drop for JournalMutationLease {
    fn drop(&mut self) {
        self.gates.release(&self.authorization);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationLeaseError {
    InvalidIdentity,
    AlreadyOwned,
    GenerationExhausted,
    Capacity,
    Poisoned,
    EntryNotAuthorized,
    OperationAlreadyStarted,
    Fenced,
}

impl fmt::Display for MutationLeaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentity => formatter.write_str("invalid journal mutation identity"),
            Self::AlreadyOwned => formatter.write_str("journal mutation group is already owned"),
            Self::GenerationExhausted => {
                formatter.write_str("journal mutation generation exhausted")
            }
            Self::Capacity => formatter.write_str("journal mutation registry capacity exhausted"),
            Self::Poisoned => formatter.write_str("journal mutation registry is unavailable"),
            Self::EntryNotAuthorized => formatter.write_str("journal entry is not authorized"),
            Self::OperationAlreadyStarted => {
                formatter.write_str("journal mutation lease already started an operation")
            }
            Self::Fenced => formatter.write_str("journal mutation generation is fenced"),
        }
    }
}

impl std::error::Error for MutationLeaseError {}

impl StorageRootRegistry {
    pub fn bootstrap(
        data_dir: &Path,
        static_dir: Option<&Path>,
        space_budget: &DataDirSpaceBudget,
    ) -> std::io::Result<Self> {
        let data_directory = open_directory_without_symlinks(data_dir)?;
        probe_openat2(&data_directory)?;
        let canonical_data_dir = data_directory
            .metadata()
            .and_then(|_| data_dir.canonicalize())?;
        for id in StorageRootId::ALL {
            if id == StorageRootId::Static {
                continue;
            }
            let path = data_dir.join(id.directory_name());
            match std::fs::symlink_metadata(&path) {
                Ok(metadata) => {
                    if metadata.file_type().is_symlink() || !metadata.is_dir() {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            format!(
                                "storage root {} is not a non-symlink directory",
                                id.as_str()
                            ),
                        ));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    let token = reserve_bootstrap_journal_space(
                        space_budget,
                        &format!("root-{}", id.as_str()),
                        space_budget
                            .filesystem_entry_metadata_bytes()
                            .map_err(std::io::Error::other)?,
                    )?;
                    std::fs::create_dir(&path)?;
                    data_directory.sync_all()?;
                    token
                        .publish_ephemeral_journal_allocation()
                        .map_err(std::io::Error::other)?;
                }
                Err(error) => return Err(error),
            }
            open_directory_without_symlinks(&path)?;
        }
        let result_inbox_directory = data_dir
            .join(StorageRootId::Journal.directory_name())
            .join(LLM_RESULT_INBOX_DIRECTORY);
        match std::fs::symlink_metadata(&result_inbox_directory) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "LLM result inbox is not a non-symlink directory",
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let token = reserve_bootstrap_journal_space(
                    space_budget,
                    "llm-result-inbox-directory",
                    space_budget
                        .filesystem_entry_metadata_bytes()
                        .map_err(std::io::Error::other)?,
                )?;
                std::fs::create_dir(&result_inbox_directory)?;
                open_directory_without_symlinks(
                    &data_dir.join(StorageRootId::Journal.directory_name()),
                )?
                .sync_all()?;
                token
                    .publish_ephemeral_journal_allocation()
                    .map_err(std::io::Error::other)?;
            }
            Err(error) => return Err(error),
        }
        open_directory_without_symlinks(&result_inbox_directory)?;
        let validated_static_dir = match static_dir {
            Some(path) => match std::fs::symlink_metadata(path) {
                Ok(metadata) => {
                    if metadata.file_type().is_symlink() || !metadata.is_dir() {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "configured static directory must be a non-symlink directory",
                        ));
                    }
                    let canonical_static_dir = path.canonicalize()?;
                    if paths_overlap(&canonical_data_dir, &canonical_static_dir) {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "configured static directory must not overlap the data directory",
                        ));
                    }
                    Some(path)
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            },
            None => None,
        };
        let registry = Self::open_existing(data_dir, validated_static_dir)?;
        let entry_metadata_bytes = space_budget
            .filesystem_entry_metadata_bytes()
            .map_err(std::io::Error::other)?;
        let probe_peak = entry_metadata_bytes
            .checked_mul(2)
            .and_then(|value| value.checked_add(entry_metadata_bytes / 4))
            .ok_or_else(|| std::io::Error::other("rename probe capacity overflow"))?;
        let probe_token =
            reserve_bootstrap_journal_space(space_budget, "rename-probe", probe_peak)?;
        registry.verify_writable_roots_share_mount_and_atomic_rename()?;
        probe_token
            .publish_ephemeral_journal_allocation()
            .map_err(std::io::Error::other)?;
        Ok(registry)
    }

    pub fn open_existing(data_dir: &Path, static_dir: Option<&Path>) -> std::io::Result<Self> {
        let mut capabilities = Vec::new();
        capabilities
            .try_reserve_exact(StorageRootId::COUNT)
            .map_err(|error| {
                std::io::Error::other(format!("could not reserve storage root registry: {error}"))
            })?;
        for id in StorageRootId::ALL {
            let path = if id == StorageRootId::Static {
                static_dir.map(Path::to_path_buf)
            } else {
                Some(data_dir.join(id.directory_name()))
            };
            let directory = path
                .map(|path| open_directory_without_symlinks(&path).map(Arc::new))
                .transpose()?;
            capabilities.push(StorageRootCapability { id, directory });
        }
        Ok(Self { capabilities })
    }

    pub fn directory(&self, id: StorageRootId) -> Result<&File, StorageRootUnavailable> {
        self.capabilities
            .iter()
            .find(|capability| capability.id == id)
            .and_then(|capability| capability.directory.as_deref())
            .ok_or(StorageRootUnavailable(id))
    }

    pub fn is_available(&self, id: StorageRootId) -> bool {
        self.directory(id).is_ok()
    }

    fn verify_writable_roots_share_mount_and_atomic_rename(&self) -> std::io::Result<()> {
        let journal = self
            .directory(StorageRootId::Journal)
            .map_err(std::io::Error::other)?;
        let journal_identity = root_identity(journal)?;
        for root_id in StorageRootId::ALL {
            if root_id == StorageRootId::Static {
                continue;
            }
            let root = self.directory(root_id).map_err(std::io::Error::other)?;
            if root_identity(root)? != journal_identity {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::CrossesDevices,
                    format!(
                        "writable storage root {} does not share the Journal mount",
                        root_id.as_str()
                    ),
                ));
            }
        }

        let probe_name = CString::new(format!(
            "{RENAME_PROBE_PREFIX}{}",
            uuid::Uuid::new_v4().simple()
        ))
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        let descriptor = unsafe {
            libc::openat(
                journal.as_raw_fd(),
                probe_name.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_WRONLY | libc::O_CLOEXEC,
                0o600,
            )
        };
        if descriptor < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut probe = unsafe { File::from_raw_fd(descriptor) };
        probe.write_all(b"momento-atomic-rename-probe")?;
        probe.sync_all()?;
        drop(probe);
        journal.sync_all()?;

        let result = self.run_rename_probe(journal, &probe_name);
        if result.is_err() {
            for root_id in StorageRootId::ALL {
                if root_id == StorageRootId::Static {
                    continue;
                }
                if let Ok(root) = self.directory(root_id) {
                    unsafe {
                        libc::unlinkat(root.as_raw_fd(), probe_name.as_ptr(), 0);
                    }
                    let _ = root.sync_all();
                }
            }
        }
        result
    }

    fn run_rename_probe(&self, journal: &File, probe_name: &CString) -> std::io::Result<()> {
        for root_id in StorageRootId::ALL {
            if matches!(root_id, StorageRootId::Journal | StorageRootId::Static) {
                continue;
            }
            let root = self.directory(root_id).map_err(std::io::Error::other)?;
            rename_between(journal, root, probe_name)?;
            journal.sync_all()?;
            root.sync_all()?;
            rename_between(root, journal, probe_name)?;
            root.sync_all()?;
            journal.sync_all()?;
        }
        let removed = unsafe { libc::unlinkat(journal.as_raw_fd(), probe_name.as_ptr(), 0) };
        if removed != 0 {
            return Err(std::io::Error::last_os_error());
        }
        journal.sync_all()
    }
}

fn reserve_bootstrap_journal_space(
    budget: &DataDirSpaceBudget,
    purpose: &str,
    peak_additional_bytes: u64,
) -> std::io::Result<super::space_budget::ProvisionalSpaceToken> {
    let reservation_id = format!("bootstrap-{purpose}-{}", uuid::Uuid::new_v4().simple());
    budget
        .reserve_journal(reservation_id, peak_additional_bytes)
        .map_err(std::io::Error::other)?
        .into_result()
        .map_err(std::io::Error::other)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RootIdentity {
    device_major: u32,
    device_minor: u32,
    mount_id: u64,
}

fn root_identity(directory: &File) -> std::io::Result<RootIdentity> {
    let status = descriptor_statx(directory)?;
    if !status.has_mount_id() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "statx did not return a mount identifier",
        ));
    }
    Ok(RootIdentity {
        device_major: status.device_major,
        device_minor: status.device_minor,
        mount_id: status.mount_id,
    })
}

fn rename_between(source: &File, destination: &File, name: &CString) -> std::io::Result<()> {
    rename_descriptor_entry(
        source.as_raw_fd(),
        name,
        destination.as_raw_fd(),
        name,
        libc::RENAME_NOREPLACE,
    )
}

fn probe_openat2(directory: &File) -> std::io::Result<()> {
    let path = std::ffi::CString::new(".")
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let how = directory_open_how();
    let descriptor = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            directory.as_raw_fd(),
            path.as_ptr(),
            &how,
            size_of::<libc::open_how>(),
        ) as libc::c_int
    };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error());
    }
    drop(unsafe { OwnedFd::from_raw_fd(descriptor) });
    Ok(())
}

pub(crate) fn directory_open_how() -> libc::open_how {
    let mut how: libc::open_how = unsafe { std::mem::zeroed() };
    how.flags = (libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC) as u64;
    how.resolve = libc::RESOLVE_BENEATH | libc::RESOLVE_NO_MAGICLINKS | libc::RESOLVE_NO_SYMLINKS;
    how
}

fn paths_overlap(first: &Path, second: &Path) -> bool {
    first == second || first.starts_with(second) || second.starts_with(first)
}

fn open_directory_without_symlinks(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageRootUnavailable(StorageRootId);

impl fmt::Display for StorageRootUnavailable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "storage root {} is unavailable", self.0.as_str())
    }
}

impl std::error::Error for StorageRootUnavailable {}

impl StorageRootId {
    pub const COUNT: usize = 13;

    pub const ALL: [Self; Self::COUNT] = [
        Self::Originals,
        Self::Thumbnails,
        Self::TinyThumbnails,
        Self::PlaceThumbnails,
        Self::Previews,
        Self::Imports,
        Self::Albums,
        Self::Trash,
        Self::WebDav,
        Self::Backups,
        Self::Logs,
        Self::Journal,
        Self::Static,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Originals => "originals",
            Self::Thumbnails => "thumbnails",
            Self::TinyThumbnails => "tiny_thumbnails",
            Self::PlaceThumbnails => "place_thumbnails",
            Self::Previews => "previews",
            Self::Imports => "imports",
            Self::Albums => "albums",
            Self::Trash => "trash",
            Self::WebDav => "webdav",
            Self::Backups => "backups",
            Self::Logs => "logs",
            Self::Journal => "journal",
            Self::Static => "static",
        }
    }

    pub const fn directory_name(self) -> &'static str {
        match self {
            Self::TinyThumbnails => "thumbnails_tiny",
            Self::PlaceThumbnails => "thumbnails_places",
            Self::WebDav => "webdav",
            _ => self.as_str(),
        }
    }
}

impl TryFrom<&str> for StorageRootId {
    type Error = StorageRootIdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::ALL
            .into_iter()
            .find(|root| root.as_str() == value)
            .ok_or(StorageRootIdError)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageRootIdError;

impl fmt::Display for StorageRootIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown storage root")
    }
}

impl std::error::Error for StorageRootIdError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedStoragePath {
    relative_path: String,
    path_key: Vec<u8>,
    ancestor_keys: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathClaimMode {
    Read,
    Write,
}

impl PathClaimMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathClaimScope {
    Exact,
    Subtree,
}

impl PathClaimScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Subtree => "subtree",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathClaim {
    pub storage_root: StorageRootId,
    pub path: NormalizedStoragePath,
    pub mode: PathClaimMode,
    pub scope: PathClaimScope,
}

impl PathClaim {
    pub fn conflicts_with(&self, other: &Self) -> bool {
        if self.storage_root != other.storage_root
            || (self.mode == PathClaimMode::Read && other.mode == PathClaimMode::Read)
        {
            return false;
        }
        self.path.path_key == other.path.path_key
            || (self.scope == PathClaimScope::Subtree
                && other.path.path_key.starts_with(&self.path.path_key))
            || (other.scope == PathClaimScope::Subtree
                && self.path.path_key.starts_with(&other.path.path_key))
    }

    pub fn covers_write_path(
        &self,
        storage_root: StorageRootId,
        path: &NormalizedStoragePath,
    ) -> bool {
        self.storage_root == storage_root
            && self.mode == PathClaimMode::Write
            && (self.path.path_key == path.path_key
                || (self.scope == PathClaimScope::Subtree
                    && path.path_key.starts_with(&self.path.path_key)))
    }
}

impl NormalizedStoragePath {
    pub fn parse(path: &str) -> Result<Self, StoragePathError> {
        if path.is_empty() || path.len() > MAX_STORAGE_PATH_BYTES || path.starts_with('/') {
            return Err(StoragePathError::InvalidPath);
        }
        let declared_components = path
            .as_bytes()
            .iter()
            .filter(|byte| **byte == b'/')
            .count()
            .checked_add(1)
            .ok_or(StoragePathError::Capacity)?;
        if declared_components > MAX_STORAGE_PATH_COMPONENTS {
            return Err(StoragePathError::InvalidComponent);
        }
        let key_capacity = path
            .len()
            .checked_add(
                4usize
                    .checked_mul(declared_components)
                    .ok_or(StoragePathError::Capacity)?,
            )
            .ok_or(StoragePathError::Capacity)?;
        let mut path_key = Vec::new();
        path_key
            .try_reserve_exact(key_capacity)
            .map_err(|_| StoragePathError::Capacity)?;
        let mut ancestor_keys = Vec::new();
        ancestor_keys
            .try_reserve_exact(declared_components)
            .map_err(|_| StoragePathError::Capacity)?;
        let mut component_count = 0usize;
        for component in path.split('/') {
            component_count = component_count
                .checked_add(1)
                .ok_or(StoragePathError::Capacity)?;
            if component_count > MAX_STORAGE_PATH_COMPONENTS
                || component.is_empty()
                || matches!(component, "." | "..")
                || component.len() > MAX_STORAGE_PATH_COMPONENT_BYTES
                || component.as_bytes().contains(&0)
            {
                return Err(StoragePathError::InvalidComponent);
            }
            let component_length =
                u32::try_from(component.len()).map_err(|_| StoragePathError::InvalidComponent)?;
            path_key.extend_from_slice(&component_length.to_be_bytes());
            path_key.extend_from_slice(component.as_bytes());
            if path_key.len() > MAX_STORAGE_PATH_KEY_BYTES {
                return Err(StoragePathError::Capacity);
            }
            let mut ancestor_key = Vec::new();
            ancestor_key
                .try_reserve_exact(path_key.len())
                .map_err(|_| StoragePathError::Capacity)?;
            ancestor_key.extend_from_slice(&path_key);
            ancestor_keys.push(ancestor_key);
        }
        Ok(Self {
            relative_path: path.to_string(),
            path_key,
            ancestor_keys,
        })
    }

    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }

    pub fn path_key(&self) -> &[u8] {
        &self.path_key
    }

    pub fn ancestor_keys(&self) -> &[Vec<u8>] {
        &self.ancestor_keys
    }

    pub fn subtree_upper_bound(&self) -> Vec<u8> {
        let mut upper_bound = Vec::with_capacity(self.path_key.len() + 1);
        upper_bound.extend_from_slice(&self.path_key);
        upper_bound.push(0xff);
        upper_bound
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoragePathError {
    InvalidPath,
    InvalidComponent,
    Capacity,
}

impl fmt::Display for StoragePathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath => formatter.write_str("storage path must be bounded and relative"),
            Self::InvalidComponent => {
                formatter.write_str("storage path contains an invalid component")
            }
            Self::Capacity => formatter.write_str("storage path key exceeds its bounded capacity"),
        }
    }
}

impl std::error::Error for StoragePathError {}
