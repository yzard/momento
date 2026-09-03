use axum::{
    body::Body,
    extract::{Extension, Path, State},
    http::{header, HeaderMap},
    response::Response,
    routing::{post, put},
    Router,
};
use chrono::DateTime;
use futures::StreamExt;
use std::path::Path as FilePath;

use crate::auth::{AppState, CurrentUser};
use crate::constants::{IMAGE_EXTENSIONS, SUPPORTED_EXTENSIONS, VIDEO_EXTENSIONS};
use crate::database::operations::{
    AbandonBackupChunk, CancelBackupUpload, CancelBackupUploadOutcome, ClaimBackupChunk,
    ClaimBackupChunkOutcome, CreateBackupUpload, CreateBackupUploadOutcome, FinishBackupChunk,
    FinishBackupChunkOutcome, LoadBackupUpload, PrepareBackupCompletion,
    PrepareBackupCompletionOutcome, QueueBackupCompletion, QueueBackupCompletionOutcome,
    RegisterBackupDevice,
};
use crate::error::{AppError, AppResult};
use crate::executor::{CpuExecutorHandle, FileIoExecutorHandle, Sha256Session};
use crate::io::file::{NormalizedStoragePath, StorageRootId};
use crate::io::StorageFileSession;
use crate::models::{
    BackupDeviceRegisterRequest, BackupDeviceRegisterResponse, BackupUploadCreateRequest,
    BackupUploadIdRequest,
};
use crate::routes::{render_json, CpuJson};
use crate::runtime::{ExecutorHandles, HttpRequestAdmission};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/backup/device/register", post(register_device))
        .route("/backup/upload/create", post(create_upload))
        .route("/backup/upload/status", post(upload_status))
        .route("/backup/upload/chunk/:upload_id", put(upload_chunk))
        .route("/backup/upload/complete", post(complete_upload))
        .route("/backup/upload/cancel", post(cancel_upload))
}

async fn register_device(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<BackupDeviceRegisterRequest>,
) -> AppResult<Response> {
    validate_identifier(&request.device_id, "deviceId")?;
    validate_device_name(&request.device_name)?;

    state
        .executors
        .sqlite
        .register_backup_device_request(RegisterBackupDevice {
            user_id: current_user.id,
            device_id: request.device_id,
            device_name: request.device_name.trim().to_string(),
        })
        .await?;

    render_json(&state, BackupDeviceRegisterResponse { registered: true }).await
}

async fn create_upload(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<BackupUploadCreateRequest>,
) -> AppResult<Response> {
    let config = state.config.current();
    validate_identifier(&request.device_id, "deviceId")?;
    validate_identifier(&request.client_asset_id, "clientAssetId")?;
    validate_identifier(&request.operation_id, "operationId")?;
    validate_upload_metadata(&request, config.backup.max_upload_bytes)?;
    let metadata_json = state
        .executors
        .cpu
        .serialize_backup_metadata(request.metadata)
        .await?;

    let upload_id = uuid::Uuid::new_v4().simple().to_string();
    let extension = FilePath::new(&request.original_filename)
        .extension()
        .and_then(|value| value.to_str())
        .ok_or_else(|| AppError::BadRequest("invalid originalFilename".to_string()))?;
    let staged_path = format!(
        "{}/{}/{}.{}",
        current_user.id, request.device_id, upload_id, extension
    );
    let outcome = state
        .executors
        .sqlite
        .create_backup_upload_request(CreateBackupUpload {
            user_id: current_user.id,
            upload_id,
            device_id: request.device_id,
            client_asset_id: request.client_asset_id,
            operation_id: request.operation_id,
            original_filename: request.original_filename,
            mime_type: request.mime_type,
            expected_size: i64::try_from(request.byte_size)
                .map_err(|_| AppError::BadRequest("byteSize is too large".to_string()))?,
            source_modified_at: request.source_modified_at,
            staged_path,
            protocol_version: request.protocol_version,
            content_hash: request.content_hash,
            metadata_json,
            session_expiry_hours: config.backup.session_expiry_hours,
        })
        .await?;
    match outcome {
        CreateBackupUploadOutcome::Created(response) => {
            state.scheduler.wake_backup_import();
            render_json(&state, response).await
        }
        CreateBackupUploadOutcome::Existing(response) => render_json(&state, response).await,
        CreateBackupUploadOutcome::DeviceNotFound => {
            Err(AppError::NotFound("backup device not found".to_string()))
        }
        CreateBackupUploadOutcome::ContractConflict => Err(AppError::Conflict(
            "idempotency key already belongs to a different backup manifest".to_string(),
        )),
    }
}

