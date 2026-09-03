use axum::{
    body::Body,
    extract::Request,
    http::{header, HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use futures::StreamExt;
use http_body_util::BodyExt;
use percent_encoding::percent_decode_str;
use std::collections::VecDeque;
use tokio::sync::OwnedRwLockReadGuard;
use tracing::{debug, error, trace};

use crate::executor::{ExecutorErrorKind, StorageDirectoryEntryKind};
use crate::io::file::{NormalizedStoragePath, PathClaimMode, PathClaimScope, StorageRootId};
use crate::io::journal::{
    DirectoryCopyConstruction, DirectoryCopyConstructionPlan, DirectoryCopyCursor,
    DirectoryCopyEntryCheckpoint, DirectoryCopyFinishedCheckpoint, FileEntryAction, FileEntryPlan,
    FileOperationPlan, FilePathClaimPlan, JournalCheckpointOutcome, JournalSpaceReservationPlan,
    PrepareJournalOutcome,
};
use crate::io::{StorageFileSession, StorageFileSnapshot};
use crate::processor::artifact::prepare_artifact_publication;
use crate::routes::file_stream::{serve_file, ContentDisposition, FileResponseOptions};
use crate::runtime::{ExecutorHandles, HttpRequestAdmission};

const MINIMUM_JOURNAL_RESERVATION_BYTES: u64 = 4096;

#[derive(Debug, Clone, Copy)]
enum WebDavResourceSnapshot {
    File(StorageFileSnapshot),
    Directory,
}

impl WebDavResourceSnapshot {
    fn is_directory(&self) -> bool {
        matches!(self, Self::Directory)
    }

    fn byte_size(self) -> Option<u64> {
        match self {
            Self::File(snapshot) => Some(snapshot.byte_size),
            Self::Directory => None,
        }
    }

    fn identity_version(self) -> Option<String> {
        match self {
            Self::File(snapshot) => Some(snapshot.identity_version()),
            Self::Directory => None,
        }
    }
}

#[derive(Debug)]
enum WebDavMutationError {
    NotFound,
    Conflict,
    Unavailable(String),
    Internal(String),
}

pub fn validate_upload_size(
    method: &Method,
    headers: &HeaderMap,
    max_upload_bytes: u64,
) -> Result<(), StatusCode> {
    if !matches!(*method, Method::PUT | Method::PATCH) {
        return Ok(());
    }

    let declared_size = headers
        .get(header::CONTENT_LENGTH)
        .or_else(|| headers.get("x-expected-entity-length"))
        .map(|value| {
            value
                .to_str()
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or(StatusCode::BAD_REQUEST)
        })
        .transpose()?;
    let Some(declared_size) = declared_size else {
        return if *method == Method::PATCH {
            Err(StatusCode::LENGTH_REQUIRED)
        } else {
            Ok(())
        };
    };
    if declared_size > max_upload_bytes {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    Ok(())
}

pub fn request_mutates_staging(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
        && method.as_str() != "PROPFIND"
}

pub fn contains_reserved_path(path: &str) -> bool {
    let Ok(decoded_path) = percent_decode_str(path).decode_utf8() else {
        return true;
    };
    decoded_path
        .split('/')
        .any(|segment| segment == ".processing")
}

pub fn contains_reserved_destination(headers: &HeaderMap) -> bool {
    headers
        .get("destination")
        .and_then(|value| value.to_str().ok())
        .and_then(destination_request_path)
        .is_some_and(|path| contains_reserved_path(&path))
}

pub fn guard_response_body(
    response: Response,
    request_guard: OwnedRwLockReadGuard<()>,
) -> Response {
    let (parts, body) = response.into_parts();
    let guarded_body = body.map_frame(move |frame| {
        let _request_guard = &request_guard;
        frame
    });
    Response::from_parts(parts, Body::new(guarded_body))
}

pub async fn handle_webdav_request(
    executors: &ExecutorHandles,
    username: &str,
    admission: &HttpRequestAdmission,
    request: Request,
    mount_path: &str,
    maximum_upload_bytes: u64,
) -> Response {
    let method = request.method().clone();
    let request_path = request.uri().path().to_string();
    let content_length = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    trace!(
        method = %method,
        path = request_path,
        content_length,
        "WebDAV request"
    );

    let relative = match relative_webdav_path(mount_path, &request_path) {
        Ok(path) => path,
        Err(status) => return status.into_response(),
    };
    let storage_path = match user_storage_path(username, relative.as_deref()) {
        Ok(path) => path,
        Err(status) => return status.into_response(),
    };
    if method != Method::OPTIONS {
        if let Err(error) = ensure_webdav_user_root(executors, username).await {
            return mutation_error_response("WebDAV user root failed", error);
        }
    }

    let response = match method.as_str() {
        "OPTIONS" => options_response(),
        "GET" | "HEAD" => {
            let (parts, _) = request.into_parts();
            let filename = relative
                .as_deref()
                .and_then(|path| path.rsplit('/').next())
                .unwrap_or(username);
            let content_type = mime_guess::from_path(filename)
                .first_or_octet_stream()
                .to_string();
            serve_file(
                &executors.file_io,
                StorageRootId::WebDav,
                storage_path,
                FileResponseOptions {
                    admission,
                    content_type: &content_type,
                    headers: &parts.headers,
                    filename: Some(filename),
                    allow_ranges: true,
                    content_disposition: ContentDisposition::Inline,
                    cache_control: "private, no-cache",
                    head_only: method == Method::HEAD,
                },
            )
            .await
            .map(IntoResponse::into_response)
            .unwrap_or_else(IntoResponse::into_response)
        }
        "PUT" => {
            put_file(
                executors,
                storage_path,
                request.into_body(),
                content_length,
                maximum_upload_bytes,
            )
            .await
        }
        "PATCH" => {
            let (parts, body) = request.into_parts();
            patch_file(
                executors,
                storage_path,
                &parts.headers,
                body,
                maximum_upload_bytes,
            )
            .await
        }
        "COPY" => {
            let destination =
                match destination_storage_path(username, mount_path, request.headers()) {
                    Ok(path) => path,
                    Err(status) => return status.into_response(),
                };
            copy_resource(executors, storage_path, destination).await
        }
        "MOVE" => {
            let destination =
                match destination_storage_path(username, mount_path, request.headers()) {
                    Ok(path) => path,
                    Err(status) => return status.into_response(),
                };
            move_file(executors, storage_path, destination).await
        }
        "DELETE" => match retire_webdav_resource(executors, storage_path).await {
            Ok(()) => StatusCode::NO_CONTENT.into_response(),
            Err(WebDavMutationError::NotFound) => StatusCode::NOT_FOUND.into_response(),
            Err(error) => mutation_error_response("WebDAV delete failed", error),
        },
        "MKCOL" => match create_collection(executors, storage_path).await {
            Ok(()) => StatusCode::CREATED.into_response(),
            Err(WebDavMutationError::Conflict) => StatusCode::METHOD_NOT_ALLOWED.into_response(),
            Err(WebDavMutationError::NotFound) => StatusCode::CONFLICT.into_response(),
            Err(error) => mutation_error_response("WebDAV MKCOL failed", error),
        },
        "PROPFIND" => {
            propfind(
                executors,
                storage_path,
                mount_path,
                relative.as_deref(),
                request.headers(),
            )
            .await
        }
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    };

    if response.status().is_server_error() {
        error!(method = %method, path = request_path, status = %response.status(), "WebDAV request failed");
    } else {
        debug!(method = %method, path = request_path, status = %response.status(), "WebDAV response");
    }
    response
}

async fn put_file(
    executors: &ExecutorHandles,
    destination: NormalizedStoragePath,
    body: Body,
    declared_length: Option<u64>,
    maximum_upload_bytes: u64,
) -> Response {
    let maximum_bytes = declared_length
        .unwrap_or(maximum_upload_bytes)
        .max(MINIMUM_JOURNAL_RESERVATION_BYTES);
    let publication = match prepare_artifact_publication(
        executors,
        StorageRootId::WebDav,
        destination,
        maximum_bytes,
        "webdav_put",
        crate::processor::artifact::ArtifactPublicationOwner::JournalGroup,
    )
    .await
    {
        Ok(publication) => publication,
        Err(error) => return internal_error("WebDAV PUT admission failed", error),
    };
    match stream_body_to_publication(
        executors,
        publication,
        body,
        0,
        declared_length,
        maximum_upload_bytes,
    )
    .await
    {
        Ok(()) => StatusCode::CREATED.into_response(),
        Err((StatusCode::PAYLOAD_TOO_LARGE, _)) => StatusCode::PAYLOAD_TOO_LARGE.into_response(),
        Err((status, error)) => internal_error_with_status(status, "WebDAV PUT failed", error),
    }
}

async fn patch_file(
    executors: &ExecutorHandles,
    destination: NormalizedStoragePath,
    headers: &HeaderMap,
    body: Body,
    maximum_upload_bytes: u64,
) -> Response {
    let Some((start, end, total)) = headers
        .get(header::CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_range)
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let declared_length = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    if total > maximum_upload_bytes
        || end < start
        || end
            .checked_sub(start)
            .and_then(|value| value.checked_add(1))
            != declared_length
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let (source_session, source_snapshot) = match executors
        .file_io
        .open_storage_read_session_request(StorageRootId::WebDav, destination.clone())
        .await
    {
        Ok(opened) => opened,
        Err(error) if error.kind == ExecutorErrorKind::FileNotFound => {
            return StatusCode::NOT_FOUND.into_response()
        }
        Err(error) => return internal_error("WebDAV PATCH source failed", error.to_string()),
    };
    if source_snapshot.byte_size != start {
        drop(source_session);
        return StatusCode::CONFLICT.into_response();
    }
    let publication = match prepare_artifact_publication(
        executors,
        StorageRootId::WebDav,
        destination,
        total,
        "webdav_patch",
        crate::processor::artifact::ArtifactPublicationOwner::JournalGroup,
    )
    .await
    {
        Ok(publication) => publication,
        Err(error) => {
            drop(source_session);
            return internal_error("WebDAV PATCH admission failed", error);
        }
    };
    let temporary_path = publication.temporary_path().clone();
    let output_session = match executors
        .file_io
        .open_storage_write_session_durable(StorageRootId::WebDav, temporary_path, 0)
        .await
    {
        Ok(session) => session,
        Err(error) => {
            drop(source_session);
            publication.cancel(executors).await;
            return internal_error("WebDAV PATCH temporary failed", error.to_string());
        }
    };
    let copy_result = copy_session_bytes(
        &executors.file_io,
        &executors.scheduler,
        source_session,
        output_session,
        start,
    )
    .await;
    let (input_session, output_session) = match copy_result {
        Ok(sessions) => sessions,
        Err(error) => {
            publication.cancel(executors).await;
            return internal_error("WebDAV PATCH prefix copy failed", error);
        }
    };
    drop(input_session);
    stream_body_into_open_publication(
        executors,
        publication,
        output_session,
        body,
        start,
        declared_length,
        maximum_upload_bytes,
    )
    .await
    .map(|()| StatusCode::NO_CONTENT.into_response())
    .unwrap_or_else(|(status, error)| {
        internal_error_with_status(status, "WebDAV PATCH failed", error)
    })
}

async fn copy_file(
    executors: &ExecutorHandles,
    source: NormalizedStoragePath,
    destination: NormalizedStoragePath,
) -> Response {
    let (source_session, snapshot) = match executors
        .file_io
        .open_storage_read_session_request(StorageRootId::WebDav, source)
        .await
    {
        Ok(opened) => opened,
        Err(error) if error.kind == ExecutorErrorKind::FileNotFound => {
            return StatusCode::NOT_FOUND.into_response()
        }
        Err(error) => return internal_error("WebDAV COPY source failed", error.to_string()),
    };
    let publication = match prepare_artifact_publication(
        executors,
        StorageRootId::WebDav,
        destination,
        snapshot.byte_size.max(MINIMUM_JOURNAL_RESERVATION_BYTES),
        "webdav_copy",
        crate::processor::artifact::ArtifactPublicationOwner::JournalGroup,
    )
    .await
    {
        Ok(publication) => publication,
        Err(error) => {
            drop(source_session);
            return internal_error("WebDAV COPY admission failed", error);
        }
    };
    let output_session = match executors
        .file_io
        .open_storage_write_session_durable(
            StorageRootId::WebDav,
            publication.temporary_path().clone(),
            0,
        )
        .await
    {
        Ok(session) => session,
        Err(error) => {
            drop(source_session);
            publication.cancel(executors).await;
            return internal_error("WebDAV COPY temporary failed", error.to_string());
        }
    };
    let (source_session, output_session) = match copy_session_bytes(
        &executors.file_io,
        &executors.scheduler,
        source_session,
        output_session,
        snapshot.byte_size,
    )
    .await
    {
        Ok(sessions) => sessions,
        Err(error) => {
            publication.cancel(executors).await;
            return internal_error("WebDAV COPY transfer failed", error);
        }
    };
    drop(source_session);
    if let Err(error) = executors
        .file_io
        .commit_storage_session_durable(output_session)
        .await
    {
        publication.cancel(executors).await;
        return internal_error("WebDAV COPY sync failed", error.to_string());
    }
    match publication.publish(executors).await {
        Ok(()) => StatusCode::CREATED.into_response(),
        Err(error) => internal_error("WebDAV COPY publication failed", error),
    }
}

async fn copy_resource(
    executors: &ExecutorHandles,
    source: NormalizedStoragePath,
    destination: NormalizedStoragePath,
) -> Response {
    match inspect_webdav_resource(executors, source.clone()).await {
        Ok(WebDavResourceSnapshot::File(_)) => copy_file(executors, source, destination).await,
        Ok(WebDavResourceSnapshot::Directory) => {
            match copy_directory(executors, source, destination).await {
                Ok(()) => StatusCode::CREATED.into_response(),
                Err(error) => mutation_error_response("WebDAV directory COPY failed", error),
            }
        }
        Err(error) => mutation_error_response("WebDAV COPY source failed", error),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DirectoryTreeMeasurement {
    file_bytes: u64,
    entry_count: u64,
    fingerprint: [u8; 32],
}

struct DirectoryWalkFrame {
    session: Option<StorageFileSession>,
    source: NormalizedStoragePath,
    pending: VecDeque<crate::executor::StorageDirectoryEntry>,
    finished: bool,
}

async fn copy_directory(
    executors: &ExecutorHandles,
    source: NormalizedStoragePath,
    destination: NormalizedStoragePath,
) -> Result<(), WebDavMutationError> {
    if path_is_equal_or_descendant(&destination, &source) {
        return Err(WebDavMutationError::Conflict);
    }
    let measurement = measure_directory_tree(executors, source.clone()).await?;
    let existing_destination = match inspect_webdav_resource(executors, destination.clone()).await {
        Ok(snapshot) => Some(snapshot),
        Err(WebDavMutationError::NotFound) => None,
        Err(error) => return Err(error),
    };
    let operation_id = uuid::Uuid::new_v4();
    let group_id = format!("webdav-copy-{operation_id}");
    let temporary = sibling_path(&destination, &format!(".momento-copy-{operation_id}.tmp"));
    let tombstone = sibling_path(
        &destination,
        &format!(".momento-copy-{operation_id}.replaced"),
    );
    let metadata_bytes = measurement
        .entry_count
        .checked_add(1)
        .and_then(|count| count.checked_mul(MINIMUM_JOURNAL_RESERVATION_BYTES))
        .ok_or_else(|| WebDavMutationError::Internal("directory COPY size overflowed".into()))?;
    let reservation_bytes = measurement
        .file_bytes
        .checked_add(metadata_bytes)
        .ok_or_else(|| WebDavMutationError::Internal("directory COPY size overflowed".into()))?
        .max(MINIMUM_JOURNAL_RESERVATION_BYTES);
    let reservation = executors
        .file_io
        .reserve_journal_space(group_id.clone(), reservation_bytes)
        .map_err(|error| WebDavMutationError::Unavailable(error.to_string()))?
        .into_result()
        .map_err(|error| WebDavMutationError::Unavailable(error.to_string()))?;
    let mut entries = Vec::with_capacity(if existing_destination.is_some() { 3 } else { 1 });
    if let Some(snapshot) = existing_destination {
        entries.push(FileEntryPlan {
            action: FileEntryAction::Tombstone,
            storage_root: StorageRootId::WebDav,
            source_path: Some(destination.clone()),
            temporary_path: None,
            destination_path: None,
            tombstone_path: Some(tombstone.clone()),
            expected_size: snapshot.byte_size(),
            expected_sha256: None,
            expected_version: snapshot.identity_version(),
        });
    }
    entries.push(FileEntryPlan {
        action: FileEntryAction::Publish,
        storage_root: StorageRootId::WebDav,
        source_path: None,
        temporary_path: Some(temporary.clone()),
        destination_path: Some(destination.clone()),
        tombstone_path: None,
        expected_size: None,
        expected_sha256: None,
        expected_version: None,
    });
    if existing_destination.is_some() {
        entries.push(FileEntryPlan {
            action: FileEntryAction::Cleanup,
            storage_root: StorageRootId::WebDav,
            source_path: Some(tombstone.clone()),
            temporary_path: None,
            destination_path: None,
            tombstone_path: None,
            expected_size: None,
            expected_sha256: None,
            expected_version: None,
        });
    }
    let mut claims = vec![
        resource_read_claim(source.clone(), "copy_source"),
        resource_write_claim(temporary.clone(), true, "copy_temporary"),
        resource_write_claim(destination, true, "copy_destination"),
    ];
    if existing_destination.is_some() {
        claims.push(resource_write_claim(
            tombstone,
            existing_destination
                .as_ref()
                .is_some_and(WebDavResourceSnapshot::is_directory),
            "copy_replaced_destination",
        ));
    }
    let plan = FileOperationPlan {
        group_id: group_id.clone(),
        kind: "webdav_directory_copy".to_string(),
        owner_kind: "webdav".to_string(),
        owner_id: operation_id.to_string(),
        claim_token: None,
        product_target: None,
        product_version: None,
        entries,
        claims,
        space_reservation: Some(
            JournalSpaceReservationPlan::new(reservation)
                .map_err(|error| WebDavMutationError::Internal(error.to_string()))?,
        ),
    };
    let construction = DirectoryCopyConstructionPlan {
        storage_root: StorageRootId::WebDav,
        source_root: source,
        temporary_root: temporary,
        expected_file_bytes: measurement.file_bytes,
        expected_entry_count: measurement.entry_count,
        expected_fingerprint: measurement.fingerprint,
    };
    if executors
        .sqlite
        .prepare_directory_copy_operation_durable(plan, construction)
        .await
        .map_err(map_executor_error)?
        == PrepareJournalOutcome::PathConflict
    {
        return Err(WebDavMutationError::Conflict);
    }
    if let Err(error) = resume_directory_copy(executors, Some(group_id.clone())).await {
        cancel_mutation_group(executors, group_id).await?;
        return Err(error);
    }
    Ok(())
}

async fn measure_directory_tree(
    executors: &ExecutorHandles,
    source: NormalizedStoragePath,
) -> Result<DirectoryTreeMeasurement, WebDavMutationError> {
    let session = executors
        .file_io
        .open_storage_directory_session_durable(StorageRootId::WebDav, Some(source.clone()))
        .await
        .map_err(map_executor_error)?;
    let mut stack = vec![DirectoryWalkFrame {
        session: Some(session),
        source,
        pending: VecDeque::new(),
        finished: false,
    }];
    let mut measurement = DirectoryTreeMeasurement {
        file_bytes: 0,
        entry_count: 0,
        fingerprint: [0; 32],
    };
    while let Some(frame) = stack.last_mut() {
        if let Some(entry) = frame.pending.pop_front() {
            let child_source = child_storage_path(&frame.source, &entry.name)?;
            match entry.kind {
                StorageDirectoryEntryKind::Directory => {
                    add_directory_measurement(executors, &mut measurement, &child_source).await?;
                    let session = executors
                        .file_io
                        .open_storage_directory_session_durable(
                            StorageRootId::WebDav,
                            Some(child_source.clone()),
                        )
                        .await
                        .map_err(map_executor_error)?;
                    stack.push(DirectoryWalkFrame {
                        session: Some(session),
                        source: child_source,
                        pending: VecDeque::new(),
                        finished: false,
                    });
                }
                StorageDirectoryEntryKind::File => {
                    let (session, snapshot) = executors
                        .file_io
                        .open_storage_read_session_durable(
                            StorageRootId::WebDav,
                            child_source.clone(),
                        )
                        .await
                        .map_err(map_executor_error)?;
                    executors
                        .file_io
                        .close_storage_session_durable(session)
                        .await
                        .map_err(map_executor_error)?;
                    add_file_measurement(executors, &mut measurement, &child_source, snapshot)
                        .await?;
                }
            }
            continue;
        }
        if frame.finished {
            let session = frame.session.take().ok_or_else(|| {
                WebDavMutationError::Internal("directory measurement session is missing".into())
            })?;
            executors
                .file_io
                .close_storage_session_durable(session)
                .await
                .map_err(map_executor_error)?;
            stack.pop();
            continue;
        }
        let session = frame.session.take().ok_or_else(|| {
            WebDavMutationError::Internal("directory measurement session is missing".into())
        })?;
        let (session, entries, finished) = executors
            .file_io
            .read_storage_directory_session_durable(session)
            .await
            .map_err(map_executor_error)?;
        let frame = stack
            .last_mut()
            .expect("directory measurement frame remains while read is awaited");
        frame.session = Some(session);
        frame.pending = entries.into();
        frame.finished = finished;
    }
    Ok(measurement)
}

pub async fn resume_prepared_directory_copies_after_restart(
    executors: &ExecutorHandles,
) -> Result<usize, String> {
    let mut resumed = 0_usize;
    loop {
        let construction = executors
            .sqlite
            .load_directory_copy_durable(None)
            .await
            .map_err(|error| error.to_string())?;
        let Some(construction) = construction else {
            return Ok(resumed);
        };
        let group_id = construction.group_id.clone();
        match resume_directory_copy(executors, Some(group_id.clone())).await {
            Ok(()) => {}
            Err(WebDavMutationError::Unavailable(error)) => {
                return Err(format!(
                    "directory COPY {group_id} is temporarily unavailable: {error}"
                ));
            }
            Err(error) => cancel_mutation_group(executors, group_id.clone())
                .await
                .map_err(|cancel_error| {
                    format!(
                        "directory COPY {group_id} failed ({error:?}) and rollback could not be scheduled: {cancel_error:?}"
                    )
                })?,
        }
        resumed = resumed
            .checked_add(1)
            .ok_or_else(|| "resumed directory COPY count overflowed".to_string())?;
    }
}

async fn resume_directory_copy(
    executors: &ExecutorHandles,
    group_id: Option<String>,
) -> Result<(), WebDavMutationError> {
    loop {
        let construction = executors
            .sqlite
            .load_directory_copy_durable(group_id.clone())
            .await
            .map_err(map_executor_error)?
            .ok_or(WebDavMutationError::Conflict)?;
        if construction.complete {
            return execute_prepared_mutation(
                executors,
                construction.group_id,
                construction.publication_entry_count,
                construction.has_cleanup,
            )
            .await;
        }
        advance_directory_copy(executors, &construction).await?;
    }
}

async fn advance_directory_copy(
    executors: &ExecutorHandles,
    construction: &DirectoryCopyConstruction,
) -> Result<(), WebDavMutationError> {
    let cursor =
        construction.cursors.last().cloned().ok_or_else(|| {
            WebDavMutationError::Internal("directory COPY cursor is missing".into())
        })?;
    ensure_webdav_directory(executors, cursor.temporary_path.clone()).await?;
    let mut directory_session = executors
        .file_io
        .open_storage_directory_session_durable(
            construction.storage_root,
            Some(cursor.source_path.clone()),
        )
        .await
        .map_err(map_executor_error)?;
    if cursor.resume_offset > 0 {
        directory_session = executors
            .file_io
            .seek_storage_read_session_durable(directory_session, cursor.resume_offset)
            .await
            .map_err(map_executor_error)?;
    }
    let (directory_session, entry) = loop {
        let (returned_session, entries, finished) = executors
            .file_io
            .read_storage_directory_session_durable(directory_session)
            .await
            .map_err(map_executor_error)?;
        if let Some(entry) = entries.into_iter().next() {
            break (returned_session, entry);
        }
        if finished {
            executors
                .file_io
                .commit_storage_session_durable(returned_session)
                .await
                .map_err(map_executor_error)?;
            let changed = executors
                .sqlite
                .checkpoint_directory_copy_finished_durable(DirectoryCopyFinishedCheckpoint {
                    group_id: construction.group_id.clone(),
                    depth: cursor.depth,
                    expected_resume_offset: cursor.resume_offset,
                })
                .await
                .map_err(map_executor_error)?;
            return if changed {
                Ok(())
            } else {
                Err(WebDavMutationError::Conflict)
            };
        }
        directory_session = returned_session;
    };
    executors
        .file_io
        .close_storage_session_durable(directory_session)
        .await
        .map_err(map_executor_error)?;
    let child_source = child_storage_path(&cursor.source_path, &entry.name)?;
    let child_temporary = child_storage_path(&cursor.temporary_path, &entry.name)?;
    let (file_bytes, fingerprint, child) = match entry.kind {
        StorageDirectoryEntryKind::Directory => {
            ensure_webdav_directory(executors, child_temporary.clone()).await?;
            let fingerprint = tree_entry_fingerprint(executors, b'd', &child_source, None).await?;
            let child_depth = cursor.depth.checked_add(1).ok_or_else(|| {
                WebDavMutationError::Internal("directory COPY depth overflowed".into())
            })?;
            (
                0,
                fingerprint,
                Some(DirectoryCopyCursor {
                    depth: child_depth,
                    source_path: child_source,
                    temporary_path: child_temporary,
                    resume_offset: 0,
                }),
            )
        }
        StorageDirectoryEntryKind::File => {
            let (source_session, snapshot) = executors
                .file_io
                .open_storage_read_session_durable(construction.storage_root, child_source.clone())
                .await
                .map_err(map_executor_error)?;
            let fingerprint = tree_entry_fingerprint(
                executors,
                b'f',
                &child_source,
                Some(snapshot.identity_version()),
            )
            .await?;
            let output_session = executors
                .file_io
                .open_storage_write_session_durable(construction.storage_root, child_temporary, 0)
                .await
                .map_err(map_executor_error)?;
            let (source_session, output_session) = copy_session_bytes(
                &executors.file_io,
                &executors.scheduler,
                source_session,
                output_session,
                snapshot.byte_size,
            )
            .await
            .map_err(WebDavMutationError::Unavailable)?;
            executors
                .file_io
                .close_storage_session_durable(source_session)
                .await
                .map_err(map_executor_error)?;
            executors
                .file_io
                .commit_storage_session_durable(output_session)
                .await
                .map_err(map_executor_error)?;
            (snapshot.byte_size, fingerprint, None)
        }
    };
    let changed = executors
        .sqlite
        .checkpoint_directory_copy_entry_durable(DirectoryCopyEntryCheckpoint {
            group_id: construction.group_id.clone(),
            depth: cursor.depth,
            expected_resume_offset: cursor.resume_offset,
            next_resume_offset: entry.resume_offset,
            file_bytes,
            fingerprint,
            child,
        })
        .await
        .map_err(map_executor_error)?;
    if changed {
        Ok(())
    } else {
        Err(WebDavMutationError::Conflict)
    }
}

async fn ensure_webdav_directory(
    executors: &ExecutorHandles,
    path: NormalizedStoragePath,
) -> Result<(), WebDavMutationError> {
    match executors
        .file_io
        .create_storage_directory_durable(StorageRootId::WebDav, path.clone())
        .await
    {
        Ok(()) => Ok(()),
        Err(error) if error.kind == ExecutorErrorKind::FileConflict => {
            match inspect_webdav_resource(executors, path).await? {
                WebDavResourceSnapshot::Directory => Ok(()),
                WebDavResourceSnapshot::File(_) => Err(WebDavMutationError::Conflict),
            }
        }
        Err(error) => Err(map_executor_error(error)),
    }
}

async fn add_directory_measurement(
    executors: &ExecutorHandles,
    measurement: &mut DirectoryTreeMeasurement,
    path: &NormalizedStoragePath,
) -> Result<(), WebDavMutationError> {
    add_tree_measurement(executors, measurement, b'd', path, None).await
}

async fn add_file_measurement(
    executors: &ExecutorHandles,
    measurement: &mut DirectoryTreeMeasurement,
    path: &NormalizedStoragePath,
    snapshot: StorageFileSnapshot,
) -> Result<(), WebDavMutationError> {
    measurement.file_bytes = measurement
        .file_bytes
        .checked_add(snapshot.byte_size)
        .ok_or_else(|| WebDavMutationError::Internal("directory byte size overflowed".into()))?;
    add_tree_measurement(
        executors,
        measurement,
        b'f',
        path,
        Some(snapshot.identity_version()),
    )
    .await
}

async fn add_tree_measurement(
    executors: &ExecutorHandles,
    measurement: &mut DirectoryTreeMeasurement,
    kind: u8,
    path: &NormalizedStoragePath,
    identity: Option<String>,
) -> Result<(), WebDavMutationError> {
    measurement.entry_count = measurement
        .entry_count
        .checked_add(1)
        .ok_or_else(|| WebDavMutationError::Internal("directory entry count overflowed".into()))?;
    let digest = tree_entry_fingerprint(executors, kind, path, identity).await?;
    for (accumulator, byte) in measurement.fingerprint.iter_mut().zip(digest) {
        *accumulator ^= byte;
    }
    Ok(())
}

async fn tree_entry_fingerprint(
    executors: &ExecutorHandles,
    kind: u8,
    path: &NormalizedStoragePath,
    identity: Option<String>,
) -> Result<[u8; 32], WebDavMutationError> {
    let identity = identity.unwrap_or_default();
    let mut record = Vec::new();
    record
        .try_reserve_exact(1 + path.relative_path().len() + identity.len() + 2)
        .map_err(|error| WebDavMutationError::Internal(error.to_string()))?;
    record.push(kind);
    record.extend_from_slice(path.relative_path().as_bytes());
    record.push(0);
    record.extend_from_slice(identity.as_bytes());
    executors
        .cpu
        .sha256_durable(record)
        .await
        .map_err(map_executor_error)
}

fn child_storage_path(
    parent: &NormalizedStoragePath,
    child: &str,
) -> Result<NormalizedStoragePath, WebDavMutationError> {
    NormalizedStoragePath::parse(&format!("{}/{child}", parent.relative_path()))
        .map_err(|error| WebDavMutationError::Internal(error.to_string()))
}

async fn move_file(
    executors: &ExecutorHandles,
    source: NormalizedStoragePath,
    destination: NormalizedStoragePath,
) -> Response {
    if source == destination {
        return StatusCode::NO_CONTENT.into_response();
    }
    let source_snapshot = match inspect_webdav_resource(executors, source.clone()).await {
        Ok(snapshot) => snapshot,
        Err(WebDavMutationError::NotFound) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return mutation_error_response("WebDAV MOVE source failed", error),
    };
    let existing_destination = match inspect_webdav_resource(executors, destination.clone()).await {
        Ok(snapshot) => Some(snapshot),
        Err(WebDavMutationError::NotFound) => None,
        Err(error) => return mutation_error_response("WebDAV MOVE destination failed", error),
    };
    if source_snapshot.is_directory() && path_is_equal_or_descendant(&destination, &source) {
        return StatusCode::CONFLICT.into_response();
    }
    let operation_id = uuid::Uuid::new_v4();
    let group_id = format!("webdav-move-{operation_id}");
    let tombstone = sibling_path(&destination, &format!(".momento-move-{operation_id}"));
    let mut entries = Vec::with_capacity(if existing_destination.is_some() { 3 } else { 1 });
    if let Some(snapshot) = existing_destination {
        entries.push(FileEntryPlan {
            action: FileEntryAction::Tombstone,
            storage_root: StorageRootId::WebDav,
            source_path: Some(destination.clone()),
            temporary_path: None,
            destination_path: None,
            tombstone_path: Some(tombstone.clone()),
            expected_size: snapshot.byte_size(),
            expected_sha256: None,
            expected_version: snapshot.identity_version(),
        });
    }
    entries.push(FileEntryPlan {
        action: FileEntryAction::Move,
        storage_root: StorageRootId::WebDav,
        source_path: Some(source.clone()),
        temporary_path: None,
        destination_path: Some(destination.clone()),
        tombstone_path: None,
        expected_size: source_snapshot.byte_size(),
        expected_sha256: None,
        expected_version: source_snapshot.identity_version(),
    });
    if existing_destination.is_some() {
        entries.push(FileEntryPlan {
            action: FileEntryAction::Cleanup,
            storage_root: StorageRootId::WebDav,
            source_path: Some(tombstone.clone()),
            temporary_path: None,
            destination_path: None,
            tombstone_path: None,
            expected_size: None,
            expected_sha256: None,
            expected_version: None,
        });
    }
    let mut claims = vec![
        resource_write_claim(source, source_snapshot.is_directory(), "source"),
        resource_write_claim(
            destination,
            source_snapshot.is_directory()
                || existing_destination
                    .as_ref()
                    .is_some_and(WebDavResourceSnapshot::is_directory),
            "destination",
        ),
    ];
    if existing_destination.is_some() {
        claims.push(resource_write_claim(
            tombstone,
            existing_destination
                .as_ref()
                .is_some_and(WebDavResourceSnapshot::is_directory),
            "replaced_destination",
        ));
    }
    let publication_entries = if existing_destination.is_some() { 2 } else { 1 };
    match execute_mutation_plan(
        executors,
        FileOperationPlan {
            group_id,
            kind: "webdav_move".to_string(),
            owner_kind: "webdav".to_string(),
            owner_id: operation_id.to_string(),
            claim_token: None,
            product_target: None,
            product_version: None,
            entries,
            claims,
            space_reservation: None,
        },
        publication_entries,
        existing_destination.is_some(),
    )
    .await
    {
        Ok(()) => StatusCode::CREATED.into_response(),
        Err(error) => mutation_error_response("WebDAV MOVE failed", error),
    }
}

async fn inspect_webdav_resource(
    executors: &ExecutorHandles,
    path: NormalizedStoragePath,
) -> Result<WebDavResourceSnapshot, WebDavMutationError> {
    match executors
        .file_io
        .open_storage_read_session_request(StorageRootId::WebDav, path.clone())
        .await
    {
        Ok((session, snapshot)) => {
            executors
                .file_io
                .close_storage_session_request(session)
                .await
                .map_err(map_executor_error)?;
            return Ok(WebDavResourceSnapshot::File(snapshot));
        }
        Err(error)
            if matches!(
                error.kind,
                ExecutorErrorKind::FileInvalidData | ExecutorErrorKind::FileSystem
            ) => {}
        Err(error) if error.kind == ExecutorErrorKind::FileNotFound => {}
        Err(error) => return Err(map_executor_error(error)),
    }
    match executors
        .file_io
        .open_storage_directory_session_request(StorageRootId::WebDav, Some(path))
        .await
    {
        Ok(session) => {
            executors
                .file_io
                .close_storage_session_request(session)
                .await
                .map_err(map_executor_error)?;
            Ok(WebDavResourceSnapshot::Directory)
        }
        Err(error) if error.kind == ExecutorErrorKind::FileNotFound => {
            Err(WebDavMutationError::NotFound)
        }
        Err(error) => Err(map_executor_error(error)),
    }
}

async fn ensure_webdav_user_root(
    executors: &ExecutorHandles,
    username: &str,
) -> Result<(), WebDavMutationError> {
    let path = user_storage_path(username, None)
        .map_err(|_| WebDavMutationError::Internal("username is not a storage component".into()))?;
    match executors
        .file_io
        .create_storage_directory_durable(StorageRootId::WebDav, path.clone())
        .await
    {
        Ok(()) => Ok(()),
        Err(error) if error.kind == ExecutorErrorKind::FileConflict => {
            match inspect_webdav_resource(executors, path).await? {
                WebDavResourceSnapshot::Directory => Ok(()),
                WebDavResourceSnapshot::File(_) => Err(WebDavMutationError::Conflict),
            }
        }
        Err(error) => Err(map_executor_error(error)),
    }
}

async fn retire_webdav_resource(
    executors: &ExecutorHandles,
    source: NormalizedStoragePath,
) -> Result<(), WebDavMutationError> {
    let snapshot = inspect_webdav_resource(executors, source.clone()).await?;
    let operation_id = uuid::Uuid::new_v4();
    let group_id = format!("webdav-delete-{operation_id}");
    let tombstone = sibling_path(&source, &format!(".momento-delete-{operation_id}"));
    execute_mutation_plan(
        executors,
        FileOperationPlan {
            group_id,
            kind: "webdav_delete".to_string(),
            owner_kind: "webdav".to_string(),
            owner_id: operation_id.to_string(),
            claim_token: None,
            product_target: None,
            product_version: None,
            entries: vec![
                FileEntryPlan {
                    action: FileEntryAction::Tombstone,
                    storage_root: StorageRootId::WebDav,
                    source_path: Some(source.clone()),
                    temporary_path: None,
                    destination_path: None,
                    tombstone_path: Some(tombstone.clone()),
                    expected_size: snapshot.byte_size(),
                    expected_sha256: None,
                    expected_version: snapshot.identity_version(),
                },
                FileEntryPlan {
                    action: FileEntryAction::Cleanup,
                    storage_root: StorageRootId::WebDav,
                    source_path: Some(tombstone.clone()),
                    temporary_path: None,
                    destination_path: None,
                    tombstone_path: None,
                    expected_size: snapshot.byte_size(),
                    expected_sha256: None,
                    expected_version: None,
                },
            ],
            claims: vec![
                resource_write_claim(source, snapshot.is_directory(), "delete_source"),
                resource_write_claim(tombstone, snapshot.is_directory(), "delete_tombstone"),
            ],
            space_reservation: None,
        },
        1,
        true,
    )
    .await
}

async fn create_collection(
    executors: &ExecutorHandles,
    destination: NormalizedStoragePath,
) -> Result<(), WebDavMutationError> {
    match inspect_webdav_resource(executors, destination.clone()).await {
        Ok(_) => return Err(WebDavMutationError::Conflict),
        Err(WebDavMutationError::NotFound) => {}
        Err(error) => return Err(error),
    }
    let operation_id = uuid::Uuid::new_v4();
    let group_id = format!("webdav-mkcol-{operation_id}");
    let temporary = sibling_path(
        &destination,
        &format!(".momento-collection-{operation_id}.tmp"),
    );
    let reservation = executors
        .file_io
        .reserve_journal_space(group_id.clone(), MINIMUM_JOURNAL_RESERVATION_BYTES)
        .map_err(|error| WebDavMutationError::Unavailable(error.to_string()))?
        .into_result()
        .map_err(|error| WebDavMutationError::Unavailable(error.to_string()))?;
    let plan = FileOperationPlan {
        group_id: group_id.clone(),
        kind: "webdav_mkcol".to_string(),
        owner_kind: "webdav".to_string(),
        owner_id: operation_id.to_string(),
        claim_token: None,
        product_target: None,
        product_version: None,
        entries: vec![FileEntryPlan {
            action: FileEntryAction::Publish,
            storage_root: StorageRootId::WebDav,
            source_path: None,
            temporary_path: Some(temporary.clone()),
            destination_path: Some(destination.clone()),
            tombstone_path: None,
            expected_size: None,
            expected_sha256: None,
            expected_version: None,
        }],
        claims: vec![
            resource_write_claim(temporary.clone(), true, "collection_temporary"),
            resource_write_claim(destination, true, "collection_destination"),
        ],
        space_reservation: Some(
            JournalSpaceReservationPlan::new(reservation)
                .map_err(|error| WebDavMutationError::Internal(error.to_string()))?,
        ),
    };
    prepare_mutation_plan(executors, plan).await?;
    if let Err(error) = executors
        .file_io
        .create_storage_directory_durable(StorageRootId::WebDav, temporary)
        .await
    {
        cancel_mutation_group(executors, group_id).await?;
        return Err(map_executor_error(error));
    }
    execute_prepared_mutation(executors, group_id, 1, false).await
}

async fn execute_mutation_plan(
    executors: &ExecutorHandles,
    plan: FileOperationPlan,
    publication_entries: u16,
    has_cleanup: bool,
) -> Result<(), WebDavMutationError> {
    let group_id = plan.group_id.clone();
    prepare_mutation_plan(executors, plan).await?;
    execute_prepared_mutation(executors, group_id, publication_entries, has_cleanup).await
}

async fn prepare_mutation_plan(
    executors: &ExecutorHandles,
    plan: FileOperationPlan,
) -> Result<(), WebDavMutationError> {
    if executors
        .sqlite
        .prepare_file_operation_durable(plan)
        .await
        .map_err(map_executor_error)?
        == PrepareJournalOutcome::PathConflict
    {
        return Err(WebDavMutationError::Conflict);
    }
    Ok(())
}

async fn execute_prepared_mutation(
    executors: &ExecutorHandles,
    group_id: String,
    publication_entries: u16,
    has_cleanup: bool,
) -> Result<(), WebDavMutationError> {
    let ticket = executors
        .file_io
        .reserve_journal_mutation(&group_id, 2)
        .map_err(|error| WebDavMutationError::Unavailable(error.to_string()))?;
    let grant = executors
        .sqlite
        .begin_file_operation_publication_durable(&ticket, 1)
        .await
        .map_err(map_executor_error)?
        .ok_or(WebDavMutationError::Conflict)?;
    let mut lease =
        crate::io::recovery::acquire_verified_journal_mutation(executors, ticket, grant)
            .await
            .map_err(map_executor_error)?;
    let mut version = 2_i64;
    for sequence in 0..publication_entries {
        executors
            .file_io
            .apply_next_journal_entry_durable(&mut lease)
            .await
            .map_err(map_executor_error)?;
        let checkpoint = executors
            .sqlite
            .record_file_entry_published_durable(group_id.clone(), version, sequence)
            .await
            .map_err(map_executor_error)?
            .ok_or(WebDavMutationError::Conflict)?;
        version = checkpoint.version;
        if sequence + 1 == publication_entries && !checkpoint.phase_complete {
            return Err(WebDavMutationError::Internal(
                "WebDAV mutation publication phase did not complete".to_string(),
            ));
        }
    }
    drop(lease);
    if executors
        .sqlite
        .complete_no_product_file_operation_durable(group_id, version)
        .await
        .map_err(map_executor_error)?
        != (JournalCheckpointOutcome::Advanced {
            version: version + 1,
        })
    {
        return Err(WebDavMutationError::Conflict);
    }
    if has_cleanup {
        executors.scheduler.wake_journal_recovery();
    }
    Ok(())
}

async fn cancel_mutation_group(
    executors: &ExecutorHandles,
    group_id: String,
) -> Result<(), WebDavMutationError> {
    let Some(status) = executors
        .sqlite
        .load_file_operation_cancellation_status_durable(group_id.clone())
        .await
        .map_err(map_executor_error)?
    else {
        return Ok(());
    };
    match crate::io::recovery::cancel_generic_file_operation(executors, group_id, status.version)
        .await
        .map_err(map_executor_error)?
    {
        crate::io::journal::JournalCancellationOutcome::Requested { .. }
        | crate::io::journal::JournalCancellationOutcome::AlreadyRequested { .. } => Ok(()),
        crate::io::journal::JournalCancellationOutcome::VersionConflict
        | crate::io::journal::JournalCancellationOutcome::NotCancellable => {
            Err(WebDavMutationError::Conflict)
        }
    }
}

async fn stream_body_to_publication(
    executors: &ExecutorHandles,
    publication: crate::processor::artifact::PreparedArtifactPublication,
    body: Body,
    starting_length: u64,
    declared_body_length: Option<u64>,
    maximum_total_bytes: u64,
) -> Result<(), (StatusCode, String)> {
    let session = match executors
        .file_io
        .open_storage_write_session_durable(
            publication.storage_root(),
            publication.temporary_path().clone(),
            starting_length,
        )
        .await
    {
        Ok(session) => session,
        Err(error) => {
            publication.cancel(executors).await;
            return Err((StatusCode::SERVICE_UNAVAILABLE, error.to_string()));
        }
    };
    stream_body_into_open_publication(
        executors,
        publication,
        session,
        body,
        starting_length,
        declared_body_length,
        maximum_total_bytes,
    )
    .await
}

async fn stream_body_into_open_publication(
    executors: &ExecutorHandles,
    publication: crate::processor::artifact::PreparedArtifactPublication,
    session: crate::io::StorageFileSession,
    body: Body,
    starting_length: u64,
    declared_body_length: Option<u64>,
    maximum_total_bytes: u64,
) -> Result<(), (StatusCode, String)> {
    let mut session = Some(session);
    let mut received = 0_u64;
    let mut stream = body.into_data_stream();
    while let Some(frame) = stream.next().await {
        let bytes = match frame {
            Ok(bytes) => bytes,
            Err(error) => {
                abort_publication(executors, publication, session.take()).await;
                return Err((StatusCode::BAD_REQUEST, error.to_string()));
            }
        };
        received = match received.checked_add(bytes.len() as u64) {
            Some(received) => received,
            None => {
                abort_publication(executors, publication, session.take()).await;
                return Err((
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "upload size overflowed".to_string(),
                ));
            }
        };
        if starting_length
            .checked_add(received)
            .is_none_or(|total| total > maximum_total_bytes)
        {
            abort_publication(executors, publication, session.take()).await;
            return Err((
                StatusCode::PAYLOAD_TOO_LARGE,
                "upload exceeded its limit".to_string(),
            ));
        }
        let mut offset = 0_usize;
        while offset < bytes.len() {
            let _chunk_admission = match executors.scheduler.acquire_file_chunk().await {
                Ok(admission) => admission,
                Err(error) => {
                    abort_publication(executors, publication, session.take()).await;
                    return Err((StatusCode::SERVICE_UNAVAILABLE, error));
                }
            };
            let end = offset
                .saturating_add(crate::runtime::FILE_IO_CHUNK_BYTES as usize)
                .min(bytes.len());
            let chunk = bytes.slice(offset..end).to_vec();
            let chunk_length = chunk.len();
            let current = session.take().ok_or_else(|| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "upload session is unavailable".to_string(),
                )
            })?;
            match executors
                .file_io
                .write_storage_session_durable(current, chunk)
                .await
            {
                Ok((returned, written)) if written == chunk_length => {
                    session = Some(returned);
                    offset = end;
                }
                Ok((returned, _)) => {
                    abort_publication(executors, publication, Some(returned)).await;
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "file worker made no upload progress".to_string(),
                    ));
                }
                Err(error) => {
                    abort_publication(executors, publication, None).await;
                    return Err((StatusCode::SERVICE_UNAVAILABLE, error.to_string()));
                }
            }
        }
    }
    if declared_body_length.is_some_and(|declared| declared != received) {
        abort_publication(executors, publication, session.take()).await;
        return Err((
            StatusCode::BAD_REQUEST,
            "request body length did not match Content-Length".to_string(),
        ));
    }
    let session = session.ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "upload session is unavailable".to_string(),
        )
    })?;
    if let Err(error) = executors
        .file_io
        .commit_storage_session_durable(session)
        .await
    {
        publication.cancel(executors).await;
        return Err((StatusCode::SERVICE_UNAVAILABLE, error.to_string()));
    }
    publication
        .publish(executors)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))
}

