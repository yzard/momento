use std::collections::HashSet;

use crate::auth::{AppState, CurrentUser, RequireAdmin};
use crate::database::operations::{FaceGroupQuery, FaceGroupsPageQuery};
use crate::error::{AppError, AppResult};
use crate::executor::FaceGroupMergeResponse;
use crate::io::file::{NormalizedStoragePath, StorageRootId};
use crate::models::{
    FaceGroupRequest, FaceGroupResponse, FaceGroupsListRequest, FaceGroupsMergeRequest,
};
use crate::processor::face_detection::MergeFaceGroupsOutcome;
use crate::routes::{
    file_stream::{serve_file, ContentDisposition, FileResponseOptions},
    render_json, CpuJson,
};
use crate::runtime::HttpRequestAdmission;
use axum::{
    extract::{Extension, Path, State},
    http::HeaderMap,
    response::Response,
    routing::{get, post},
    Router,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/faces/groups/list", post(list_groups))
        .route("/faces/groups/get", post(get_group))
        .route("/faces/groups/:face_group_id/thumbnail", get(get_thumbnail))
        .route("/faces/groups/merge", post(merge_groups))
}

async fn list_groups(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<FaceGroupsListRequest>,
) -> AppResult<Response> {
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
    let page = state
        .executors
        .sqlite
        .load_face_groups_page_request(FaceGroupsPageQuery {
            user_id: current_user.id,
            limit,
            offset,
        })
        .await?;
    render_json(&state, page).await
}

async fn get_group(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<FaceGroupRequest>,
) -> AppResult<Response> {
    let group = state
        .executors
        .sqlite
        .load_face_group_request(FaceGroupQuery {
            user_id: current_user.id,
            face_group_id: request.face_group_id,
        })
        .await?
        .ok_or_else(|| AppError::NotFound("Face group not found".to_string()))?;
    render_json(&state, group).await
}

async fn get_thumbnail(
    State(state): State<AppState>,
    Extension(admission): Extension<HttpRequestAdmission>,
    current_user: CurrentUser,
    Path(face_group_id): Path<i64>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let config = state.config.current();
    let crop_path = state
        .executors
        .sqlite
        .load_visible_face_representative_request(
            face_group_id,
            current_user.id,
            config.face_group.clone(),
        )
        .await?
        .ok_or_else(|| AppError::NotFound("Face group thumbnail not found".to_string()))?;
    let path = NormalizedStoragePath::parse(&crop_path)
        .map_err(|_| AppError::NotFound("Face group thumbnail not found".to_string()))?;
    serve_file(
        &state.executors.file_io,
        StorageRootId::Previews,
        path,
        FileResponseOptions {
            admission: &admission,
            content_type: "image/jpeg",
            headers: &headers,
            filename: None,
            allow_ranges: false,
            content_disposition: ContentDisposition::Inline,
            cache_control: "private",
            head_only: false,
        },
    )
    .await
}

async fn merge_groups(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    CpuJson(request): CpuJson<FaceGroupsMergeRequest>,
) -> AppResult<Response> {
    let config = state.config.current();
    let group_ids = request.face_group_ids.into_iter().collect::<HashSet<_>>();
    if group_ids.len() < 2 {
        return Err(AppError::BadRequest(
            "faceGroupIds must contain at least two unique groups".to_string(),
        ));
    }
    let mut ordered_ids = group_ids.into_iter().collect::<Vec<_>>();
    ordered_ids.sort_unstable();
    let MergeFaceGroupsOutcome::Merged(group) = state
        .executors
        .sqlite
        .merge_face_groups_request(ordered_ids, config.face_group.clone())
        .await?
    else {
        return Err(AppError::NotFound("Face group not found".to_string()));
    };
    render_json(
        &state,
        FaceGroupMergeResponse {
            group: FaceGroupResponse {
                face_group_id: group.face_group_id,
                face_count: group.face_count,
                media_count: group.media_count,
            },
        },
    )
    .await
}
