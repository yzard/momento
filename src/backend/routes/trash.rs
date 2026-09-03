use axum::{
    extract::{Extension, Path, State},
    http::HeaderMap,
    response::Response,
    routing::{get, post},
    Router,
};
use chrono::{Duration, Utc};

use crate::auth::{AppState, CurrentUser};
use crate::constants::TRASH_RETENTION_DAYS;
use crate::database::operations::{
    DeleteExpiredTrashPage, DeleteTrashMedia, DeleteTrashPage, RestoreTrash, TrashDeletionOutcome,
};
use crate::error::{AppError, AppResult};
use crate::models::{TrashDeleteRequest, TrashListResponse, TrashResponse, TrashRestoreRequest};
use crate::routes::{render_json, CpuJson};
use crate::runtime::HttpRequestAdmission;

const TRASH_DELETE_PAGE_SIZE: u16 = 256;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/trash/list", post(list_trash))
        .route(
            "/trash/:media_id/thumbnail/tiny",
            get(get_deleted_tiny_thumbnail),
        )
        .route("/trash/restore", post(restore_from_trash))
        .route("/trash/delete", post(permanently_delete))
        .route("/trash/empty", post(empty_trash))
}

async fn get_deleted_tiny_thumbnail(
    State(state): State<AppState>,
    Extension(admission): Extension<HttpRequestAdmission>,
    current_user: CurrentUser,
    Path(media_id): Path<i64>,
    headers: HeaderMap,
) -> AppResult<Response> {
    crate::routes::media::serve_deleted_tiny_thumbnail(
        &state,
        &admission,
        current_user.id,
        media_id,
        &headers,
    )
    .await
}

async fn list_trash(
    State(state): State<AppState>,
    current_user: CurrentUser,
) -> AppResult<Response> {
    let items = state
        .executors
        .sqlite
        .load_trash_request(current_user.id)
        .await?;
    let total_count = items.len() as i64;

    render_json(&state, TrashListResponse { items, total_count }).await
}

async fn restore_from_trash(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<TrashRestoreRequest>,
) -> AppResult<Response> {
    if request.media_ids.is_empty() {
        return render_json(
            &state,
            TrashResponse {
                message: "No media to restore".to_string(),
                affected_count: 0,
            },
        )
        .await;
    }

    let media_ids = crate::routes::media::unique_batch_ids(request.media_ids)?;
    let affected_count = state
        .executors
        .sqlite
        .restore_trash_request(RestoreTrash {
            user_id: current_user.id,
            media_ids,
        })
        .await?;

    render_json(
        &state,
        TrashResponse {
            message: "Media restored successfully".to_string(),
            affected_count: affected_count as i64,
        },
    )
    .await
}

async fn permanently_delete(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<TrashDeleteRequest>,
) -> AppResult<Response> {
    if request.media_ids.is_empty() {
        return render_json(
            &state,
            TrashResponse {
                message: "No media to delete".to_string(),
                affected_count: 0,
            },
        )
        .await;
    }

    let media_ids = crate::routes::media::unique_batch_ids(request.media_ids)?;
    let outcome = state
        .executors
        .sqlite
        .delete_trash_media_request(DeleteTrashMedia {
            user_id: current_user.id,
            media_ids,
        })
        .await?;
    let deleted_count = apply_trash_deletion_outcome(&state.scheduler, outcome)?.0;

    render_json(
        &state,
        TrashResponse {
            message: "Media permanently deleted".to_string(),
            affected_count: i64::try_from(deleted_count)
                .map_err(|_| AppError::Internal("trash deletion count overflow".to_string()))?,
        },
    )
    .await
}

async fn empty_trash(
    State(state): State<AppState>,
    current_user: CurrentUser,
) -> AppResult<Response> {
    let mut deleted_count = 0usize;
    loop {
        let outcome = state
            .executors
            .sqlite
            .delete_trash_page_request(DeleteTrashPage {
                user_id: current_user.id,
                limit: TRASH_DELETE_PAGE_SIZE,
            })
            .await?;
        let (affected, has_more) = apply_trash_deletion_outcome(&state.scheduler, outcome)?;
        deleted_count = deleted_count
            .checked_add(affected)
            .ok_or_else(|| AppError::Internal("trash deletion count overflow".to_string()))?;
        if !has_more {
            break;
        }
    }

    render_json(
        &state,
        TrashResponse {
            message: "Trash emptied".to_string(),
            affected_count: i64::try_from(deleted_count)
                .map_err(|_| AppError::Internal("trash deletion count overflow".to_string()))?,
        },
    )
    .await
}

pub async fn cleanup_expired_trash(
    sqlite: &crate::executor::SqliteExecutorHandle,
    scheduler: &crate::runtime::SchedulerHandle,
) -> AppResult<i64> {
    let cutoff_date = (Utc::now() - Duration::days(TRASH_RETENTION_DAYS)).to_rfc3339();
    let mut deleted_count = 0usize;
    loop {
        let outcome = sqlite
            .delete_expired_trash_page_durable(DeleteExpiredTrashPage {
                cutoff: cutoff_date.clone(),
                limit: TRASH_DELETE_PAGE_SIZE,
            })
            .await?;
        let (affected, has_more) = apply_trash_deletion_outcome(scheduler, outcome)?;
        deleted_count = deleted_count
            .checked_add(affected)
            .ok_or_else(|| AppError::Internal("expired trash count overflow".to_string()))?;
        if !has_more {
            break;
        }
    }
    i64::try_from(deleted_count)
        .map_err(|_| AppError::Internal("expired trash count overflow".to_string()))
}

fn apply_trash_deletion_outcome(
    scheduler: &crate::runtime::SchedulerHandle,
    outcome: TrashDeletionOutcome,
) -> AppResult<(usize, bool)> {
    match outcome {
        TrashDeletionOutcome::Deleted {
            affected_count,
            cleanup_groups,
            has_more,
        } => {
            if cleanup_groups > 0 {
                scheduler.wake_journal_recovery();
            }
            Ok((affected_count, has_more))
        }
        TrashDeletionOutcome::PathConflict => Err(AppError::Conflict(
            "media files are being changed by another operation".to_string(),
        )),
    }
}