async fn abort_publication(
    executors: &ExecutorHandles,
    publication: crate::processor::artifact::PreparedArtifactPublication,
    session: Option<crate::io::StorageFileSession>,
) {
    if let Some(session) = session {
        let _ = executors
            .file_io
            .abort_storage_session_durable(session)
            .await;
    }
    publication.cancel(executors).await;
}

async fn copy_session_bytes(
    file_io: &crate::executor::FileIoExecutorHandle,
    scheduler: &crate::runtime::SchedulerHandle,
    mut input: crate::io::StorageFileSession,
    mut output: crate::io::StorageFileSession,
    byte_count: u64,
) -> Result<(crate::io::StorageFileSession, crate::io::StorageFileSession), String> {
    let mut copied = 0_u64;
    while copied < byte_count {
        let _chunk_admission = scheduler.acquire_file_chunk().await?;
        let maximum =
            usize::try_from((byte_count - copied).min(crate::runtime::FILE_IO_CHUNK_BYTES))
                .map_err(|_| "copy chunk size exceeds this platform".to_string())?;
        let (returned_input, bytes) = file_io
            .read_storage_session_durable(input, maximum)
            .await
            .map_err(|error| error.to_string())?;
        input = returned_input;
        if bytes.is_empty() {
            return Err("source ended before its descriptor snapshot length".to_string());
        }
        let (returned_output, written) = file_io
            .write_storage_session_durable(output, bytes)
            .await
            .map_err(|error| error.to_string())?;
        output = returned_output;
        if written == 0 {
            return Err("file worker made no copy progress".to_string());
        }
        copied = copied
            .checked_add(written as u64)
            .ok_or_else(|| "copy byte count overflowed".to_string())?;
    }
    Ok((input, output))
}

