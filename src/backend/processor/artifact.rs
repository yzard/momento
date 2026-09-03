use crate::io::file::{NormalizedStoragePath, PathClaimMode, PathClaimScope, StorageRootId};
use crate::io::journal::{
    FileEntryAction, FileEntryPlan, FileOperationPlan, FilePathClaimPlan, JournalCheckpointOutcome,
    JournalSpaceReservationPlan, PrepareJournalOutcome,
};
use crate::runtime::ExecutorHandles;

pub(crate) struct PreparedArtifactPublication {
    group_id: String,
    storage_root: StorageRootId,
    temporary_path: NormalizedStoragePath,
    publication_entry_count: u16,
    has_cleanup: bool,
}

pub(crate) struct PreparedArtifactBatch {
    group_id: String,
    storage_root: StorageRootId,
    temporary_paths: Vec<NormalizedStoragePath>,
}

pub(crate) struct PreparedMetadataArtifactBatch {
    group_id: String,
    targets: Vec<PreparedMetadataArtifactTarget>,
}

#[derive(Clone)]
pub(crate) struct PreparedMetadataArtifactTarget {
    storage_root: StorageRootId,
    temporary_path: NormalizedStoragePath,
}

#[derive(Debug)]
pub(crate) struct CommittedMetadataArtifactGroup {
    pub group_id: String,
    pub version: i64,
    pub product_version: i64,
}

impl PreparedMetadataArtifactTarget {
    pub(crate) fn temporary_file(&self) -> crate::processor::thumbnails::StorageMediaFile {
        crate::processor::thumbnails::StorageMediaFile {
            storage_root: self.storage_root,
            path: self.temporary_path.clone(),
        }
    }
}

impl PreparedMetadataArtifactBatch {
    pub(crate) fn target(&self, index: usize) -> Option<&PreparedMetadataArtifactTarget> {
        self.targets.get(index)
    }

    pub(crate) async fn publish(
        &self,
        executors: &ExecutorHandles,
        product_version: i64,
    ) -> Result<CommittedMetadataArtifactGroup, String> {
        let ticket = executors
            .file_io
            .reserve_journal_mutation(&self.group_id, 2)
            .map_err(|error| error.to_string())?;
        let grant = executors
            .sqlite
            .begin_file_operation_publication_durable(&ticket, 1)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "metadata artifact publication changed before it began".to_string())?;
        let mut lease =
            crate::io::recovery::acquire_verified_journal_mutation(executors, ticket, grant)
                .await
                .map_err(|error| error.to_string())?;
        let mut version = 2_i64;
        for sequence in 0..self.targets.len() {
            executors
                .file_io
                .apply_next_journal_entry_durable(&mut lease)
                .await
                .map_err(|error| error.to_string())?;
            let checkpoint = executors
                .sqlite
                .record_file_entry_published_durable(
                    self.group_id.clone(),
                    version,
                    u16::try_from(sequence)
                        .map_err(|_| "metadata artifact sequence overflowed".to_string())?,
                )
                .await
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "metadata artifact publication checkpoint changed".to_string())?;
            version = checkpoint.version;
            if sequence + 1 == self.targets.len() && !checkpoint.phase_complete {
                return Err(
                    "metadata artifact publication did not reach files_committed".to_string(),
                );
            }
        }
        drop(lease);
        Ok(CommittedMetadataArtifactGroup {
            group_id: self.group_id.clone(),
            version,
            product_version,
        })
    }

    pub(crate) async fn cancel(&self, executors: &ExecutorHandles) {
        let Ok(Some(status)) = executors
            .sqlite
            .load_file_operation_cancellation_status_durable(self.group_id.clone())
            .await
        else {
            return;
        };
        let _ = crate::io::recovery::cancel_generic_file_operation(
            executors,
            self.group_id.clone(),
            status.version,
        )
        .await;
    }
}

