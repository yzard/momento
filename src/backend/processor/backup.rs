use chrono::DateTime;
use futures::{stream, StreamExt};

use crate::database::operations::{
    BackupProcessingAsset, BackupProcessingTransition, BackupProcessingTransitionOutcome,
    BackupRecoveryPageQuery, ClaimedBackupAsset, StoreBackupContentHash,
};
use crate::error::{AppError, AppResult};
use crate::executor::{ExecutorErrorKind, Sha256Session, SqliteExecutorHandle};
use crate::io::file::{NormalizedStoragePath, StorageRootId};
use crate::processor::import::{
    import_staged_file, ImportSource, StagedImportCleanup, StagedImportFile,
};
use crate::runtime::{DurableSourceId, ExecutorHandles, SchedulerAdmissionKind, SchedulerHandle};

const BACKUP_RECOVERY_PAGE_SIZE: u16 = 256;

pub async fn run(executors: ExecutorHandles) {
    let scheduler = executors.scheduler.clone();
    let sqlite = executors.sqlite.clone();
    match scheduler
        .acquire_durable(
            DurableSourceId::BackupImport,
            SchedulerAdmissionKind::RecoveryHandoff,
        )
        .await
    {
        Ok(_worker_permit) => {
            if let Err(error) = recover(&executors).await {
                tracing::warn!("backup recovery failed: {error}");
            }
        }
        Err(error) => {
            tracing::warn!(error, "backup recovery stopped");
            return;
        }
    }

    loop {
        if let Err(error) = run_cycle(&executors).await {
            tracing::warn!("backup worker cycle failed: {error}");
        }
        let next_expiration = match scheduler
            .acquire_durable(
                DurableSourceId::BackupImport,
                SchedulerAdmissionKind::NewClaim,
            )
            .await
        {
            Ok(_worker_permit) => match sqlite.maintain_backup_sessions_durable().await {
                Ok(maintenance) => maintenance
                    .next_expiration_seconds
                    .map(std::time::Duration::from_secs),
                Err(error) => {
                    tracing::warn!(error = %error, "backup expiration maintenance deferred");
                    Some(std::time::Duration::from_secs(1))
                }
            },
            Err(_) => return,
        };
        if let Some(delay) = next_expiration {
            tokio::select! {
                () = scheduler.backup_import_notified() => {}
                () = tokio::time::sleep(delay) => {}
            }
        } else {
            scheduler.backup_import_notified().await;
        }
    }
}

pub async fn recover(executors: &ExecutorHandles) -> AppResult<()> {
    let sqlite = &executors.sqlite;
    recover_processing_assets(executors).await?;
    sqlite.recover_backup_writing_sessions_durable().await?;
    let mut after_id = 0;
    loop {
        let page = sqlite
            .load_backup_resumable_page_durable(BackupRecoveryPageQuery {
                after_id,
                limit: BACKUP_RECOVERY_PAGE_SIZE,
            })
            .await?;
        for file in page.rows {
            match truncate_to_durable_offset(executors, &file.staged_path, file.uploaded_size).await
            {
                Ok(()) => {}
                Err(AppError::Internal(_)) if file.uploaded_size > 0 => {
                    transition_backup_processing(
                        sqlite,
                        &executors.scheduler,
                        BackupProcessingTransition::FailMissingStaging {
                            asset_id: file.asset_id,
                        },
                    )
                    .await?;
                }
                Err(error) => return Err(error),
            }
        }
        let Some(next_after_id) = page.next_after_id else {
            break;
        };
        after_id = next_after_id;
    }
    sqlite.maintain_backup_sessions_durable().await?;
    Ok(())
}

async fn recover_processing_assets(executors: &ExecutorHandles) -> AppResult<()> {
    let sqlite = &executors.sqlite;
    let mut after_id = 0;
    loop {
        let page = sqlite
            .load_backup_processing_page_durable(BackupRecoveryPageQuery {
                after_id,
                limit: BACKUP_RECOVERY_PAGE_SIZE,
            })
            .await?;
        for asset in page.rows {
            recover_processing_asset(executors, asset).await?;
        }
        let Some(next_after_id) = page.next_after_id else {
            break;
        };
        after_id = next_after_id;
    }
    Ok(())
}

async fn recover_processing_asset(
    executors: &ExecutorHandles,
    asset: BackupProcessingAsset,
) -> AppResult<()> {
    let sqlite = &executors.sqlite;
    let recovered_media_id = if let Some(content_hash) = asset.content_hash {
        sqlite
            .load_recovered_backup_media_durable(content_hash, asset.user_id)
            .await?
    } else {
        None
    };
    if let Some(media_id) = recovered_media_id {
        transition_backup_processing(
            sqlite,
            &executors.scheduler,
            BackupProcessingTransition::Complete {
                asset_id: asset.asset_id,
                media_id,
            },
        )
        .await?;
        return Ok(());
    }

    if !storage_file_exists(executors, &asset.staged_path).await? {
        transition_backup_processing(
            sqlite,
            &executors.scheduler,
            BackupProcessingTransition::Fail {
                asset_id: asset.asset_id,
                error: "backup staging file is missing".to_string(),
            },
        )
        .await?;
        return Ok(());
    }
    transition_backup_processing(
        sqlite,
        &executors.scheduler,
        BackupProcessingTransition::Requeue {
            asset_id: asset.asset_id,
        },
    )
    .await
}