async fn propfind(
    executors: &ExecutorHandles,
    path: NormalizedStoragePath,
    mount_path: &str,
    relative: Option<&str>,
    headers: &HeaderMap,
) -> Response {
    let depth = match PropfindDepth::from_headers(headers) {
        Ok(depth) => depth,
        Err(status) => return status.into_response(),
    };
    let snapshot = match inspect_webdav_resource(executors, path.clone()).await {
        Ok(snapshot) => snapshot,
        Err(error) => return mutation_error_response("WebDAV PROPFIND failed", error),
    };
    let base = relative.unwrap_or("");
    let session = if snapshot.is_directory() && depth.traverses_children() {
        match executors
            .file_io
            .open_storage_directory_session_request(StorageRootId::WebDav, Some(path.clone()))
            .await
        {
            Ok(session) => Some(session),
            Err(error) => {
                return mutation_error_response(
                    "WebDAV PROPFIND traversal failed",
                    map_executor_error(error),
                )
            }
        }
    } else {
        None
    };
    let state = PropfindStreamState::new(
        executors.clone(),
        mount_path.to_string(),
        base.to_string(),
        path,
        snapshot.is_directory(),
        depth,
        session,
    );
    let stream = futures::stream::unfold(state, PropfindStreamState::next_chunk);
    Response::builder()
        .status(StatusCode::MULTI_STATUS)
        .header(header::CONTENT_TYPE, "application/xml; charset=utf-8")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|error| {
            internal_error("WebDAV PROPFIND response failed", error.to_string())
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PropfindDepth {
    Zero,
    One,
    Infinity,
}

impl PropfindDepth {
    fn from_headers(headers: &HeaderMap) -> Result<Self, StatusCode> {
        match headers
            .get("depth")
            .map(|value| value.to_str().map(str::trim))
            .transpose()
            .map_err(|_| StatusCode::BAD_REQUEST)?
        {
            Some("0") => Ok(Self::Zero),
            Some("1") => Ok(Self::One),
            Some("infinity") | None => Ok(Self::Infinity),
            Some(_) => Err(StatusCode::BAD_REQUEST),
        }
    }

    fn traverses_children(self) -> bool {
        self != Self::Zero
    }

    fn is_recursive(self) -> bool {
        self == Self::Infinity
    }
}

struct PendingPropfindDirectory {
    path: NormalizedStoragePath,
    relative: String,
    depth: usize,
}

struct PropfindDirectoryFrame {
    session: Option<StorageFileSession>,
    path: NormalizedStoragePath,
    relative: String,
    depth: usize,
    pending_children: VecDeque<PendingPropfindDirectory>,
    finished: bool,
}

enum PropfindStreamPhase {
    Start,
    Traverse,
    End,
    Done,
}

struct PropfindStreamState {
    executors: ExecutorHandles,
    mount_path: String,
    root_relative: String,
    root_is_directory: bool,
    depth: PropfindDepth,
    phase: PropfindStreamPhase,
    stack: Vec<PropfindDirectoryFrame>,
}

impl PropfindStreamState {
    fn new(
        executors: ExecutorHandles,
        mount_path: String,
        root_relative: String,
        root_path: NormalizedStoragePath,
        root_is_directory: bool,
        depth: PropfindDepth,
        session: Option<StorageFileSession>,
    ) -> Self {
        let mut stack = Vec::new();
        if let Some(session) = session {
            stack.push(PropfindDirectoryFrame {
                session: Some(session),
                path: root_path,
                relative: root_relative.clone(),
                depth: 0,
                pending_children: VecDeque::new(),
                finished: false,
            });
        }
        Self {
            executors,
            mount_path,
            root_relative,
            root_is_directory,
            depth,
            phase: PropfindStreamPhase::Start,
            stack,
        }
    }

    async fn next_chunk(mut state: Self) -> Option<(Result<Vec<u8>, std::io::Error>, Self)> {
        loop {
            match state.phase {
                PropfindStreamPhase::Start => {
                    state.phase = if state.stack.is_empty() {
                        PropfindStreamPhase::End
                    } else {
                        PropfindStreamPhase::Traverse
                    };
                    let mut xml = String::from(
                        "<?xml version=\"1.0\" encoding=\"utf-8\"?><D:multistatus xmlns:D=\"DAV:\">",
                    );
                    xml.push_str(&prop_response(
                        &webdav_href(
                            &state.mount_path,
                            &state.root_relative,
                            state.root_is_directory,
                        ),
                        state.root_is_directory,
                    ));
                    return Some((Ok(xml.into_bytes()), state));
                }
                PropfindStreamPhase::Traverse => {
                    let Some(frame) = state.stack.last_mut() else {
                        state.phase = PropfindStreamPhase::End;
                        continue;
                    };
                    if let Some(child) = frame.pending_children.pop_front() {
                        let session = match state
                            .executors
                            .file_io
                            .open_storage_directory_session_request(
                                StorageRootId::WebDav,
                                Some(child.path.clone()),
                            )
                            .await
                        {
                            Ok(session) => session,
                            Err(error) => {
                                state.phase = PropfindStreamPhase::Done;
                                return Some((
                                    Err(std::io::Error::other(error.to_string())),
                                    state,
                                ));
                            }
                        };
                        state.stack.push(PropfindDirectoryFrame {
                            session: Some(session),
                            path: child.path,
                            relative: child.relative,
                            depth: child.depth,
                            pending_children: VecDeque::new(),
                            finished: false,
                        });
                        continue;
                    }
                    if frame.finished {
                        if let Some(session) = frame.session.take() {
                            if let Err(error) = state
                                .executors
                                .file_io
                                .close_storage_session_request(session)
                                .await
                            {
                                state.phase = PropfindStreamPhase::Done;
                                return Some((
                                    Err(std::io::Error::other(error.to_string())),
                                    state,
                                ));
                            }
                        }
                        state.stack.pop();
                        continue;
                    }
                    let Some(session) = frame.session.take() else {
                        state.phase = PropfindStreamPhase::Done;
                        return Some((
                            Err(std::io::Error::other(
                                "WebDAV directory traversal session is missing",
                            )),
                            state,
                        ));
                    };
                    let (session, entries, finished) = match state
                        .executors
                        .file_io
                        .read_storage_directory_session_request(session)
                        .await
                    {
                        Ok(page) => page,
                        Err(error) => {
                            state.phase = PropfindStreamPhase::Done;
                            return Some((Err(std::io::Error::other(error.to_string())), state));
                        }
                    };
                    let frame = state
                        .stack
                        .last_mut()
                        .expect("PROPFIND frame remains while its read is awaited");
                    frame.session = Some(session);
                    frame.finished = finished;
                    let mut xml = String::new();
                    for entry in entries {
                        let child_relative = if frame.relative.is_empty() {
                            entry.name.clone()
                        } else {
                            format!("{}/{}", frame.relative, entry.name)
                        };
                        let is_directory = entry.kind == StorageDirectoryEntryKind::Directory;
                        xml.push_str(&prop_response(
                            &webdav_href(&state.mount_path, &child_relative, is_directory),
                            is_directory,
                        ));
                        if is_directory
                            && state.depth.is_recursive()
                            && frame.depth + 1 < crate::io::file::MAX_STORAGE_PATH_COMPONENTS
                        {
                            let path = NormalizedStoragePath::parse(&format!(
                                "{}/{}",
                                frame.path.relative_path(),
                                entry.name
                            ))
                            .map_err(|error| std::io::Error::other(error.to_string()));
                            match path {
                                Ok(path) => {
                                    frame.pending_children.push_back(PendingPropfindDirectory {
                                        path,
                                        relative: child_relative,
                                        depth: frame.depth + 1,
                                    })
                                }
                                Err(error) => {
                                    state.phase = PropfindStreamPhase::Done;
                                    return Some((Err(error), state));
                                }
                            }
                        }
                    }
                    if xml.is_empty() {
                        continue;
                    }
                    return Some((Ok(xml.into_bytes()), state));
                }
                PropfindStreamPhase::End => {
                    state.phase = PropfindStreamPhase::Done;
                    return Some((Ok(b"</D:multistatus>".to_vec()), state));
                }
                PropfindStreamPhase::Done => return None,
            }
        }
    }
}

fn options_response() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header("dav", "1, 2")
        .header(
            "allow",
            "OPTIONS, GET, HEAD, PUT, PATCH, DELETE, MKCOL, COPY, MOVE, PROPFIND",
        )
        .body(Body::empty())
        .expect("static WebDAV OPTIONS response is valid")
}

fn prop_response(href: &str, directory: bool) -> String {
    let resource_type = if directory { "<D:collection/>" } else { "" };
    format!(
        "<D:response><D:href>{}</D:href><D:propstat><D:prop><D:resourcetype>{resource_type}</D:resourcetype></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>",
        xml_escape(href)
    )
}

fn webdav_href(mount_path: &str, relative: &str, directory: bool) -> String {
    let mut href = mount_path.trim_end_matches('/').to_string();
    href.push('/');
    href.push_str(&percent_encode_path(relative));
    if directory && !href.ends_with('/') {
        href.push('/');
    }
    href
}

fn percent_encode_path(path: &str) -> String {
    let mut encoded = String::new();
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push_str(&format!("{byte:02X}"));
        }
    }
    encoded
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn destination_storage_path(
    username: &str,
    mount_path: &str,
    headers: &HeaderMap,
) -> Result<NormalizedStoragePath, StatusCode> {
    let destination = headers
        .get("destination")
        .and_then(|value| value.to_str().ok())
        .and_then(destination_request_path)
        .ok_or(StatusCode::BAD_REQUEST)?;
    let relative =
        relative_webdav_path(mount_path, &destination)?.ok_or(StatusCode::BAD_REQUEST)?;
    user_storage_path(username, Some(&relative))
}