pub(crate) async fn prepare_metadata_artifact_batch(
    executors: &ExecutorHandles,
    destinations: Vec<(StorageRootId, NormalizedStoragePath)>,
    maximum_new_bytes: u64,
    media_id: i64,
    claim_token: &str,
    product_version: i64,
) -> Result<PreparedMetadataArtifactBatch, String> {
    if destinations.is_empty()
        || destinations.len() > crate::io::journal::MAX_FILE_OPERATION_ENTRIES_PER_GROUP
        || maximum_new_bytes == 0
        || media_id <= 0
        || uuid::Uuid::parse_str(claim_token).is_err()
        || product_version <= 0
    {
        return Err("metadata artifact batch is outside its bounded contract".to_string());
    }
    let operation_id = uuid::Uuid::new_v4();
    let group_id = format!("metadata-artifacts-{operation_id}");
    let reservation = executors
        .file_io
        .reserve_journal_space(group_id.clone(), maximum_new_bytes)
        .map_err(|error| error.to_string())?
        .into_result()
        .map_err(|error| error.to_string())?;
    let mut targets = Vec::with_capacity(destinations.len());
    let mut entries = Vec::with_capacity(destinations.len());
    let mut claims = Vec::with_capacity(destinations.len() * 2);
    for (index, (storage_root, destination_path)) in destinations.into_iter().enumerate() {
        let destination = std::path::Path::new(destination_path.relative_path());
        let parent = destination
            .parent()
            .unwrap_or_else(|| std::path::Path::new(""));
        let temporary_path = NormalizedStoragePath::parse(
            &parent
                .join(format!(".momento-metadata-{operation_id}-{index}.tmp"))
                .to_string_lossy(),
        )
        .map_err(|error| error.to_string())?;
        entries.push(FileEntryPlan {
            action: FileEntryAction::Publish,
            storage_root,
            source_path: None,
            temporary_path: Some(temporary_path.clone()),
            destination_path: Some(destination_path.clone()),
            tombstone_path: None,
            expected_size: None,
            expected_sha256: None,
            expected_version: None,
        });
        claims.push(write_claim(
            storage_root,
            temporary_path.clone(),
            "metadata_artifact_temporary",
        ));
        claims.push(write_claim(
            storage_root,
            destination_path.clone(),
            "metadata_artifact_destination",
        ));
        targets.push(PreparedMetadataArtifactTarget {
            storage_root,
            temporary_path,
        });
    }
    let plan = FileOperationPlan {
        group_id: group_id.clone(),
        kind: "metadata_artifacts".to_string(),
        owner_kind: "metadata_generation".to_string(),
        owner_id: media_id.to_string(),
        claim_token: Some(claim_token.to_string()),
        product_target: Some("metadata_artifacts".to_string()),
        product_version: Some(product_version),
        entries,
        claims,
        space_reservation: Some(
            JournalSpaceReservationPlan::new(reservation).map_err(|error| error.to_string())?,
        ),
    };
    if executors
        .sqlite
        .prepare_file_operation_durable(plan)
        .await
        .map_err(|error| error.to_string())?
        != PrepareJournalOutcome::Prepared
    {
        return Err("metadata artifact paths are owned by another operation".to_string());
    }
    Ok(PreparedMetadataArtifactBatch { group_id, targets })
}

#[derive(Debug)]
pub(crate) struct CommittedResultArtifactGroup {
    pub group_id: String,
    pub version: i64,
    pub product_version: i64,
}

impl CommittedResultArtifactGroup {
    pub(crate) async fn discard(&self, executors: &ExecutorHandles) {
        let _ = crate::io::recovery::cancel_generic_file_operation(
            executors,
            self.group_id.clone(),
            self.version,
        )
        .await;
    }
}

#[derive(Clone, Copy)]
pub enum ArtifactPublicationOwner<'a> {
    JournalGroup,
    MetadataClaim(&'a str),
}

impl PreparedArtifactBatch {
    pub(crate) fn temporary_path(&self, index: usize) -> Option<&NormalizedStoragePath> {
        self.temporary_paths.get(index)
    }

    pub(crate) fn storage_root(&self) -> StorageRootId {
        self.storage_root
    }