async fn upload_status(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<BackupUploadIdRequest>,
) -> AppResult<Response> {
    validate_identifier(&request.upload_id, "uploadId")?;
    let upload = state
        .executors
        .sqlite
        .load_backup_upload_request(LoadBackupUpload {
            user_id: current_user.id,
            upload_id: request.upload_id,
        })
        .await?
        .ok_or_else(|| AppError::NotFound("backup upload not found".to_string()))?;
    render_json(&state, upload).await
}

async fn complete_upload(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<BackupUploadIdRequest>,
) -> AppResult<Response> {
    validate_identifier(&request.upload_id, "uploadId")?;
    let (asset_id, staged_relative_path, expected_content_hash) = match state
        .executors
        .sqlite
        .prepare_backup_completion_request(PrepareBackupCompletion {
            user_id: current_user.id,
            upload_id: request.upload_id.clone(),
        })
        .await?
    {
        PrepareBackupCompletionOutcome::AlreadyQueued(response) => {
            return render_json(&state, response).await
        }
        PrepareBackupCompletionOutcome::Ready {
            asset_id,
            staged_path,
            expected_content_hash,
        } => (asset_id, staged_path, expected_content_hash),
        PrepareBackupCompletionOutcome::NotFound => {
            return Err(AppError::NotFound("backup upload not found".to_string()))
        }
        PrepareBackupCompletionOutcome::NotReady => {
            return Err(AppError::Conflict(
                "backup upload is not ready to complete".to_string(),
            ))
        }
        PrepareBackupCompletionOutcome::MissingManifest => {
            return Err(AppError::Conflict(
                "backup upload is missing a lossless manifest".to_string(),
            ))
        }
    };
    let staged_path = NormalizedStoragePath::parse(&staged_relative_path)
        .map_err(|error| AppError::Internal(format!("invalid stored backup path: {error}")))?;
    let actual_content_hash =
        calculate_backup_hash(&state.executors.file_io, &state.executors.cpu, staged_path).await?;
    if actual_content_hash != expected_content_hash {
        return Err(AppError::Conflict(
            "backup upload content hash does not match the original".to_string(),
        ));
    }
    match state
        .executors
        .sqlite
        .queue_backup_completion_request(QueueBackupCompletion {
            user_id: current_user.id,
            upload_id: request.upload_id,
            asset_id,
        })
        .await?
    {
        QueueBackupCompletionOutcome::Queued(response) => {
            state.scheduler.wake_backup_import();
            render_json(&state, response).await
        }
        QueueBackupCompletionOutcome::Changed => Err(AppError::Conflict(
            "backup upload changed concurrently".to_string(),
        )),
    }
}

async fn cancel_upload(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<BackupUploadIdRequest>,
) -> AppResult<Response> {
    validate_identifier(&request.upload_id, "uploadId")?;
    match state
        .executors
        .sqlite
        .cancel_backup_upload_request(CancelBackupUpload {
            user_id: current_user.id,
            upload_id: request.upload_id,
        })
        .await?
    {
        CancelBackupUploadOutcome::Cancelled(response) => {
            state.scheduler.wake_journal_recovery();
            render_json(&state, response).await
        }
        CancelBackupUploadOutcome::AlreadyCancelled(response) => {
            render_json(&state, response).await
        }
        CancelBackupUploadOutcome::NotFound => {
            Err(AppError::NotFound("backup upload not found".to_string()))
        }
        CancelBackupUploadOutcome::Writing => Err(AppError::Conflict(
            "backup upload chunk is still being written".to_string(),
        )),
        CancelBackupUploadOutcome::NotCancellable => Err(AppError::Conflict(
            "backup upload can no longer be cancelled".to_string(),
        )),
        CancelBackupUploadOutcome::Changed => Err(AppError::Conflict(
            "backup upload changed concurrently".to_string(),
        )),
        CancelBackupUploadOutcome::PathConflict => Err(AppError::Conflict(
            "backup staging cleanup conflicts with another file operation".to_string(),
        )),
    }
}

