use std::collections::HashMap;

use axum::{extract::State, routing::post, Json, Router};

use crate::auth::{AppState, CurrentUser};
use crate::database::{fetch_all, queries};
use crate::error::{AppError, AppResult};
use crate::models::{
    map_media_response, DeduplicateGroup, DeduplicateGroupsRequest, DeduplicateGroupsResponse,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/duplicates/list", post(list))
}

async fn list(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<DeduplicateGroupsRequest>,
) -> AppResult<Json<DeduplicateGroupsResponse>> {
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
    let connection = state.pool.get().map_err(AppError::Pool)?;
    let page_rows = fetch_all(
        &connection,
        queries::deduplicate::SELECT_VISIBLE_CLUSTER_PAGE,
        &[&current_user.id, &cursor, &(request.limit + 1)],
        |row| {
            Ok((
                row.get::<_, Option<i64>>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        },
    )?;
    let (total_groups, total_media) = page_rows
        .first()
        .map(|row| (row.1, row.2))
        .unwrap_or((0, 0));
    let cluster_ids = page_rows
        .into_iter()
        .filter_map(|row| row.0)
        .collect::<Vec<_>>();
    let has_more = cluster_ids.len() > request.limit as usize;
    let selected_ids = cluster_ids
        .into_iter()
        .take(request.limit as usize)
        .collect::<Vec<_>>();
    let mut items_by_cluster = HashMap::new();
    if !selected_ids.is_empty() {
        let query = queries::deduplicate::build_visible_cluster_media_query(selected_ids.len());
        let mut parameters = selected_ids
            .iter()
            .map(|cluster_id| cluster_id as &dyn rusqlite::ToSql)
            .collect::<Vec<_>>();
        parameters.push(&current_user.id);
        let rows = fetch_all(&connection, &query, &parameters, |row| {
            Ok((row.get::<_, i64>(28)?, map_media_response(row)?))
        })?;
        for (cluster_id, media) in rows {
            items_by_cluster
                .entry(cluster_id)
                .or_insert_with(Vec::new)
                .push(media);
        }
    }
    let groups = selected_ids
        .into_iter()
        .filter_map(|cluster_id| {
            let items = items_by_cluster.remove(&cluster_id)?;
            (items.len() >= 2).then_some(DeduplicateGroup { cluster_id, items })
        })
        .collect::<Vec<_>>();
    let next_cursor = if has_more {
        groups.last().map(|group| group.cluster_id.to_string())
    } else {
        None
    };
    Ok(Json(DeduplicateGroupsResponse {
        groups,
        next_cursor,
        has_more,
        total_groups,
        total_media,
    }))
}