    pub(crate) async fn publish_result(
        &self,
        executors: &ExecutorHandles,
        product_version: i64,
    ) -> Result<CommittedResultArtifactGroup, String> {
        let ticket = executors
            .file_io
            .reserve_journal_mutation(&self.group_id, 2)
            .map_err(|error| error.to_string())?;
        let grant = executors
            .sqlite
            .begin_file_operation_publication_durable(&ticket, 1)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "result artifact publication changed before it began".to_string())?;
        let mut lease =
            crate::io::recovery::acquire_verified_journal_mutation(executors, ticket, grant)
                .await
                .map_err(|error| error.to_string())?;
        let mut version = 2_i64;
        for sequence in 0..self.temporary_paths.len() {
            executors
                .file_io
                .apply_next_journal_entry_durable(&mut lease)
                .await
                .map_err(|error| error.to_string())?;
            let checkpoint = executors
                .sqlite
                .record_file_entry_published_durable(
                    self.group_id.clone(),
                    version,
                    u16::try_from(sequence)
                        .map_err(|_| "result artifact sequence overflowed".to_string())?,
                )
                .await
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "result artifact publication checkpoint changed".to_string())?;
            version = checkpoint.version;
            if sequence + 1 == self.temporary_paths.len() && !checkpoint.phase_complete {
                return Err("result artifact publication did not reach files_committed".to_string());
            }
        }
        drop(lease);
        Ok(CommittedResultArtifactGroup {
            group_id: self.group_id.clone(),
            version,
            product_version,
        })
    }

    pub(crate) async fn cancel(&self, executors: &ExecutorHandles) {
        let Ok(Some(status)) = executors
            .sqlite
            .load_file_operation_cancellation_status_durable(self.group_id.clone())
            .await
        else {
            return;
        };
        let _ = crate::io::recovery::cancel_generic_file_operation(
            executors,
            self.group_id.clone(),
            status.version,
        )
        .await;
    }
}

pub(crate) async fn prepare_result_artifact_batch(
    executors: &ExecutorHandles,
    storage_root: StorageRootId,
    destination_paths: Vec<NormalizedStoragePath>,
    maximum_new_bytes_per_artifact: u64,
    job_id: &str,
    claim_token: &str,
    product_version: i64,
) -> Result<PreparedArtifactBatch, String> {
    if destination_paths.is_empty()
        || destination_paths.len() > crate::io::journal::MAX_FILE_OPERATION_ENTRIES_PER_GROUP
        || maximum_new_bytes_per_artifact == 0
        || uuid::Uuid::parse_str(claim_token).is_err()
        || product_version <= 0
    {
        return Err("result artifact batch is outside its bounded contract".to_string());
    }
    let operation_id = uuid::Uuid::new_v4();
    let group_id = format!("result-artifacts-{operation_id}");
    let maximum_new_bytes = maximum_new_bytes_per_artifact
        .checked_mul(
            u64::try_from(destination_paths.len())
                .map_err(|_| "result artifact count overflowed".to_string())?,
        )
        .ok_or_else(|| "result artifact reservation overflowed".to_string())?;
    let reservation = executors
        .file_io
        .reserve_journal_space(group_id.clone(), maximum_new_bytes)
        .map_err(|error| error.to_string())?
        .into_result()
        .map_err(|error| error.to_string())?;
    let mut temporary_paths = Vec::with_capacity(destination_paths.len());
    let mut entries = Vec::with_capacity(destination_paths.len());
    let mut claims = Vec::with_capacity(destination_paths.len() * 2);
    for (index, destination_path) in destination_paths.into_iter().enumerate() {
        let destination = std::path::Path::new(destination_path.relative_path());
        let parent = destination
            .parent()
            .unwrap_or_else(|| std::path::Path::new(""));
        let temporary_path = NormalizedStoragePath::parse(
            &parent
                .join(format!(".momento-result-{operation_id}-{index}.tmp"))
                .to_string_lossy(),
        )
        .map_err(|error| error.to_string())?;
        entries.push(FileEntryPlan {
            action: FileEntryAction::Publish,
            storage_root,
            source_path: None,
            temporary_path: Some(temporary_path.clone()),
            destination_path: Some(destination_path.clone()),
            tombstone_path: None,
            expected_size: None,
            expected_sha256: None,
            expected_version: None,
        });
        claims.push(write_claim(
            storage_root,
            temporary_path.clone(),
            "result_artifact_temporary",
        ));
        claims.push(write_claim(
            storage_root,
            destination_path,
            "result_artifact_destination",
        ));
        temporary_paths.push(temporary_path);
    }
    let plan = FileOperationPlan {
        group_id: group_id.clone(),
        kind: "llm_result_artifacts".to_string(),
        owner_kind: "llm_result".to_string(),
        owner_id: job_id.to_string(),
        claim_token: Some(claim_token.to_string()),
        product_target: Some("llm_result_face_crops".to_string()),
        product_version: Some(product_version),
        entries,
        claims,
        space_reservation: Some(
            JournalSpaceReservationPlan::new(reservation).map_err(|error| error.to_string())?,
        ),
    };
    if executors
        .sqlite
        .prepare_file_operation_durable(plan)
        .await
        .map_err(|error| error.to_string())?
        != PrepareJournalOutcome::Prepared
    {
        return Err("result artifact paths are owned by another operation".to_string());
    }
    Ok(PreparedArtifactBatch {
        group_id,
        storage_root,
        temporary_paths,
    })
}

