use crate::executor::{
    ExecutorError, ExecutorErrorKind, FileIoExecutorHandle, SqliteExecutorHandle,
};
use crate::runtime::{ExecutorHandles, SchedulerHandle};

use super::file::{JournalMutationLease, JournalMutationTicket};
use super::journal::{
    FileEntryAction, JournalCancellationOutcome, JournalCheckpointOutcome, JournalFailureStage,
    JournalMutationGrant, JournalMutationStage, JournalRecoveryScope, JournalRecoveryState,
};

pub(crate) async fn acquire_verified_journal_mutation(
    executors: &ExecutorHandles,
    ticket: JournalMutationTicket,
    mut grant: JournalMutationGrant,
) -> Result<JournalMutationLease, ExecutorError> {
    let stage = grant.stage();
    for entry in grant.entries_mut() {
        let Some(expected_sha256) = entry.expected_sha256 else {
            continue;
        };
        let (path, fallback_path) = match (stage, entry.action) {
            (JournalMutationStage::Publication, FileEntryAction::Publish) => {
                (entry.temporary_path.clone(), entry.destination_path.clone())
            }
            (JournalMutationStage::Rollback, FileEntryAction::Publish) => {
                (entry.temporary_path.clone(), None)
            }
            (JournalMutationStage::Publication, FileEntryAction::Move) => {
                (entry.source_path.clone(), entry.destination_path.clone())
            }
            (JournalMutationStage::Publication, FileEntryAction::Tombstone) => {
                (entry.source_path.clone(), entry.tombstone_path.clone())
            }
            (JournalMutationStage::Publication, FileEntryAction::Cleanup) => (None, None),
            (JournalMutationStage::Cleanup, _) => (entry.source_path.clone(), None),
            (JournalMutationStage::Rollback, _) => (None, None),
        };
        let path = path.ok_or_else(|| {
            ExecutorError::new(
                ExecutorErrorKind::FileInvalidData,
                "verify_journal_entry_evidence",
                "journal entry with a content hash has no verifiable source path",
            )
        })?;
        let opened = executors
            .file_io
            .open_storage_read_session_durable(entry.storage_root, path)
            .await;
        let (mut session, snapshot) = match opened {
            Ok(opened) => opened,
            Err(error) if error.kind == ExecutorErrorKind::FileNotFound => {
                let Some(fallback_path) = fallback_path else {
                    if stage != JournalMutationStage::Publication {
                        continue;
                    }
                    return Err(error);
                };
                executors
                    .file_io
                    .open_storage_read_session_durable(entry.storage_root, fallback_path)
                    .await?
            }
            Err(error) => return Err(error),
        };
        let observed_version = snapshot.identity_version();
        if entry
            .expected_version
            .as_ref()
            .is_some_and(|expected| expected != &observed_version)
        {
            executors
                .file_io
                .close_storage_session_durable(session)
                .await?;
            return Err(ExecutorError::new(
                ExecutorErrorKind::FileInvalidData,
                "verify_journal_entry_evidence",
                "journal entry identity changed before hash verification",
            ));
        }
        let mut hasher = executors.cpu.start_sha256_session_durable().await?;
        loop {
            let (returned, bytes) = executors
                .file_io
                .read_storage_session_durable(session, crate::runtime::FILE_IO_CHUNK_BYTES as usize)
                .await?;
            session = returned;
            if bytes.is_empty() {
                break;
            }
            let (returned_hasher, _) = executors
                .cpu
                .update_sha256_session_durable(hasher, bytes)
                .await?;
            hasher = returned_hasher;
        }
        executors
            .file_io
            .close_storage_session_durable(session)
            .await?;
        let observed_sha256 = executors.cpu.finish_sha256_session_durable(hasher).await?;
        let expected_sha256 = super::journal::encode_sha256(&expected_sha256).map_err(|error| {
            ExecutorError::new(
                ExecutorErrorKind::Internal,
                "verify_journal_entry_evidence",
                error.to_string(),
            )
        })?;
        if observed_sha256 != expected_sha256 {
            return Err(ExecutorError::new(
                ExecutorErrorKind::FileInvalidData,
                "verify_journal_entry_evidence",
                "journal entry content hash changed before mutation",
            ));
        }
        entry.expected_version = Some(observed_version);
    }
    ticket.acquire(grant).map_err(|error| {
        ExecutorError::new(
            ExecutorErrorKind::Conflict,
            "acquire_verified_journal_mutation",
            error.to_string(),
        )
    })
}

