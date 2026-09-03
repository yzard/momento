use axum::{extract::State, response::Response, routing::post, Router};

use crate::auth::{AppState, CurrentUser};
use crate::database::operations::DuplicateGroupsQuery;
use crate::error::{AppError, AppResult};
use crate::models::DeduplicateGroupsRequest;
use crate::routes::{render_json, CpuJson};

pub fn router() -> Router<AppState> {
    Router::new().route("/duplicates/list", post(list))
}

async fn list(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<DeduplicateGroupsRequest>,
) -> AppResult<Response> {
    if !(1..=100).contains(&request.limit) {
        return Err(AppError::BadRequest(
            "limit must be between 1 and 100".to_string(),
        ));
    }
    let cursor = request
        .cursor
        .as_deref()
        .unwrap_or("0")
        .parse::<i64>()
        .map_err(|_| AppError::BadRequest("cursor must be a cluster ID".to_string()))?;
    let response = state
        .executors
        .sqlite
        .load_duplicate_groups_request(DuplicateGroupsQuery {
            user_id: current_user.id,
            cursor,
            limit: request.limit,
        })
        .await?;
    render_json(&state, response).await
}