impl PreparedArtifactPublication {
    pub(crate) fn storage_root(&self) -> StorageRootId {
        self.storage_root
    }

    pub(crate) fn temporary_path(&self) -> &NormalizedStoragePath {
        &self.temporary_path
    }

    pub(crate) async fn publish(self, executors: &ExecutorHandles) -> Result<(), String> {
        let ticket = executors
            .file_io
            .reserve_journal_mutation(&self.group_id, 2)
            .map_err(|error| error.to_string())?;
        let grant = executors
            .sqlite
            .begin_file_operation_publication_durable(&ticket, 1)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "artifact publication changed before it began".to_string())?;
        let mut lease =
            crate::io::recovery::acquire_verified_journal_mutation(executors, ticket, grant)
                .await
                .map_err(|error| error.to_string())?;
        let mut version = 2_i64;
        for sequence in 0..self.publication_entry_count {
            executors
                .file_io
                .apply_next_journal_entry_durable(&mut lease)
                .await
                .map_err(|error| error.to_string())?;
            let checkpoint = executors
                .sqlite
                .record_file_entry_published_durable(self.group_id.clone(), version, sequence)
                .await
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "artifact publication checkpoint changed".to_string())?;
            version = checkpoint.version;
            if sequence + 1 == self.publication_entry_count && !checkpoint.phase_complete {
                return Err("artifact publication phase did not complete".to_string());
            }
        }
        drop(lease);
        let completion = executors
            .sqlite
            .complete_no_product_file_operation_durable(self.group_id.clone(), version)
            .await
            .map_err(|error| error.to_string())?;
        if completion
            != (JournalCheckpointOutcome::Advanced {
                version: version + 1,
            })
        {
            return Err("artifact publication changed before completion".to_string());
        }
        if self.has_cleanup {
            executors.scheduler.wake_journal_recovery();
        }
        Ok(())
    }

    pub(crate) async fn cancel(self, executors: &ExecutorHandles) {
        let Ok(Some(status)) = executors
            .sqlite
            .load_file_operation_cancellation_status_durable(self.group_id.clone())
            .await
        else {
            return;
        };
        let _ = crate::io::recovery::cancel_generic_file_operation(
            executors,
            self.group_id,
            status.version,
        )
        .await;
    }
}

