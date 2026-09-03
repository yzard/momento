use super::file::{NormalizedStoragePath, PathClaimMode, PathClaimScope, StorageRootId};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::collections::BTreeMap;

use crate::database::queries;
use crate::io::space_budget::{
    DurableSpaceCheckout, ProvisionalSpaceToken, SpaceBudgetError, SpaceReservationClass,
};
use crate::models::{
    FileOperationCompactedSummary, FileOperationDetailResponse, FileOperationEntryDetail,
    FileOperationListResponse, FileOperationPathClaimDetail, FileOperationSummary,
    FILE_OPERATION_LIST_LIMIT_MAX,
};

pub const MAX_FILE_OPERATION_ENTRIES_PER_GROUP: usize = 256;
pub const MAX_FILE_OPERATION_CLAIMS_PER_GROUP: usize = 512;
pub const MAX_LIVE_RETRY_RECEIPTS_PER_OPERATION: i64 = 64;
const MAX_JOURNAL_ID_BYTES: usize = 128;
const MAX_JOURNAL_TEXT_BYTES: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileEntryAction {
    Publish,
    Move,
    Tombstone,
    Cleanup,
}

impl FileEntryAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Publish => "publish",
            Self::Move => "move",
            Self::Tombstone => "tombstone",
            Self::Cleanup => "cleanup",
        }
    }
}

