use chrono::DateTime;
use filetime::{set_file_mtime, FileTime};
use futures::{stream, StreamExt};
use rusqlite::OptionalExtension;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::config::Config;
use crate::constants::paths;
use crate::database::{queries, DbPool};
use crate::error::{AppError, AppResult};
use crate::processor::import::{import_staged_file, ImportSource};
use crate::utils::hash::calculate_file_hash;
use crate::utils::path::resolve_storage_path;

struct ClaimedAsset {
    id: i64,
    user_id: i64,
    staged_path: String,
    source_modified_at: String,
    expected_content_hash: String,
    metadata_json: String,
}

struct ProcessingAsset {
    id: i64,
    user_id: i64,
    staged_path: String,
    content_hash: Option<String>,
}

pub async fn run(config: Arc<Config>, pool: DbPool) {
    if let Err(error) = recover(&pool).await {
        tracing::warn!("backup recovery failed: {error}");
    }

    let poll_interval = Duration::from_secs(config.backup.worker_poll_interval_seconds);
    loop {
        if let Err(error) = run_cycle(&pool, config.backup.worker_concurrency).await {
            tracing::warn!("backup worker cycle failed: {error}");
        }
        tokio::time::sleep(poll_interval).await;
    }
}

pub async fn recover(pool: &DbPool) -> AppResult<()> {
    recover_processing_assets(pool).await?;
    let resumable_files = {
        let connection = pool.get().map_err(AppError::Pool)?;
        connection.execute(queries::backup::RECOVER_WRITING_SESSIONS, [])?;

        let mut statement = connection.prepare(queries::backup::SELECT_RESUMABLE_FILES)?;
        let resumable_files = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        resumable_files
    };

    for (asset_id, staged_path, uploaded_size) in resumable_files {
        let path = resolve_storage_path(&paths().backups, &staged_path)?;
        match truncate_to_durable_offset(&path, uploaded_size as u64).await {
            Ok(()) => {}
            Err(AppError::Internal(_)) if uploaded_size > 0 => {
                fail_missing_staged_file(pool, asset_id)?;
            }
            Err(error) => return Err(error),
        }
    }
    expire_stale_sessions(pool)?;
    Ok(())
}

