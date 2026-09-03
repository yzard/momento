use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::sync::{Arc, Mutex, MutexGuard};

const GIBIBYTE: u64 = 1024 * 1024 * 1024;
const MAX_RECOVERY_FLOOR_BYTES: u64 = 5 * GIBIBYTE;
const MAX_SQLITE_WAL_BYTES: u64 = 2 * GIBIBYTE;
const MAX_LOG_QUOTA_BYTES: u64 = 2 * GIBIBYTE;
const MAX_RESERVATION_ID_BYTES: usize = 128;
const MAX_OWNER_TEXT_BYTES: usize = 1024;
pub const MAX_SPACE_RECONSTRUCTION_PAGE: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpaceReservationClass {
    Journal,
    Sqlite,
    Log,
}

impl SpaceReservationClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Journal => "journal",
            Self::Sqlite => "sqlite",
            Self::Log => "log",
        }
    }
}

impl TryFrom<&str> for SpaceReservationClass {
    type Error = SpaceBudgetError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "journal" => Ok(Self::Journal),
            "sqlite" => Ok(Self::Sqlite),
            "log" => Ok(Self::Log),
            _ => Err(SpaceBudgetError::InvalidReconstruction(
                "unknown reservation class",
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceBudgetMode {
    RecoveryOnly,
    RecoveryReady,
    Running,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceBudgetHealth {
    Healthy,
    LogOverQuota,
    ExternalDeficit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesystemSpaceSnapshot {
    pub filesystem_id: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub fragment_size: u64,
}

impl FilesystemSpaceSnapshot {
    pub fn validate(&self) -> Result<(), SpaceBudgetError> {
        validate_identity(&self.filesystem_id)?;
        if self.total_bytes == 0
            || self.free_bytes > self.total_bytes
            || self.fragment_size == 0
            || !self.fragment_size.is_power_of_two()
        {
            return Err(SpaceBudgetError::InvalidFilesystemSnapshot);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableSpaceReservationRecord {
    pub reservation_id: String,
    pub class: SpaceReservationClass,
    pub owner_kind: String,
    pub owner_id: String,
    pub journal_group_id: Option<String>,
    pub filesystem_id: String,
    pub reserved_peak_additional_bytes: u64,
    pub newly_allocated_blocks: u64,
    pub version: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerSnapshot {
    pub mode: SpaceBudgetMode,
    pub health: SpaceBudgetHealth,
    pub epoch: u64,
    pub filesystem_total_bytes: u64,
    pub filesystem_free_bytes: u64,
    pub recovery_floor_bytes: u64,
    pub sqlite_wal_limit_bytes: u64,
    pub log_quota_bytes: u64,
    pub data_hard_limit_bytes: u64,
    pub sqlite_allocated_bytes: u64,
    pub sqlite_outstanding_bytes: u64,
    pub log_allocated_bytes: u64,
    pub log_outstanding_bytes: u64,
    pub journal_outstanding_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SqliteRecoveryFootprintSpec {
    pub page_size_bytes: u64,
    pub main_allocated_bytes: u64,
    pub wal_allocated_bytes: u64,
    pub shm_allocated_bytes: u64,
    pub wal_frame_count: u64,
    pub peak_additional_bytes: u64,
}

impl SqliteRecoveryFootprintSpec {
    pub fn inspect(
        database_path: &std::path::Path,
        fragment_size: u64,
    ) -> Result<Self, SpaceBudgetError> {
        if fragment_size == 0 || !fragment_size.is_power_of_two() {
            return Err(SpaceBudgetError::InvalidFilesystemSnapshot);
        }
        let mut database = File::open(database_path).map_err(|error| {
            SpaceBudgetError::FilesystemObservation(format!(
                "could not open SQLite main file {}: {error}",
                database_path.display()
            ))
        })?;
        let mut header = [0_u8; 100];
        database.read_exact(&mut header).map_err(|error| {
            SpaceBudgetError::FilesystemObservation(format!(
                "could not read SQLite main header {}: {error}",
                database_path.display()
            ))
        })?;
        if &header[..16] != b"SQLite format 3\0" {
            return Err(SpaceBudgetError::InvalidReconstruction(
                "SQLite main header is invalid",
            ));
        }
        let encoded_page_size = u16::from_be_bytes([header[16], header[17]]);
        let page_size_bytes = if encoded_page_size == 1 {
            65_536
        } else {
            u64::from(encoded_page_size)
        };
        if !(512..=65_536).contains(&page_size_bytes) || !page_size_bytes.is_power_of_two() {
            return Err(SpaceBudgetError::InvalidReconstruction(
                "SQLite page size is invalid",
            ));
        }

        let main_allocated_bytes = allocated_regular_file(database_path)?;
        let wal_path = database_path.with_extension("sqlite-wal");
        let shm_path = database_path.with_extension("sqlite-shm");
        let (wal_allocated_bytes, wal_frame_count, wal_logical_bytes) =
            inspect_wal(&wal_path, page_size_bytes)?;
        let shm_allocated_bytes = allocated_optional_regular_file(&shm_path)?;
        let checkpoint_main_peak = wal_frame_count
            .checked_mul(page_size_bytes)
            .ok_or(SpaceBudgetError::ArithmeticOverflow)?;
        let shm_regions = if wal_frame_count == 0 {
            1
        } else {
            wal_frame_count
                .checked_add(4_095)
                .ok_or(SpaceBudgetError::ArithmeticOverflow)?
                / 4_096
        };
        let required_shm = round_up(
            shm_regions
                .checked_mul(32 * 1024)
                .ok_or(SpaceBudgetError::ArithmeticOverflow)?,
            fragment_size,
        )?;
        let required_wal = round_up(
            wal_logical_bytes.max(
                32_u64
                    .checked_add(24)
                    .and_then(|value| value.checked_add(page_size_bytes))
                    .ok_or(SpaceBudgetError::ArithmeticOverflow)?,
            ),
            fragment_size,
        )?;
        let directory_metadata_peak = fragment_size
            .checked_mul(8)
            .ok_or(SpaceBudgetError::ArithmeticOverflow)?;
        let peak_additional_bytes = checkpoint_main_peak
            .checked_add(required_shm.saturating_sub(shm_allocated_bytes))
            .and_then(|value| value.checked_add(required_wal.saturating_sub(wal_allocated_bytes)))
            .and_then(|value| value.checked_add(directory_metadata_peak))
            .ok_or(SpaceBudgetError::ArithmeticOverflow)?;
        Ok(Self {
            page_size_bytes,
            main_allocated_bytes,
            wal_allocated_bytes,
            shm_allocated_bytes,
            wal_frame_count,
            peak_additional_bytes,
        })
    }
}

#[derive(Debug)]
pub enum SpaceAdmission<T> {
    Fits(T),
    TemporarilyUnavailable {
        required_bytes: u64,
        available_bytes: u64,
    },
    ExceedsHardLimit {
        required_bytes: u64,
        class_limit_bytes: u64,
    },
}

impl<T> SpaceAdmission<T> {
    pub fn into_result(self) -> Result<T, SpaceBudgetError> {
        match self {
            Self::Fits(token) => Ok(token),
            Self::TemporarilyUnavailable {
                required_bytes,
                available_bytes,
            } => Err(SpaceBudgetError::TemporarilyUnavailable {
                required_bytes,
                available_bytes,
            }),
            Self::ExceedsHardLimit {
                required_bytes,
                class_limit_bytes,
            } => Err(SpaceBudgetError::ExceedsHardLimit {
                required_bytes,
                class_limit_bytes,
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceBudgetError {
    FilesystemObservation(String),
    InvalidFilesystemSnapshot,
    InvalidIdentity,
    InvalidReservationSize,
    InvalidReconstruction(&'static str),
    ReconstructionPageTooLarge,
    ReconstructionAlreadyPublished,
    ReconstructionNotPublished,
    LedgerNotHealthy(SpaceBudgetHealth),
    ReservationConflict,
    ReservationNotFound,
    ReservationGenerationMismatch,
    ReservationStateMismatch,
    ReservationAlreadyCheckedOut,
    ArithmeticOverflow,
    CapacityStateAllocation,
    Poisoned,
    TemporarilyUnavailable {
        required_bytes: u64,
        available_bytes: u64,
    },
    ExceedsHardLimit {
        required_bytes: u64,
        class_limit_bytes: u64,
    },
}

impl fmt::Display for SpaceBudgetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FilesystemObservation(detail) => {
                write!(formatter, "filesystem capacity observation failed: {detail}")
            }
            Self::InvalidFilesystemSnapshot => {
                formatter.write_str("filesystem capacity snapshot is invalid")
            }
            Self::InvalidIdentity => formatter.write_str("space reservation identity is invalid"),
            Self::InvalidReservationSize => {
                formatter.write_str("space reservation size must be positive")
            }
            Self::InvalidReconstruction(detail) => {
                write!(formatter, "durable space reconstruction is invalid: {detail}")
            }
            Self::ReconstructionPageTooLarge => {
                formatter.write_str("space reconstruction page exceeds 256 records")
            }
            Self::ReconstructionAlreadyPublished => {
                formatter.write_str("space reconstruction was already published")
            }
            Self::ReconstructionNotPublished => {
                formatter.write_str("space reconstruction is not ready")
            }
            Self::LedgerNotHealthy(health) => {
                write!(formatter, "space ledger is not healthy: {health:?}")
            }
            Self::ReservationConflict => {
                formatter.write_str("space reservation identity already exists")
            }
            Self::ReservationNotFound => formatter.write_str("space reservation was not found"),
            Self::ReservationGenerationMismatch => {
                formatter.write_str("space reservation generation does not match")
            }
            Self::ReservationStateMismatch => {
                formatter.write_str("space reservation ownership state does not match")
            }
            Self::ReservationAlreadyCheckedOut => {
                formatter.write_str("space reservation already has a live checkout")
            }
            Self::ArithmeticOverflow => formatter.write_str("space budget arithmetic overflowed"),
            Self::CapacityStateAllocation => {
                formatter.write_str("space budget could not reserve bounded ledger state")
            }
            Self::Poisoned => formatter.write_str("space budget lock is poisoned"),
            Self::TemporarilyUnavailable {
                required_bytes,
                available_bytes,
            } => write!(
                formatter,
                "space is temporarily unavailable: required {required_bytes} bytes, available {available_bytes} bytes"
            ),
            Self::ExceedsHardLimit {
                required_bytes,
                class_limit_bytes,
            } => write!(
                formatter,
                "space request exceeds its hard limit: required {required_bytes} bytes, limit {class_limit_bytes} bytes"
            ),
        }
    }
}

impl std::error::Error for SpaceBudgetError {}

#[derive(Clone)]
pub struct DataDirSpaceBudget {
    inner: Arc<BudgetInner>,
}

impl fmt::Debug for DataDirSpaceBudget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DataDirSpaceBudget")
            .field("filesystem_id", &self.inner.layout.filesystem_id)
            .finish_non_exhaustive()
    }
}

struct BudgetInner {
    observation: ObservationSource,
    layout: BudgetLayout,
    state: Mutex<LedgerState>,
}

enum ObservationSource {
    Directory(File),
    Fixed(FilesystemSpaceSnapshot),
}

#[derive(Debug)]
struct BudgetLayout {
    filesystem_id: String,
    total_bytes: u64,
    fragment_size: u64,
    recovery_floor_bytes: u64,
    sqlite_wal_limit_bytes: u64,
    log_quota_bytes: u64,
    data_hard_limit_bytes: u64,
}

#[derive(Debug)]
struct LedgerState {
    mode: SpaceBudgetMode,
    health: SpaceBudgetHealth,
    epoch: u64,
    last_free_bytes: u64,
    sqlite_allocated_bytes: u64,
    sqlite_outstanding_bytes: u64,
    log_allocated_bytes: u64,
    log_outstanding_bytes: u64,
    journal_outstanding_bytes: u64,
    reservations: HashMap<String, ReservationState>,
    journal_groups: HashMap<String, String>,
    recovery_checked_out: bool,
}

#[derive(Debug)]
struct ReservationState {
    class: SpaceReservationClass,
    reserved_peak_additional_bytes: u64,
    newly_allocated_blocks: u64,
    generation: u64,
    ownership: ReservationOwnership,
    checked_out: bool,
}

#[derive(Debug)]
enum ReservationOwnership {
    Provisional,
    Durable(DurableOwner),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DurableOwner {
    owner_kind: String,
    owner_id: String,
    journal_group_id: Option<String>,
}

impl DataDirSpaceBudget {
    pub fn from_directory(directory: File) -> Result<Self, SpaceBudgetError> {
        let snapshot = observe_directory(&directory)?;
        Self::from_source(ObservationSource::Directory(directory), snapshot)
    }

    pub fn from_snapshot(snapshot: FilesystemSpaceSnapshot) -> Result<Self, SpaceBudgetError> {
        Self::from_source(ObservationSource::Fixed(snapshot.clone()), snapshot)
    }

    fn from_source(
        observation: ObservationSource,
        snapshot: FilesystemSpaceSnapshot,
    ) -> Result<Self, SpaceBudgetError> {
        snapshot.validate()?;
        let recovery_floor_bytes = MAX_RECOVERY_FLOOR_BYTES.min(snapshot.total_bytes / 20);
        let operational_bytes = snapshot
            .total_bytes
            .checked_sub(recovery_floor_bytes)
            .ok_or(SpaceBudgetError::ArithmeticOverflow)?;
        let log_quota_bytes = MAX_LOG_QUOTA_BYTES.min(operational_bytes / 100);
        let data_hard_limit_bytes = operational_bytes
            .checked_sub(log_quota_bytes)
            .ok_or(SpaceBudgetError::ArithmeticOverflow)?;
        let sqlite_wal_limit_bytes = MAX_SQLITE_WAL_BYTES.min(data_hard_limit_bytes / 4);
        Ok(Self {
            inner: Arc::new(BudgetInner {
                observation,
                layout: BudgetLayout {
                    filesystem_id: snapshot.filesystem_id,
                    total_bytes: snapshot.total_bytes,
                    fragment_size: snapshot.fragment_size,
                    recovery_floor_bytes,
                    sqlite_wal_limit_bytes,
                    log_quota_bytes,
                    data_hard_limit_bytes,
                },
                state: Mutex::new(LedgerState {
                    mode: SpaceBudgetMode::RecoveryOnly,
                    health: SpaceBudgetHealth::ExternalDeficit,
                    epoch: 0,
                    last_free_bytes: snapshot.free_bytes,
                    sqlite_allocated_bytes: 0,
                    sqlite_outstanding_bytes: 0,
                    log_allocated_bytes: 0,
                    log_outstanding_bytes: 0,
                    journal_outstanding_bytes: 0,
                    reservations: HashMap::new(),
                    journal_groups: HashMap::new(),
                    recovery_checked_out: false,
                }),
            }),
        })
    }

    pub fn filesystem_id(&self) -> &str {
        &self.inner.layout.filesystem_id
    }

    pub fn filesystem_entry_metadata_bytes(&self) -> Result<u64, SpaceBudgetError> {
        self.inner
            .layout
            .fragment_size
            .checked_mul(4)
            .ok_or(SpaceBudgetError::ArithmeticOverflow)
    }

    pub(crate) fn filesystem_fragment_size(&self) -> u64 {
        self.inner.layout.fragment_size
    }

    pub(crate) fn sqlite_wal_limit_bytes(&self) -> u64 {
        self.inner.layout.sqlite_wal_limit_bytes
    }

    pub fn begin_reconstruction(&self) -> SpaceReconstruction {
        SpaceReconstruction {
            budget: self.clone(),
            sqlite_allocated_bytes: 0,
            log_allocated_bytes: 0,
            reservations: HashMap::new(),
        }
    }

    pub fn mark_running(&self) -> Result<(), SpaceBudgetError> {
        let observed = self.observe()?;
        let mut state = self.lock_state()?;
        if state.mode != SpaceBudgetMode::RecoveryReady {
            return Err(SpaceBudgetError::ReconstructionNotPublished);
        }
        refresh_health(&self.inner.layout, &mut state, observed.free_bytes)?;
        if state.health != SpaceBudgetHealth::Healthy {
            return Err(SpaceBudgetError::LedgerNotHealthy(state.health));
        }
        state.mode = SpaceBudgetMode::Running;
        state.epoch = checked_increment(state.epoch)?;
        Ok(())
    }

    pub fn snapshot(&self) -> Result<LedgerSnapshot, SpaceBudgetError> {
        let observed = self.observe()?;
        let mut state = self.lock_state()?;
        refresh_health(&self.inner.layout, &mut state, observed.free_bytes)?;
        snapshot_from_state(&self.inner.layout, &state)
    }

    pub fn publish_log_cleanup_allocation(
        &self,
        allocated_bytes: u64,
    ) -> Result<LedgerSnapshot, SpaceBudgetError> {
        let observed = self.observe()?;
        let mut state = self.lock_state()?;
        if state.mode != SpaceBudgetMode::RecoveryReady
            || !matches!(
                state.health,
                SpaceBudgetHealth::LogOverQuota | SpaceBudgetHealth::ExternalDeficit
            )
        {
            return Err(SpaceBudgetError::ReservationStateMismatch);
        }
        if allocated_bytes > state.log_allocated_bytes {
            return Err(SpaceBudgetError::InvalidReconstruction(
                "log cleanup increased retained allocation",
            ));
        }
        state.log_allocated_bytes = allocated_bytes;
        state.epoch = checked_increment(state.epoch)?;
        refresh_health(&self.inner.layout, &mut state, observed.free_bytes)?;
        snapshot_from_state(&self.inner.layout, &state)
    }

    pub(crate) fn publish_runtime_log_cleanup_allocation(
        &self,
        allocated_bytes: u64,
    ) -> Result<LedgerSnapshot, SpaceBudgetError> {
        let observed = self.observe()?;
        let mut state = self.lock_state()?;
        if state.mode != SpaceBudgetMode::Running || state.log_outstanding_bytes != 0 {
            return Err(SpaceBudgetError::ReservationStateMismatch);
        }
        if allocated_bytes > state.log_allocated_bytes {
            return Err(SpaceBudgetError::InvalidReconstruction(
                "runtime log cleanup increased retained allocation",
            ));
        }
        state.log_allocated_bytes = allocated_bytes;
        state.epoch = checked_increment(state.epoch)?;
        refresh_health(&self.inner.layout, &mut state, observed.free_bytes)?;
        snapshot_from_state(&self.inner.layout, &state)
    }

    pub fn reserve_journal(
        &self,
        reservation_id: String,
        peak_additional_bytes: u64,
    ) -> Result<SpaceAdmission<ProvisionalSpaceToken>, SpaceBudgetError> {
        self.reserve(
            reservation_id,
            SpaceReservationClass::Journal,
            peak_additional_bytes,
        )
    }

    pub(crate) fn reserve_sqlite(
        &self,
        reservation_id: String,
        peak_additional_bytes: u64,
    ) -> Result<SpaceAdmission<ProvisionalSpaceToken>, SpaceBudgetError> {
        self.reserve(
            reservation_id,
            SpaceReservationClass::Sqlite,
            peak_additional_bytes,
        )
    }

    pub(crate) fn reserve_log(
        &self,
        reservation_id: String,
        peak_additional_bytes: u64,
    ) -> Result<SpaceAdmission<ProvisionalSpaceToken>, SpaceBudgetError> {
        self.reserve(
            reservation_id,
            SpaceReservationClass::Log,
            peak_additional_bytes,
        )
    }

    pub(crate) fn reserve_recovery_sqlite(
        &self,
        peak_additional_bytes: u64,
    ) -> Result<SpaceAdmission<RecoverySpaceToken>, SpaceBudgetError> {
        if peak_additional_bytes == 0 {
            return Err(SpaceBudgetError::InvalidReservationSize);
        }
        let observed = self.observe()?;
        let mut state = self.lock_state()?;
        if state.mode != SpaceBudgetMode::RecoveryOnly {
            return Err(SpaceBudgetError::ReservationStateMismatch);
        }
        if peak_additional_bytes > self.inner.layout.recovery_floor_bytes {
            return Ok(SpaceAdmission::ExceedsHardLimit {
                required_bytes: peak_additional_bytes,
                class_limit_bytes: self.inner.layout.recovery_floor_bytes,
            });
        }
        if state.recovery_checked_out {
            return Ok(SpaceAdmission::TemporarilyUnavailable {
                required_bytes: peak_additional_bytes,
                available_bytes: 0,
            });
        }
        if observed.free_bytes < peak_additional_bytes {
            return Ok(SpaceAdmission::TemporarilyUnavailable {
                required_bytes: peak_additional_bytes,
                available_bytes: observed.free_bytes,
            });
        }
        state.recovery_checked_out = true;
        Ok(SpaceAdmission::Fits(RecoverySpaceToken {
            budget: self.clone(),
            peak_additional_bytes,
            armed: true,
        }))
    }

    pub(crate) fn reserve(
        &self,
        reservation_id: String,
        class: SpaceReservationClass,
        peak_additional_bytes: u64,
    ) -> Result<SpaceAdmission<ProvisionalSpaceToken>, SpaceBudgetError> {
        validate_identity(&reservation_id)?;
        if peak_additional_bytes == 0 {
            return Err(SpaceBudgetError::InvalidReservationSize);
        }
        let observed = self.observe()?;
        let mut state = self.lock_state()?;
        refresh_health(&self.inner.layout, &mut state, observed.free_bytes)?;
        if state.mode == SpaceBudgetMode::RecoveryOnly {
            return Ok(SpaceAdmission::TemporarilyUnavailable {
                required_bytes: peak_additional_bytes,
                available_bytes: 0,
            });
        }
        let class_limit_bytes = class_limit(&self.inner.layout, class);
        if peak_additional_bytes > class_limit_bytes {
            return Ok(SpaceAdmission::ExceedsHardLimit {
                required_bytes: peak_additional_bytes,
                class_limit_bytes,
            });
        }
        let available_bytes = available_for_class(&self.inner.layout, &state, class)?;
        if state.health != SpaceBudgetHealth::Healthy || peak_additional_bytes > available_bytes {
            return Ok(SpaceAdmission::TemporarilyUnavailable {
                required_bytes: peak_additional_bytes,
                available_bytes,
            });
        }
        if state.reservations.contains_key(&reservation_id) {
            return Err(SpaceBudgetError::ReservationConflict);
        }
        state
            .reservations
            .try_reserve(1)
            .map_err(|_| SpaceBudgetError::CapacityStateAllocation)?;
        state
            .journal_groups
            .try_reserve(1)
            .map_err(|_| SpaceBudgetError::CapacityStateAllocation)?;
        add_outstanding(&mut state, class, peak_additional_bytes)?;
        state.reservations.insert(
            reservation_id.clone(),
            ReservationState {
                class,
                reserved_peak_additional_bytes: peak_additional_bytes,
                newly_allocated_blocks: 0,
                generation: 1,
                ownership: ReservationOwnership::Provisional,
                checked_out: true,
            },
        );
        state.epoch = checked_increment(state.epoch)?;
        Ok(SpaceAdmission::Fits(ProvisionalSpaceToken {
            budget: self.clone(),
            reservation_id,
            class,
            peak_additional_bytes,
            generation: 1,
            armed: true,
        }))
    }

    pub fn reacquire_durable(
        &self,
        record: &DurableSpaceReservationRecord,
    ) -> Result<DurableSpaceCheckout, SpaceBudgetError> {
        record.validate_for(&self.inner.layout.filesystem_id)?;
        let mut state = self.lock_state()?;
        let entry = state
            .reservations
            .get_mut(&record.reservation_id)
            .ok_or(SpaceBudgetError::ReservationNotFound)?;
        let expected_owner = DurableOwner::from_record(record);
        if entry.class != record.class
            || entry.generation != record.version
            || !matches!(&entry.ownership, ReservationOwnership::Durable(owner) if owner == &expected_owner)
        {
            return Err(SpaceBudgetError::ReservationStateMismatch);
        }
        if entry.checked_out {
            return Err(SpaceBudgetError::ReservationAlreadyCheckedOut);
        }
        entry.checked_out = true;
        Ok(DurableSpaceCheckout {
            budget: self.clone(),
            reservation_id: record.reservation_id.clone(),
            class: record.class,
            peak_additional_bytes: record.reserved_peak_additional_bytes,
            generation: record.version,
            owner: expected_owner,
            armed: true,
        })
    }

    pub(crate) fn release_journal_after_terminal_commit(
        &self,
        journal_group_id: &str,
    ) -> Result<bool, SpaceBudgetError> {
        validate_identity(journal_group_id)?;
        let mut state = self.lock_state()?;
        let Some(reservation_id) = state.journal_groups.get(journal_group_id).cloned() else {
            return Ok(false);
        };
        let entry = state
            .reservations
            .get(&reservation_id)
            .ok_or(SpaceBudgetError::ReservationNotFound)?;
        if entry.class != SpaceReservationClass::Journal
            || entry.checked_out
            || !matches!(
                &entry.ownership,
                ReservationOwnership::Durable(owner)
                    if owner.journal_group_id.as_deref() == Some(journal_group_id)
            )
        {
            return Err(SpaceBudgetError::ReservationStateMismatch);
        }
        let entry = state
            .reservations
            .remove(&reservation_id)
            .ok_or(SpaceBudgetError::ReservationNotFound)?;
        state.journal_groups.remove(journal_group_id);
        let remaining = entry
            .reserved_peak_additional_bytes
            .checked_sub(entry.newly_allocated_blocks)
            .ok_or(SpaceBudgetError::ArithmeticOverflow)?;
        subtract_outstanding(&mut state, entry.class, remaining)?;
        state.epoch = checked_increment(state.epoch)?;
        Ok(true)
    }

    pub(crate) fn release_sqlite_after_terminal_commit(
        &self,
        reservation_id: &str,
        allocated_bytes: u64,
    ) -> Result<bool, SpaceBudgetError> {
        validate_identity(reservation_id)?;
        let observed = self.observe()?;
        let mut state = self.lock_state()?;
        let Some(entry) = state.reservations.get(reservation_id) else {
            return Ok(false);
        };
        if entry.class != SpaceReservationClass::Sqlite
            || entry.checked_out
            || !matches!(entry.ownership, ReservationOwnership::Durable(_))
        {
            return Err(SpaceBudgetError::ReservationStateMismatch);
        }
        let maximum_declared_allocation = state
            .sqlite_allocated_bytes
            .checked_add(state.sqlite_outstanding_bytes)
            .ok_or(SpaceBudgetError::ArithmeticOverflow)?;
        if allocated_bytes > maximum_declared_allocation {
            return Err(SpaceBudgetError::InvalidReconstruction(
                "SQLite allocation exceeded all live declared growth",
            ));
        }
        let entry = state
            .reservations
            .remove(reservation_id)
            .ok_or(SpaceBudgetError::ReservationNotFound)?;
        let remaining = entry
            .reserved_peak_additional_bytes
            .checked_sub(entry.newly_allocated_blocks)
            .ok_or(SpaceBudgetError::ArithmeticOverflow)?;
        subtract_outstanding(&mut state, SpaceReservationClass::Sqlite, remaining)?;
        state.sqlite_allocated_bytes = allocated_bytes;
        state.epoch = checked_increment(state.epoch)?;
        refresh_health(&self.inner.layout, &mut state, observed.free_bytes)?;
        Ok(true)
    }

    fn observe(&self) -> Result<FilesystemSpaceSnapshot, SpaceBudgetError> {
        let snapshot = match &self.inner.observation {
            ObservationSource::Directory(directory) => observe_directory(directory)?,
            ObservationSource::Fixed(snapshot) => snapshot.clone(),
        };
        snapshot.validate()?;
        if snapshot.filesystem_id != self.inner.layout.filesystem_id
            || snapshot.total_bytes != self.inner.layout.total_bytes
            || snapshot.fragment_size != self.inner.layout.fragment_size
        {
            return Err(SpaceBudgetError::InvalidFilesystemSnapshot);
        }
        Ok(snapshot)
    }

    fn lock_state(&self) -> Result<MutexGuard<'_, LedgerState>, SpaceBudgetError> {
        self.inner
            .state
            .lock()
            .map_err(|_| SpaceBudgetError::Poisoned)
    }

    fn release_provisional(&self, reservation_id: &str, generation: u64) {
        let Ok(mut state) = self.inner.state.lock() else {
            return;
        };
        let removable = state.reservations.get(reservation_id).is_some_and(|entry| {
            entry.generation == generation
                && matches!(entry.ownership, ReservationOwnership::Provisional)
        });
        if !removable {
            return;
        }
        if let Some(entry) = state.reservations.remove(reservation_id) {
            let remaining = entry
                .reserved_peak_additional_bytes
                .saturating_sub(entry.newly_allocated_blocks);
            subtract_outstanding_saturating(&mut state, entry.class, remaining);
            state.epoch = state.epoch.saturating_add(1);
        }
    }

    fn drop_durable_checkout(&self, reservation_id: &str, generation: u64) {
        let Ok(mut state) = self.inner.state.lock() else {
            return;
        };
        if let Some(entry) = state.reservations.get_mut(reservation_id) {
            if entry.generation == generation
                && matches!(entry.ownership, ReservationOwnership::Durable(_))
            {
                entry.checked_out = false;
            }
        }
    }
}

pub(crate) struct RecoverySpaceToken {
    budget: DataDirSpaceBudget,
    peak_additional_bytes: u64,
    armed: bool,
}

impl RecoverySpaceToken {
    pub(crate) fn publish_sqlite_recovery(
        mut self,
        baseline_allocated_bytes: u64,
        recovered_allocated_bytes: u64,
    ) -> Result<(), SpaceBudgetError> {
        let growth = recovered_allocated_bytes.saturating_sub(baseline_allocated_bytes);
        let mut state = self.budget.lock_state()?;
        if !state.recovery_checked_out {
            return Err(SpaceBudgetError::ReservationStateMismatch);
        }
        state.recovery_checked_out = false;
        self.armed = false;
        if growth > self.peak_additional_bytes {
            return Err(SpaceBudgetError::ExceedsHardLimit {
                required_bytes: growth,
                class_limit_bytes: self.peak_additional_bytes,
            });
        }
        Ok(())
    }
}

impl Drop for RecoverySpaceToken {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let Ok(mut state) = self.budget.inner.state.lock() else {
            return;
        };
        state.recovery_checked_out = false;
    }
}

pub struct SpaceReconstruction {
    budget: DataDirSpaceBudget,
    sqlite_allocated_bytes: u64,
    log_allocated_bytes: u64,
    reservations: HashMap<String, ReservationState>,
}

impl SpaceReconstruction {
    pub fn set_allocated_bytes(&mut self, sqlite_allocated_bytes: u64, log_allocated_bytes: u64) {
        self.sqlite_allocated_bytes = sqlite_allocated_bytes;
        self.log_allocated_bytes = log_allocated_bytes;
    }

    pub fn add_page(
        &mut self,
        records: &[DurableSpaceReservationRecord],
    ) -> Result<(), SpaceBudgetError> {
        if records.len() > MAX_SPACE_RECONSTRUCTION_PAGE {
            return Err(SpaceBudgetError::ReconstructionPageTooLarge);
        }
        self.reservations
            .try_reserve(records.len())
            .map_err(|_| SpaceBudgetError::CapacityStateAllocation)?;
        for record in records {
            record.validate_for(&self.budget.inner.layout.filesystem_id)?;
            if self.reservations.contains_key(&record.reservation_id) {
                return Err(SpaceBudgetError::InvalidReconstruction(
                    "duplicate reservation identity",
                ));
            }
            self.reservations.insert(
                record.reservation_id.clone(),
                ReservationState {
                    class: record.class,
                    reserved_peak_additional_bytes: record.reserved_peak_additional_bytes,
                    newly_allocated_blocks: record.newly_allocated_blocks,
                    generation: record.version,
                    ownership: ReservationOwnership::Durable(DurableOwner::from_record(record)),
                    checked_out: false,
                },
            );
        }
        Ok(())
    }

    pub fn publish(self) -> Result<LedgerSnapshot, SpaceBudgetError> {
        let observed = self.budget.observe()?;
        let mut state = self.budget.lock_state()?;
        if state.mode != SpaceBudgetMode::RecoveryOnly
            || !state.reservations.is_empty()
            || !state.journal_groups.is_empty()
        {
            return Err(SpaceBudgetError::ReconstructionAlreadyPublished);
        }
        let mut sqlite_outstanding_bytes = 0_u64;
        let mut log_outstanding_bytes = 0_u64;
        let mut journal_outstanding_bytes = 0_u64;
        let mut journal_groups = HashMap::new();
        journal_groups
            .try_reserve(self.reservations.len())
            .map_err(|_| SpaceBudgetError::CapacityStateAllocation)?;
        for (reservation_id, entry) in &self.reservations {
            let remaining = entry
                .reserved_peak_additional_bytes
                .checked_sub(entry.newly_allocated_blocks)
                .ok_or(SpaceBudgetError::ArithmeticOverflow)?;
            match entry.class {
                SpaceReservationClass::Journal => {
                    journal_outstanding_bytes = journal_outstanding_bytes
                        .checked_add(remaining)
                        .ok_or(SpaceBudgetError::ArithmeticOverflow)?;
                    let ReservationOwnership::Durable(owner) = &entry.ownership else {
                        return Err(SpaceBudgetError::InvalidReconstruction(
                            "Journal reservation is not durably owned",
                        ));
                    };
                    let group_id = owner.journal_group_id.as_ref().ok_or(
                        SpaceBudgetError::InvalidReconstruction(
                            "Journal reservation has no group owner",
                        ),
                    )?;
                    if journal_groups
                        .insert(group_id.clone(), reservation_id.clone())
                        .is_some()
                    {
                        return Err(SpaceBudgetError::InvalidReconstruction(
                            "duplicate Journal group reservation",
                        ));
                    }
                }
                SpaceReservationClass::Sqlite => {
                    sqlite_outstanding_bytes = sqlite_outstanding_bytes
                        .checked_add(remaining)
                        .ok_or(SpaceBudgetError::ArithmeticOverflow)?;
                }
                SpaceReservationClass::Log => {
                    log_outstanding_bytes = log_outstanding_bytes
                        .checked_add(remaining)
                        .ok_or(SpaceBudgetError::ArithmeticOverflow)?;
                }
            }
        }
        state.sqlite_allocated_bytes = self.sqlite_allocated_bytes;
        state.sqlite_outstanding_bytes = sqlite_outstanding_bytes;
        state.log_allocated_bytes = self.log_allocated_bytes;
        state.log_outstanding_bytes = log_outstanding_bytes;
        state.journal_outstanding_bytes = journal_outstanding_bytes;
        state.reservations = self.reservations;
        state.journal_groups = journal_groups;
        state.mode = SpaceBudgetMode::RecoveryReady;
        state.epoch = checked_increment(state.epoch)?;
        refresh_health(&self.budget.inner.layout, &mut state, observed.free_bytes)?;
        snapshot_from_state(&self.budget.inner.layout, &state)
    }
}

pub struct ProvisionalSpaceToken {
    budget: DataDirSpaceBudget,
    reservation_id: String,
    class: SpaceReservationClass,
    peak_additional_bytes: u64,
    generation: u64,
    armed: bool,
}

impl fmt::Debug for ProvisionalSpaceToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProvisionalSpaceToken")
            .field("reservation_id", &self.reservation_id)
            .field("class", &self.class)
            .field("peak_additional_bytes", &self.peak_additional_bytes)
            .field("generation", &self.generation)
            .finish()
    }
}

impl ProvisionalSpaceToken {
    pub fn reservation_id(&self) -> &str {
        &self.reservation_id
    }

    pub fn filesystem_id(&self) -> &str {
        self.budget.filesystem_id()
    }

    pub fn class(&self) -> SpaceReservationClass {
        self.class
    }

    pub fn peak_additional_bytes(&self) -> u64 {
        self.peak_additional_bytes
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn commit_to_durable_owner(
        mut self,
        owner_kind: String,
        owner_id: String,
        journal_group_id: Option<String>,
    ) -> Result<DurableSpaceCheckout, SpaceBudgetError> {
        validate_owner(&owner_kind)?;
        validate_owner(&owner_id)?;
        if let Some(group_id) = &journal_group_id {
            validate_identity(group_id)?;
        }
        let owner = DurableOwner {
            owner_kind,
            owner_id,
            journal_group_id,
        };
        let mut state = self.budget.lock_state()?;
        {
            let entry = state
                .reservations
                .get(&self.reservation_id)
                .ok_or(SpaceBudgetError::ReservationNotFound)?;
            if entry.class != self.class || entry.generation != self.generation {
                return Err(SpaceBudgetError::ReservationGenerationMismatch);
            }
            if !matches!(entry.ownership, ReservationOwnership::Provisional) || !entry.checked_out {
                return Err(SpaceBudgetError::ReservationStateMismatch);
            }
        }
        if let Some(group_id) = &owner.journal_group_id {
            if state
                .journal_groups
                .insert(group_id.clone(), self.reservation_id.clone())
                .is_some()
            {
                return Err(SpaceBudgetError::ReservationConflict);
            }
        }
        let entry = state
            .reservations
            .get_mut(&self.reservation_id)
            .ok_or(SpaceBudgetError::ReservationNotFound)?;
        entry.ownership = ReservationOwnership::Durable(owner.clone());
        self.armed = false;
        drop(state);
        Ok(DurableSpaceCheckout {
            budget: self.budget.clone(),
            reservation_id: self.reservation_id.clone(),
            class: self.class,
            peak_additional_bytes: self.peak_additional_bytes,
            generation: self.generation,
            owner,
            armed: true,
        })
    }

    pub(crate) fn publish_ephemeral_sqlite_allocation(
        mut self,
        allocated_bytes: u64,
    ) -> Result<(), SpaceBudgetError> {
        if self.class != SpaceReservationClass::Sqlite {
            return Err(SpaceBudgetError::ReservationStateMismatch);
        }
        let observed = self.budget.observe()?;
        let mut state = self.budget.lock_state()?;
        let entry = state
            .reservations
            .get(&self.reservation_id)
            .ok_or(SpaceBudgetError::ReservationNotFound)?;
        if entry.class != SpaceReservationClass::Sqlite
            || entry.generation != self.generation
            || !matches!(entry.ownership, ReservationOwnership::Provisional)
            || !entry.checked_out
        {
            return Err(SpaceBudgetError::ReservationStateMismatch);
        }
        let maximum_declared_allocation = state
            .sqlite_allocated_bytes
            .checked_add(state.sqlite_outstanding_bytes)
            .ok_or(SpaceBudgetError::ArithmeticOverflow)?;
        let exceeded_declaration = allocated_bytes > maximum_declared_allocation;
        let entry = state
            .reservations
            .remove(&self.reservation_id)
            .ok_or(SpaceBudgetError::ReservationNotFound)?;
        subtract_outstanding(
            &mut state,
            SpaceReservationClass::Sqlite,
            entry.reserved_peak_additional_bytes,
        )?;
        state.sqlite_allocated_bytes = allocated_bytes;
        state.epoch = checked_increment(state.epoch)?;
        refresh_health(&self.budget.inner.layout, &mut state, observed.free_bytes)?;
        self.armed = false;
        if exceeded_declaration {
            return Err(SpaceBudgetError::InvalidReconstruction(
                "SQLite allocation exceeded all live declared growth",
            ));
        }
        Ok(())
    }

    pub(crate) fn publish_ephemeral_log_allocation(
        mut self,
        allocated_bytes: u64,
    ) -> Result<(), SpaceBudgetError> {
        if self.class != SpaceReservationClass::Log {
            return Err(SpaceBudgetError::ReservationStateMismatch);
        }
        let observed = self.budget.observe()?;
        let mut state = self.budget.lock_state()?;
        let entry = state
            .reservations
            .get(&self.reservation_id)
            .ok_or(SpaceBudgetError::ReservationNotFound)?;
        if entry.class != SpaceReservationClass::Log
            || entry.generation != self.generation
            || !matches!(entry.ownership, ReservationOwnership::Provisional)
            || !entry.checked_out
        {
            return Err(SpaceBudgetError::ReservationStateMismatch);
        }
        let maximum_declared_allocation = state
            .log_allocated_bytes
            .checked_add(state.log_outstanding_bytes)
            .ok_or(SpaceBudgetError::ArithmeticOverflow)?;
        let exceeded_declaration = allocated_bytes > maximum_declared_allocation;
        let entry = state
            .reservations
            .remove(&self.reservation_id)
            .ok_or(SpaceBudgetError::ReservationNotFound)?;
        subtract_outstanding(
            &mut state,
            SpaceReservationClass::Log,
            entry.reserved_peak_additional_bytes,
        )?;
        state.log_allocated_bytes = allocated_bytes;
        state.epoch = checked_increment(state.epoch)?;
        refresh_health(&self.budget.inner.layout, &mut state, observed.free_bytes)?;
        self.armed = false;
        if allocated_bytes > self.budget.inner.layout.log_quota_bytes {
            return Err(SpaceBudgetError::ExceedsHardLimit {
                required_bytes: allocated_bytes,
                class_limit_bytes: self.budget.inner.layout.log_quota_bytes,
            });
        }
        if exceeded_declaration {
            return Err(SpaceBudgetError::InvalidReconstruction(
                "log allocation exceeded all live declared growth",
            ));
        }
        Ok(())
    }

    pub(crate) fn publish_ephemeral_journal_allocation(mut self) -> Result<(), SpaceBudgetError> {
        if self.class != SpaceReservationClass::Journal {
            return Err(SpaceBudgetError::ReservationStateMismatch);
        }
        let observed = self.budget.observe()?;
        let mut state = self.budget.lock_state()?;
        let entry = state
            .reservations
            .get(&self.reservation_id)
            .ok_or(SpaceBudgetError::ReservationNotFound)?;
        if entry.class != SpaceReservationClass::Journal
            || entry.generation != self.generation
            || !matches!(entry.ownership, ReservationOwnership::Provisional)
            || !entry.checked_out
        {
            return Err(SpaceBudgetError::ReservationStateMismatch);
        }
        let entry = state
            .reservations
            .remove(&self.reservation_id)
            .ok_or(SpaceBudgetError::ReservationNotFound)?;
        subtract_outstanding(
            &mut state,
            SpaceReservationClass::Journal,
            entry.reserved_peak_additional_bytes,
        )?;
        state.epoch = checked_increment(state.epoch)?;
        refresh_health(&self.budget.inner.layout, &mut state, observed.free_bytes)?;
        self.armed = false;
        if state.health != SpaceBudgetHealth::Healthy {
            return Err(SpaceBudgetError::LedgerNotHealthy(state.health));
        }
        Ok(())
    }
}

impl Drop for ProvisionalSpaceToken {
    fn drop(&mut self) {
        if self.armed {
            self.budget
                .release_provisional(&self.reservation_id, self.generation);
        }
    }
}

pub struct DurableSpaceCheckout {
    budget: DataDirSpaceBudget,
    reservation_id: String,
    class: SpaceReservationClass,
    peak_additional_bytes: u64,
    generation: u64,
    owner: DurableOwner,
    armed: bool,
}

impl fmt::Debug for DurableSpaceCheckout {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DurableSpaceCheckout")
            .field("reservation_id", &self.reservation_id)
            .field("class", &self.class)
            .field("peak_additional_bytes", &self.peak_additional_bytes)
            .field("generation", &self.generation)
            .field("owner", &self.owner)
            .finish()
    }
}

impl DurableSpaceCheckout {
    pub fn reservation_id(&self) -> &str {
        &self.reservation_id
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn publish_sqlite_child(
        mut self,
        record: &DurableSpaceReservationRecord,
        allocated_bytes: u64,
    ) -> Result<(), SpaceBudgetError> {
        if self.class != SpaceReservationClass::Sqlite {
            return Err(SpaceBudgetError::ReservationStateMismatch);
        }
        record.validate_for(self.budget.filesystem_id())?;
        let expected_owner = DurableOwner::from_record(record);
        let cleanup_owner_transition = self.owner.owner_kind == "llm_result"
            && expected_owner.owner_kind == "llm_result_cleanup"
            && self.owner.owner_id == expected_owner.owner_id
            && self.owner.journal_group_id == expected_owner.journal_group_id;
        if record.reservation_id != self.reservation_id
            || record.class != self.class
            || (expected_owner != self.owner && !cleanup_owner_transition)
            || record.version < self.generation
        {
            return Err(SpaceBudgetError::ReservationStateMismatch);
        }
        let observed = self.budget.observe()?;
        let mut state = self.budget.lock_state()?;
        let current = state
            .reservations
            .get(&self.reservation_id)
            .ok_or(SpaceBudgetError::ReservationNotFound)?;
        if current.class != self.class
            || current.generation != self.generation
            || !current.checked_out
            || !matches!(&current.ownership, ReservationOwnership::Durable(owner) if owner == &self.owner)
            || record.newly_allocated_blocks < current.newly_allocated_blocks
        {
            return Err(SpaceBudgetError::ReservationStateMismatch);
        }
        let consumed = record
            .newly_allocated_blocks
            .checked_sub(current.newly_allocated_blocks)
            .ok_or(SpaceBudgetError::ArithmeticOverflow)?;
        let maximum_declared_allocation = state
            .sqlite_allocated_bytes
            .checked_add(state.sqlite_outstanding_bytes)
            .ok_or(SpaceBudgetError::ArithmeticOverflow)?;
        if allocated_bytes > maximum_declared_allocation {
            return Err(SpaceBudgetError::InvalidReconstruction(
                "SQLite allocation exceeded its live result reservation",
            ));
        }
        subtract_outstanding(&mut state, SpaceReservationClass::Sqlite, consumed)?;
        let current = state
            .reservations
            .get_mut(&self.reservation_id)
            .ok_or(SpaceBudgetError::ReservationNotFound)?;
        current.newly_allocated_blocks = record.newly_allocated_blocks;
        current.generation = record.version;
        current.ownership = ReservationOwnership::Durable(expected_owner);
        current.checked_out = false;
        state.sqlite_allocated_bytes = allocated_bytes;
        state.epoch = checked_increment(state.epoch)?;
        refresh_health(&self.budget.inner.layout, &mut state, observed.free_bytes)?;
        self.armed = false;
        Ok(())
    }
}

impl Drop for DurableSpaceCheckout {
    fn drop(&mut self) {
        if self.armed {
            self.budget
                .drop_durable_checkout(&self.reservation_id, self.generation);
        }
    }
}

impl DurableSpaceReservationRecord {
    fn validate_for(&self, filesystem_id: &str) -> Result<(), SpaceBudgetError> {
        validate_identity(&self.reservation_id)?;
        validate_owner(&self.owner_kind)?;
        validate_owner(&self.owner_id)?;
        if let Some(group_id) = &self.journal_group_id {
            validate_identity(group_id)?;
        }
        if self.filesystem_id != filesystem_id
            || self.reserved_peak_additional_bytes == 0
            || self.newly_allocated_blocks > self.reserved_peak_additional_bytes
            || self.version == 0
        {
            return Err(SpaceBudgetError::InvalidReconstruction(
                "reservation evidence does not match the data filesystem",
            ));
        }
        if self.class == SpaceReservationClass::Journal && self.journal_group_id.is_none() {
            return Err(SpaceBudgetError::InvalidReconstruction(
                "Journal reservation is missing its group owner",
            ));
        }
        Ok(())
    }
}

impl DurableOwner {
    fn from_record(record: &DurableSpaceReservationRecord) -> Self {
        Self {
            owner_kind: record.owner_kind.clone(),
            owner_id: record.owner_id.clone(),
            journal_group_id: record.journal_group_id.clone(),
        }
    }
}

fn observe_directory(directory: &File) -> Result<FilesystemSpaceSnapshot, SpaceBudgetError> {
    let mut status = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::fstatvfs(directory.as_raw_fd(), status.as_mut_ptr()) } != 0 {
        return Err(SpaceBudgetError::FilesystemObservation(
            std::io::Error::last_os_error().to_string(),
        ));
    }
    let status = unsafe { status.assume_init() };
    let fragment_size = if status.f_frsize == 0 {
        status.f_bsize
    } else {
        status.f_frsize
    };
    let total_blocks = status.f_blocks;
    let available_blocks = status.f_bavail;
    let total_bytes = total_blocks
        .checked_mul(fragment_size)
        .ok_or(SpaceBudgetError::ArithmeticOverflow)?;
    let free_bytes = available_blocks
        .checked_mul(fragment_size)
        .ok_or(SpaceBudgetError::ArithmeticOverflow)?;
    let mut file_status = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(directory.as_raw_fd(), file_status.as_mut_ptr()) } != 0 {
        return Err(SpaceBudgetError::FilesystemObservation(
            std::io::Error::last_os_error().to_string(),
        ));
    }
    let file_status = unsafe { file_status.assume_init() };
    let filesystem_id = format!("{:x}", file_status.st_dev);
    let snapshot = FilesystemSpaceSnapshot {
        filesystem_id,
        total_bytes,
        free_bytes,
        fragment_size,
    };
    snapshot.validate()?;
    Ok(snapshot)
}

fn refresh_health(
    layout: &BudgetLayout,
    state: &mut LedgerState,
    free_bytes: u64,
) -> Result<(), SpaceBudgetError> {
    state.last_free_bytes = free_bytes;
    if state.log_allocated_bytes > layout.log_quota_bytes {
        state.health = SpaceBudgetHealth::LogOverQuota;
        return Ok(());
    }
    let protected = protected_free_requirement(layout, state)?;
    state.health = if free_bytes >= protected {
        SpaceBudgetHealth::Healthy
    } else {
        SpaceBudgetHealth::ExternalDeficit
    };
    Ok(())
}

fn protected_free_requirement(
    layout: &BudgetLayout,
    state: &LedgerState,
) -> Result<u64, SpaceBudgetError> {
    let log_remainder = layout
        .log_quota_bytes
        .checked_sub(state.log_allocated_bytes)
        .ok_or(SpaceBudgetError::ArithmeticOverflow)?;
    layout
        .recovery_floor_bytes
        .checked_add(log_remainder)
        .and_then(|value| value.checked_add(state.journal_outstanding_bytes))
        .and_then(|value| value.checked_add(state.sqlite_outstanding_bytes))
        .ok_or(SpaceBudgetError::ArithmeticOverflow)
}

fn available_for_class(
    layout: &BudgetLayout,
    state: &LedgerState,
    class: SpaceReservationClass,
) -> Result<u64, SpaceBudgetError> {
    match class {
        SpaceReservationClass::Journal | SpaceReservationClass::Sqlite => {
            let protected = protected_free_requirement(layout, state)?;
            Ok(state.last_free_bytes.saturating_sub(protected))
        }
        SpaceReservationClass::Log => layout
            .log_quota_bytes
            .checked_sub(state.log_allocated_bytes)
            .and_then(|value| value.checked_sub(state.log_outstanding_bytes))
            .ok_or(SpaceBudgetError::ArithmeticOverflow),
    }
}

fn class_limit(layout: &BudgetLayout, class: SpaceReservationClass) -> u64 {
    match class {
        SpaceReservationClass::Journal | SpaceReservationClass::Sqlite => {
            layout.data_hard_limit_bytes
        }
        SpaceReservationClass::Log => layout.log_quota_bytes,
    }
}

fn add_outstanding(
    state: &mut LedgerState,
    class: SpaceReservationClass,
    bytes: u64,
) -> Result<(), SpaceBudgetError> {
    let target = match class {
        SpaceReservationClass::Journal => &mut state.journal_outstanding_bytes,
        SpaceReservationClass::Sqlite => &mut state.sqlite_outstanding_bytes,
        SpaceReservationClass::Log => &mut state.log_outstanding_bytes,
    };
    *target = target
        .checked_add(bytes)
        .ok_or(SpaceBudgetError::ArithmeticOverflow)?;
    Ok(())
}

fn subtract_outstanding_saturating(
    state: &mut LedgerState,
    class: SpaceReservationClass,
    bytes: u64,
) {
    let target = match class {
        SpaceReservationClass::Journal => &mut state.journal_outstanding_bytes,
        SpaceReservationClass::Sqlite => &mut state.sqlite_outstanding_bytes,
        SpaceReservationClass::Log => &mut state.log_outstanding_bytes,
    };
    *target = target.saturating_sub(bytes);
}

fn subtract_outstanding(
    state: &mut LedgerState,
    class: SpaceReservationClass,
    bytes: u64,
) -> Result<(), SpaceBudgetError> {
    let target = match class {
        SpaceReservationClass::Journal => &mut state.journal_outstanding_bytes,
        SpaceReservationClass::Sqlite => &mut state.sqlite_outstanding_bytes,
        SpaceReservationClass::Log => &mut state.log_outstanding_bytes,
    };
    *target = target
        .checked_sub(bytes)
        .ok_or(SpaceBudgetError::ArithmeticOverflow)?;
    Ok(())
}

fn snapshot_from_state(
    layout: &BudgetLayout,
    state: &LedgerState,
) -> Result<LedgerSnapshot, SpaceBudgetError> {
    Ok(LedgerSnapshot {
        mode: state.mode,
        health: state.health,
        epoch: state.epoch,
        filesystem_total_bytes: layout.total_bytes,
        filesystem_free_bytes: state.last_free_bytes,
        recovery_floor_bytes: layout.recovery_floor_bytes,
        sqlite_wal_limit_bytes: layout.sqlite_wal_limit_bytes,
        log_quota_bytes: layout.log_quota_bytes,
        data_hard_limit_bytes: layout.data_hard_limit_bytes,
        sqlite_allocated_bytes: state.sqlite_allocated_bytes,
        sqlite_outstanding_bytes: state.sqlite_outstanding_bytes,
        log_allocated_bytes: state.log_allocated_bytes,
        log_outstanding_bytes: state.log_outstanding_bytes,
        journal_outstanding_bytes: state.journal_outstanding_bytes,
    })
}

pub(crate) fn measure_sqlite_allocation(
    database_path: &std::path::Path,
) -> Result<u64, SpaceBudgetError> {
    use std::os::unix::fs::MetadataExt;

    [
        database_path.to_path_buf(),
        database_path.with_extension("sqlite-wal"),
        database_path.with_extension("sqlite-shm"),
    ]
    .into_iter()
    .try_fold(0_u64, |total, path| {
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                Err(SpaceBudgetError::FilesystemObservation(format!(
                    "SQLite storage path is not a regular file: {}",
                    path.display()
                )))
            }
            Ok(metadata) => metadata
                .blocks()
                .checked_mul(512)
                .and_then(|bytes| total.checked_add(bytes))
                .ok_or(SpaceBudgetError::ArithmeticOverflow),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(total),
            Err(error) => Err(SpaceBudgetError::FilesystemObservation(format!(
                "could not measure SQLite storage {}: {error}",
                path.display()
            ))),
        }
    })
}

fn inspect_wal(
    wal_path: &std::path::Path,
    expected_page_size: u64,
) -> Result<(u64, u64, u64), SpaceBudgetError> {
    let metadata = match std::fs::symlink_metadata(wal_path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(SpaceBudgetError::FilesystemObservation(format!(
                "SQLite WAL path is not a regular file: {}",
                wal_path.display()
            )))
        }
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((0, 0, 0)),
        Err(error) => {
            return Err(SpaceBudgetError::FilesystemObservation(format!(
                "could not inspect SQLite WAL {}: {error}",
                wal_path.display()
            )))
        }
    };
    use std::os::unix::fs::MetadataExt;
    let allocated = metadata
        .blocks()
        .checked_mul(512)
        .ok_or(SpaceBudgetError::ArithmeticOverflow)?;
    let logical_bytes = metadata.len();
    if logical_bytes == 0 {
        return Ok((allocated, 0, 0));
    }
    if logical_bytes < 32 {
        return Err(SpaceBudgetError::InvalidReconstruction(
            "SQLite WAL header is truncated",
        ));
    }
    let mut wal = File::open(wal_path).map_err(|error| {
        SpaceBudgetError::FilesystemObservation(format!(
            "could not open SQLite WAL {}: {error}",
            wal_path.display()
        ))
    })?;
    let mut header = [0_u8; 32];
    wal.read_exact(&mut header).map_err(|error| {
        SpaceBudgetError::FilesystemObservation(format!(
            "could not read SQLite WAL header {}: {error}",
            wal_path.display()
        ))
    })?;
    let magic = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
    if !matches!(magic, 0x377f_0682 | 0x377f_0683) {
        return Err(SpaceBudgetError::InvalidReconstruction(
            "SQLite WAL magic is invalid",
        ));
    }
    let wal_page_size = u64::from(u32::from_be_bytes([
        header[8], header[9], header[10], header[11],
    ]));
    if wal_page_size != expected_page_size {
        return Err(SpaceBudgetError::InvalidReconstruction(
            "SQLite WAL page size does not match the main database",
        ));
    }
    let frame_size = expected_page_size
        .checked_add(24)
        .ok_or(SpaceBudgetError::ArithmeticOverflow)?;
    let frame_bytes = logical_bytes
        .checked_sub(32)
        .ok_or(SpaceBudgetError::ArithmeticOverflow)?;
    if frame_bytes % frame_size != 0 {
        return Err(SpaceBudgetError::InvalidReconstruction(
            "SQLite WAL frame data is truncated",
        ));
    }
    Ok((allocated, frame_bytes / frame_size, logical_bytes))
}

fn allocated_regular_file(path: &std::path::Path) -> Result<u64, SpaceBudgetError> {
    match allocated_optional_regular_file(path)? {
        0 => Err(SpaceBudgetError::InvalidReconstruction(
            "SQLite main file has no allocated blocks",
        )),
        allocated => Ok(allocated),
    }
}

fn allocated_optional_regular_file(path: &std::path::Path) -> Result<u64, SpaceBudgetError> {
    use std::os::unix::fs::MetadataExt;

    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(SpaceBudgetError::FilesystemObservation(format!(
                "SQLite storage path is not a regular file: {}",
                path.display()
            )))
        }
        Ok(metadata) => metadata
            .blocks()
            .checked_mul(512)
            .ok_or(SpaceBudgetError::ArithmeticOverflow),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(SpaceBudgetError::FilesystemObservation(format!(
            "could not measure SQLite storage {}: {error}",
            path.display()
        ))),
    }
}

fn round_up(value: u64, alignment: u64) -> Result<u64, SpaceBudgetError> {
    let mask = alignment
        .checked_sub(1)
        .ok_or(SpaceBudgetError::ArithmeticOverflow)?;
    value
        .checked_add(mask)
        .map(|rounded| rounded & !mask)
        .ok_or(SpaceBudgetError::ArithmeticOverflow)
}

fn validate_identity(value: &str) -> Result<(), SpaceBudgetError> {
    if value.is_empty()
        || value.len() > MAX_RESERVATION_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(SpaceBudgetError::InvalidIdentity);
    }
    Ok(())
}

fn validate_owner(value: &str) -> Result<(), SpaceBudgetError> {
    if value.is_empty() || value.len() > MAX_OWNER_TEXT_BYTES || value.bytes().any(|byte| byte == 0)
    {
        return Err(SpaceBudgetError::InvalidIdentity);
    }
    Ok(())
}

fn checked_increment(value: u64) -> Result<u64, SpaceBudgetError> {
    value
        .checked_add(1)
        .ok_or(SpaceBudgetError::ArithmeticOverflow)
}