impl TryFrom<&str> for FileEntryAction {
    type Error = JournalPlanError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "publish" => Ok(Self::Publish),
            "move" => Ok(Self::Move),
            "tombstone" => Ok(Self::Tombstone),
            "cleanup" => Ok(Self::Cleanup),
            _ => Err(JournalPlanError::InvalidEntry),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileEntryPlan {
    pub action: FileEntryAction,
    pub storage_root: StorageRootId,
    pub source_path: Option<NormalizedStoragePath>,
    pub temporary_path: Option<NormalizedStoragePath>,
    pub destination_path: Option<NormalizedStoragePath>,
    pub tombstone_path: Option<NormalizedStoragePath>,
    pub expected_size: Option<u64>,
    pub expected_sha256: Option<[u8; 32]>,
    pub expected_version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FilePathClaimPlan {
    pub storage_root: StorageRootId,
    pub path: NormalizedStoragePath,
    pub mode: PathClaimMode,
    pub scope: PathClaimScope,
    pub role: String,
    pub expected_version: Option<String>,
}

#[derive(Debug)]
pub struct JournalSpaceReservationPlan {
    token: ProvisionalSpaceToken,
}

impl JournalSpaceReservationPlan {
    pub fn new(token: ProvisionalSpaceToken) -> Result<Self, SpaceBudgetError> {
        if token.class() != SpaceReservationClass::Journal {
            return Err(SpaceBudgetError::ReservationStateMismatch);
        }
        Ok(Self { token })
    }

    fn reservation_id(&self) -> &str {
        self.token.reservation_id()
    }

    fn filesystem_id(&self) -> &str {
        self.token.filesystem_id()
    }

    fn reserved_peak_additional_bytes(&self) -> u64 {
        self.token.peak_additional_bytes()
    }

    fn commit(self, group_id: String) -> Result<DurableSpaceCheckout, SpaceBudgetError> {
        self.token.commit_to_durable_owner(
            "file_operation_group".to_string(),
            group_id.clone(),
            Some(group_id),
        )
    }
}

#[derive(Debug)]
pub struct FileOperationPlan {
    pub group_id: String,
    pub kind: String,
    pub owner_kind: String,
    pub owner_id: String,
    pub claim_token: Option<String>,
    pub product_target: Option<String>,
    pub product_version: Option<i64>,
    pub entries: Vec<FileEntryPlan>,
    pub claims: Vec<FilePathClaimPlan>,
    pub space_reservation: Option<JournalSpaceReservationPlan>,
}

#[derive(Debug, Clone)]
pub struct DirectoryCopyConstructionPlan {
    pub storage_root: StorageRootId,
    pub source_root: NormalizedStoragePath,
    pub temporary_root: NormalizedStoragePath,
    pub expected_file_bytes: u64,
    pub expected_entry_count: u64,
    pub expected_fingerprint: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryCopyCursor {
    pub depth: u16,
    pub source_path: NormalizedStoragePath,
    pub temporary_path: NormalizedStoragePath,
    pub resume_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryCopyConstruction {
    pub group_id: String,
    pub storage_root: StorageRootId,
    pub expected_file_bytes: u64,
    pub expected_entry_count: u64,
    pub expected_fingerprint: [u8; 32],
    pub copied_file_bytes: u64,
    pub copied_entry_count: u64,
    pub copied_fingerprint: [u8; 32],
    pub complete: bool,
    pub publication_entry_count: u16,
    pub has_cleanup: bool,
    pub cursors: Vec<DirectoryCopyCursor>,
}

#[derive(Debug, Clone)]
pub struct DirectoryCopyEntryCheckpoint {
    pub group_id: String,
    pub depth: u16,
    pub expected_resume_offset: u64,
    pub next_resume_offset: u64,
    pub file_bytes: u64,
    pub fingerprint: [u8; 32],
    pub child: Option<DirectoryCopyCursor>,
}

#[derive(Debug, Clone)]
pub struct DirectoryCopyFinishedCheckpoint {
    pub group_id: String,
    pub depth: u16,
    pub expected_resume_offset: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrepareJournalOutcome {
    Prepared,
    PathConflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalCheckpointOutcome {
    Advanced { version: i64 },
    VersionConflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JournalEntryCheckpoint {
    pub version: i64,
    pub phase_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalRecoveryGroup {
    pub group_id: String,
    pub state: JournalRecoveryState,
    pub version: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalRecoveryState {
    Publishing,
    FilesCommitted,
    CleanupPending,
    RollbackPending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JournalRecoveryScope {
    All,
    StartupCritical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalFailureStage {
    Publication,
    Cleanup,
    Rollback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalRetryOutcome {
    Accepted {
        state: String,
        version: i64,
        replayed: bool,
    },
    VersionConflict,
    RequestConflict,
    ReceiptLimitReached,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalCancellationOutcome {
    Requested { state: String, version: i64 },
    AlreadyRequested { state: String, version: i64 },
    VersionConflict,
    NotCancellable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalCancellationStatus {
    pub state: String,
    pub version: i64,
    pub cancel_requested: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JournalMaintenanceOutcome {
    pub expired_retry_receipts: usize,
    pub expired_result_receipts: usize,
    pub compacted_groups: usize,
    pub pruned_groups: usize,
}

#[derive(Debug)]
pub struct JournalMutationGrant {
    group_id: String,
    group_version: i64,
    stage: JournalMutationStage,
    entries: Vec<AuthorizedJournalEntry>,
}

#[derive(Debug, Clone)]
pub(crate) struct AuthorizedJournalEntry {
    pub sequence: u16,
    pub action: FileEntryAction,
    pub storage_root: StorageRootId,
    pub source_path: Option<NormalizedStoragePath>,
    pub temporary_path: Option<NormalizedStoragePath>,
    pub destination_path: Option<NormalizedStoragePath>,
    pub tombstone_path: Option<NormalizedStoragePath>,
    pub expected_size: Option<u64>,
    pub expected_sha256: Option<[u8; 32]>,
    pub expected_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JournalMutationStage {
    Publication,
    Cleanup,
    Rollback,
}

impl JournalMutationGrant {
    pub(crate) fn stage(&self) -> JournalMutationStage {
        self.stage
    }

    pub(crate) fn entries_mut(&mut self) -> &mut [AuthorizedJournalEntry] {
        &mut self.entries
    }

    pub(crate) fn first_sequence(&self) -> Option<u16> {
        self.entries.first().map(|entry| entry.sequence)
    }

    pub(crate) fn publication(
        group_id: String,
        group_version: i64,
        entries: Vec<AuthorizedJournalEntry>,
    ) -> Self {
        Self {
            group_id,
            group_version,
            stage: JournalMutationStage::Publication,
            entries,
        }
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        String,
        i64,
        JournalMutationStage,
        Vec<AuthorizedJournalEntry>,
    ) {
        (self.group_id, self.group_version, self.stage, self.entries)
    }

    pub(crate) fn cleanup(
        group_id: String,
        group_version: i64,
        entries: Vec<AuthorizedJournalEntry>,
    ) -> Self {
        Self {
            group_id,
            group_version,
            stage: JournalMutationStage::Cleanup,
            entries,
        }
    }

    pub(crate) fn rollback(
        group_id: String,
        group_version: i64,
        entries: Vec<AuthorizedJournalEntry>,
    ) -> Self {
        Self {
            group_id,
            group_version,
            stage: JournalMutationStage::Rollback,
            entries,
        }
    }
}

impl FileOperationPlan {
    pub fn validate(&self) -> Result<(), JournalPlanError> {
        validate_id(&self.group_id)?;
        validate_text(&self.kind)?;
        validate_text(&self.owner_kind)?;
        validate_id(&self.owner_id)?;
        if self
            .claim_token
            .as_ref()
            .is_some_and(|token| uuid::Uuid::parse_str(token).is_err())
        {
            return Err(JournalPlanError::InvalidPlan);
        }
        if self
            .product_target
            .as_ref()
            .is_some_and(|value| validate_text(value).is_err())
            || self.product_version.is_some_and(|version| version < 0)
            || !(1..=MAX_FILE_OPERATION_ENTRIES_PER_GROUP).contains(&self.entries.len())
            || self.claims.is_empty()
            || self.claims.len() > MAX_FILE_OPERATION_CLAIMS_PER_GROUP
        {
            return Err(JournalPlanError::InvalidPlan);
        }
        if self.entries.iter().any(|entry| {
            !entry_paths_match_action(entry)
                || entry
                    .expected_size
                    .is_some_and(|size| size > i64::MAX as u64)
                || entry
                    .expected_version
                    .as_ref()
                    .is_some_and(|value| validate_text(value).is_err())
        }) {
            return Err(JournalPlanError::InvalidEntry);
        }
        for claim in &self.claims {
            validate_text(&claim.role)?;
            if claim
                .expected_version
                .as_ref()
                .is_some_and(|value| validate_text(value).is_err())
            {
                return Err(JournalPlanError::InvalidPlan);
            }
        }
        for entry in &self.entries {
            for path in entry_mutation_paths(entry) {
                if !self
                    .claims
                    .iter()
                    .any(|claim| claim.as_claim().covers_write_path(entry.storage_root, path))
                {
                    return Err(JournalPlanError::UnclaimedMutationPath);
                }
            }
        }
        for (index, claim) in self.claims.iter().enumerate() {
            if self.claims[index + 1..].iter().any(|other| {
                let left = claim.as_claim();
                let right = other.as_claim();
                left.conflicts_with(&right)
            }) {
                return Err(JournalPlanError::ConflictingClaims);
            }
        }
        let produces_bytes = self
            .entries
            .iter()
            .any(|entry| entry.action == FileEntryAction::Publish);
        if produces_bytes != self.space_reservation.is_some() {
            return Err(JournalPlanError::InvalidReservation);
        }
        if let Some(reservation) = &self.space_reservation {
            validate_id(reservation.reservation_id())?;
            validate_id(reservation.filesystem_id())?;
            if reservation.reserved_peak_additional_bytes() == 0
                || reservation.reserved_peak_additional_bytes() > i64::MAX as u64
            {
                return Err(JournalPlanError::InvalidReservation);
            }
        }
        Ok(())
    }
}

pub(crate) fn prepare_file_operation(
    connection: &mut Connection,
    plan: FileOperationPlan,
) -> rusqlite::Result<PrepareJournalOutcome> {
    prepare_file_operation_with(connection, plan, |_| Ok(()))
}

pub(crate) fn prepare_directory_copy_operation(
    connection: &mut Connection,
    plan: FileOperationPlan,
    construction: DirectoryCopyConstructionPlan,
) -> rusqlite::Result<PrepareJournalOutcome> {
    if plan.kind != "webdav_directory_copy"
        || construction.storage_root != StorageRootId::WebDav
        || construction.expected_file_bytes > i64::MAX as u64
        || construction.expected_entry_count > i64::MAX as u64
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let group_id = plan.group_id.clone();
    prepare_file_operation_with(connection, plan, |transaction| {
        transaction.execute(
            queries::file_operations::INSERT_DIRECTORY_COPY,
            params![
                group_id,
                construction.storage_root.as_str(),
                construction.source_root.relative_path(),
                construction.temporary_root.relative_path(),
                construction.expected_file_bytes as i64,
                construction.expected_entry_count as i64,
                construction.expected_fingerprint.to_vec(),
            ],
        )?;
        transaction.execute(
            queries::file_operations::INSERT_DIRECTORY_COPY_ROOT_CURSOR,
            params![
                group_id,
                construction.source_root.relative_path(),
                construction.temporary_root.relative_path(),
            ],
        )?;
        Ok(())
    })
}

pub(crate) fn load_directory_copy(
    connection: &Connection,
    group_id: Option<&str>,
) -> rusqlite::Result<Option<DirectoryCopyConstruction>> {
    let header = connection
        .query_row(
            queries::file_operations::SELECT_DIRECTORY_COPY,
            params![group_id, group_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, Vec<u8>>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, i64>(9)?,
                ))
            },
        )
        .optional()?;
    let Some((
        group_id,
        storage_root,
        expected_file_bytes,
        expected_entry_count,
        expected_fingerprint,
        copied_file_bytes,
        copied_entry_count,
        copied_fingerprint,
        state,
        entry_count,
    )) = header
    else {
        return Ok(None);
    };
    let storage_root = StorageRootId::try_from(storage_root.as_str())
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let expected_fingerprint: [u8; 32] = expected_fingerprint
        .try_into()
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let copied_fingerprint: [u8; 32] = copied_fingerprint
        .try_into()
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let mut statement =
        connection.prepare(queries::file_operations::SELECT_DIRECTORY_COPY_CURSORS)?;
    let cursors = statement
        .query_map([&group_id], |row| {
            let depth = row.get::<_, i64>(0)?;
            let resume_offset = row.get::<_, i64>(3)?;
            Ok(DirectoryCopyCursor {
                depth: u16::try_from(depth).map_err(|_| rusqlite::Error::InvalidQuery)?,
                source_path: NormalizedStoragePath::parse(&row.get::<_, String>(1)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                temporary_path: NormalizedStoragePath::parse(&row.get::<_, String>(2)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                resume_offset: u64::try_from(resume_offset)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !matches!(entry_count, 1 | 3)
        || !matches!(state.as_str(), "building" | "complete")
        || state == "building" && cursors.is_empty()
        || state == "complete" && !cursors.is_empty()
        || cursors
            .iter()
            .enumerate()
            .any(|(depth, cursor)| usize::from(cursor.depth) != depth)
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(Some(DirectoryCopyConstruction {
        group_id,
        storage_root,
        expected_file_bytes: u64::try_from(expected_file_bytes)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        expected_entry_count: u64::try_from(expected_entry_count)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        expected_fingerprint,
        copied_file_bytes: u64::try_from(copied_file_bytes)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        copied_entry_count: u64::try_from(copied_entry_count)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        copied_fingerprint,
        complete: state == "complete",
        publication_entry_count: if entry_count == 3 { 2 } else { 1 },
        has_cleanup: entry_count == 3,
        cursors,
    }))
}

pub(crate) fn checkpoint_directory_copy_entry(
    connection: &mut Connection,
    checkpoint: DirectoryCopyEntryCheckpoint,
) -> rusqlite::Result<bool> {
    if checkpoint.next_resume_offset > i64::MAX as u64
        || checkpoint.expected_resume_offset > i64::MAX as u64
        || checkpoint.file_bytes > i64::MAX as u64
        || checkpoint.child.as_ref().is_some_and(|child| {
            checkpoint
                .depth
                .checked_add(1)
                .is_none_or(|expected| child.depth != expected || child.resume_offset != 0)
        })
        || checkpoint.next_resume_offset == checkpoint.expected_resume_offset
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let top_depth: Option<i64> = transaction.query_row(
        "SELECT MAX(depth) FROM directory_copy_cursors WHERE group_id = ?",
        [&checkpoint.group_id],
        |row| row.get(0),
    )?;
    if top_depth != Some(i64::from(checkpoint.depth)) {
        transaction.rollback()?;
        return Ok(false);
    }
    let current_fingerprint: Vec<u8> = transaction.query_row(
        "SELECT copied_fingerprint FROM directory_copy_constructions WHERE group_id = ? AND state = 'building'",
        [&checkpoint.group_id],
        |row| row.get(0),
    )?;
    let mut updated_fingerprint: [u8; 32] = current_fingerprint
        .try_into()
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    for (accumulator, byte) in updated_fingerprint.iter_mut().zip(checkpoint.fingerprint) {
        *accumulator ^= byte;
    }
    let changed = transaction.execute(
        queries::file_operations::ADVANCE_DIRECTORY_COPY_CURSOR,
        params![
            checkpoint.next_resume_offset as i64,
            checkpoint.group_id,
            i64::from(checkpoint.depth),
            checkpoint.expected_resume_offset as i64,
        ],
    )?;
    if changed == 0 {
        transaction.rollback()?;
        return Ok(false);
    }
    if let Some(child) = checkpoint.child {
        transaction.execute(
            queries::file_operations::INSERT_DIRECTORY_COPY_CURSOR,
            params![
                checkpoint.group_id,
                i64::from(child.depth),
                child.source_path.relative_path(),
                child.temporary_path.relative_path(),
            ],
        )?;
    }
    let measured = transaction.execute(
        queries::file_operations::UPDATE_DIRECTORY_COPY_MEASUREMENT,
        params![
            checkpoint.file_bytes as i64,
            updated_fingerprint.to_vec(),
            checkpoint.group_id,
            checkpoint.file_bytes as i64,
        ],
    )?;
    if measured != 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    transaction.commit()?;
    Ok(true)
}

pub(crate) fn checkpoint_directory_copy_finished(
    connection: &mut Connection,
    checkpoint: DirectoryCopyFinishedCheckpoint,
) -> rusqlite::Result<bool> {
    if checkpoint.expected_resume_offset > i64::MAX as u64 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let top_depth: Option<i64> = transaction.query_row(
        "SELECT MAX(depth) FROM directory_copy_cursors WHERE group_id = ?",
        [&checkpoint.group_id],
        |row| row.get(0),
    )?;
    if top_depth != Some(i64::from(checkpoint.depth)) {
        transaction.rollback()?;
        return Ok(false);
    }
    let current_offset: Option<i64> = transaction
        .query_row(
            queries::file_operations::SELECT_DIRECTORY_COPY_CURSOR,
            params![checkpoint.group_id, i64::from(checkpoint.depth)],
            |row| row.get(2),
        )
        .optional()?;
    if current_offset != Some(checkpoint.expected_resume_offset as i64) {
        transaction.rollback()?;
        return Ok(false);
    }
    transaction.execute(
        queries::file_operations::DELETE_DIRECTORY_COPY_CURSOR,
        params![checkpoint.group_id, i64::from(checkpoint.depth)],
    )?;
    if checkpoint.depth == 0 {
        let changed = transaction.execute(
            queries::file_operations::COMPLETE_DIRECTORY_COPY,
            [&checkpoint.group_id],
        )?;
        if changed != 1 {
            return Err(rusqlite::Error::InvalidQuery);
        }
    }
    transaction.commit()?;
    Ok(true)
}

pub(crate) fn prepare_file_operation_with<PrepareOwner>(
    connection: &mut Connection,
    mut plan: FileOperationPlan,
    prepare_owner: PrepareOwner,
) -> rusqlite::Result<PrepareJournalOutcome>
where
    PrepareOwner: FnOnce(&rusqlite::Transaction<'_>) -> rusqlite::Result<()>,
{
    plan.validate().map_err(|_| rusqlite::Error::InvalidQuery)?;
    let reservation = plan.space_reservation.take();
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let outcome = insert_file_operation(&transaction, &plan, reservation.as_ref(), false)?;
    if outcome == PrepareJournalOutcome::PathConflict {
        transaction.rollback()?;
        return Ok(outcome);
    }
    prepare_owner(&transaction)?;
    transaction.commit()?;
    if let Some(reservation) = reservation {
        let checkout = reservation
            .commit(plan.group_id.clone())
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        drop(checkout);
    }
    Ok(outcome)
}

pub(crate) fn prepare_committed_cleanup(
    transaction: &rusqlite::Transaction<'_>,
    plan: FileOperationPlan,
) -> rusqlite::Result<PrepareJournalOutcome> {
    plan.validate().map_err(|_| rusqlite::Error::InvalidQuery)?;
    if plan
        .entries
        .iter()
        .any(|entry| entry.action != FileEntryAction::Cleanup)
        || plan.space_reservation.is_some()
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    insert_file_operation(transaction, &plan, None, true)
}

fn insert_file_operation(
    transaction: &rusqlite::Transaction<'_>,
    plan: &FileOperationPlan,
    reservation: Option<&JournalSpaceReservationPlan>,
    committed_cleanup: bool,
) -> rusqlite::Result<PrepareJournalOutcome> {
    if let Some(claim_token) = &plan.claim_token {
        transaction.query_row(
            queries::file_operations::VERIFY_OPERATION_CLAIM_OWNER,
            [claim_token],
            |_| Ok(()),
        )?;
    }
    for claim in &plan.claims {
        let mode = claim.mode.as_str();
        let root = claim.storage_root.as_str();
        let equal = transaction
            .query_row(
                queries::file_operations::FIND_EQUAL_CLAIM_CONFLICT,
                params![root, claim.path.path_key(), mode],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        let mut ancestor = false;
        for ancestor_key in claim.path.ancestor_keys() {
            ancestor = transaction
                .query_row(
                    queries::file_operations::FIND_SUBTREE_ANCESTOR_CONFLICT,
                    params![root, ancestor_key, mode],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if ancestor {
                break;
            }
        }
        if equal || ancestor {
            return Ok(PrepareJournalOutcome::PathConflict);
        }
        if claim.scope == PathClaimScope::Subtree {
            let upper_bound = claim.path.subtree_upper_bound();
            let descendant = transaction
                .query_row(
                    queries::file_operations::FIND_SUBTREE_DESCENDANT_CONFLICT,
                    params![root, claim.path.path_key(), upper_bound, mode],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if descendant {
                return Ok(PrepareJournalOutcome::PathConflict);
            }
        }
    }
    transaction.execute(
        if committed_cleanup {
            queries::file_operations::INSERT_COMMITTED_CLEANUP_GROUP
        } else {
            queries::file_operations::INSERT_GROUP
        },
        params![
            plan.group_id,
            plan.kind,
            plan.owner_kind,
            plan.owner_id,
            plan.claim_token,
            plan.product_target,
            plan.product_version,
            plan.entries.len() as i64,
        ],
    )?;
    for (sequence, entry) in plan.entries.iter().enumerate() {
        transaction.execute(
            queries::file_operations::INSERT_ENTRY,
            params![
                plan.group_id,
                sequence as i64,
                entry.action.as_str(),
                entry.storage_root.as_str(),
                entry
                    .source_path
                    .as_ref()
                    .map(|path| path.relative_path().to_string()),
                entry
                    .temporary_path
                    .as_ref()
                    .map(|path| path.relative_path().to_string()),
                entry
                    .destination_path
                    .as_ref()
                    .map(|path| path.relative_path().to_string()),
                entry
                    .tombstone_path
                    .as_ref()
                    .map(|path| path.relative_path().to_string()),
                entry.expected_size.map(|size| size as i64),
                entry.expected_sha256.map(Vec::from),
                entry.expected_version.as_deref(),
            ],
        )?;
    }
    for (sequence, claim) in plan.claims.iter().enumerate() {
        transaction.execute(
            queries::file_operations::INSERT_PATH_CLAIM,
            params![
                plan.group_id,
                sequence as i64,
                claim.storage_root.as_str(),
                claim.path.relative_path(),
                claim.path.path_key(),
                claim.mode.as_str(),
                claim.scope.as_str(),
                claim.role,
                claim.expected_version.as_deref(),
            ],
        )?;
    }
    if let Some(reservation) = reservation {
        transaction.execute(
            queries::file_operations::INSERT_JOURNAL_RESERVATION,
            params![
                reservation.reservation_id(),
                "file_operation_group",
                plan.group_id,
                plan.group_id,
                reservation.filesystem_id(),
                reservation.reserved_peak_additional_bytes() as i64,
            ],
        )?;
    }
    Ok(PrepareJournalOutcome::Prepared)
}

pub(crate) fn begin_file_operation_publication(
    connection: &mut Connection,
    group_id: &str,
    expected_version: i64,
) -> rusqlite::Result<Option<JournalMutationGrant>> {
    validate_id(group_id).map_err(|_| rusqlite::Error::InvalidQuery)?;
    if expected_version < 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let changed = transaction.execute(
        queries::file_operations::BEGIN_PUBLICATION,
        params![group_id, expected_version],
    )?;
    if changed == 0 {
        transaction.rollback()?;
        return Ok(None);
    }
    let version = expected_version + 1;
    let entries = load_authorized_entries(
        &transaction,
        queries::file_operations::SELECT_PENDING_PUBLICATION_ENTRIES,
        group_id,
    )?;
    transaction.commit()?;
    Ok(Some(JournalMutationGrant::publication(
        group_id.to_string(),
        version,
        entries,
    )))
}

pub(crate) fn verify_file_operation_publication(
    connection: &mut Connection,
    group_id: &str,
    expected_version: i64,
) -> rusqlite::Result<Option<JournalMutationGrant>> {
    validate_id(group_id).map_err(|_| rusqlite::Error::InvalidQuery)?;
    if expected_version < 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let transaction = connection.transaction()?;
    let matches = transaction
        .query_row(
            queries::file_operations::VERIFY_PUBLICATION,
            params![group_id, expected_version],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !matches {
        transaction.rollback()?;
        return Ok(None);
    }
    let entries = load_authorized_entries(
        &transaction,
        queries::file_operations::SELECT_PENDING_PUBLICATION_ENTRIES,
        group_id,
    )?;
    transaction.commit()?;
    Ok(Some(JournalMutationGrant::publication(
        group_id.to_string(),
        expected_version,
        entries,
    )))
}

pub(crate) fn record_file_entry_published(
    connection: &mut Connection,
    group_id: &str,
    expected_group_version: i64,
    sequence: u16,
) -> rusqlite::Result<Option<JournalEntryCheckpoint>> {
    validate_id(group_id).map_err(|_| rusqlite::Error::InvalidQuery)?;
    if expected_group_version < 1 || sequence > 255 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let changed = transaction.execute(
        queries::file_operations::COMMIT_ENTRY,
        params![group_id, i64::from(sequence)],
    )?;
    if changed == 0 {
        transaction.rollback()?;
        return Ok(None);
    }
    let remaining: i64 = transaction.query_row(
        queries::file_operations::COUNT_UNCOMMITTED_ENTRIES,
        params![group_id],
        |row| row.get(0),
    )?;
    let files_committed = remaining == 0;
    let state = if files_committed {
        "files_committed"
    } else {
        "publishing"
    };
    let group_changed = transaction.execute(
        queries::file_operations::CHECKPOINT_PUBLICATION,
        params![state, group_id, expected_group_version],
    )?;
    if group_changed == 0 {
        transaction.rollback()?;
        return Ok(None);
    }
    transaction.commit()?;
    Ok(Some(JournalEntryCheckpoint {
        version: expected_group_version + 1,
        phase_complete: files_committed,
    }))
}

pub(crate) fn complete_file_operation(
    connection: &mut Connection,
    group_id: &str,
    expected_version: i64,
) -> rusqlite::Result<JournalCheckpointOutcome> {
    validate_id(group_id).map_err(|_| rusqlite::Error::InvalidQuery)?;
    if expected_version < 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let changed = transaction.execute(
        queries::file_operations::COMPLETE_PUBLICATION,
        params![group_id, expected_version],
    )?;
    if changed == 0 {
        transaction.rollback()?;
        return Ok(JournalCheckpointOutcome::VersionConflict);
    }
    transaction.execute(
        queries::file_operations::RELEASE_GROUP_CLAIMS,
        params![group_id],
    )?;
    transaction.execute(
        queries::file_operations::RELEASE_GROUP_RESERVATION,
        params![group_id],
    )?;
    transaction.commit()?;
    Ok(JournalCheckpointOutcome::Advanced {
        version: expected_version + 1,
    })
}

pub(crate) fn verify_file_operation_cleanup(
    connection: &mut Connection,
    group_id: &str,
    expected_version: i64,
) -> rusqlite::Result<Option<JournalMutationGrant>> {
    validate_id(group_id).map_err(|_| rusqlite::Error::InvalidQuery)?;
    if expected_version < 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let transaction = connection.transaction()?;
    let matches = transaction
        .query_row(
            queries::file_operations::VERIFY_CLEANUP,
            params![group_id, expected_version],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !matches {
        transaction.rollback()?;
        return Ok(None);
    }
    let entries = load_authorized_entries(
        &transaction,
        queries::file_operations::SELECT_PENDING_CLEANUP_ENTRIES,
        group_id,
    )?;
    transaction.commit()?;
    Ok(Some(JournalMutationGrant::cleanup(
        group_id.to_string(),
        expected_version,
        entries,
    )))
}

pub(crate) fn record_file_entry_cleaned(
    connection: &mut Connection,
    group_id: &str,
    expected_group_version: i64,
    sequence: u16,
) -> rusqlite::Result<Option<JournalEntryCheckpoint>> {
    validate_id(group_id).map_err(|_| rusqlite::Error::InvalidQuery)?;
    if expected_group_version < 1 || sequence > 255 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let changed = transaction.execute(
        queries::file_operations::CLEAN_ENTRY,
        params![group_id, i64::from(sequence)],
    )?;
    if changed == 0 {
        transaction.rollback()?;
        return Ok(None);
    }
    let remaining: i64 = transaction.query_row(
        queries::file_operations::COUNT_UNCLEANED_ENTRIES,
        params![group_id],
        |row| row.get(0),
    )?;
    let cleaned = remaining == 0;
    let state = if cleaned {
        "cleaned"
    } else {
        "cleanup_pending"
    };
    let group_changed = transaction.execute(
        queries::file_operations::CHECKPOINT_CLEANUP,
        params![state, state, group_id, expected_group_version],
    )?;
    if group_changed == 0 {
        transaction.rollback()?;
        return Ok(None);
    }
    if cleaned {
        release_group_ownership(&transaction, group_id)?;
    }
    transaction.commit()?;
    Ok(Some(JournalEntryCheckpoint {
        version: expected_group_version + 1,
        phase_complete: cleaned,
    }))
}

pub(crate) fn request_file_operation_cancellation(
    connection: &mut Connection,
    group_id: &str,
    expected_version: i64,
) -> rusqlite::Result<JournalCancellationOutcome> {
    validate_id(group_id).map_err(|_| rusqlite::Error::InvalidQuery)?;
    if expected_version < 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let group = transaction
        .query_row(
            queries::file_operations::SELECT_GROUP_FOR_CANCELLATION,
            [group_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, bool>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((state, current_version, cancel_requested)) = group else {
        transaction.rollback()?;
        return Ok(JournalCancellationOutcome::VersionConflict);
    };
    if current_version != expected_version {
        transaction.rollback()?;
        return Ok(JournalCancellationOutcome::VersionConflict);
    }
    if cancel_requested {
        transaction.rollback()?;
        return Ok(JournalCancellationOutcome::AlreadyRequested {
            state,
            version: current_version,
        });
    }
    let next_version = expected_version + 1;
    let next_state = match state.as_str() {
        "prepared" => {
            if transaction.execute(
                queries::file_operations::REQUEST_PRECOMMIT_ROLLBACK,
                params![group_id, expected_version],
            )? != 1
            {
                transaction.rollback()?;
                return Ok(JournalCancellationOutcome::VersionConflict);
            }
            transaction.execute(
                queries::file_operations::MARK_NON_PUBLISH_ENTRIES_ROLLED_BACK,
                [group_id],
            )?;
            let pending: i64 = transaction.query_row(
                queries::file_operations::COUNT_PENDING_ROLLBACK_ENTRIES,
                [group_id],
                |row| row.get(0),
            )?;
            if pending == 0 {
                if transaction.execute(
                    queries::file_operations::COMPLETE_EMPTY_ROLLBACK,
                    [group_id],
                )? != 1
                {
                    transaction.rollback()?;
                    return Ok(JournalCancellationOutcome::VersionConflict);
                }
                release_group_ownership(&transaction, group_id)?;
                "rolled_back"
            } else {
                "rollback_pending"
            }
        }
        "publishing" | "publication_failed" | "files_committed" | "finalize_failed" => {
            if transaction.execute(
                queries::file_operations::REQUEST_FORWARD_DISCARD,
                params![group_id, expected_version],
            )? != 1
            {
                transaction.rollback()?;
                return Ok(JournalCancellationOutcome::VersionConflict);
            }
            state.as_str()
        }
        _ => {
            transaction.rollback()?;
            return Ok(JournalCancellationOutcome::NotCancellable);
        }
    };
    transaction.execute(
        queries::file_operations::DETACH_CANCELLED_DISCARDABLE_PRODUCT,
        [group_id],
    )?;
    transaction.commit()?;
    Ok(JournalCancellationOutcome::Requested {
        state: next_state.to_string(),
        version: next_version,
    })
}

pub(crate) fn load_file_operation_cancellation_status(
    connection: &Connection,
    group_id: &str,
) -> rusqlite::Result<Option<JournalCancellationStatus>> {
    validate_id(group_id).map_err(|_| rusqlite::Error::InvalidQuery)?;
    connection
        .query_row(
            queries::file_operations::SELECT_GROUP_FOR_CANCELLATION,
            [group_id],
            |row| {
                Ok(JournalCancellationStatus {
                    state: row.get(0)?,
                    version: row.get(1)?,
                    cancel_requested: row.get(2)?,
                })
            },
        )
        .optional()
}

pub(crate) fn verify_file_operation_rollback(
    connection: &mut Connection,
    group_id: &str,
    expected_version: i64,
) -> rusqlite::Result<Option<JournalMutationGrant>> {
    validate_id(group_id).map_err(|_| rusqlite::Error::InvalidQuery)?;
    if expected_version < 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let transaction = connection.transaction()?;
    let matches = transaction
        .query_row(
            queries::file_operations::VERIFY_ROLLBACK,
            params![group_id, expected_version],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !matches {
        transaction.rollback()?;
        return Ok(None);
    }
    let entries = load_authorized_entries(
        &transaction,
        queries::file_operations::SELECT_PENDING_ROLLBACK_ENTRIES,
        group_id,
    )?;
    transaction.commit()?;
    Ok(Some(JournalMutationGrant::rollback(
        group_id.to_string(),
        expected_version,
        entries,
    )))
}

pub(crate) fn record_file_entry_rolled_back(
    connection: &mut Connection,
    group_id: &str,
    expected_group_version: i64,
    sequence: u16,
) -> rusqlite::Result<Option<JournalEntryCheckpoint>> {
    validate_id(group_id).map_err(|_| rusqlite::Error::InvalidQuery)?;
    if expected_group_version < 1 || sequence > 255 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if transaction.execute(
        queries::file_operations::ROLLBACK_ENTRY,
        params![group_id, i64::from(sequence)],
    )? != 1
    {
        transaction.rollback()?;
        return Ok(None);
    }
    let remaining: i64 = transaction.query_row(
        queries::file_operations::COUNT_PENDING_ROLLBACK_ENTRIES,
        [group_id],
        |row| row.get(0),
    )?;
    let complete = remaining == 0;
    let state = if complete {
        "rolled_back"
    } else {
        "rollback_pending"
    };
    if transaction.execute(
        queries::file_operations::CHECKPOINT_ROLLBACK,
        params![state, state, group_id, expected_group_version],
    )? != 1
    {
        transaction.rollback()?;
        return Ok(None);
    }
    if complete {
        release_group_ownership(&transaction, group_id)?;
    }
    transaction.commit()?;
    Ok(Some(JournalEntryCheckpoint {
        version: expected_group_version + 1,
        phase_complete: complete,
    }))
}

fn release_group_ownership(connection: &Connection, group_id: &str) -> rusqlite::Result<()> {
    connection.execute(queries::file_operations::RELEASE_GROUP_CLAIMS, [group_id])?;
    connection.execute(
        queries::file_operations::RELEASE_GROUP_RESERVATION,
        [group_id],
    )?;
    Ok(())
}

pub(crate) fn load_next_generic_recovery_group(
    connection: &Connection,
    scope: JournalRecoveryScope,
) -> rusqlite::Result<Option<JournalRecoveryGroup>> {
    let query = match scope {
        JournalRecoveryScope::All => queries::file_operations::SELECT_NEXT_GENERIC_RECOVERY_GROUP,
        JournalRecoveryScope::StartupCritical => {
            queries::file_operations::SELECT_NEXT_STARTUP_CRITICAL_RECOVERY_GROUP
        }
    };
    connection
        .query_row(query, [], |row| {
            let state = match row.get::<_, String>(1)?.as_str() {
                "publishing" => JournalRecoveryState::Publishing,
                "files_committed" => JournalRecoveryState::FilesCommitted,
                "cleanup_pending" => JournalRecoveryState::CleanupPending,
                "rollback_pending" => JournalRecoveryState::RollbackPending,
                _ => return Err(rusqlite::Error::InvalidQuery),
            };
            Ok(JournalRecoveryGroup {
                group_id: row.get(0)?,
                state,
                version: row.get(2)?,
            })
        })
        .optional()
}

pub(crate) fn yield_file_operation_progress(
    connection: &mut Connection,
    group_id: &str,
    expected_group_version: i64,
) -> rusqlite::Result<JournalCheckpointOutcome> {
    validate_id(group_id).map_err(|_| rusqlite::Error::InvalidQuery)?;
    if expected_group_version < 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if transaction.execute(
        queries::file_operations::YIELD_RECOVERY_PROGRESS,
        params![group_id, expected_group_version],
    )? != 1
    {
        transaction.rollback()?;
        return Ok(JournalCheckpointOutcome::VersionConflict);
    }
    let version = transaction.query_row(
        queries::file_operations::SELECT_GROUP_VERSION,
        [group_id],
        |row| row.get(0),
    )?;
    transaction.commit()?;
    Ok(JournalCheckpointOutcome::Advanced { version })
}

pub(crate) fn record_file_operation_failure(
    connection: &mut Connection,
    group_id: &str,
    expected_group_version: i64,
    sequence: u16,
    stage: JournalFailureStage,
    error_kind: &str,
    error: &str,
) -> rusqlite::Result<JournalCheckpointOutcome> {
    validate_id(group_id).map_err(|_| rusqlite::Error::InvalidQuery)?;
    if expected_group_version < 1
        || sequence > 255
        || error_kind.is_empty()
        || error_kind.len() > 64
        || error.is_empty()
        || error.len() > MAX_JOURNAL_TEXT_BYTES
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let entry_query = match stage {
        JournalFailureStage::Publication => {
            queries::file_operations::RECORD_PUBLICATION_FAILURE_ENTRY
        }
        JournalFailureStage::Cleanup => queries::file_operations::RECORD_CLEANUP_FAILURE_ENTRY,
        JournalFailureStage::Rollback => queries::file_operations::RECORD_ROLLBACK_FAILURE_ENTRY,
    };
    if transaction.execute(
        entry_query,
        params![error_kind, error, group_id, i64::from(sequence)],
    )? == 0
    {
        transaction.rollback()?;
        return Ok(JournalCheckpointOutcome::VersionConflict);
    }
    let group_query = match stage {
        JournalFailureStage::Publication => {
            queries::file_operations::RECORD_PUBLICATION_FAILURE_GROUP
        }
        JournalFailureStage::Cleanup => queries::file_operations::RECORD_CLEANUP_FAILURE_GROUP,
        JournalFailureStage::Rollback => queries::file_operations::RECORD_ROLLBACK_FAILURE_GROUP,
    };
    if transaction.execute(
        group_query,
        params![error_kind, error, group_id, expected_group_version],
    )? == 0
    {
        transaction.rollback()?;
        return Ok(JournalCheckpointOutcome::VersionConflict);
    }
    transaction.commit()?;
    Ok(JournalCheckpointOutcome::Advanced {
        version: expected_group_version + 1,
    })
}

pub(crate) fn record_file_operation_finalization_failure(
    connection: &mut Connection,
    group_id: &str,
    expected_group_version: i64,
    error_kind: &str,
    error: &str,
) -> rusqlite::Result<JournalCheckpointOutcome> {
    validate_id(group_id).map_err(|_| rusqlite::Error::InvalidQuery)?;
    if expected_group_version < 1
        || error_kind.is_empty()
        || error_kind.len() > 64
        || error.is_empty()
        || error.len() > MAX_JOURNAL_TEXT_BYTES
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let changed = connection.execute(
        queries::file_operations::RECORD_FINALIZE_FAILURE,
        params![error_kind, error, group_id, expected_group_version],
    )?;
    if changed == 0 {
        return Ok(JournalCheckpointOutcome::VersionConflict);
    }
    Ok(JournalCheckpointOutcome::Advanced {
        version: expected_group_version + 1,
    })
}

pub(crate) fn retry_file_operation(
    connection: &mut Connection,
    retry_request_id: &str,
    group_id: &str,
    expected_version: i64,
    request_hash: [u8; 32],
) -> rusqlite::Result<JournalRetryOutcome> {
    validate_id(retry_request_id).map_err(|_| rusqlite::Error::InvalidQuery)?;
    validate_id(group_id).map_err(|_| rusqlite::Error::InvalidQuery)?;
    if expected_version < 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let prior = transaction
        .query_row(
            queries::file_operations::SELECT_RETRY_RECEIPT,
            [retry_request_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, bool>(5)?,
                ))
            },
        )
        .optional()?;
    if let Some((prior_group, prior_version, prior_hash, state, version, live)) = prior {
        transaction.rollback()?;
        if !live {
            return Ok(JournalRetryOutcome::VersionConflict);
        }
        if prior_group != group_id
            || prior_version != expected_version
            || prior_hash.as_slice() != request_hash
        {
            return Ok(JournalRetryOutcome::RequestConflict);
        }
        return Ok(JournalRetryOutcome::Accepted {
            state,
            version,
            replayed: true,
        });
    }
    let live_receipts: i64 = transaction.query_row(
        queries::file_operations::COUNT_LIVE_RETRY_RECEIPTS,
        [group_id],
        |row| row.get(0),
    )?;
    if live_receipts >= MAX_LIVE_RETRY_RECEIPTS_PER_OPERATION {
        transaction.rollback()?;
        return Ok(JournalRetryOutcome::ReceiptLimitReached);
    }
    let group = transaction
        .query_row(
            queries::file_operations::SELECT_FAILED_GROUP_FOR_RETRY,
            [group_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?;
    let Some((failed_state, current_version)) = group else {
        transaction.rollback()?;
        return Ok(JournalRetryOutcome::VersionConflict);
    };
    if current_version != expected_version {
        transaction.rollback()?;
        return Ok(JournalRetryOutcome::VersionConflict);
    }
    let next_state = match failed_state.as_str() {
        "publication_failed" => "publishing",
        "finalize_failed" => "files_committed",
        "cleanup_failed" => "cleanup_pending",
        _ => {
            transaction.rollback()?;
            return Ok(JournalRetryOutcome::VersionConflict);
        }
    };
    if transaction.execute(
        queries::file_operations::RETRY_FAILED_GROUP,
        params![next_state, group_id, expected_version, failed_state],
    )? == 0
    {
        transaction.rollback()?;
        return Ok(JournalRetryOutcome::VersionConflict);
    }
    match next_state {
        "publishing" => {
            transaction.execute(
                queries::file_operations::RESET_PUBLICATION_ENTRY_FAILURES,
                [group_id],
            )?;
        }
        "cleanup_pending" => {
            transaction.execute(
                queries::file_operations::RESET_CLEANUP_ENTRY_FAILURES,
                [group_id],
            )?;
        }
        _ => {}
    }
    let response_version = expected_version + 1;
    transaction.execute(
        queries::file_operations::INSERT_RETRY_RECEIPT,
        params![
            retry_request_id,
            group_id,
            expected_version,
            request_hash.as_slice(),
            next_state,
            response_version,
        ],
    )?;
    transaction.commit()?;
    Ok(JournalRetryOutcome::Accepted {
        state: next_state.to_string(),
        version: response_version,
        replayed: false,
    })
}

pub(crate) fn list_file_operations(
    connection: &Connection,
    states: Vec<String>,
    cursor: Option<String>,
    limit: u16,
) -> rusqlite::Result<FileOperationListResponse> {
    if states.is_empty() || limit == 0 || limit > FILE_OPERATION_LIST_LIMIT_MAX {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let states = serde_json::to_string(&states).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let query_limit = i64::from(limit) + 1;
    let mut statement = connection.prepare(queries::file_operations::LIST_GROUPS)?;
    let mut rows =
        statement.query(params![states, cursor, cursor, cursor, cursor, query_limit,])?;
    let mut operations = Vec::new();
    operations
        .try_reserve_exact(usize::from(limit) + 1)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    while let Some(row) = rows.next()? {
        operations.push(read_file_operation_summary(row)?);
    }
    let has_more = operations.len() > usize::from(limit);
    if has_more {
        operations.pop();
    }
    let next_cursor = has_more
        .then(|| {
            operations
                .last()
                .map(|operation| operation.operation_id.clone())
        })
        .flatten();
    Ok(FileOperationListResponse {
        operations,
        next_cursor,
    })
}

pub(crate) fn load_file_operation_detail(
    connection: &Connection,
    group_id: &str,
) -> rusqlite::Result<Option<FileOperationDetailResponse>> {
    validate_id(group_id).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let detail = connection
        .query_row(
            queries::file_operations::SELECT_GROUP_DETAIL,
            [group_id],
            |row| {
                Ok((
                    read_file_operation_summary(row)?,
                    row.get::<_, String>(18)?,
                    row.get::<_, Option<String>>(19)?,
                    row.get::<_, Option<String>>(20)?,
                    row.get::<_, Option<String>>(21)?,
                ))
            },
        )
        .optional()?;
    let Some((summary, detail_level, action_summary, state_summary, cleanup_summary)) = detail
    else {
        return Ok(None);
    };
    if detail_level == "compacted" {
        return Ok(Some(FileOperationDetailResponse {
            summary,
            detail_level,
            entries: None,
            path_claims: None,
            compacted: Some(FileOperationCompactedSummary {
                entry_actions: parse_count_summary(action_summary)?,
                entry_states: parse_count_summary(state_summary)?,
                cleanup_states: parse_count_summary(cleanup_summary)?,
            }),
        }));
    }
    if detail_level != "full" {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(Some(FileOperationDetailResponse {
        summary,
        detail_level,
        entries: Some(load_file_operation_entries(connection, group_id)?),
        path_claims: Some(load_file_operation_claims(connection, group_id)?),
        compacted: None,
    }))
}

pub(crate) fn maintain_file_operation_journal(
    connection: &mut Connection,
) -> rusqlite::Result<JournalMaintenanceOutcome> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let expired_ids = {
        let mut statement =
            transaction.prepare(queries::file_operations::SELECT_EXPIRED_RETRY_RECEIPTS)?;
        let mut rows = statement.query([])?;
        let mut ids = Vec::new();
        ids.try_reserve_exact(256)
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        while let Some(row) = rows.next()? {
            if ids.len() >= 256 {
                return Err(rusqlite::Error::InvalidQuery);
            }
            ids.push(row.get::<_, String>(0)?);
        }
        ids
    };
    let mut expired_retry_receipts = 0usize;
    for retry_id in expired_ids {
        expired_retry_receipts = expired_retry_receipts
            .checked_add(
                transaction.execute(queries::file_operations::DELETE_RETRY_RECEIPT, [retry_id])?,
            )
            .ok_or(rusqlite::Error::InvalidQuery)?;
    }

    let expired_result_ids = {
        let mut statement =
            transaction.prepare(queries::file_operations::SELECT_EXPIRED_LLM_RESULT_RECEIPTS)?;
        let mut rows = statement.query([])?;
        let mut ids = Vec::new();
        ids.try_reserve_exact(64)
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        while let Some(row) = rows.next()? {
            if ids.len() >= 64 {
                return Err(rusqlite::Error::InvalidQuery);
            }
            ids.push(row.get::<_, String>(0)?);
        }
        ids
    };
    let mut expired_result_receipts = 0usize;
    for job_id in expired_result_ids {
        expired_result_receipts = expired_result_receipts
            .checked_add(transaction.execute(
                queries::file_operations::DELETE_EXPIRED_LLM_RESULT_RECEIPT,
                [job_id],
            )?)
            .ok_or(rusqlite::Error::InvalidQuery)?;
    }

    let compaction_candidate = transaction
        .query_row(
            queries::file_operations::SELECT_COMPACTION_CANDIDATE,
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?;
    let compacted_groups = if let Some((group_id, state, version)) = compaction_candidate {
        let action_summary = count_entry_values(
            &transaction,
            queries::file_operations::COUNT_ENTRY_ACTIONS,
            &group_id,
        )?;
        let state_summary = count_entry_values(
            &transaction,
            queries::file_operations::COUNT_ENTRY_STATES,
            &group_id,
        )?;
        let cleanup_summary = count_entry_values(
            &transaction,
            queries::file_operations::COUNT_CLEANUP_STATES,
            &group_id,
        )?;
        let action_summary =
            serde_json::to_string(&action_summary).map_err(|_| rusqlite::Error::InvalidQuery)?;
        let state_summary =
            serde_json::to_string(&state_summary).map_err(|_| rusqlite::Error::InvalidQuery)?;
        let cleanup_summary =
            serde_json::to_string(&cleanup_summary).map_err(|_| rusqlite::Error::InvalidQuery)?;
        transaction.execute(queries::file_operations::DELETE_GROUP_CLAIMS, [&group_id])?;
        transaction.execute(queries::file_operations::DELETE_DIRECTORY_COPY, [&group_id])?;
        transaction.execute(queries::file_operations::DELETE_GROUP_ENTRIES, [&group_id])?;
        let updated = transaction.execute(
            queries::file_operations::COMPACT_GROUP,
            params![
                action_summary,
                state_summary,
                cleanup_summary,
                group_id,
                state,
                version,
            ],
        )?;
        if updated != 1 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        1
    } else {
        0
    };

    let prune_candidate = transaction
        .query_row(
            queries::file_operations::SELECT_PRUNE_CANDIDATE,
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let pruned_groups = if let Some(group_id) = prune_candidate {
        transaction.execute(queries::file_operations::PRUNE_GROUP, [group_id])?
    } else {
        0
    };
    transaction.commit()?;
    Ok(JournalMaintenanceOutcome {
        expired_retry_receipts,
        expired_result_receipts,
        compacted_groups,
        pruned_groups,
    })
}

fn count_entry_values(
    connection: &Connection,
    query: &str,
    group_id: &str,
) -> rusqlite::Result<BTreeMap<String, u16>> {
    let mut statement = connection.prepare(query)?;
    let mut rows = statement.query([group_id])?;
    let mut counts = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let value = row.get::<_, String>(0)?;
        let count =
            u16::try_from(row.get::<_, i64>(1)?).map_err(|_| rusqlite::Error::InvalidQuery)?;
        if counts.insert(value, count).is_some() {
            return Err(rusqlite::Error::InvalidQuery);
        }
    }
    Ok(counts)
}

fn parse_count_summary(value: Option<String>) -> rusqlite::Result<BTreeMap<String, u16>> {
    let value = value.ok_or(rusqlite::Error::InvalidQuery)?;
    serde_json::from_str(&value).map_err(|_| rusqlite::Error::InvalidQuery)
}

fn read_file_operation_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<FileOperationSummary> {
    let entry_count =
        u16::try_from(row.get::<_, i64>(13)?).map_err(|_| rusqlite::Error::InvalidQuery)?;
    Ok(FileOperationSummary {
        operation_id: row.get(0)?,
        kind: row.get(1)?,
        owner_kind: row.get(2)?,
        owner_id: row.get(3)?,
        state: row.get(4)?,
        product_target: row.get(5)?,
        product_version: row.get(6)?,
        cancel_requested: row.get(7)?,
        completion_outcome: row.get(8)?,
        finalization_error_kind: row.get(9)?,
        finalization_error: row.get(10)?,
        rollback_error_kind: row.get(11)?,
        rollback_error: row.get(12)?,
        entry_count,
        version: row.get(14)?,
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
        terminal_at: row.get(17)?,
    })
}

fn load_file_operation_entries(
    connection: &Connection,
    group_id: &str,
) -> rusqlite::Result<Vec<FileOperationEntryDetail>> {
    let mut statement = connection.prepare(queries::file_operations::SELECT_GROUP_ENTRIES)?;
    let mut rows = statement.query([group_id])?;
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(MAX_FILE_OPERATION_ENTRIES_PER_GROUP)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    while let Some(row) = rows.next()? {
        if entries.len() >= MAX_FILE_OPERATION_ENTRIES_PER_GROUP {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let expected_hash = row
            .get::<_, Option<Vec<u8>>>(8)?
            .map(|hash| encode_sha256(&hash))
            .transpose()?;
        entries.push(FileOperationEntryDetail {
            sequence: u16::try_from(row.get::<_, i64>(0)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            action: row.get(1)?,
            storage_root: row.get(2)?,
            source_path: row.get(3)?,
            temporary_path: row.get(4)?,
            destination_path: row.get(5)?,
            tombstone_path: row.get(6)?,
            expected_size: row
                .get::<_, Option<i64>>(7)?
                .map(u64::try_from)
                .transpose()
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            expected_sha256: expected_hash,
            expected_version: row.get(9)?,
            state: row.get(10)?,
            cleanup_state: row.get(11)?,
            last_error_kind: row.get(12)?,
            last_error: row.get(13)?,
        });
    }
    Ok(entries)
}

fn load_file_operation_claims(
    connection: &Connection,
    group_id: &str,
) -> rusqlite::Result<Vec<FileOperationPathClaimDetail>> {
    let mut statement = connection.prepare(queries::file_operations::SELECT_GROUP_CLAIMS)?;
    let mut rows = statement.query([group_id])?;
    let mut claims = Vec::new();
    claims
        .try_reserve_exact(MAX_FILE_OPERATION_CLAIMS_PER_GROUP)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    while let Some(row) = rows.next()? {
        if claims.len() >= MAX_FILE_OPERATION_CLAIMS_PER_GROUP {
            return Err(rusqlite::Error::InvalidQuery);
        }
        claims.push(FileOperationPathClaimDetail {
            sequence: u16::try_from(row.get::<_, i64>(0)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            storage_root: row.get(1)?,
            relative_path: row.get(2)?,
            mode: row.get(3)?,
            scope: row.get(4)?,
            role: row.get(5)?,
            expected_version: row.get(6)?,
        });
    }
    Ok(claims)
}

pub(crate) fn encode_sha256(bytes: &[u8]) -> rusqlite::Result<String> {
    if bytes.len() != 32 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::new();
    encoded
        .try_reserve_exact(64)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok(encoded)
}

impl FilePathClaimPlan {
    fn as_claim(&self) -> super::file::PathClaim {
        super::file::PathClaim {
            storage_root: self.storage_root,
            path: self.path.clone(),
            mode: self.mode,
            scope: self.scope,
        }
    }
}

fn load_authorized_entries(
    connection: &Connection,
    query: &str,
    group_id: &str,
) -> rusqlite::Result<Vec<AuthorizedJournalEntry>> {
    let mut statement = connection.prepare(query)?;
    let mut rows = statement.query([group_id])?;
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(MAX_FILE_OPERATION_ENTRIES_PER_GROUP)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    while let Some(row) = rows.next()? {
        if entries.len() >= MAX_FILE_OPERATION_ENTRIES_PER_GROUP {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let sequence =
            u16::try_from(row.get::<_, i64>(0)?).map_err(|_| rusqlite::Error::InvalidQuery)?;
        let action_text = row.get::<_, String>(1)?;
        let action = FileEntryAction::try_from(action_text.as_str())
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        let root_text = row.get::<_, String>(2)?;
        let storage_root = StorageRootId::try_from(root_text.as_str())
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        let expected_size = row
            .get::<_, Option<i64>>(7)?
            .map(u64::try_from)
            .transpose()
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        let expected_sha256 = row
            .get::<_, Option<Vec<u8>>>(8)?
            .map(|value| value.try_into().map_err(|_| rusqlite::Error::InvalidQuery))
            .transpose()?;
        entries.push(AuthorizedJournalEntry {
            sequence,
            action,
            storage_root,
            source_path: parse_optional_path(row.get(3)?)?,
            temporary_path: parse_optional_path(row.get(4)?)?,
            destination_path: parse_optional_path(row.get(5)?)?,
            tombstone_path: parse_optional_path(row.get(6)?)?,
            expected_size,
            expected_sha256,
            expected_version: row.get(9)?,
        });
    }
    if entries.is_empty() {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(entries)
}

fn parse_optional_path(value: Option<String>) -> rusqlite::Result<Option<NormalizedStoragePath>> {
    value
        .map(|path| NormalizedStoragePath::parse(&path).map_err(|_| rusqlite::Error::InvalidQuery))
        .transpose()
}

fn entry_paths_match_action(entry: &FileEntryPlan) -> bool {
    let source = entry.source_path.is_some();
    let temporary = entry.temporary_path.is_some();
    let destination = entry.destination_path.is_some();
    let tombstone = entry.tombstone_path.is_some();
    match entry.action {
        FileEntryAction::Publish => !source && temporary && destination && !tombstone,
        FileEntryAction::Move => source && !temporary && destination && !tombstone,
        FileEntryAction::Tombstone => source && !temporary && !destination && tombstone,
        FileEntryAction::Cleanup => source && !temporary && !destination && !tombstone,
    }
}

fn entry_mutation_paths(entry: &FileEntryPlan) -> impl Iterator<Item = &NormalizedStoragePath> {
    [
        entry.source_path.as_ref(),
        entry.temporary_path.as_ref(),
        entry.destination_path.as_ref(),
        entry.tombstone_path.as_ref(),
    ]
    .into_iter()
    .flatten()
}

fn validate_id(value: &str) -> Result<(), JournalPlanError> {
    if value.is_empty() || value.len() > MAX_JOURNAL_ID_BYTES {
        return Err(JournalPlanError::InvalidPlan);
    }
    validate_text(value)
}

fn validate_text(value: &str) -> Result<(), JournalPlanError> {
    if value.is_empty() || value.len() > MAX_JOURNAL_TEXT_BYTES || value.as_bytes().contains(&0) {
        return Err(JournalPlanError::InvalidPlan);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalPlanError {
    InvalidPlan,
    InvalidEntry,
    ConflictingClaims,
    UnclaimedMutationPath,
    InvalidReservation,
}