pub(crate) async fn prepare_artifact_publication(
    executors: &ExecutorHandles,
    storage_root: StorageRootId,
    destination_path: NormalizedStoragePath,
    maximum_new_bytes: u64,
    kind: &str,
    owner: ArtifactPublicationOwner<'_>,
) -> Result<PreparedArtifactPublication, String> {
    if maximum_new_bytes == 0 {
        return Err("artifact maximum byte size must be positive".to_string());
    }
    let operation_id = uuid::Uuid::new_v4();
    let group_id = format!("artifact-{operation_id}");
    let claim_token = match owner {
        ArtifactPublicationOwner::JournalGroup => None,
        ArtifactPublicationOwner::MetadataClaim(claim_token) => {
            if uuid::Uuid::parse_str(claim_token).is_err() {
                return Err("metadata artifact claim token is invalid".to_string());
            }
            Some(claim_token.to_string())
        }
    };
    let destination = std::path::Path::new(destination_path.relative_path());
    let parent = destination
        .parent()
        .unwrap_or_else(|| std::path::Path::new(""));
    let temporary_path = NormalizedStoragePath::parse(
        &parent
            .join(format!(".momento-artifact-{operation_id}.tmp"))
            .to_string_lossy(),
    )
    .map_err(|error| error.to_string())?;
    let tombstone_path = NormalizedStoragePath::parse(
        &parent
            .join(format!(".momento-artifact-{operation_id}.replaced"))
            .to_string_lossy(),
    )
    .map_err(|error| error.to_string())?;
    let existing = match executors
        .file_io
        .open_storage_read_session_durable(storage_root, destination_path.clone())
        .await
    {
        Ok((session, snapshot)) => {
            executors
                .file_io
                .close_storage_session_durable(session)
                .await
                .map_err(|error| error.to_string())?;
            Some(snapshot)
        }
        Err(error) if error.kind == crate::executor::ExecutorErrorKind::FileNotFound => None,
        Err(error) => return Err(error.to_string()),
    };
    let reservation = executors
        .file_io
        .reserve_journal_space(group_id.clone(), maximum_new_bytes)
        .map_err(|error| error.to_string())?
        .into_result()
        .map_err(|error| error.to_string())?;
    let mut entries = Vec::with_capacity(if existing.is_some() { 3 } else { 1 });
    if let Some(snapshot) = existing {
        entries.push(FileEntryPlan {
            action: FileEntryAction::Tombstone,
            storage_root,
            source_path: Some(destination_path.clone()),
            temporary_path: None,
            destination_path: None,
            tombstone_path: Some(tombstone_path.clone()),
            expected_size: Some(snapshot.byte_size),
            expected_sha256: None,
            expected_version: Some(snapshot.identity_version()),
        });
    }
    entries.push(FileEntryPlan {
        action: FileEntryAction::Publish,
        storage_root,
        source_path: None,
        temporary_path: Some(temporary_path.clone()),
        destination_path: Some(destination_path.clone()),
        tombstone_path: None,
        expected_size: None,
        expected_sha256: None,
        expected_version: None,
    });
    if existing.is_some() {
        entries.push(FileEntryPlan {
            action: FileEntryAction::Cleanup,
            storage_root,
            source_path: Some(tombstone_path.clone()),
            temporary_path: None,
            destination_path: None,
            tombstone_path: None,
            expected_size: None,
            expected_sha256: None,
            expected_version: None,
        });
    }
    let mut claims = vec![
        write_claim(storage_root, temporary_path.clone(), "artifact_temporary"),
        write_claim(storage_root, destination_path, "artifact_destination"),
    ];
    if existing.is_some() {
        claims.push(write_claim(
            storage_root,
            tombstone_path,
            "artifact_replaced",
        ));
    }
    let publication_entry_count = if existing.is_some() { 2 } else { 1 };
    let plan = FileOperationPlan {
        group_id: group_id.clone(),
        kind: kind.to_string(),
        owner_kind: "generated_artifact".to_string(),
        owner_id: group_id.clone(),
        claim_token,
        product_target: None,
        product_version: None,
        entries,
        claims,
        space_reservation: Some(
            JournalSpaceReservationPlan::new(reservation).map_err(|error| error.to_string())?,
        ),
    };
    if executors
        .sqlite
        .prepare_file_operation_durable(plan)
        .await
        .map_err(|error| error.to_string())?
        == PrepareJournalOutcome::PathConflict
    {
        return Err("artifact paths are owned by another operation".to_string());
    }
    Ok(PreparedArtifactPublication {
        group_id,
        storage_root,
        temporary_path,
        publication_entry_count,
        has_cleanup: existing.is_some(),
    })
}

