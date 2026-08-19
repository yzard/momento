use axum::{extract::State, routing::post, Json, Router};
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use serde::{Deserialize, Serialize};

use crate::auth::{AppState, CurrentUser};
use crate::constants::paths;
use crate::database::{fetch_all, fetch_one, queries};
use crate::error::{AppError, AppResult};
use crate::models::{
    PlaceGetRequest, PlaceGetResponse, PlaceSummary, PlaceThumbnailRequest, PlaceThumbnailResponse,
    PlacesListRequest, PlacesListResponse,
};

use super::media::map_media_row;

const MAX_PAGE_LIMIT: i64 = 200;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlaceIdentity {
    city: String,
    state: Option<String>,
    country: String,
}

struct PlaceRow {
    city: String,
    state: Option<String>,
    country: String,
    media_count: i64,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/places/list", post(list_places))
        .route("/places/get", post(get_place))
        .route("/places/thumbnail", post(get_place_thumbnail))
}

async fn get_place_thumbnail(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<PlaceThumbnailRequest>,
) -> AppResult<Json<PlaceThumbnailResponse>> {
    let identity = decode_place_id(&request.place_id)?;
    let connection = state.pool.get()?;
    let cover_query = queries::places::select_cover_query();
    let cover = fetch_one(
        &connection,
        &cover_query,
        &[
            &current_user.id,
            &identity.city,
            &identity.state,
            &identity.country,
        ],
        |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?)),
    )?
    .ok_or_else(|| AppError::NotFound("Place not found".to_string()))?;
    drop(connection);
    let thumbnail_relative = super::media::thumbnail_relative_path(cover.0.as_deref(), &cover.1);
    let thumbnail_path = paths().thumbnails_places.join(thumbnail_relative);
    let thumbnail = match tokio::fs::read(thumbnail_path).await {
        Ok(bytes) => Some(format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    Ok(Json(PlaceThumbnailResponse { thumbnail }))
}

async fn list_places(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<PlacesListRequest>,
) -> AppResult<Json<PlacesListResponse>> {
    validate_limit(request.limit)?;
    let offset = decode_cursor(request.cursor.as_deref())?;
    let connection = state.pool.get()?;
    let query = queries::places::select_page_query();
    let mut rows = fetch_all(
        &connection,
        &query,
        &[&current_user.id, &(request.limit + 1), &offset],
        map_place_row,
    )?;
    let has_more = rows.len() > request.limit as usize;
    rows.truncate(request.limit as usize);
    let places = rows
        .into_iter()
        .map(place_summary)
        .collect::<AppResult<Vec<_>>>()?;
    let next_cursor = has_more.then(|| encode_cursor(offset + request.limit));
    Ok(Json(PlacesListResponse {
        places,
        next_cursor,
        has_more,
    }))
}

async fn get_place(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<PlaceGetRequest>,
) -> AppResult<Json<PlaceGetResponse>> {
    validate_limit(request.limit)?;
    let identity = decode_place_id(&request.place_id)?;
    let offset = decode_cursor(request.cursor.as_deref())?;
    let connection = state.pool.get()?;
    let summary_query = queries::places::select_summary_query();
    let place = fetch_one(
        &connection,
        &summary_query,
        &[
            &current_user.id,
            &identity.city,
            &identity.state,
            &identity.country,
        ],
        map_place_row,
    )?
    .ok_or_else(|| AppError::NotFound("Place not found".to_string()))?;
    let place = place_summary(place)?;
    let mut media = fetch_all(
        &connection,
        queries::places::SELECT_MEDIA_PAGE,
        &[
            &current_user.id,
            &identity.city,
            &identity.state,
            &identity.country,
            &(request.limit + 1),
            &offset,
        ],
        map_media_row,
    )?;
    let has_more = media.len() > request.limit as usize;
    media.truncate(request.limit as usize);
    let next_cursor = has_more.then(|| encode_cursor(offset + request.limit));
    Ok(Json(PlaceGetResponse {
        place,
        media,
        next_cursor,
        has_more,
    }))
}

fn validate_limit(limit: i64) -> AppResult<()> {
    if !(1..=MAX_PAGE_LIMIT).contains(&limit) {
        return Err(AppError::Validation(format!(
            "limit must be between 1 and {MAX_PAGE_LIMIT}"
        )));
    }
    Ok(())
}

fn map_place_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PlaceRow> {
    Ok(PlaceRow {
        city: row.get(0)?,
        state: row.get(1)?,
        country: row.get(2)?,
        media_count: row.get(3)?,
    })
}

fn place_summary(row: PlaceRow) -> AppResult<PlaceSummary> {
    let identity = PlaceIdentity {
        city: row.city,
        state: row.state,
        country: row.country,
    };
    let place_id = encode_place_id(&identity)?;
    Ok(PlaceSummary {
        place_id,
        city: identity.city,
        state: identity.state,
        country: identity.country,
        media_count: row.media_count,
    })
}

fn encode_place_id(identity: &PlaceIdentity) -> AppResult<String> {
    Ok(URL_SAFE_NO_PAD.encode(serde_json::to_vec(identity)?))
}

fn decode_place_id(place_id: &str) -> AppResult<PlaceIdentity> {
    let bytes = URL_SAFE_NO_PAD
        .decode(place_id)
        .map_err(|_| AppError::Validation("placeId is invalid".to_string()))?;
    let identity: PlaceIdentity = serde_json::from_slice(&bytes)
        .map_err(|_| AppError::Validation("placeId is invalid".to_string()))?;
    if identity.city.trim().is_empty() || identity.country.trim().is_empty() {
        return Err(AppError::Validation("placeId is invalid".to_string()));
    }
    Ok(identity)
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
