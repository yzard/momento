use super::*;
use axum::response::IntoResponse;
use std::collections::HashSet;

use crate::config::Config;
use crate::executor::ControlRequestDto;
use crate::models::{
    BackupBatchCapabilities, BackupBatchEntry, BackupBatchError, BackupBatchManifest,
    BackupBatchPrepareRequest, BackupBatchResponse, BackupBatchResult, BackupUploadIdsRequest,
};

const MAX_ITEMS: usize = 256;
const MAX_MANIFEST_BYTES: usize = 1024 * 1024;

pub(crate) fn batch_capabilities(config: &Config) -> BackupBatchCapabilities {
    BackupBatchCapabilities {
        version: 1,
        max_bytes: config.backup.max_batch_bytes,
        max_items: MAX_ITEMS,
        max_manifest_bytes: config
            .server
            .api_request_body_max_bytes
            .min(MAX_MANIFEST_BYTES),
    }
}

fn validate_keys<'a>(keys: impl Iterator<Item = &'a str>) -> AppResult<()> {
    let mut seen = HashSet::new();
    for key in keys {
        validate_identifier(key, "batch key")?;
        if !seen.insert(key) || seen.len() > MAX_ITEMS {
            return Err(AppError::BadRequest(
                "batch keys must be unique and within the item limit".to_string(),
            ));
        }
    }
    if seen.is_empty() {
        return Err(AppError::BadRequest(
            "backup batch must not be empty".to_string(),
        ));
    }
    Ok(())
}

fn item(key: String, result: AppResult<BackupUploadResponse>) -> BackupBatchResult {
    match result {
        Ok(upload) => BackupBatchResult {
            key,
            upload: Some(upload),
            error: None,
        },
        Err(error) => {
            let detail = match &error {
                AppError::BadRequest(s)
                | AppError::Conflict(s)
                | AppError::NotFound(s)
                | AppError::Validation(s)
                | AppError::UnprocessableEntity(s)
                | AppError::PayloadTooLarge(s) => s.clone(),
                _ => "Backup operation could not be completed; retry later".to_string(),
            };
            let status = error.into_response().status().as_u16();
            BackupBatchResult {
                key,
                upload: None,
                error: Some(BackupBatchError { status, detail }),
            }
        }
    }
}

pub(super) async fn prepare(
    State(state): State<AppState>,
    user: CurrentUser,
    CpuJson(request): CpuJson<BackupBatchPrepareRequest>,
) -> AppResult<Response> {
    let _backup_activity = state.scheduler.begin_backup_transfer();
    validate_keys(request.uploads.iter().map(|x| x.operation_id.as_str()))?;
    let mut results = Vec::with_capacity(request.uploads.len());
    for upload in request.uploads {
        let key = upload.operation_id.clone();
        results.push(item(key, create_upload_item(&state, user.id, upload).await));
    }
    render_json(&state, BackupBatchResponse { results }).await
}

pub(super) async fn status(
    State(state): State<AppState>,
    user: CurrentUser,
    CpuJson(request): CpuJson<BackupUploadIdsRequest>,
) -> AppResult<Response> {
    validate_keys(request.upload_ids.iter().map(String::as_str))?;
    let mut results = Vec::with_capacity(request.upload_ids.len());
    for id in request.upload_ids {
        let result = load(&state, user.id, id.clone()).await;
        results.push(item(id, result));
    }
    render_json(&state, BackupBatchResponse { results }).await
}

async fn load(
    state: &AppState,
    user_id: i64,
    upload_id: String,
) -> AppResult<BackupUploadResponse> {
    state
        .executors
        .sqlite
        .load_backup_upload_request(LoadBackupUpload { user_id, upload_id })
        .await?
        .ok_or_else(|| AppError::NotFound("backup upload not found".to_string()))
}