pub async fn run_cycle(executors: &ExecutorHandles) -> AppResult<()> {
    let sqlite = &executors.sqlite;
    let scheduler = &executors.scheduler;
    let concurrency = scheduler.durable_capacity();
    let claimed_assets = {
        let _worker_permit = scheduler
            .acquire_durable(
                DurableSourceId::BackupImport,
                SchedulerAdmissionKind::NewClaim,
            )
            .await
            .map_err(AppError::Internal)?;
        sqlite.maintain_backup_sessions_durable().await?;
        let mut claimed_assets = Vec::with_capacity(concurrency);
        for _ in 0..concurrency {
            let Some(asset) = sqlite.claim_backup_asset_durable().await? else {
                break;
            };
            claimed_assets.push(asset);
        }
        claimed_assets
    };

    let mut processing = stream::iter(claimed_assets)
        .map(|asset| async move {
            let worker_permit = match scheduler
                .acquire_durable(
                    DurableSourceId::BackupImport,
                    SchedulerAdmissionKind::ExistingClaimCompletion,
                )
                .await
            {
                Ok(worker_permit) => worker_permit,
                Err(error) => {
                    return Err(AppError::Unavailable(error));
                }
            };
            match process_claimed_asset(executors, asset, worker_permit).await {
                Ok(()) => {
                    scheduler.wake_metadata();
                    Ok(())
                }
                Err(error) => Err(error),
            }
        })
        .buffer_unordered(concurrency);
    let mut first_error = None;
    while let Some(result) = processing.next().await {
        if let Err(error) = result {
            tracing::warn!(error = %error, "backup asset processing failed");
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
    }
    first_error.map_or(Ok(()), Err)
}

async fn process_claimed_asset(
    executors: &ExecutorHandles,
    asset: ClaimedBackupAsset,
    mut worker_permit: crate::runtime::DurableAdmission,
) -> AppResult<()> {
    let sqlite = &executors.sqlite;
    let scheduler = &executors.scheduler;
    let result = async {
        set_source_modified_time(executors, &asset.staged_path, &asset.source_modified_at).await?;
        let content_hash = calculate_backup_hash(executors, &asset.staged_path).await?;
        if content_hash != asset.expected_content_hash {
            return Err(AppError::Conflict(
                "staged backup no longer matches the Android original".to_string(),
            ));
        }
        write_supplemental_metadata(
            executors,
            asset.asset_id,
            &asset.staged_path,
            &asset.metadata_json,
        )
        .await?;
        if !sqlite
            .store_backup_content_hash_durable(StoreBackupContentHash {
                asset_id: asset.asset_id,
                content_hash,
            })
            .await?
        {
            return Err(AppError::Conflict(
                "backup asset changed while preparing import".to_string(),
            ));
        }
        let staged_source = StagedImportFile::new(
            StorageRootId::Backups,
            NormalizedStoragePath::parse(&asset.staged_path)
                .map_err(|error| AppError::Validation(error.to_string()))?,
        )?;
        let mut attempt = import_staged_file(
            staged_source,
            ImportSource::MobileBackup,
            asset.user_id,
            executors,
            StagedImportCleanup {
                source: false,
                supplemental_metadata: false,
            },
            &worker_permit,
        )
        .await?;
        loop {
            match attempt {
                crate::processor::import::ImportStagedFileOutcome::Completed(media_id) => {
                    break Ok(media_id);
                }
                crate::processor::import::ImportStagedFileOutcome::Deferred(prepared) => {
                    drop(worker_permit);
                    tokio::task::yield_now().await;
                    worker_permit = scheduler
                        .acquire_durable(
                            DurableSourceId::BackupImport,
                            SchedulerAdmissionKind::ExistingClaimCompletion,
                        )
                        .await
                        .map_err(AppError::Unavailable)?;
                    attempt = crate::processor::import::resume_staged_file_import(
                        *prepared,
                        executors,
                        &worker_permit,
                    )
                    .await?;
                }
            }
        }
    }
    .await;

    match result {
        Ok(media_id) => {
            transition_backup_processing(
                sqlite,
                scheduler,
                BackupProcessingTransition::Complete {
                    asset_id: asset.asset_id,
                    media_id,
                },
            )
            .await?;
        }
        Err(error) => {
            transition_backup_processing(
                sqlite,
                scheduler,
                BackupProcessingTransition::Fail {
                    asset_id: asset.asset_id,
                    error: error.to_string(),
                },
            )
            .await?;
        }
    }
    Ok(())
}

async fn transition_backup_processing(
    sqlite: &SqliteExecutorHandle,
    scheduler: &SchedulerHandle,
    transition: BackupProcessingTransition,
) -> AppResult<()> {
    match sqlite
        .transition_backup_processing_durable(transition)
        .await?
    {
        BackupProcessingTransitionOutcome::Transitioned { cleanup_group } => {
            if cleanup_group {
                scheduler.wake_journal_recovery();
            }
            Ok(())
        }
        BackupProcessingTransitionOutcome::Unchanged => Err(AppError::Conflict(
            "backup processing state changed concurrently".to_string(),
        )),
        BackupProcessingTransitionOutcome::PathConflict => Err(AppError::Conflict(
            "backup staging files are being changed by another operation".to_string(),
        )),
    }
}

async fn set_source_modified_time(
    executors: &ExecutorHandles,
    staged_path: &str,
    source_modified_at: &str,
) -> AppResult<()> {
    let timestamp = DateTime::parse_from_rfc3339(source_modified_at)
        .map_err(|_| AppError::BadRequest("invalid stored sourceModifiedAt".to_string()))?;
    let path = NormalizedStoragePath::parse(staged_path)
        .map_err(|error| AppError::Internal(error.to_string()))?;
    executors
        .file_io
        .set_storage_modified_time_durable(
            StorageRootId::Backups,
            path,
            timestamp.timestamp(),
            timestamp.timestamp_subsec_nanos(),
        )
        .await?;
    Ok(())
}

async fn truncate_to_durable_offset(
    executors: &ExecutorHandles,
    staged_path: &str,
    uploaded_size: u64,
) -> AppResult<()> {
    let path = NormalizedStoragePath::parse(staged_path)
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let exists = storage_file_exists(executors, staged_path).await?;
    if !exists && uploaded_size == 0 {
        return Ok(());
    }
    if !exists {
        return Err(AppError::Internal(format!(
            "backup staging file is missing with durable offset {uploaded_size}"
        )));
    }
    let session = executors
        .file_io
        .open_storage_write_session_durable(StorageRootId::Backups, path, uploaded_size)
        .await?;
    executors
        .file_io
        .commit_storage_session_durable(session)
        .await?;
    Ok(())
}

async fn calculate_backup_hash(
    executors: &ExecutorHandles,
    staged_path: &str,
) -> AppResult<String> {
    let path = NormalizedStoragePath::parse(staged_path)
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let (opened_session, _) = executors
        .file_io
        .open_storage_read_session_durable(StorageRootId::Backups, path)
        .await?;
    let mut file_session = Some(opened_session);
    let mut hash_session: Option<Sha256Session> =
        Some(executors.cpu.start_sha256_session_durable().await?);
    loop {
        let active_file = file_session.take().ok_or_else(|| {
            AppError::Internal("backup hash file session is unavailable".to_string())
        })?;
        let (returned_file, bytes) = executors
            .file_io
            .read_storage_session_durable(active_file, crate::runtime::FILE_IO_CHUNK_BYTES as usize)
            .await?;
        file_session = Some(returned_file);
        if bytes.is_empty() {
            break;
        }
        let active_hash = hash_session.take().ok_or_else(|| {
            AppError::Internal("backup hash CPU session is unavailable".to_string())
        })?;
        let (returned_hash, _) = executors
            .cpu
            .update_sha256_session_durable(active_hash, bytes)
            .await?;
        hash_session = Some(returned_hash);
    }
    executors
        .file_io
        .close_storage_session_durable(file_session.take().ok_or_else(|| {
            AppError::Internal("backup hash file session is unavailable".to_string())
        })?)
        .await?;
    Ok(executors
        .cpu
        .finish_sha256_session_durable(hash_session.take().ok_or_else(|| {
            AppError::Internal("backup hash CPU session is unavailable".to_string())
        })?)
        .await?)
}

async fn storage_file_exists(executors: &ExecutorHandles, staged_path: &str) -> AppResult<bool> {
    let path = NormalizedStoragePath::parse(staged_path)
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let session = match executors
        .file_io
        .open_storage_read_session_durable(StorageRootId::Backups, path)
        .await
    {
        Ok((session, _)) => session,
        Err(error) if error.kind == ExecutorErrorKind::FileNotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    executors
        .file_io
        .close_storage_session_durable(session)
        .await?;
    Ok(true)
}

async fn write_supplemental_metadata(
    executors: &ExecutorHandles,
    asset_id: i64,
    staged_path: &str,
    metadata_json: &str,
) -> AppResult<()> {
    let contents = executors
        .cpu
        .validate_json_durable(metadata_json.as_bytes().to_vec())
        .await?;
    let temporary_path =
        NormalizedStoragePath::parse(&format!(".momento-pending/backup-{asset_id}-metadata.json"))
            .map_err(|error| AppError::Internal(error.to_string()))?;
    let destination_path =
        NormalizedStoragePath::parse(&format!("{staged_path}.supplemental-metadata.json"))
            .map_err(|error| AppError::Internal(error.to_string()))?;
    executors
        .file_io
        .atomic_replace_storage_file_durable(
            StorageRootId::Backups,
            temporary_path,
            destination_path,
            contents,
        )
        .await?;
    Ok(())
}