async fn upload_chunk(
    State(state): State<AppState>,
    Extension(admission): Extension<HttpRequestAdmission>,
    current_user: CurrentUser,
    Path(upload_id): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> AppResult<Response> {
    let config = state.config.current();
    validate_identifier(&upload_id, "uploadId")?;
    let declared_length = content_length(&headers)?;
    let expected_chunk_hash = content_hash_header(&headers, "X-Content-SHA256")?;
    if declared_length == 0 || declared_length > config.backup.max_chunk_bytes {
        return Err(AppError::BadRequest(
            "chunk exceeds the backup chunk limit".to_string(),
        ));
    }
    let (start, end, total) = content_range(&headers)?;
    if end
        .checked_sub(start)
        .and_then(|length| length.checked_add(1))
        != Some(declared_length)
    {
        return Err(AppError::BadRequest(
            "Content-Range does not match Content-Length".to_string(),
        ));
    }

    if end >= total || total > config.backup.max_upload_bytes {
        return Err(AppError::BadRequest("invalid Content-Range".to_string()));
    }
    admission
        .convert_to_stream()
        .map_err(AppError::Unavailable)?;
    let staged_path = match state
        .executors
        .sqlite
        .claim_backup_chunk_request(ClaimBackupChunk {
            user_id: current_user.id,
            upload_id: upload_id.clone(),
            start,
            total,
        })
        .await?
    {
        ClaimBackupChunkOutcome::Accepted { staged_path } => staged_path,
        ClaimBackupChunkOutcome::Rejected => {
            return Err(AppError::Conflict(
                "upload offset or status does not accept this chunk".to_string(),
            ))
        }
        ClaimBackupChunkOutcome::NotFound => {
            return Err(AppError::NotFound("backup upload not found".to_string()))
        }
    };

    let staged_path = NormalizedStoragePath::parse(&staged_path)
        .map_err(|error| AppError::Internal(format!("invalid stored backup path: {error}")))?;
    let write_result = write_chunk(
        &state.executors,
        &staged_path,
        start,
        declared_length,
        &expected_chunk_hash,
        body,
    )
    .await;
    if let Err(error) = write_result {
        if let Err(abandon_error) = state
            .executors
            .sqlite
            .abandon_backup_chunk_request(AbandonBackupChunk {
                user_id: current_user.id,
                upload_id,
            })
            .await
        {
            tracing::error!(
                write_error = %error,
                abandon_error = %abandon_error,
                "backup chunk write failed and its durable claim could not be released"
            );
            return Err(abandon_error.into());
        }
        return Err(error);
    }
    match state
        .executors
        .sqlite
        .finish_backup_chunk_request(FinishBackupChunk {
            user_id: current_user.id,
            upload_id,
            start,
            next_offset: end + 1,
        })
        .await?
    {
        FinishBackupChunkOutcome::Completed(response) => render_json(&state, response).await,
        FinishBackupChunkOutcome::Changed => Err(AppError::Conflict(
            "backup upload changed while the chunk was written".to_string(),
        )),
    }
}

async fn write_chunk(
    executors: &ExecutorHandles,
    staged_path: &NormalizedStoragePath,
    start: u64,
    declared_length: u64,
    expected_chunk_hash: &str,
    body: Body,
) -> AppResult<()> {
    let mut session = Some(
        executors
            .file_io
            .open_storage_write_session_request(StorageRootId::Backups, staged_path.clone(), start)
            .await?,
    );
    let mut written = 0_u64;
    let mut hash_session = Some(executors.cpu.start_sha256_session_request().await?);
    let mut stream = body.into_data_stream();
    while let Some(frame) = stream.next().await {
        let chunk = match frame {
            Ok(chunk) => chunk,
            Err(error) => {
                abort_backup_session(&executors.file_io, session.take()).await?;
                return Err(AppError::BadRequest(format!("invalid chunk body: {error}")));
            }
        };
        let Some(next_written) = written.checked_add(chunk.len() as u64) else {
            abort_backup_session(&executors.file_io, session.take()).await?;
            return Err(AppError::BadRequest(
                "chunk body exceeds Content-Length".to_string(),
            ));
        };
        if next_written > declared_length {
            abort_backup_session(&executors.file_io, session.take()).await?;
            return Err(AppError::BadRequest(
                "chunk body exceeds Content-Length".to_string(),
            ));
        }
        for bytes in chunk.chunks(crate::runtime::FILE_IO_CHUNK_BYTES as usize) {
            let _chunk_admission = executors
                .scheduler
                .acquire_file_chunk()
                .await
                .map_err(AppError::Unavailable)?;
            if start.checked_add(written).is_none() {
                abort_backup_session(&executors.file_io, session.take()).await?;
                return Err(AppError::BadRequest("chunk offset overflow".to_string()));
            }
            let active_session = session.take().ok_or_else(|| {
                AppError::Internal("backup file session is unavailable".to_string())
            })?;
            let active_hash_session = hash_session.take().ok_or_else(|| {
                AppError::Internal("backup hash session is unavailable".to_string())
            })?;
            let (returned_hash_session, bytes) = executors
                .cpu
                .update_sha256_session_request(active_hash_session, bytes.to_vec())
                .await?;
            hash_session = Some(returned_hash_session);
            let byte_count = bytes.len();
            let result = executors
                .file_io
                .write_storage_session_request(active_session, bytes)
                .await;
            match result {
                Ok((returned_session, count)) if count == byte_count => {
                    session = Some(returned_session);
                    written = written.checked_add(count as u64).ok_or_else(|| {
                        AppError::BadRequest("chunk body length overflow".to_string())
                    })?;
                }
                Ok((returned_session, _)) => {
                    session = Some(returned_session);
                    abort_backup_session(&executors.file_io, session.take()).await?;
                    return Err(AppError::Internal(
                        "file worker returned a short backup write".to_string(),
                    ));
                }
                Err(error) => {
                    return Err(error.into());
                }
            }
        }
    }
    if written != declared_length {
        abort_backup_session(&executors.file_io, session.take()).await?;
        return Err(AppError::BadRequest(
            "chunk body does not match Content-Length".to_string(),
        ));
    }
    let actual_chunk_hash =
        executors
            .cpu
            .finish_sha256_session_request(hash_session.take().ok_or_else(|| {
                AppError::Internal("backup hash session is unavailable".to_string())
            })?)
            .await?;
    if actual_chunk_hash != expected_chunk_hash {
        abort_backup_session(&executors.file_io, session.take()).await?;
        return Err(AppError::BadRequest(
            "chunk content hash does not match X-Content-SHA256".to_string(),
        ));
    }
    executors
        .file_io
        .commit_storage_session_request(
            session.take().ok_or_else(|| {
                AppError::Internal("backup file session is unavailable".to_string())
            })?,
        )
        .await?;
    Ok(())
}

async fn abort_backup_session(
    file_io: &FileIoExecutorHandle,
    session: Option<StorageFileSession>,
) -> AppResult<()> {
    if let Some(session) = session {
        file_io.abort_storage_session_request(session).await?;
    }
    Ok(())
}

async fn calculate_backup_hash(
    file_io: &FileIoExecutorHandle,
    cpu: &CpuExecutorHandle,
    staged_path: NormalizedStoragePath,
) -> AppResult<String> {
    let (opened_session, _) = file_io
        .open_storage_read_session_request(StorageRootId::Backups, staged_path)
        .await?;
    let mut session = Some(opened_session);
    let mut hash_session: Option<Sha256Session> = Some(cpu.start_sha256_session_request().await?);
    loop {
        let active_session = session.take().ok_or_else(|| {
            AppError::Internal("backup hash file session is unavailable".to_string())
        })?;
        let (returned_session, bytes) = file_io
            .read_storage_session_request(
                active_session,
                crate::runtime::FILE_IO_CHUNK_BYTES as usize,
            )
            .await?;
        session = Some(returned_session);
        if bytes.is_empty() {
            break;
        }
        let active_hash_session = hash_session
            .take()
            .ok_or_else(|| AppError::Internal("backup hash session is unavailable".to_string()))?;
        let (returned_hash_session, _) = cpu
            .update_sha256_session_request(active_hash_session, bytes)
            .await?;
        hash_session = Some(returned_hash_session);
    }
    file_io
        .close_storage_session_request(session.take().ok_or_else(|| {
            AppError::Internal("backup hash file session is unavailable".to_string())
        })?)
        .await?;
    Ok(cpu
        .finish_sha256_session_request(
            hash_session.take().ok_or_else(|| {
                AppError::Internal("backup hash session is unavailable".to_string())
            })?,
        )
        .await?)
}

fn validate_identifier(value: &str, name: &str) -> AppResult<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(AppError::BadRequest(format!(
            "{name} must contain 1 to 128 letters, numbers, hyphens, or underscores"
        )));
    }
    Ok(())
}

