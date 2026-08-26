use std::collections::HashSet;

use axum::{
    body::Body, extract::State, http::header, response::Response, routing::post, Json, Router,
};
use rusqlite::{Transaction, TransactionBehavior};

use crate::auth::{AppState, CurrentUser, RequireAdmin};
use crate::database::{fetch_all, fetch_one, queries};
use crate::error::{AppError, AppResult};
use crate::models::{
    FaceGroupMediaResponse, FaceGroupRequest, FaceGroupResponse, FaceGroupsListRequest,
    FaceGroupsListResponse, FaceGroupsMergeRequest,
};
use crate::processor::face_detection;
use crate::routes::media::map_media_row;
use crate::utils::path::resolve_existing_storage_path;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/faces/groups/list", post(list_groups))
        .route("/faces/groups/get", post(get_group))
        .route("/faces/thumbnails/get", post(get_thumbnail))
        .route("/faces/groups/merge", post(merge_groups))
}

async fn list_groups(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<FaceGroupsListRequest>,
) -> AppResult<Json<FaceGroupsListResponse>> {
    let limit = request.limit.unwrap_or(100).clamp(1, 200);
    let offset = request
        .cursor
        .as_deref()
        .unwrap_or("0")
        .parse::<i64>()
        .map_err(|_| AppError::BadRequest("cursor must be a numeric offset".to_string()))?;
    if offset < 0 {
        return Err(AppError::BadRequest(
            "cursor must be a non-negative numeric offset".to_string(),
        ));
    }
    let connection = state.pool.get().map_err(AppError::Pool)?;
    let groups = fetch_all(
        &connection,
        queries::faces::LIST_GROUPS,
        &[&current_user.id, &limit, &offset],
        |row| {
            Ok(FaceGroupResponse {
                face_group_id: row.get(0)?,
                face_count: row.get(1)?,
                media_count: row.get(2)?,
            })
        },
    )?;
    let total: i64 = connection.query_row(
        queries::faces::COUNT_VISIBLE_GROUPS,
        [current_user.id],
        |row| row.get(0),
    )?;
    let next_offset = offset + groups.len() as i64;
    Ok(Json(FaceGroupsListResponse {
        has_more: next_offset < total,
        next_cursor: (next_offset < total).then(|| next_offset.to_string()),
        groups,
    }))
}

async fn get_group(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<FaceGroupRequest>,
) -> AppResult<Json<FaceGroupMediaResponse>> {
    let connection = state.pool.get().map_err(AppError::Pool)?;
    let group = fetch_one(
        &connection,
        queries::faces::SELECT_GROUP,
        &[&request.face_group_id, &current_user.id],
        |row| {
            Ok(FaceGroupResponse {
                face_group_id: row.get(0)?,
                face_count: row.get(1)?,
                media_count: row.get(2)?,
            })
        },
    )?
    .ok_or_else(|| AppError::NotFound("Face group not found".to_string()))?;
    let media = fetch_all(
        &connection,
        queries::faces::SELECT_GROUP_MEDIA,
        &[&request.face_group_id, &current_user.id],
        map_media_row,
    )?;
    Ok(Json(FaceGroupMediaResponse { group, media }))
}

async fn get_thumbnail(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<FaceGroupRequest>,
) -> AppResult<Response> {
    let connection = state.pool.get().map_err(AppError::Pool)?;
    let crop_path = face_detection::visible_representative_crop(
        &connection,
        request.face_group_id,
        current_user.id,
        &state.config.face_group_representative,
    )?
    .ok_or_else(|| AppError::NotFound("Face group thumbnail not found".to_string()))?;
    let path =
        resolve_existing_storage_path(&crate::constants::paths().previews, &crop_path).await?;
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|_| AppError::NotFound("Face group thumbnail not found".to_string()))?;
    Response::builder()
        .header(header::CONTENT_TYPE, "image/jpeg")
        .body(Body::from(bytes))
        .map_err(|error| AppError::Internal(error.to_string()))
}

async fn merge_groups(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(request): Json<FaceGroupsMergeRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let group_ids = request.face_group_ids.into_iter().collect::<HashSet<_>>();
    if group_ids.len() < 2 {
        return Err(AppError::BadRequest(
            "faceGroupIds must contain at least two unique groups".to_string(),
        ));
    }
    let mut ordered_ids = group_ids.into_iter().collect::<Vec<_>>();
    ordered_ids.sort_unstable();
    let parameters = ordered_ids
        .iter()
        .map(|id| id as &dyn rusqlite::ToSql)
        .collect::<Vec<_>>();
    let connection = state.pool.get().map_err(AppError::Pool)?;
    let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)?;
    let found = transaction
        .prepare(&queries::faces::build_existing_groups_query(
            ordered_ids.len(),
        ))?
        .query_map(parameters.as_slice(), |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if found.len() != ordered_ids.len() {
        return Err(AppError::NotFound("Face group not found".to_string()));
    }
    let target_id = ordered_ids[0];
    let members = transaction
        .prepare(&queries::faces::build_merge_members_query(
            ordered_ids.len(),
        ))?
        .query_map(parameters.as_slice(), |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for face_id in members {
        transaction.execute(queries::faces::INSERT_MANUAL_MEMBER, [target_id, face_id])?;
    }
    transaction.execute(queries::faces::UPDATE_MANUAL_GROUP, [target_id])?;
    for source_id in ordered_ids.into_iter().skip(1) {
        transaction.execute(queries::faces::DELETE_GROUP, [source_id])?;
    }
    face_detection::update_group_representative(
        &transaction,
        target_id,
        &state.config.face_group_representative,
    )?;
    let face_count: i64 =
        transaction.query_row(queries::faces::COUNT_GROUP_MEMBERS, [target_id], |row| {
            row.get(0)
        })?;
    let media_count: i64 =
        transaction.query_row(queries::faces::COUNT_GROUP_MEDIA, [target_id], |row| {
            row.get(0)
        })?;
    transaction.commit()?;
    Ok(Json(
        serde_json::json!({"group": {"faceGroupId": target_id, "faceCount": face_count, "mediaCount": media_count}}),
    ))
}
