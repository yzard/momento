use axum::{extract::State, response::Response, routing::post, Router};

use crate::auth::{AppState, RequireAdmin};
use crate::error::{AppError, AppResult};
use crate::io::journal::JournalRetryOutcome;
use crate::models::{
    FileOperationGetRequest, FileOperationListRequest, FileOperationRetryRequest,
    FileOperationRetryResponse,
};
use crate::routes::{render_json, CpuJson};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/file-operations/list", post(list))
        .route("/admin/file-operations/get", post(get))
        .route("/admin/file-operations/retry", post(retry))
}

async fn list(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    CpuJson(request): CpuJson<FileOperationListRequest>,
) -> AppResult<Response> {
    request
        .validate()
        .map_err(|error| AppError::Validation(error.to_string()))?;
    let response = state
        .executors
        .sqlite
        .list_file_operations_request(request.states, request.cursor, request.limit)
        .await?;
    render_json(&state, response).await
}

async fn get(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    CpuJson(request): CpuJson<FileOperationGetRequest>,
) -> AppResult<Response> {
    request
        .validate()
        .map_err(|error| AppError::Validation(error.to_string()))?;
    let response = state
        .executors
        .sqlite
        .load_file_operation_detail_request(request.operation_id)
        .await?
        .ok_or_else(|| AppError::NotFound("file operation not found".to_string()))?;
    render_json(&state, response).await
}

async fn retry(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    CpuJson(request): CpuJson<FileOperationRetryRequest>,
) -> AppResult<Response> {
    request
        .validate()
        .map_err(|error| AppError::Validation(error.to_string()))?;
    let request_hash = state
        .executors
        .cpu
        .try_sha256(
            request
                .canonical_hash_input()
                .map_err(|error| AppError::Internal(error.to_string()))?,
        )
        .await?;
    let operation_id = request.operation_id;
    match state
        .executors
        .sqlite
        .retry_file_operation_request(
            request.retry_request_id,
            operation_id.clone(),
            request.expected_version,
            request_hash,
        )
        .await?
    {
        JournalRetryOutcome::Accepted {
            state: operation_state,
            version,
            replayed,
        } => {
            if !replayed {
                state.scheduler.wake_journal_recovery();
            }
            render_json(
                &state,
                FileOperationRetryResponse {
                    operation_id,
                    state: operation_state,
                    version,
                    replayed,
                },
            )
            .await
        }
        JournalRetryOutcome::VersionConflict => Err(AppError::Conflict(
            "file operation version or state no longer permits retry".to_string(),
        )),
        JournalRetryOutcome::RequestConflict => Err(AppError::Conflict(
            "retryRequestId was already used with different request content".to_string(),
        )),
        JournalRetryOutcome::ReceiptLimitReached => Err(AppError::ResourceLimit(
            "file operation has reached its live retry receipt limit".to_string(),
        )),
    }
}