fn destination_request_path(value: &str) -> Option<String> {
    let uri = value.parse::<Uri>().ok()?;
    let path = uri.path();
    (!path.is_empty()).then(|| path.to_string())
}

fn parse_content_range(value: &str) -> Option<(u64, u64, u64)> {
    let (unit, range_and_total) = value.split_once(' ')?;
    if unit != "bytes" {
        return None;
    }
    let (range, total) = range_and_total.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    Some((start.parse().ok()?, end.parse().ok()?, total.parse().ok()?))
}

fn content_range_completes_upload(value: &str) -> bool {
    parse_content_range(value).is_some_and(|(_, end, total)| end.checked_add(1) == Some(total))
}

pub fn invalidated_upload_paths(
    method: &Method,
    headers: &HeaderMap,
    request_path: &str,
    mount_path: &str,
) -> Vec<String> {
    let source_path = relative_webdav_path(mount_path, request_path)
        .ok()
        .flatten();
    let destination_path = headers
        .get("destination")
        .and_then(|value| value.to_str().ok())
        .and_then(destination_request_path)
        .and_then(|path| relative_webdav_path(mount_path, &path).ok().flatten());
    match method.as_str() {
        "PUT" | "PATCH" | "DELETE" => source_path.into_iter().collect(),
        "MOVE" => source_path.into_iter().chain(destination_path).collect(),
        "COPY" => destination_path.into_iter().collect(),
        _ => Vec::new(),
    }
}