pub(super) async fn upload(
    State(state): State<AppState>,
    Extension(admission): Extension<HttpRequestAdmission>,
    user: CurrentUser,
    headers: HeaderMap,
    body: Body,
) -> AppResult<Response> {
    let _backup_activity = state.scheduler.begin_backup_transfer();
    let declared = content_length(&headers)?;
    admission
        .convert_to_stream()
        .await
        .map_err(AppError::Unavailable)?;
    let config = state.config.current();
    let limits = batch_capabilities(&config);
    let mut reader = BackupBodyReader::new(body);
    let prefix = reader.exact(4).await?;
    let length = u32::from_be_bytes(prefix.try_into().expect("four byte batch prefix")) as usize;
    if length == 0 || length > limits.max_manifest_bytes {
        return Err(AppError::BadRequest(
            "backup batch manifest exceeds its limit".to_string(),
        ));
    }
    let bytes = reader.exact(length).await?;
    let parsed = state
        .executors
        .cpu
        .parse_control_request(BackupBatchManifest::KIND, bytes)
        .await?;
    let dto = parsed
        .request
        .map_err(|e| AppError::BadRequest(format!("invalid backup manifest: {}", e.detail)))?;
    let manifest = BackupBatchManifest::from_control_request(dto).ok_or_else(|| {
        AppError::Internal("CPU parser returned the wrong batch manifest".to_string())
    })?;
    if manifest.version != 1 {
        return Err(AppError::BadRequest(
            "backup batch version must be 1".to_string(),
        ));
    }
    validate_keys(manifest.entries.iter().map(|x| x.upload_id.as_str()))?;
    let mut total = 0_u64;
    for entry in &manifest.entries {
        validate_content_hash(&entry.content_hash, "contentHash")?;
        if entry.byte_size == 0 || entry.byte_size > config.backup.max_upload_bytes {
            return Err(AppError::BadRequest(
                "backup batch media exceeds the upload limit".to_string(),
            ));
        }
        total = total
            .checked_add(entry.byte_size)
            .ok_or_else(|| AppError::BadRequest("backup batch size overflow".to_string()))?;
    }
    if (manifest.entries.len() > 1 && total > limits.max_bytes)
        || total.checked_add(length as u64 + 4) != Some(declared)
    {
        return Err(AppError::BadRequest(
            "backup batch exceeds its byte limit or Content-Length".to_string(),
        ));
    }
    let mut results = Vec::with_capacity(manifest.entries.len());
    let count = manifest.entries.len();
    for (index, entry) in manifest.entries.into_iter().enumerate() {
        let before = reader.position;
        let result = receive(&state, user.id, &entry, &mut reader, index + 1 == count).await;
        let consumed = reader.position - before;
        if consumed < entry.byte_size {
            reader.discard(entry.byte_size - consumed).await?;
        }
        results.push(item(entry.upload_id, result));
    }
    reader.ensure_eof().await?;
    render_json(&state, BackupBatchResponse { results }).await
}

async fn receive(
    state: &AppState,
    user_id: i64,
    entry: &BackupBatchEntry,
    reader: &mut BackupBodyReader,
    last: bool,
) -> AppResult<BackupUploadResponse> {
    let current = load(state, user_id, entry.upload_id.clone()).await?;
    if current.expected_size != entry.byte_size as i64
        || current.content_hash.as_deref() != Some(&entry.content_hash)
    {
        return Err(AppError::Conflict(
            "batch media does not match its protected original manifest".to_string(),
        ));
    }
    if current.uploaded_size == current.expected_size
        && matches!(
            current.status.as_str(),
            "uploading" | "queued" | "processing" | "completed"
        )
    {
        reader.discard(entry.byte_size).await?;
        return if current.status == "uploading" {
            complete_upload_item(state, user_id, entry.upload_id.clone()).await
        } else {
            Ok(current)
        };
    }
    let path = match state
        .executors
        .sqlite
        .claim_backup_chunk_request(ClaimBackupChunk {
            user_id,
            upload_id: entry.upload_id.clone(),
            start: 0,
            total: entry.byte_size,
        })
        .await?
    {
        ClaimBackupChunkOutcome::Accepted { staged_path } => staged_path,
        ClaimBackupChunkOutcome::NotFound => {
            return Err(AppError::NotFound("backup upload not found".to_string()))
        }
        ClaimBackupChunkOutcome::Rejected => {
            return Err(AppError::Conflict(
                "resume this upload from its durable offset".to_string(),
            ))
        }
    };
    let path = NormalizedStoragePath::parse(&path)
        .map_err(|e| AppError::Internal(format!("invalid stored backup path: {e}")))?;
    let written = write_chunk_reader(
        &state.executors,
        &path,
        0,
        entry.byte_size,
        &entry.content_hash,
        reader,
        last,
    )
    .await;
    if let Err(error) = written {
        state
            .executors
            .sqlite
            .abandon_backup_chunk_request(AbandonBackupChunk {
                user_id,
                upload_id: entry.upload_id.clone(),
            })
            .await?;
        state.scheduler.wake_backup_import();
        return Err(error);
    }
    match state
        .executors
        .sqlite
        .finish_backup_chunk_request(FinishBackupChunk {
            user_id,
            upload_id: entry.upload_id.clone(),
            start: 0,
            next_offset: entry.byte_size,
            session_expiry_hours: state.config.current().backup.session_expiry_hours,
        })
        .await?
    {
        FinishBackupChunkOutcome::Completed(_) => {
            complete_upload_item(state, user_id, entry.upload_id.clone()).await
        }
        FinishBackupChunkOutcome::Changed => Err(AppError::Conflict(
            "backup upload changed while the batch was written".to_string(),
        )),
    }
}
