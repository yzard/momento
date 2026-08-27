use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap},
    routing::{post, put},
    Json, Router,
};
use chrono::DateTime;
use futures::StreamExt;
use rusqlite::{OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::path::Path as FilePath;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

use crate::auth::{AppState, CurrentUser};
use crate::constants::{paths, IMAGE_EXTENSIONS, SUPPORTED_EXTENSIONS, VIDEO_EXTENSIONS};
use crate::database::queries;
use crate::error::{AppError, AppResult};
use crate::models::{
    BackupDeviceRegisterRequest, BackupDeviceRegisterResponse, BackupUploadCreateRequest,
    BackupUploadIdRequest, BackupUploadResponse,
};
use crate::utils::path::resolve_storage_path;

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
    Json(request): Json<BackupDeviceRegisterRequest>,
) -> AppResult<Json<BackupDeviceRegisterResponse>> {
    validate_identifier(&request.device_id, "deviceId")?;
    validate_device_name(&request.device_name)?;

    let connection = state.pool.get().map_err(AppError::Pool)?;
    connection.execute(
        queries::backup::UPSERT_DEVICE,
        rusqlite::params![
            current_user.id,
            request.device_id,
            request.device_name.trim()
        ],
    )?;

    Ok(Json(BackupDeviceRegisterResponse { registered: true }))
}

async fn create_upload(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<BackupUploadCreateRequest>,
) -> AppResult<Json<BackupUploadResponse>> {
    let config = state.config.current();
    validate_identifier(&request.device_id, "deviceId")?;
    validate_identifier(&request.client_asset_id, "clientAssetId")?;
    validate_identifier(&request.operation_id, "operationId")?;
    validate_upload_metadata(&request, config.backup.max_upload_bytes)?;

    let mut connection = state.pool.get().map_err(AppError::Pool)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;

    if let Some(upload) =
        select_upload_by_operation(&transaction, current_user.id, &request.operation_id)?
    {
        validate_idempotent_create(&transaction, current_user.id, &upload.upload_id, &request)?;
        let upgraded_upload =
            select_upload_by_operation(&transaction, current_user.id, &request.operation_id)?
                .ok_or_else(|| {
                    AppError::Internal("upgraded backup upload disappeared".to_string())
                })?;
        transaction.commit()?;
        return Ok(Json(upgraded_upload));
    }
    if let Some(upload) = select_upload_by_client_asset(
        &transaction,
        current_user.id,
        &request.device_id,
        &request.client_asset_id,
    )? {
        validate_idempotent_create(&transaction, current_user.id, &upload.upload_id, &request)?;
        let upgraded_upload = select_upload_by_client_asset(
            &transaction,
            current_user.id,
            &request.device_id,
            &request.client_asset_id,
        )?
        .ok_or_else(|| AppError::Internal("upgraded backup upload disappeared".to_string()))?;
        transaction.commit()?;
        return Ok(Json(upgraded_upload));
    }

    let device_exists: bool = transaction.query_row(
        queries::backup::DEVICE_EXISTS,
        rusqlite::params![current_user.id, request.device_id],
        |row| row.get(0),
    )?;
    if !device_exists {
        return Err(AppError::NotFound("backup device not found".to_string()));
    }

    let active_uploads: i64 = transaction.query_row(
        queries::backup::COUNT_ACTIVE_UPLOADS,
        [current_user.id],
        |row| row.get(0),
    )?;
    if active_uploads >= config.backup.max_active_uploads_per_user as i64 {
        return Err(AppError::Conflict(
            "maximum active backup uploads reached".to_string(),
        ));
    }

    let upload_id = uuid::Uuid::new_v4().simple().to_string();
    let extension = FilePath::new(&request.original_filename)
        .extension()
        .and_then(|value| value.to_str())
        .ok_or_else(|| AppError::BadRequest("invalid originalFilename".to_string()))?;
    let staged_path = format!(
        "{}/{}/{}.{}",
        current_user.id, request.device_id, upload_id, extension
    );
    transaction.execute(
        queries::backup::INSERT_ASSET,
        rusqlite::params![
            current_user.id,
            request.device_id,
            request.client_asset_id,
            request.operation_id,
            request.original_filename,
            request.mime_type,
            request.byte_size as i64,
            request.source_modified_at,
            staged_path,
        ],
    )?;
    let asset_id = transaction.last_insert_rowid();
    let expiry = format!("+{} hours", config.backup.session_expiry_hours);
    transaction.execute(
        queries::backup::INSERT_SESSION,
        rusqlite::params![
            upload_id,
            asset_id,
            current_user.id,
            request.byte_size as i64,
            expiry
        ],
    )?;
    transaction.execute(
        queries::backup::INSERT_MANIFEST,
        rusqlite::params![
            asset_id,
            request.protocol_version,
            request.content_hash,
            serde_json::to_string(&request.metadata)?,
        ],
    )?;
    transaction.commit()?;

    Ok(Json(BackupUploadResponse {
        upload_id,
        status: "uploading".to_string(),
        uploaded_size: 0,
        expected_size: request.byte_size as i64,
        content_hash: Some(request.content_hash),
        media_id: None,
        error: None,
    }))
}