fn validate_device_name(value: &str) -> AppResult<()> {
    if value.trim().is_empty() || value.len() > 256 {
        return Err(AppError::BadRequest(
            "deviceName must contain 1 to 256 characters".to_string(),
        ));
    }
    Ok(())
}

fn validate_upload_metadata(
    request: &BackupUploadCreateRequest,
    maximum_upload_bytes: u64,
) -> AppResult<()> {
    if request.protocol_version != 2 {
        return Err(AppError::BadRequest(
            "protocolVersion must be 2".to_string(),
        ));
    }
    validate_content_hash(&request.content_hash, "contentHash")?;
    if !request.metadata.is_object() {
        return Err(AppError::BadRequest(
            "metadata must be an object".to_string(),
        ));
    }
    if request.byte_size == 0 || request.byte_size > maximum_upload_bytes {
        return Err(AppError::BadRequest(
            "byteSize exceeds the backup upload limit".to_string(),
        ));
    }
    let filename = FilePath::new(&request.original_filename);
    let Some(filename_text) = filename.file_name().and_then(|name| name.to_str()) else {
        return Err(AppError::BadRequest("invalid originalFilename".to_string()));
    };
    if filename_text != request.original_filename
        || filename_text.is_empty()
        || filename_text.len() > 255
        || filename_text.contains(['/', '\\'])
        || filename_text.chars().any(char::is_control)
    {
        return Err(AppError::BadRequest("invalid originalFilename".to_string()));
    }
    let extension = filename
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{}", value.to_ascii_lowercase()));
    if extension
        .as_deref()
        .is_none_or(|value| !SUPPORTED_EXTENSIONS.contains(value))
    {
        return Err(AppError::BadRequest(
            "unsupported media filename".to_string(),
        ));
    }
    let extension =
        extension.ok_or_else(|| AppError::BadRequest("invalid originalFilename".to_string()))?;
    if request.mime_type.len() > 127
        || !request.mime_type.contains('/')
        || request
            .mime_type
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(AppError::BadRequest("invalid mimeType".to_string()));
    }
    if (IMAGE_EXTENSIONS.contains(extension.as_str()) && !request.mime_type.starts_with("image/"))
        || (VIDEO_EXTENSIONS.contains(extension.as_str())
            && !request.mime_type.starts_with("video/"))
    {
        return Err(AppError::BadRequest(
            "mimeType does not match the media filename".to_string(),
        ));
    }
    let source_modified_at =
        DateTime::parse_from_rfc3339(&request.source_modified_at).map_err(|_| {
            AppError::BadRequest("sourceModifiedAt must be an RFC 3339 timestamp".to_string())
        })?;
    if source_modified_at.timestamp() < 0 {
        return Err(AppError::BadRequest(
            "sourceModifiedAt must not predate the Unix epoch".to_string(),
        ));
    }
    Ok(())
}