fn write_claim(
    storage_root: StorageRootId,
    path: NormalizedStoragePath,
    role: &str,
) -> FilePathClaimPlan {
    FilePathClaimPlan {
        storage_root,
        path,
        mode: PathClaimMode::Write,
        scope: PathClaimScope::Exact,
        role: role.to_string(),
        expected_version: None,
    }
}

pub(crate) async fn retire_artifact(
    executors: &ExecutorHandles,
    storage_root: StorageRootId,
    source_path: NormalizedStoragePath,
) -> Result<(), String> {
    let (session, snapshot) = executors
        .file_io
        .open_storage_read_session_durable(storage_root, source_path.clone())
        .await
        .map_err(|error| error.to_string())?;
    executors
        .file_io
        .close_storage_session_durable(session)
        .await
        .map_err(|error| error.to_string())?;
    let operation_id = uuid::Uuid::new_v4();
    let group_id = format!("artifact-retire-{operation_id}");
    let source = std::path::Path::new(source_path.relative_path());
    let parent = source.parent().unwrap_or_else(|| std::path::Path::new(""));
    let tombstone_path = NormalizedStoragePath::parse(
        &parent
            .join(format!(".momento-retired-{operation_id}"))
            .to_string_lossy(),
    )
    .map_err(|error| error.to_string())?;
    let plan = FileOperationPlan {
        group_id: group_id.clone(),
        kind: "generated_artifact_retirement".to_string(),
        owner_kind: "generated_artifact".to_string(),
        owner_id: group_id.clone(),
        claim_token: None,
        product_target: None,
        product_version: None,
        entries: vec![
            FileEntryPlan {
                action: FileEntryAction::Tombstone,
                storage_root,
                source_path: Some(source_path.clone()),
                temporary_path: None,
                destination_path: None,
                tombstone_path: Some(tombstone_path.clone()),
                expected_size: Some(snapshot.byte_size),
                expected_sha256: None,
                expected_version: Some(snapshot.identity_version()),
            },
            FileEntryPlan {
                action: FileEntryAction::Cleanup,
                storage_root,
                source_path: Some(tombstone_path.clone()),
                temporary_path: None,
                destination_path: None,
                tombstone_path: None,
                expected_size: Some(snapshot.byte_size),
                expected_sha256: None,
                expected_version: None,
            },
        ],
        claims: vec![
            write_claim(storage_root, source_path, "artifact_retire_source"),
            write_claim(storage_root, tombstone_path, "artifact_retire_tombstone"),
        ],
        space_reservation: None,
    };
    if executors
        .sqlite
        .prepare_file_operation_durable(plan)
        .await
        .map_err(|error| error.to_string())?
        == PrepareJournalOutcome::PathConflict
    {
        return Err("artifact retirement paths are owned by another operation".to_string());
    }
    let ticket = executors
        .file_io
        .reserve_journal_mutation(&group_id, 2)
        .map_err(|error| error.to_string())?;
    let grant = executors
        .sqlite
        .begin_file_operation_publication_durable(&ticket, 1)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "artifact retirement changed before it began".to_string())?;
    let mut lease =
        crate::io::recovery::acquire_verified_journal_mutation(executors, ticket, grant)
            .await
            .map_err(|error| error.to_string())?;
    executors
        .file_io
        .apply_next_journal_entry_durable(&mut lease)
        .await
        .map_err(|error| error.to_string())?;
    let checkpoint = executors
        .sqlite
        .record_file_entry_published_durable(group_id.clone(), 2, 0)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "artifact retirement checkpoint changed".to_string())?;
    drop(lease);
    if !checkpoint.phase_complete {
        return Err("artifact retirement publication did not complete".to_string());
    }
    if executors
        .sqlite
        .complete_no_product_file_operation_durable(group_id, checkpoint.version)
        .await
        .map_err(|error| error.to_string())?
        != (JournalCheckpointOutcome::Advanced {
            version: checkpoint.version + 1,
        })
    {
        return Err("artifact retirement changed before completion".to_string());
    }
    executors.scheduler.wake_journal_recovery();
    Ok(())
}
