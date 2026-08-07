use axum::{extract::State, routing::post, Json, Router};
use chrono::Utc;

use crate::auth::{AppState, CurrentUser, RequireAdmin};
use crate::cronjob::next_scheduled_at;
use crate::database::{fetch_all, queries};
use crate::error::{AppError, AppResult};
use crate::models::{
    DeduplicateActionResponse, DeduplicateGroup, DeduplicateGroupsRequest,
    DeduplicateGroupsResponse, DeduplicateStatusResponse,
};
use crate::processor::deduplicator::{clean, create_run, latest_run, request_cancel, run_scan};

use super::media::map_media_row;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/deduplicate/start", post(start))
        .route("/deduplicate/status", post(status))
        .route("/deduplicate/cancel", post(cancel))
        .route("/deduplicate/clean", post(clean_indexes))
        .route("/deduplicate/groups", post(groups))
}

async fn start(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
) -> AppResult<Json<DeduplicateActionResponse>> {
    if !state.config.llm.deduplicate_enabled {
        return Err(AppError::Validation(
            "deduplication is disabled in LLM configuration".to_string(),
        ));
    }
    let run_id = create_run(&state.pool, "manual", None)?;
    let config = state.config.clone();
    let pool = state.pool.clone();
    tokio::spawn(async move {
        run_scan(&config, &pool, run_id).await;
    });
    Ok(Json(DeduplicateActionResponse {
        message: "Deduplicate scan started".to_string(),
        status: "running".to_string(),
    }))
}

async fn status(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
) -> AppResult<Json<DeduplicateStatusResponse>> {
    let run = latest_run(&state.pool)?;
    let next_scheduled_at = if state.config.llm.deduplicate_enabled {
        Some(
            next_scheduled_at(
                &state.config.cronjob,
                &state.config.cronjob.deduplicate_cron,
                "deduplicate",
                Utc::now(),
            )?
            .to_rfc3339(),
        )
    } else {
        None
    };
    let Some(run) = run else {
        return Ok(Json(DeduplicateStatusResponse {
            status: "idle".to_string(),
            run_id: None,
            trigger: None,
            scheduled_for: None,
            started_at: None,
            completed_at: None,
            indexed_media: 0,
            processed_media: 0,
            candidate_comparisons: 0,
            clusters_created: 0,
            error: None,
            next_scheduled_at,
        }));
    };
    Ok(Json(DeduplicateStatusResponse {
        status: run.status,
        run_id: Some(run.id),
        trigger: Some(run.trigger),
        scheduled_for: run.scheduled_for,
        started_at: Some(run.started_at),
        completed_at: run.completed_at,
        indexed_media: run.indexed_media,
        processed_media: run.processed_media,
        candidate_comparisons: run.candidate_comparisons,
        clusters_created: run.clusters_created,
        error: run.error,
        next_scheduled_at,
    }))
}

async fn cancel(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
) -> AppResult<Json<DeduplicateActionResponse>> {
    let cancelled = request_cancel(&state.pool)?;
    Ok(Json(DeduplicateActionResponse {
        message: if cancelled {
            "Deduplicate cancellation requested".to_string()
        } else {
            "No deduplicate scan is running".to_string()
        },
        status: if cancelled {
            "cancelling".to_string()
        } else {
            "idle".to_string()
        },
    }))
}

async fn clean_indexes(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
) -> AppResult<Json<DeduplicateActionResponse>> {
    clean(&state.pool)?;
    Ok(Json(DeduplicateActionResponse {
        message: "Deduplicate indexes and groups cleaned".to_string(),
        status: "idle".to_string(),
    }))
}

async fn groups(
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
    let cluster_ids = fetch_all(
        &connection,
        queries::deduplicate::SELECT_VISIBLE_CLUSTER_IDS,
        &[&current_user.id, &cursor, &(request.limit + 1)],
        |row| row.get::<_, i64>(0),
    )?;
    let has_more = cluster_ids.len() > request.limit as usize;
    let selected_ids = cluster_ids
        .into_iter()
        .take(request.limit as usize)
        .collect::<Vec<_>>();
    let mut result_groups = Vec::with_capacity(selected_ids.len());
    for cluster_id in selected_ids {
        let items = fetch_all(
            &connection,
            queries::deduplicate::SELECT_VISIBLE_CLUSTER_MEDIA,
            &[&cluster_id, &current_user.id],
            map_media_row,
        )?;
        if items.len() >= 2 {
            result_groups.push(DeduplicateGroup { cluster_id, items });
        }
    }
    let next_cursor = if has_more {
        result_groups
            .last()
            .map(|group| group.cluster_id.to_string())
    } else {
        None
    };
    Ok(Json(DeduplicateGroupsResponse {
        groups: result_groups,
        next_cursor,
        has_more,
    }))
}