async fn upload_status(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<BackupUploadIdRequest>,
) -> AppResult<Json<BackupUploadResponse>> {
    validate_identifier(&request.upload_id, "uploadId")?;
    Ok(Json(lookup_upload(
        &state,
        current_user.id,
        &request.upload_id,
    )?))
}

async fn complete_upload(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<BackupUploadIdRequest>,
) -> AppResult<Json<BackupUploadResponse>> {
    validate_identifier(&request.upload_id, "uploadId")?;
    let upload = {
        let connection = state.pool.get().map_err(AppError::Pool)?;
        select_upload(&connection, current_user.id, &request.upload_id)?
    };

    if matches!(
        upload.status.as_str(),
        "queued" | "processing" | "completed"
    ) {
        return Ok(Json(upload.response()));
    }
    if upload.status != "uploading" || upload.uploaded_size != upload.expected_size {
        return Err(AppError::Conflict(
            "backup upload is not ready to complete".to_string(),
        ));
    }
    let expected_content_hash = upload.content_hash.as_deref().ok_or_else(|| {
        AppError::Conflict("backup upload is missing a lossless manifest".to_string())
    })?;
    let staged_path = resolve_storage_path(&paths().backups, &upload.staged_path)?;
    let actual_content_hash = crate::utils::hash::calculate_file_hash(&staged_path).await?;
    if actual_content_hash != expected_content_hash {
        return Err(AppError::Conflict(
            "backup upload content hash does not match the original".to_string(),
        ));
    }

    let mut connection = state.pool.get().map_err(AppError::Pool)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let changed = transaction.execute(
        queries::backup::QUEUE_SESSION,
        rusqlite::params![request.upload_id, current_user.id],
    )?;
    if changed != 1 {
        return Err(AppError::Conflict(
            "backup upload changed concurrently".to_string(),
        ));
    }
    transaction.execute(queries::backup::QUEUE_ASSET, [upload.asset_id])?;
    transaction.commit()?;

    Ok(Json(lookup_upload(
        &state,
        current_user.id,
        &request.upload_id,
    )?))
}

async fn cancel_upload(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<BackupUploadIdRequest>,
) -> AppResult<Json<BackupUploadResponse>> {
    validate_identifier(&request.upload_id, "uploadId")?;
    let staged_path = {
        let mut connection = state.pool.get().map_err(AppError::Pool)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let upload = select_upload(&transaction, current_user.id, &request.upload_id)?;

        if upload.status == "cancelled" {
            transaction.commit()?;
            return Ok(Json(upload.response()));
        }
        if upload.session_status == "writing" {
            return Err(AppError::Conflict(
                "backup upload chunk is still being written".to_string(),
            ));
        }
        if !matches!(upload.status.as_str(), "uploading" | "queued")
            || !matches!(upload.session_status.as_str(), "uploading" | "queued")
        {
            return Err(AppError::Conflict(
                "backup upload can no longer be cancelled".to_string(),
            ));
        }

        let cancelled_session = transaction.execute(
            queries::backup::CANCEL_SESSION,
            rusqlite::params![request.upload_id, current_user.id],
        )?;
        if cancelled_session != 1 {
            return Err(AppError::Conflict(
                "backup upload changed concurrently".to_string(),
            ));
        }
        let cancelled_asset =
            transaction.execute(queries::backup::CANCEL_ASSET, [upload.asset_id])?;
        if cancelled_asset != 1 {
            return Err(AppError::Conflict(
                "backup upload changed concurrently".to_string(),
            ));
        }
        transaction.commit()?;
        upload.staged_path
    };

    remove_staged_file(&staged_path).await;
    Ok(Json(lookup_upload(
        &state,
        current_user.id,
        &request.upload_id,
    )?))
}