fn validate_content_hash(value: &str, name: &str) -> AppResult<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::BadRequest(format!(
            "{name} must be a 64-character SHA-256 hash"
        )));
    }
    if value.bytes().any(|byte| byte.is_ascii_uppercase()) {
        return Err(AppError::BadRequest(format!(
            "{name} must use lowercase hexadecimal"
        )));
    }
    Ok(())
}

fn content_hash_header(headers: &HeaderMap, name: &str) -> AppResult<String> {
    let value = headers
        .get(name)
        .and_then(|header_value| header_value.to_str().ok())
        .ok_or_else(|| AppError::BadRequest(format!("{name} is required")))?;
    validate_content_hash(value, name)?;
    Ok(value.to_string())
}

fn content_length(headers: &HeaderMap) -> AppResult<u64> {
    headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| AppError::BadRequest("Content-Length is required".to_string()))
}

fn content_range(headers: &HeaderMap) -> AppResult<(u64, u64, u64)> {
    let value = headers
        .get(header::CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| AppError::BadRequest("Content-Range is required".to_string()))?;
    parse_content_range(value)
}

fn parse_content_range(value: &str) -> AppResult<(u64, u64, u64)> {
    let value = value
        .strip_prefix("bytes ")
        .ok_or_else(|| AppError::BadRequest("invalid Content-Range".to_string()))?;
    let (range, total) = value
        .split_once('/')
        .ok_or_else(|| AppError::BadRequest("invalid Content-Range".to_string()))?;
    let (start, end) = range
        .split_once('-')
        .ok_or_else(|| AppError::BadRequest("invalid Content-Range".to_string()))?;
    let start = start
        .parse::<u64>()
        .map_err(|_| AppError::BadRequest("invalid Content-Range".to_string()))?;
    let end = end
        .parse::<u64>()
        .map_err(|_| AppError::BadRequest("invalid Content-Range".to_string()))?;
    let total = total
        .parse::<u64>()
        .map_err(|_| AppError::BadRequest("invalid Content-Range".to_string()))?;
    if total == 0 || end < start || end >= total {
        return Err(AppError::BadRequest("invalid Content-Range".to_string()));
    }
    Ok((start, end, total))
}