pub async fn cancel_generic_file_operation(
    executors: &ExecutorHandles,
    group_id: String,
    expected_version: i64,
) -> Result<JournalCancellationOutcome, ExecutorError> {
    cancel_generic_file_operation_with_components(
        &executors.sqlite,
        &executors.file_io,
        &executors.scheduler,
        group_id,
        expected_version,
    )
    .await
}

pub(crate) async fn cancel_generic_file_operation_with_components(
    sqlite: &SqliteExecutorHandle,
    file_io: &FileIoExecutorHandle,
    scheduler: &SchedulerHandle,
    group_id: String,
    expected_version: i64,
) -> Result<JournalCancellationOutcome, ExecutorError> {
    let Some(status) = sqlite
        .load_file_operation_cancellation_status_durable(group_id.clone())
        .await?
    else {
        return Ok(JournalCancellationOutcome::VersionConflict);
    };
    if status.cancel_requested {
        return Ok(JournalCancellationOutcome::AlreadyRequested {
            state: status.state,
            version: status.version,
        });
    }
    if status.version != expected_version {
        return Ok(JournalCancellationOutcome::VersionConflict);
    }
    if !matches!(
        status.state.as_str(),
        "prepared" | "publishing" | "publication_failed" | "files_committed" | "finalize_failed"
    ) {
        return Ok(JournalCancellationOutcome::NotCancellable);
    }
    file_io
        .fence_journal_mutations(&group_id, expected_version)
        .await
        .map_err(|error| {
            ExecutorError::new(
                ExecutorErrorKind::Internal,
                "cancel_generic_file_operation",
                error.to_string(),
            )
        })?;
    let reconciliation_group_id = group_id.clone();
    let outcome = sqlite
        .request_file_operation_cancellation_durable(group_id, expected_version)
        .await?;
    match outcome {
        JournalCancellationOutcome::Requested { version, .. } => {
            file_io
                .release_journal_mutation_fence(&reconciliation_group_id, version)
                .map_err(mutation_registry_error)?;
            scheduler.wake_journal_recovery();
            Ok(outcome)
        }
        JournalCancellationOutcome::VersionConflict => {
            let status = sqlite
                .load_file_operation_cancellation_status_durable(reconciliation_group_id.clone())
                .await?;
            if let Some(status) = &status {
                file_io
                    .release_journal_mutation_fence(&reconciliation_group_id, status.version)
                    .map_err(mutation_registry_error)?;
            }
            Ok(status
                .filter(|status| status.cancel_requested)
                .map(|status| JournalCancellationOutcome::AlreadyRequested {
                    state: status.state,
                    version: status.version,
                })
                .unwrap_or(JournalCancellationOutcome::VersionConflict))
        }
        _ => Ok(outcome),
    }
}

pub async fn recover_generic_file_operations(
    executors: &ExecutorHandles,
) -> Result<usize, ExecutorError> {
    recover_file_operations(executors, JournalRecoveryScope::All).await
}

pub async fn recover_startup_critical_file_operations(
    executors: &ExecutorHandles,
) -> Result<usize, ExecutorError> {
    recover_file_operations(executors, JournalRecoveryScope::StartupCritical).await
}