pub fn completed_upload_path(
    method: &Method,
    headers: &HeaderMap,
    request_path: &str,
    mount_path: &str,
) -> Option<String> {
    if *method == Method::PUT {
        return relative_webdav_path(mount_path, request_path)
            .ok()
            .flatten();
    }
    if *method == Method::PATCH {
        return headers
            .get(header::CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(content_range_completes_upload)
            .then(|| {
                relative_webdav_path(mount_path, request_path)
                    .ok()
                    .flatten()
            })
            .flatten();
    }
    if !matches!(method.as_str(), "MOVE" | "COPY") {
        return None;
    }
    headers
        .get("destination")
        .and_then(|value| value.to_str().ok())
        .and_then(destination_request_path)
        .and_then(|path| relative_webdav_path(mount_path, &path).ok().flatten())
}

fn relative_webdav_path(
    mount_path: &str,
    request_path: &str,
) -> Result<Option<String>, StatusCode> {
    let decoded = percent_decode_str(request_path)
        .decode_utf8()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let mount = mount_path.trim_end_matches('/');
    if decoded.as_ref() == mount || decoded.as_ref() == format!("{mount}/") {
        return Ok(None);
    }
    let relative = decoded
        .strip_prefix(&format!("{mount}/"))
        .ok_or(StatusCode::NOT_FOUND)?
        .trim_end_matches('/');
    if relative.is_empty() {
        return Ok(None);
    }
    NormalizedStoragePath::parse(relative).map_err(|_| StatusCode::BAD_REQUEST)?;
    Ok(Some(relative.to_string()))
}

fn user_storage_path(
    username: &str,
    relative: Option<&str>,
) -> Result<NormalizedStoragePath, StatusCode> {
    let path = match relative {
        Some(relative) => format!("{username}/{relative}"),
        None => username.to_string(),
    };
    NormalizedStoragePath::parse(&path).map_err(|_| StatusCode::BAD_REQUEST)
}

fn sibling_path(path: &NormalizedStoragePath, name: &str) -> NormalizedStoragePath {
    let path = std::path::Path::new(path.relative_path());
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new(""));
    NormalizedStoragePath::parse(&parent.join(name).to_string_lossy())
        .expect("generated WebDAV sibling path is normalized")
}