async fn upload_chunk(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Path(upload_id): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> AppResult<Json<BackupUploadResponse>> {
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

    let connection = state.pool.get().map_err(AppError::Pool)?;
    let upload = select_upload(&connection, current_user.id, &upload_id)?;
    if upload.status != "uploading"
        || upload.expected_size as u64 != total
        || upload.uploaded_size as u64 != start
    {
        return Err(AppError::Conflict(
            "upload offset or status does not accept this chunk".to_string(),
        ));
    }
    if end >= total || total > config.backup.max_upload_bytes {
        return Err(AppError::BadRequest("invalid Content-Range".to_string()));
    }

    let claimed = connection.execute(
        queries::backup::CLAIM_CHUNK,
        rusqlite::params![upload_id, current_user.id, start as i64],
    )?;
    if claimed != 1 {
        return Err(AppError::Conflict(
            "backup upload changed concurrently".to_string(),
        ));
    }

    let write_result = write_chunk(
        &upload.staged_path,
        start,
        declared_length,
        &expected_chunk_hash,
        body,
    )
    .await;
    if let Err(error) = write_result {
        let _ = connection.execute(
            queries::backup::ABANDON_CHUNK,
            rusqlite::params![upload_id, current_user.id],
        );
        return Err(error);
    }

    let changed = connection.execute(
        queries::backup::COMPLETE_CHUNK,
        rusqlite::params![end as i64 + 1, upload_id, current_user.id, start as i64],
    )?;
    if changed != 1 {
        return Err(AppError::Conflict(
            "backup upload changed while the chunk was written".to_string(),
        ));
    }

    Ok(Json(lookup_upload(&state, current_user.id, &upload_id)?))
}

async fn write_chunk(
    staged_path: &str,
    start: u64,
    declared_length: u64,
    expected_chunk_hash: &str,
    body: Body,
) -> AppResult<()> {
    let path = resolve_storage_path(&paths().backups, staged_path)?;
    let parent = path
        .parent()
        .ok_or_else(|| AppError::Internal("backup staging path has no parent".to_string()))?;
    tokio::fs::create_dir_all(parent).await?;
    let created_file = !path.exists();

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .await?;
    if created_file {
        sync_directory(parent)?;
    }
    file.set_len(start).await?;
    file.seek(std::io::SeekFrom::Start(start)).await?;

    let mut written = 0_u64;
    let mut hasher = Sha256::new();
    let mut stream = body.into_data_stream();
    while let Some(frame) = stream.next().await {
        let chunk =
            frame.map_err(|error| AppError::BadRequest(format!("invalid chunk body: {error}")))?;
        written = written
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| AppError::BadRequest("chunk body exceeds Content-Length".to_string()))?;
        if written > declared_length {
            file.set_len(start).await?;
            file.sync_data().await?;
            return Err(AppError::BadRequest(
                "chunk body exceeds Content-Length".to_string(),
            ));
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    if written != declared_length {
        file.set_len(start).await?;
        file.sync_data().await?;
        return Err(AppError::BadRequest(
            "chunk body does not match Content-Length".to_string(),
        ));
    }
    let actual_chunk_hash = format!("{:x}", hasher.finalize());
    if actual_chunk_hash != expected_chunk_hash {
        file.set_len(start).await?;
        file.sync_data().await?;
        return Err(AppError::BadRequest(
            "chunk content hash does not match X-Content-SHA256".to_string(),
        ));
    }
    file.sync_data().await?;
    Ok(())
}

fn sync_directory(path: &FilePath) -> AppResult<()> {
    std::fs::File::open(path)?.sync_all()?;
    Ok(())
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
    if serde_json::to_vec(&request.metadata)?.len() > 1024 * 1024 {
        return Err(AppError::BadRequest(
            "metadata exceeds the 1 MiB limit".to_string(),
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

struct UploadRow {
    asset_id: i64,
    upload_id: String,
    status: String,
    session_status: String,
    uploaded_size: i64,
    expected_size: i64,
    staged_path: String,
    content_hash: Option<String>,
    media_id: Option<i64>,
    error: Option<String>,
}

impl UploadRow {
    fn response(&self) -> BackupUploadResponse {
        BackupUploadResponse {
            upload_id: self.upload_id.clone(),
            status: self.status.clone(),
            uploaded_size: self.uploaded_size,
            expected_size: self.expected_size,
            content_hash: self.content_hash.clone(),
            media_id: self.media_id,
            error: self.error.clone(),
        }
    }
}

fn upload_response_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<BackupUploadResponse> {
    Ok(BackupUploadResponse {
        upload_id: row.get(0)?,
        status: row.get(1)?,
        uploaded_size: row.get(2)?,
        expected_size: row.get(3)?,
        content_hash: row.get(6)?,
        media_id: row.get(4)?,
        error: row.get(5)?,
    })
}

fn select_upload(
    connection: &rusqlite::Connection,
    user_id: i64,
    upload_id: &str,
) -> AppResult<UploadRow> {
    connection
        .query_row(
            queries::backup::SELECT_UPLOAD,
            rusqlite::params![upload_id, user_id],
            |row| {
                Ok(UploadRow {
                    asset_id: row.get(0)?,
                    upload_id: row.get(1)?,
                    status: row.get(2)?,
                    session_status: row.get(3)?,
                    uploaded_size: row.get(4)?,
                    expected_size: row.get(5)?,
                    staged_path: row.get(6)?,
                    media_id: row.get(7)?,
                    error: row.get(8)?,
                    content_hash: row.get(9)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound("backup upload not found".to_string()))
}

fn validate_idempotent_create(
    connection: &rusqlite::Connection,
    user_id: i64,
    upload_id: &str,
    request: &BackupUploadCreateRequest,
) -> AppResult<()> {
    let stored_contract = connection.query_row(
        queries::backup::SELECT_CREATE_CONTRACT,
        rusqlite::params![upload_id, user_id],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<u32>>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, Option<String>>(10)?,
            ))
        },
    )?;
    let request_metadata = serde_json::to_string(&request.metadata)?;
    let base_contract_matches = stored_contract.1 == request.device_id
        && stored_contract.2 == request.client_asset_id
        && stored_contract.3 == request.operation_id
        && stored_contract.4 == request.original_filename
        && stored_contract.5 == request.mime_type
        && stored_contract.6 == request.byte_size as i64
        && stored_contract.7.as_deref() == Some(request.source_modified_at.as_str());
    let manifest_is_missing =
        stored_contract.8.is_none() && stored_contract.9.is_none() && stored_contract.10.is_none();
    if base_contract_matches && manifest_is_missing {
        connection.execute(
            queries::backup::INSERT_MANIFEST,
            rusqlite::params![
                stored_contract.0,
                request.protocol_version,
                request.content_hash,
                request_metadata,
            ],
        )?;
        return Ok(());
    }
    let matches = base_contract_matches
        && stored_contract.8 == Some(request.protocol_version)
        && stored_contract.9.as_deref() == Some(request.content_hash.as_str())
        && stored_contract.10.as_deref() == Some(request_metadata.as_str());
    if !matches {
        return Err(AppError::Conflict(
            "idempotency key already belongs to a different backup manifest".to_string(),
        ));
    }
    Ok(())
}

fn select_upload_by_operation(
    connection: &rusqlite::Connection,
    user_id: i64,
    operation_id: &str,
) -> AppResult<Option<BackupUploadResponse>> {
    connection
        .query_row(
            queries::backup::SELECT_BY_OPERATION,
            rusqlite::params![user_id, operation_id],
            upload_response_from_row,
        )
        .optional()
        .map_err(AppError::from)
}

fn select_upload_by_client_asset(
    connection: &rusqlite::Connection,
    user_id: i64,
    device_id: &str,
    client_asset_id: &str,
) -> AppResult<Option<BackupUploadResponse>> {
    connection
        .query_row(
            queries::backup::SELECT_BY_CLIENT_ASSET,
            rusqlite::params![user_id, device_id, client_asset_id],
            upload_response_from_row,
        )
        .optional()
        .map_err(AppError::from)
}

fn lookup_upload(
    state: &AppState,
    user_id: i64,
    upload_id: &str,
) -> AppResult<BackupUploadResponse> {
    let connection = state.pool.get().map_err(AppError::Pool)?;
    Ok(select_upload(&connection, user_id, upload_id)?.response())
}

async fn remove_staged_file(staged_path: &str) {
    let Ok(path) = resolve_storage_path(&paths().backups, staged_path) else {
        tracing::warn!(staged_path, "invalid backup staging cleanup path");
        return;
    };
    match tokio::fs::remove_file(path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => tracing::warn!(staged_path, "backup staging cleanup failed: {error}"),
    }
}