async fn recover_file_operations(
    executors: &ExecutorHandles,
    scope: JournalRecoveryScope,
) -> Result<usize, ExecutorError> {
    let mut recovered_entries = 0usize;
    loop {
        let Some(group) = executors
            .sqlite
            .load_next_generic_file_operation_recovery_durable(scope)
            .await?
        else {
            return Ok(recovered_entries);
        };
        match group.state {
            JournalRecoveryState::Publishing => {
                let ticket = executors
                    .file_io
                    .reserve_journal_mutation(&group.group_id, group.version)
                    .map_err(mutation_registry_error)?;
                let Some(grant) = executors
                    .sqlite
                    .verify_file_operation_publication_durable(&ticket)
                    .await?
                else {
                    continue;
                };
                let sequence = grant.first_sequence().ok_or_else(recovery_conflict)?;
                let mut lease =
                    match acquire_verified_journal_mutation(executors, ticket, grant).await {
                        Ok(lease) => lease,
                        Err(error) if is_permanent_file_failure(error.kind) => {
                            record_permanent_failure(
                                executors,
                                group.group_id,
                                group.version,
                                sequence,
                                JournalFailureStage::Publication,
                                error,
                            )
                            .await?;
                            continue;
                        }
                        Err(error) => return Err(error),
                    };
                let sequence = lease.next_sequence().map_err(|error| {
                    ExecutorError::new(
                        ExecutorErrorKind::Internal,
                        "recover_generic_file_operations",
                        error.to_string(),
                    )
                })?;
                let applied = match executors
                    .file_io
                    .apply_next_journal_entry_durable(&mut lease)
                    .await
                {
                    Ok(applied) => applied,
                    Err(error) if is_permanent_file_failure(error.kind) => {
                        drop(lease);
                        record_permanent_failure(
                            executors,
                            group.group_id,
                            group.version,
                            sequence,
                            JournalFailureStage::Publication,
                            error,
                        )
                        .await?;
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                let checkpoint = executors
                    .sqlite
                    .record_file_entry_published_durable(
                        group.group_id,
                        group.version,
                        applied.sequence,
                    )
                    .await?;
                drop(lease);
                if checkpoint.is_none() {
                    return Err(recovery_conflict());
                }
                recovered_entries = recovered_entries
                    .checked_add(1)
                    .ok_or_else(recovery_conflict)?;
            }
            JournalRecoveryState::FilesCommitted => {
                let outcome = match executors
                    .sqlite
                    .complete_no_product_file_operation_durable(
                        group.group_id.clone(),
                        group.version,
                    )
                    .await
                {
                    Ok(outcome) => outcome,
                    Err(error) if error.kind == ExecutorErrorKind::DatabasePermanent => {
                        let detail = bounded_diagnostic(&error)?;
                        let outcome = executors
                            .sqlite
                            .record_file_operation_finalization_failure_durable(
                                group.group_id,
                                group.version,
                                format!("{:?}", error.kind),
                                detail,
                            )
                            .await?;
                        if outcome == JournalCheckpointOutcome::VersionConflict {
                            return Err(recovery_conflict());
                        }
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                if outcome == JournalCheckpointOutcome::VersionConflict {
                    return Err(recovery_conflict());
                }
                let JournalCheckpointOutcome::Advanced { version } = outcome else {
                    unreachable!();
                };
                executors
                    .file_io
                    .release_journal_mutation_fence(&group.group_id, version)
                    .map_err(mutation_registry_error)?;
            }
            JournalRecoveryState::CleanupPending => {
                let ticket = executors
                    .file_io
                    .reserve_journal_mutation(&group.group_id, group.version)
                    .map_err(mutation_registry_error)?;
                let Some(grant) = executors
                    .sqlite
                    .verify_file_operation_cleanup_durable(&ticket)
                    .await?
                else {
                    continue;
                };
                let sequence = grant.first_sequence().ok_or_else(recovery_conflict)?;
                let mut lease =
                    match acquire_verified_journal_mutation(executors, ticket, grant).await {
                        Ok(lease) => lease,
                        Err(error) if is_permanent_file_failure(error.kind) => {
                            record_permanent_failure(
                                executors,
                                group.group_id,
                                group.version,
                                sequence,
                                JournalFailureStage::Cleanup,
                                error,
                            )
                            .await?;
                            continue;
                        }
                        Err(error) => return Err(error),
                    };
                let sequence = lease.next_sequence().map_err(|error| {
                    ExecutorError::new(
                        ExecutorErrorKind::Internal,
                        "recover_generic_file_operations",
                        error.to_string(),
                    )
                })?;
                let applied = match executors
                    .file_io
                    .apply_next_journal_entry_durable(&mut lease)
                    .await
                {
                    Ok(applied) => applied,
                    Err(error) if is_permanent_file_failure(error.kind) => {
                        drop(lease);
                        record_permanent_failure(
                            executors,
                            group.group_id,
                            group.version,
                            sequence,
                            JournalFailureStage::Cleanup,
                            error,
                        )
                        .await?;
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                if applied.outcome
                    == crate::executor::JournalFileMutationOutcome::Cleaned(
                        crate::executor::CleanupJournalOutcome::ProgressPending,
                    )
                {
                    drop(lease);
                    yield_progress_to_tail(executors, group.group_id, group.version).await?;
                    return Ok(recovered_entries);
                }
                let checkpoint = executors
                    .sqlite
                    .record_file_entry_cleaned_durable(
                        group.group_id.clone(),
                        group.version,
                        applied.sequence,
                    )
                    .await?;
                drop(lease);
                let Some(checkpoint) = checkpoint else {
                    return Err(recovery_conflict());
                };
                if checkpoint.phase_complete {
                    executors
                        .file_io
                        .release_journal_mutation_fence(&group.group_id, checkpoint.version)
                        .map_err(mutation_registry_error)?;
                    executors.scheduler.wake_llm_results();
                }
                recovered_entries = recovered_entries
                    .checked_add(1)
                    .ok_or_else(recovery_conflict)?;
            }
            JournalRecoveryState::RollbackPending => {
                let ticket = executors
                    .file_io
                    .reserve_journal_mutation(&group.group_id, group.version)
                    .map_err(mutation_registry_error)?;
                let Some(grant) = executors
                    .sqlite
                    .verify_file_operation_rollback_durable(&ticket)
                    .await?
                else {
                    continue;
                };
                let sequence = grant.first_sequence().ok_or_else(recovery_conflict)?;
                let mut lease =
                    match acquire_verified_journal_mutation(executors, ticket, grant).await {
                        Ok(lease) => lease,
                        Err(error) => {
                            let error_message = error.to_string();
                            record_permanent_failure(
                                executors,
                                group.group_id,
                                group.version,
                                sequence,
                                JournalFailureStage::Rollback,
                                error,
                            )
                            .await?;
                            tracing::warn!(
                            error = error_message,
                            "Journal rollback evidence failed and was returned to the recovery tail"
                        );
                            return Ok(recovered_entries);
                        }
                    };
                let sequence = lease.next_sequence().map_err(|error| {
                    ExecutorError::new(
                        ExecutorErrorKind::Internal,
                        "recover_generic_file_operations",
                        error.to_string(),
                    )
                })?;
                let applied = match executors
                    .file_io
                    .apply_next_journal_entry_durable(&mut lease)
                    .await
                {
                    Ok(applied) => applied,
                    Err(error) => {
                        let error_message = error.to_string();
                        drop(lease);
                        record_permanent_failure(
                            executors,
                            group.group_id,
                            group.version,
                            sequence,
                            JournalFailureStage::Rollback,
                            error,
                        )
                        .await?;
                        tracing::warn!(
                            error = error_message,
                            "Journal rollback failed and was returned to the recovery tail"
                        );
                        return Ok(recovered_entries);
                    }
                };
                if applied.outcome
                    == crate::executor::JournalFileMutationOutcome::Cleaned(
                        crate::executor::CleanupJournalOutcome::ProgressPending,
                    )
                {
                    drop(lease);
                    yield_progress_to_tail(executors, group.group_id, group.version).await?;
                    return Ok(recovered_entries);
                }
                let checkpoint = executors
                    .sqlite
                    .record_file_entry_rolled_back_durable(
                        group.group_id.clone(),
                        group.version,
                        applied.sequence,
                    )
                    .await?;
                drop(lease);
                let Some(checkpoint) = checkpoint else {
                    return Err(recovery_conflict());
                };
                if checkpoint.phase_complete {
                    executors
                        .file_io
                        .release_journal_mutation_fence(&group.group_id, checkpoint.version)
                        .map_err(mutation_registry_error)?;
                    executors.scheduler.wake_llm_results();
                }
                recovered_entries = recovered_entries
                    .checked_add(1)
                    .ok_or_else(recovery_conflict)?;
            }
        }
    }
}

pub async fn rollback_prepared_file_operations_after_restart(
    executors: &ExecutorHandles,
) -> Result<usize, ExecutorError> {
    let mut requested = 0_usize;
    loop {
        let page = executors
            .sqlite
            .list_file_operations_durable(vec!["prepared".to_string()], None, 100)
            .await?;
        if page.operations.is_empty() {
            return Ok(requested);
        }
        let mut page_progressed = false;
        for operation in page.operations {
            match cancel_generic_file_operation(
                executors,
                operation.operation_id,
                operation.version,
            )
            .await?
            {
                JournalCancellationOutcome::Requested { .. }
                | JournalCancellationOutcome::AlreadyRequested { .. } => {
                    page_progressed = true;
                    requested = requested.checked_add(1).ok_or_else(|| {
                        ExecutorError::new(
                            ExecutorErrorKind::Internal,
                            "rollback_prepared_file_operations_after_restart",
                            "prepared file-operation count overflowed",
                        )
                    })?;
                }
                JournalCancellationOutcome::VersionConflict
                | JournalCancellationOutcome::NotCancellable => {}
            }
        }
        if !page_progressed {
            return Err(ExecutorError::new(
                ExecutorErrorKind::Conflict,
                "rollback_prepared_file_operations_after_restart",
                "prepared file operations changed during startup rollback",
            ));
        }
    }
}

pub async fn discard_incomplete_file_products_after_restart(
    executors: &ExecutorHandles,
) -> Result<usize, ExecutorError> {
    let states = vec![
        "publishing".to_string(),
        "publication_failed".to_string(),
        "files_committed".to_string(),
        "finalize_failed".to_string(),
    ];
    let mut cursor = None;
    let mut pending_cursor_operation = None;
    let mut discarded = 0_usize;
    loop {
        let page = executors
            .sqlite
            .list_file_operations_durable(states.clone(), cursor.clone(), 100)
            .await?;
        if let Some(operation) = pending_cursor_operation.take() {
            discarded = discard_incomplete_file_product(executors, operation, discarded).await?;
        }
        let next_cursor = page.next_cursor;
        for operation in page.operations {
            if next_cursor.as_deref() == Some(&operation.operation_id) {
                pending_cursor_operation = Some(operation);
            } else {
                discarded =
                    discard_incomplete_file_product(executors, operation, discarded).await?;
            }
        }
        match next_cursor {
            Some(next_cursor) => cursor = Some(next_cursor),
            None => return Ok(discarded),
        }
    }
}

async fn discard_incomplete_file_product(
    executors: &ExecutorHandles,
    operation: crate::models::FileOperationSummary,
    discarded: usize,
) -> Result<usize, ExecutorError> {
    let is_inbox = operation.kind == "llm_result_receive"
        && operation.product_target.as_deref() == Some("llm_result_inbox");
    let is_face_product = operation.kind == "llm_result_artifacts"
        && operation.product_target.as_deref() == Some("llm_result_face_crops");
    let is_metadata_product = operation.kind == "metadata_artifacts"
        && operation.product_target.as_deref() == Some("metadata_artifacts");
    let is_import_product = operation.kind == "import_media_publication"
        && operation.product_target.as_deref() == Some("import_media");
    if !is_inbox && !is_face_product && !is_metadata_product && !is_import_product {
        return Ok(discarded);
    }
    match cancel_generic_file_operation(executors, operation.operation_id, operation.version)
        .await?
    {
        JournalCancellationOutcome::Requested { .. }
        | JournalCancellationOutcome::AlreadyRequested { .. } => {
            discarded.checked_add(1).ok_or_else(|| {
                ExecutorError::new(
                    ExecutorErrorKind::Internal,
                    "discard_incomplete_file_products_after_restart",
                    "discarded file-product count overflowed",
                )
            })
        }
        JournalCancellationOutcome::VersionConflict
        | JournalCancellationOutcome::NotCancellable => Err(ExecutorError::new(
            ExecutorErrorKind::Conflict,
            "discard_incomplete_file_products_after_restart",
            "incomplete file product changed during startup recovery",
        )),
    }
}

async fn yield_progress_to_tail(
    executors: &ExecutorHandles,
    group_id: String,
    expected_version: i64,
) -> Result<(), ExecutorError> {
    if executors
        .sqlite
        .yield_file_operation_progress_durable(group_id, expected_version)
        .await?
        == JournalCheckpointOutcome::VersionConflict
    {
        return Err(recovery_conflict());
    }
    executors.scheduler.wake_journal_recovery();
    Ok(())
}

fn mutation_registry_error(error: crate::io::file::MutationLeaseError) -> ExecutorError {
    ExecutorError::new(
        ExecutorErrorKind::Internal,
        "recover_generic_file_operations",
        error.to_string(),
    )
}

fn is_permanent_file_failure(kind: ExecutorErrorKind) -> bool {
    matches!(
        kind,
        ExecutorErrorKind::FileNotFound
            | ExecutorErrorKind::FilePermission
            | ExecutorErrorKind::FileConflict
            | ExecutorErrorKind::FileInvalidData
    )
}

async fn record_permanent_failure(
    executors: &ExecutorHandles,
    group_id: String,
    group_version: i64,
    sequence: u16,
    stage: JournalFailureStage,
    error: ExecutorError,
) -> Result<(), ExecutorError> {
    let detail = error.to_string();
    let detail = bounded_diagnostic_text(detail)?;
    let outcome = executors
        .sqlite
        .record_file_operation_failure_durable(
            group_id,
            group_version,
            sequence,
            stage,
            format!("{:?}", error.kind),
            detail,
        )
        .await?;
    if outcome == JournalCheckpointOutcome::VersionConflict {
        return Err(recovery_conflict());
    }
    Ok(())
}

fn bounded_diagnostic(error: &ExecutorError) -> Result<String, ExecutorError> {
    bounded_diagnostic_text(error.to_string())
}

fn bounded_diagnostic_text(detail: String) -> Result<String, ExecutorError> {
    if detail.len() > 1024 {
        return Err(ExecutorError::new(
            ExecutorErrorKind::Internal,
            "recover_generic_file_operations",
            "executor error exceeded the journal diagnostic bound",
        ));
    }
    Ok(detail)
}

fn recovery_conflict() -> ExecutorError {
    ExecutorError::new(
        crate::executor::ExecutorErrorKind::Internal,
        "recover_generic_file_operations",
        "journal recovery lost its durable group version",
    )
}