fn resource_write_claim(
    path: NormalizedStoragePath,
    directory: bool,
    role: &str,
) -> FilePathClaimPlan {
    FilePathClaimPlan {
        storage_root: StorageRootId::WebDav,
        path,
        mode: PathClaimMode::Write,
        scope: if directory {
            PathClaimScope::Subtree
        } else {
            PathClaimScope::Exact
        },
        role: role.to_string(),
        expected_version: None,
    }
}

fn resource_read_claim(path: NormalizedStoragePath, role: &str) -> FilePathClaimPlan {
    FilePathClaimPlan {
        storage_root: StorageRootId::WebDav,
        path,
        mode: PathClaimMode::Read,
        scope: PathClaimScope::Subtree,
        role: role.to_string(),
        expected_version: None,
    }
}

fn path_is_equal_or_descendant(
    candidate: &NormalizedStoragePath,
    ancestor: &NormalizedStoragePath,
) -> bool {
    candidate == ancestor
        || candidate
            .relative_path()
            .strip_prefix(ancestor.relative_path())
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn map_executor_error(error: crate::executor::ExecutorError) -> WebDavMutationError {
    match error.kind {
        ExecutorErrorKind::FileNotFound | ExecutorErrorKind::NotFound => {
            WebDavMutationError::NotFound
        }
        ExecutorErrorKind::Conflict | ExecutorErrorKind::FileConflict => {
            WebDavMutationError::Conflict
        }
        ExecutorErrorKind::Overloaded
        | ExecutorErrorKind::ShuttingDown
        | ExecutorErrorKind::DatabaseBusy
        | ExecutorErrorKind::DatabaseTimeout
        | ExecutorErrorKind::FileTransient => WebDavMutationError::Unavailable(error.to_string()),
        _ => WebDavMutationError::Internal(error.to_string()),
    }
}

fn mutation_error_response(context: &'static str, error: WebDavMutationError) -> Response {
    match error {
        WebDavMutationError::NotFound => StatusCode::NOT_FOUND.into_response(),
        WebDavMutationError::Conflict => StatusCode::LOCKED.into_response(),
        WebDavMutationError::Unavailable(detail) => {
            error!(error = %detail, "{context}");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
        WebDavMutationError::Internal(detail) => internal_error(context, detail),
    }
}

fn internal_error(context: &'static str, error: impl std::fmt::Display) -> Response {
    error!(error = %error, "{context}");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

fn internal_error_with_status(
    status: StatusCode,
    context: &'static str,
    error: impl std::fmt::Display,
) -> Response {
    if status != StatusCode::PAYLOAD_TOO_LARGE {
        error!(error = %error, "{context}");
    }
    status.into_response()
}
