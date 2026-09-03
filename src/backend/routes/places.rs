use axum::{
    extract::{Extension, Path, State},
    http::HeaderMap,
    response::Response,
    routing::{get, post},
    Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

use crate::auth::{AppState, CurrentUser};
use crate::database::operations::{PlaceIdentityQuery, PlaceMediaQuery, PlacePageQuery};
use crate::error::{AppError, AppResult};
use crate::executor::PlaceIdentityDto;
use crate::io::file::StorageRootId;
use crate::models::{PlaceGetRequest, PlaceGetResponse, PlacesListRequest, PlacesListResponse};
use crate::routes::{
    file_stream::{serve_file, ContentDisposition, FileResponseOptions},
    render_json, CpuJson,
};
use crate::runtime::HttpRequestAdmission;

const MAX_PAGE_LIMIT: i64 = 200;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/places/list", post(list_places))
        .route("/places/get", post(get_place))
        .route("/places/:place_id/thumbnail", get(get_place_thumbnail))
}

async fn get_place_thumbnail(
    State(state): State<AppState>,
    Extension(admission): Extension<HttpRequestAdmission>,
    current_user: CurrentUser,
    Path(place_id): Path<String>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let identity = state.executors.cpu.decode_place_identity(place_id).await?;
    let cover = state
        .executors
        .sqlite
        .load_place_cover_request(identity_query(current_user.id, identity))
        .await?
        .ok_or_else(|| AppError::NotFound("Place not found".to_string()))?;
    let Some(thumbnail_path) =
        super::media::thumbnail_relative_path(cover.thumbnail_path.as_deref())
    else {
        return Err(AppError::NotFound("Place thumbnail not found".to_string()));
    };
    serve_file(
        &state.executors.file_io,
        StorageRootId::PlaceThumbnails,
        thumbnail_path,
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

async fn list_places(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<PlacesListRequest>,
) -> AppResult<Response> {
    validate_limit(request.limit)?;
    let offset = decode_cursor(request.cursor.as_deref())?;
    let mut rows = state
        .executors
        .sqlite
        .load_places_page_request(PlacePageQuery {
            user_id: current_user.id,
            limit: request.limit,
            offset,
        })
        .await?;
    let has_more = rows.len() > request.limit as usize;
    rows.truncate(request.limit as usize);
    let places = state.executors.cpu.build_place_summaries(rows).await?;
    let next_cursor = has_more.then(|| encode_cursor(offset + request.limit));
    render_json(
        &state,
        PlacesListResponse {
            places,
            next_cursor,
            has_more,
        },
    )
    .await
}

async fn get_place(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<PlaceGetRequest>,
) -> AppResult<Response> {
    validate_limit(request.limit)?;
    let identity = state
        .executors
        .cpu
        .decode_place_identity(request.place_id)
        .await?;
    let offset = decode_cursor(request.cursor.as_deref())?;
    let page = state
        .executors
        .sqlite
        .load_place_media_page_request(PlaceMediaQuery {
            identity: identity_query(current_user.id, identity),
            limit: request.limit,
            offset,
        })
        .await?
        .ok_or_else(|| AppError::NotFound("Place not found".to_string()))?;
    let place = state
        .executors
        .cpu
        .build_place_summaries(vec![page.place])
        .await?
        .pop()
        .ok_or_else(|| AppError::Internal("CPU returned no place summary".to_string()))?;
    let has_more = page.has_more;
    let next_cursor = has_more.then(|| encode_cursor(offset + request.limit));
    render_json(
        &state,
        PlaceGetResponse {
            place,
            media: page.media,
            next_cursor,
            has_more,
        },
    )
    .await
}

fn validate_limit(limit: i64) -> AppResult<()> {
    if !(1..=MAX_PAGE_LIMIT).contains(&limit) {
        return Err(AppError::Validation(format!(
            "limit must be between 1 and {MAX_PAGE_LIMIT}"
        )));
    }
    Ok(())
}

fn identity_query(user_id: i64, identity: PlaceIdentityDto) -> PlaceIdentityQuery {
    PlaceIdentityQuery {
        user_id,
        city: identity.city,
        state: identity.state,
        country: identity.country,
    }
}

fn encode_cursor(offset: i64) -> String {
    URL_SAFE_NO_PAD.encode(offset.to_string())
}

fn decode_cursor(cursor: Option<&str>) -> AppResult<i64> {
    let Some(cursor) = cursor else {
        return Ok(0);
    };
    let bytes = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| AppError::Validation("cursor is invalid".to_string()))?;
    let offset = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value >= 0)
        .ok_or_else(|| AppError::Validation("cursor is invalid".to_string()))?;
    Ok(offset)
}