async fn recover_processing_assets(pool: &DbPool) -> AppResult<()> {
    let processing_assets = {
        let connection = pool.get().map_err(AppError::Pool)?;
        let mut statement = connection.prepare(queries::backup::SELECT_PROCESSING_ASSETS)?;
        let processing_assets = statement
            .query_map([], |row| {
                Ok(ProcessingAsset {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    staged_path: row.get(2)?,
                    content_hash: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        processing_assets
    };

    for asset in processing_assets {
        let recovered_media_id = if let Some(content_hash) = asset.content_hash.as_deref() {
            let connection = pool.get().map_err(AppError::Pool)?;
            connection
                .query_row(
                    queries::backup::SELECT_RECOVERED_MEDIA,
                    rusqlite::params![content_hash, asset.user_id],
                    |row| row.get(0),
                )
                .optional()?
        } else {
            None
        };
        if let Some(media_id) = recovered_media_id {
            complete_recovered_asset(pool, asset.id, media_id)?;
            let staged_path = resolve_storage_path(&paths().backups, &asset.staged_path)?;
            cleanup_staged_file(&staged_path).await;
            continue;
        }

        if !resolve_storage_path(&paths().backups, &asset.staged_path)?.is_file() {
            fail_processing_asset(pool, asset.id, "backup staging file is missing")?;
            continue;
        }
        requeue_processing_asset(pool, asset.id)?;
    }
    Ok(())
}

pub async fn run_cycle(pool: &DbPool, concurrency: usize) -> AppResult<()> {
    expire_stale_sessions(pool)?;
    let concurrency = concurrency.max(1).min(pool.max_size() as usize);
    let mut claimed_assets = Vec::with_capacity(concurrency);
    for _ in 0..concurrency {
        let Some(asset) = claim_queued_asset(pool)? else {
            break;
        };
        claimed_assets.push(asset);
    }

    stream::iter(claimed_assets)
        .for_each_concurrent(concurrency, |asset| async move {
            if let Err(error) = process_claimed_asset(pool, asset).await {
                tracing::warn!("backup asset processing failed: {error}");
            }
        })
        .await;
    Ok(())
}

fn expire_stale_sessions(pool: &DbPool) -> AppResult<()> {
    let connection = pool.get().map_err(AppError::Pool)?;
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(queries::backup::EXPIRE_SESSIONS, [])?;
    transaction.execute(queries::backup::EXPIRE_ASSETS, [])?;
    transaction.commit()?;
    Ok(())
}

fn claim_queued_asset(pool: &DbPool) -> AppResult<Option<ClaimedAsset>> {
    let connection = pool.get().map_err(AppError::Pool)?;
    let transaction = connection.unchecked_transaction()?;
    let claimed_asset = transaction
        .query_row(queries::backup::CLAIM_QUEUED, [], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .optional()?;
    let Some(claimed_asset) = claimed_asset else {
        transaction.commit()?;
        return Ok(None);
    };
    let (expected_content_hash, metadata_json) = transaction
        .query_row(
            queries::backup::SELECT_MANIFEST_FOR_ASSET,
            [claimed_asset.0],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| {
            AppError::Conflict(format!(
                "backup asset is missing its lossless manifest: {error}"
            ))
        })?;
    let session_claimed =
        transaction.execute(queries::backup::MARK_SESSION_PROCESSING, [claimed_asset.0])?;
    if session_claimed != 1 {
        return Err(AppError::Conflict(
            "backup upload changed while claiming work".to_string(),
        ));
    }
    transaction.commit()?;
    Ok(Some(ClaimedAsset {
        id: claimed_asset.0,
        user_id: claimed_asset.1,
        staged_path: claimed_asset.2,
        source_modified_at: claimed_asset.3,
        expected_content_hash,
        metadata_json,
    }))
}

async fn process_claimed_asset(pool: &DbPool, asset: ClaimedAsset) -> AppResult<()> {
    let source_path = resolve_storage_path(&paths().backups, &asset.staged_path)?;
    let result = async {
        set_source_modified_time(&source_path, &asset.source_modified_at)?;
        let content_hash = calculate_file_hash(&source_path).await?;
        if content_hash != asset.expected_content_hash {
            return Err(AppError::Conflict(
                "staged backup no longer matches the Android original".to_string(),
            ));
        }
        write_supplemental_metadata(&source_path, &asset.metadata_json).await?;
        let connection = pool.get().map_err(AppError::Pool)?;
        let stored = connection.execute(
            queries::backup::STORE_CONTENT_HASH,
            rusqlite::params![content_hash, asset.id],
        )?;
        if stored != 1 {
            return Err(AppError::Conflict(
                "backup asset changed while preparing import".to_string(),
            ));
        }
        import_staged_file(
            &source_path,
            ImportSource::MobileBackup,
            asset.user_id,
            pool,
            true,
        )
        .await
    }
    .await;

    match result {
        Ok(media_id) => {
            let connection = pool.get().map_err(AppError::Pool)?;
            let transaction = connection.unchecked_transaction()?;
            transaction.execute(
                queries::backup::COMPLETE_ASSET,
                rusqlite::params![media_id, asset.id],
            )?;
            transaction.execute(queries::backup::COMPLETE_SESSION, [asset.id])?;
            transaction.commit()?;
        }
        Err(error) => {
            {
                let connection = pool.get().map_err(AppError::Pool)?;
                let transaction = connection.unchecked_transaction()?;
                transaction.execute(
                    queries::backup::FAIL_ASSET,
                    rusqlite::params![error.to_string(), asset.id],
                )?;
                transaction.execute(queries::backup::FAIL_SESSION, [asset.id])?;
                transaction.commit()?;
            }
            cleanup_staged_file(&source_path).await;
        }
    }
    Ok(())
}

fn complete_recovered_asset(pool: &DbPool, asset_id: i64, media_id: i64) -> AppResult<()> {
    let connection = pool.get().map_err(AppError::Pool)?;
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(
        queries::backup::COMPLETE_ASSET,
        rusqlite::params![media_id, asset_id],
    )?;
    transaction.execute(queries::backup::COMPLETE_SESSION, [asset_id])?;
    transaction.commit()?;
    Ok(())
}

fn requeue_processing_asset(pool: &DbPool, asset_id: i64) -> AppResult<()> {
    let connection = pool.get().map_err(AppError::Pool)?;
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(queries::backup::RECOVER_QUEUED_ASSET, [asset_id])?;
    transaction.execute(queries::backup::RECOVER_QUEUED_SESSION, [asset_id])?;
    transaction.commit()?;
    Ok(())
}

fn fail_processing_asset(pool: &DbPool, asset_id: i64, error: &str) -> AppResult<()> {
    let connection = pool.get().map_err(AppError::Pool)?;
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(
        queries::backup::FAIL_ASSET,
        rusqlite::params![error, asset_id],
    )?;
    transaction.execute(queries::backup::FAIL_SESSION, [asset_id])?;
    transaction.commit()?;
    Ok(())
}

fn fail_missing_staged_file(pool: &DbPool, asset_id: i64) -> AppResult<()> {
    let connection = pool.get().map_err(AppError::Pool)?;
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(queries::backup::FAIL_MISSING_STAGED_ASSET, [asset_id])?;
    transaction.execute(queries::backup::FAIL_MISSING_STAGED_SESSION, [asset_id])?;
    transaction.commit()?;
    Ok(())
}

fn set_source_modified_time(path: &Path, source_modified_at: &str) -> AppResult<()> {
    let timestamp = DateTime::parse_from_rfc3339(source_modified_at)
        .map_err(|_| AppError::BadRequest("invalid stored sourceModifiedAt".to_string()))?
        .timestamp();
    set_file_mtime(path, FileTime::from_unix_time(timestamp, 0))?;
    Ok(())
}

async fn truncate_to_durable_offset(path: &Path, uploaded_size: u64) -> AppResult<()> {
    let file = match tokio::fs::OpenOptions::new().write(true).open(path).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && uploaded_size == 0 => {
            return Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(AppError::Internal(format!(
                "backup staging file is missing with durable offset {uploaded_size}"
            )));
        }
        Err(error) => return Err(error.into()),
    };
    file.set_len(uploaded_size).await?;
    file.sync_data().await?;
    Ok(())
}

async fn cleanup_staged_file(path: &Path) {
    cleanup_file(&supplemental_metadata_path(path)).await;
    cleanup_file(path).await;
}

async fn cleanup_file(path: &Path) {
    match tokio::fs::remove_file(path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            tracing::warn!(path = %path.display(), "backup staging cleanup failed: {error}")
        }
    }
}

async fn write_supplemental_metadata(source_path: &Path, metadata_json: &str) -> AppResult<()> {
    let _: serde_json::Value = serde_json::from_str(metadata_json)?;
    let destination_path = supplemental_metadata_path(source_path);
    let parent = destination_path.parent().ok_or_else(|| {
        AppError::Internal("backup supplemental metadata path has no parent".to_string())
    })?;
    let pending_path =
        destination_path.with_extension(format!("json.pending-{}", uuid::Uuid::new_v4().simple()));
    let mut pending_file = tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&pending_path)
        .await?;
    use tokio::io::AsyncWriteExt;
    pending_file.write_all(metadata_json.as_bytes()).await?;
    pending_file.sync_all().await?;
    drop(pending_file);
    if let Err(error) = tokio::fs::rename(&pending_path, &destination_path).await {
        let _ = tokio::fs::remove_file(&pending_path).await;
        return Err(error.into());
    }
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn supplemental_metadata_path(source_path: &Path) -> PathBuf {
    let filename = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("backup-media");
    source_path.with_file_name(format!("{filename}.supplemental-metadata.json"))
}
