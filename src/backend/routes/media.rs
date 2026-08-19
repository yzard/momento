use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Utc, Weekday};
use indexmap::IndexMap;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

use crate::auth::{AppState, CurrentUser};
use crate::constants::{media_text_model_name, paths};
use crate::database::{execute_query, fetch_all, fetch_one, queries};
use crate::error::{AppError, AppResult};
use crate::models::{
    DeleteMediaResponse, ImageTextSearchRequest, ImageTextSearchResponse, ImageTextSearchResult,
    MediaBatchRequest, MediaBatchResponse, MediaDeleteRequest, MediaListRequest, MediaListResponse,
    MediaResponse, MediaUpdateRequest, PreviewBatchRequest, PreviewBatchResponse,
    ThumbnailBatchRequest, ThumbnailBatchResponse, ThumbnailSize, TimelineDirection,
    TimelineListRequest, TimelineListResponse, TimelineMarker, TimelineMarkersRequest,
    TimelineMarkersResponse,
};
use crate::processor::media_processor::{calculate_geohash, delete_from_rtree, insert_into_rtree};
use crate::processor::metadata::reverse_geocoding::reverse_geocode;
use crate::processor::thumbnails::generate_image_preview;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

const TIMELINE_START_DATE: &str = "0000-01-01T00:00:00";
const TIMELINE_END_DATE: &str = "9999-12-31T23:59:59";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/media/list", post(list_media))
        .route("/timeline/list", post(list_timeline))
        .route("/timeline/markers", post(get_timeline_markers))
        .route("/media/search", post(search_media))
        .route("/media/get-batch", post(get_media_batch))
        .route("/media/update", post(update_media))
        .route("/media/delete", post(delete_media))
        .route("/media/file/:media_id", get(get_media_file))
}

pub fn thumbnail_router() -> Router<AppState> {
    Router::new().route("/thumbnail/get", post(get_media_thumbnail_batch))
}

pub fn preview_router() -> Router<AppState> {
    Router::new().route("/preview/get", post(get_media_preview_batch))
}

struct MediaRowData {
    id: i64,
    filename: String,
    original_filename: String,
    media_type: String,
    mime_type: Option<String>,
    width: Option<i32>,
    height: Option<i32>,
    file_size: Option<i64>,
    duration_seconds: Option<f64>,
    date_taken: Option<String>,
    gps_latitude: Option<f64>,
    gps_longitude: Option<f64>,
    camera_make: Option<String>,
    camera_model: Option<String>,
    lens_make: Option<String>,
    lens_model: Option<String>,
    iso: Option<i32>,
    exposure_time: Option<String>,
    f_number: Option<f64>,
    focal_length: Option<f64>,
    focal_length_35mm: Option<f64>,
    gps_altitude: Option<f64>,
    location_city: Option<String>,
    location_state: Option<String>,
    location_country: Option<String>,
    video_codec: Option<String>,
    keywords: Option<String>,
    created_at: String,
}

impl MediaRowData {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            filename: row.get(1)?,
            original_filename: row.get(2)?,
            media_type: row.get(3)?,
            mime_type: row.get(4)?,
            width: row.get(5)?,
            height: row.get(6)?,
            file_size: row.get(7)?,
            duration_seconds: row.get(8)?,
            date_taken: row.get(9)?,
            gps_latitude: row.get(10)?,
            gps_longitude: row.get(11)?,
            camera_make: row.get(12)?,
            camera_model: row.get(13)?,
            lens_make: row.get(14)?,
            lens_model: row.get(15)?,
            iso: row.get(16)?,
            exposure_time: row.get(17)?,
            f_number: row.get(18)?,
            focal_length: row.get(19)?,
            focal_length_35mm: row.get(20)?,
            gps_altitude: row.get(21)?,
            location_city: row.get(22)?,
            location_state: row.get(23)?,
            location_country: row.get(24)?,
            video_codec: row.get(25)?,
            keywords: row.get(26)?,
            created_at: row.get(27)?,
        })
    }
}

fn row_to_media_response(row: MediaRowData) -> MediaResponse {
    let MediaRowData {
        id,
        filename,
        original_filename,
        media_type,
        mime_type,
        width,
        height,
        file_size,
        duration_seconds,
        date_taken,
        gps_latitude,
        gps_longitude,
        camera_make,
        camera_model,
        lens_make,
        lens_model,
        iso,
        exposure_time,
        f_number,
        focal_length,
        focal_length_35mm,
        gps_altitude,
        location_city,
        location_state,
        location_country,
        video_codec,
        keywords,
        created_at,
    } = row;
    MediaResponse {
        id,
        filename,
        original_filename,
        media_type,
        mime_type,
        width,
        height,
        file_size,
        duration_seconds,
        date_taken,
        gps_latitude,
        gps_longitude,
        camera_make,
        camera_model,
        lens_make,
        lens_model,
        iso,
        exposure_time,
        f_number,
        focal_length,
        focal_length_35mm,
        gps_altitude,
        location_city,
        location_state,
        location_country,
        video_codec,
        keywords,
        created_at,
        content_hash: None,
    }
}

async fn list_media(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<MediaListRequest>,
) -> AppResult<Json<MediaListResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;
    let search = normalize_media_text_search(request.search.as_deref());

    if request.limit.is_none() && request.cursor.is_none() {
        let items = fetch_all(
            &conn,
            queries::media::SELECT_ALL_FOR_USER,
            &[&current_user.id, &search, &search],
            map_media_row,
        )?;

        return Ok(Json(MediaListResponse {
            items,
            next_cursor: None,
            has_more: false,
        }));
    }

    let limit = request.limit.unwrap_or(100);
    let rows = if let Some(ref cursor) = request.cursor {
        let parts: Vec<&str> = cursor.split('_').collect();
        if parts.len() == 2 {
            let cursor_date = parts[0];
            let cursor_id: i64 = parts[1].parse().unwrap_or(0);
            fetch_all(
                &conn,
                queries::media::SELECT_PAGINATED_FOR_USER,
                &[
                    &current_user.id,
                    &search,
                    &search,
                    &cursor_date,
                    &cursor_date,
                    &cursor_id,
                    &(limit + 1),
                ],
                map_media_row,
            )?
        } else {
            fetch_default_media(&conn, current_user.id, limit, &search)?
        }
    } else {
        fetch_default_media(&conn, current_user.id, limit, &search)?
    };

    let has_more = rows.len() > limit as usize;
    let items: Vec<MediaResponse> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more && !items.is_empty() {
        let last = items.last().unwrap();
        last.date_taken
            .as_ref()
            .map(|dt| format!("{}_{}", dt, last.id))
    } else {
        None
    };

    Ok(Json(MediaListResponse {
        items,
        next_cursor,
        has_more,
    }))
}

async fn list_timeline(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<TimelineListRequest>,
) -> AppResult<Json<TimelineListResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;
    let search = normalize_media_text_search(Some(&request.search));
    let response = query_timeline(&conn, current_user.id, &request, &search)?;
    Ok(Json(response))
}

fn query_timeline(
    conn: &crate::database::DbConn,
    user_id: i64,
    request: &TimelineListRequest,
    search: &str,
) -> AppResult<TimelineListResponse> {
    if request.direction == TimelineDirection::Newer && request.cursor.is_none() {
        return Err(AppError::BadRequest(
            "newer timeline requests require a cursor".to_string(),
        ));
    }
    let media_type = validate_media_type(request.media_type.as_deref())?;
    let classification = validate_classification(request.classification.as_deref())?;

    let page = fetch_timeline_page(
        conn,
        TimelineQuery {
            user_id,
            cursor: request.cursor.as_deref(),
            search,
            media_type,
            classification,
            start_date: TIMELINE_START_DATE,
            end_date: TIMELINE_END_DATE,
            direction: request.direction,
            anchor_date: request.anchor_date.as_deref(),
            group_by: &request.group_by,
        },
    )?;
    let TimelinePage { rows, has_more } = page;

    let mut grouped: IndexMap<String, Vec<MediaResponse>> = IndexMap::new();
    for (media, date_taken) in &rows {
        let key = timeline_group_key(date_taken.as_deref(), &request.group_by);
        grouped.entry(key).or_default().push(media.clone());
    }
    let groups = grouped
        .into_iter()
        .map(|(date, media)| crate::models::TimelineGroup { date, media })
        .collect();

    let first_cursor = rows
        .first()
        .and_then(|(media, date)| date.as_ref().map(|date| format!("{}_{}", date, media.id)));
    let last_cursor = rows
        .last()
        .and_then(|(media, date)| date.as_ref().map(|date| format!("{}_{}", date, media.id)));
    let has_older = if request.direction == TimelineDirection::Older {
        has_more
    } else {
        first_cursor.is_some()
    };
    let has_newer = if request.direction == TimelineDirection::Newer {
        has_more
    } else if request.cursor.is_some() {
        true
    } else if let Some(cursor) = first_cursor.as_deref() {
        let filter = TimelineFilter {
            user_id,
            search,
            media_type: media_type.unwrap_or(""),
            classification: classification.unwrap_or(""),
            start_date: TIMELINE_START_DATE,
            end_date: TIMELINE_END_DATE,
        };
        !fetch_timeline_candidate(conn, filter, TimelineDirection::Newer, Some(cursor), None)?
            .is_empty()
    } else {
        false
    };

    Ok(TimelineListResponse {
        groups,
        next_cursor: if has_older {
            if request.direction == TimelineDirection::Older {
                last_cursor.clone()
            } else {
                first_cursor.clone()
            }
        } else {
            None
        },
        previous_cursor: if has_newer {
            if request.direction == TimelineDirection::Older {
                first_cursor.clone()
            } else {
                last_cursor.clone()
            }
        } else {
            None
        },
        has_older,
        has_newer,
    })
}

async fn get_timeline_markers(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<TimelineMarkersRequest>,
) -> AppResult<Json<TimelineMarkersResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;
    let search = normalize_media_text_search(Some(&request.search));
    let media_type = validate_media_type(request.media_type.as_deref())?;
    let media_type_value = media_type.unwrap_or("");
    let classification = validate_classification(request.classification.as_deref())?;
    let classification_value = classification.unwrap_or("");
    let rows: Vec<(String, String)> = fetch_all(
        &conn,
        queries::timeline::SELECT_MONTH_MARKERS,
        &[
            &current_user.id,
            &search,
            &search,
            &media_type_value,
            &media_type_value,
            &classification_value,
            &classification_value,
            &classification_value,
        ],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    let markers = rows
        .into_iter()
        .map(|(label, anchor_date)| TimelineMarker { label, anchor_date })
        .collect::<Vec<_>>();

    Ok(Json(TimelineMarkersResponse { markers }))
}

fn validate_media_type(media_type: Option<&str>) -> AppResult<Option<&str>> {
    match media_type {
        None | Some("image") | Some("video") => Ok(media_type),
        Some(_) => Err(AppError::BadRequest(
            "mediaType must be either image or video".to_string(),
        )),
    }
}

fn validate_classification(classification: Option<&str>) -> AppResult<Option<&str>> {
    match classification {
        None | Some("screenshot") | Some("document") => Ok(classification),
        Some(_) => Err(AppError::BadRequest(
            "classification must be either screenshot or document".to_string(),
        )),
    }
}

fn timeline_period_bounds(date_taken: &str, group_by: &str) -> AppResult<(String, String)> {
    if date_taken.len() < 10 {
        return Err(AppError::BadRequest("Invalid timeline date".to_string()));
    }
    let date = NaiveDate::parse_from_str(&date_taken[..10], "%Y-%m-%d")
        .map_err(|_| AppError::BadRequest("Invalid timeline date".to_string()))?;
    let start = match group_by {
        "year" => NaiveDate::from_ymd_opt(date.year(), 1, 1),
        "month" => NaiveDate::from_ymd_opt(date.year(), date.month(), 1),
        "week" => {
            let iso_week = date.iso_week();
            NaiveDate::from_isoywd_opt(iso_week.year(), iso_week.week(), Weekday::Mon)
        }
        "day" => Some(date),
        _ => return Err(AppError::BadRequest("Invalid groupBy".to_string())),
    }
    .ok_or_else(|| AppError::BadRequest("Invalid timeline period".to_string()))?;
    let end = match group_by {
        "year" => NaiveDate::from_ymd_opt(date.year() + 1, 1, 1),
        "month" => {
            let next_month = start
                .checked_add_months(chrono::Months::new(1))
                .ok_or_else(|| AppError::BadRequest("Invalid timeline period".to_string()))?;
            Some(next_month)
        }
        "week" => start.checked_add_signed(Duration::days(7)),
        "day" => start.checked_add_signed(Duration::days(1)),
        _ => return Err(AppError::BadRequest("Invalid groupBy".to_string())),
    }
    .ok_or_else(|| AppError::BadRequest("Invalid timeline period".to_string()))?;

    Ok((
        format!("{}T00:00:00", start.format("%Y-%m-%d")),
        format!("{}T00:00:00", end.format("%Y-%m-%d")),
    ))
}

async fn search_media(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<ImageTextSearchRequest>,
) -> AppResult<Json<ImageTextSearchResponse>> {
    let search = normalize_media_text_search(Some(request.search.as_str()));
    if search.is_empty() {
        return Ok(Json(ImageTextSearchResponse {
            results: Vec::new(),
        }));
    }

    let conn = state.pool.get().map_err(AppError::Pool)?;
    let matches: Vec<(i64, String)> = fetch_all(
        &conn,
        queries::media_text::SEARCH_FOR_USER,
        &[&search, &current_user.id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    let mut results: HashMap<i64, Vec<String>> = HashMap::new();
    for (image_id, model_type) in matches {
        let Some(model_name) = media_text_model_name(&model_type) else {
            continue;
        };

        let models = results.entry(image_id).or_default();
        if !models.iter().any(|name| name == model_name) {
            models.push(model_name.to_string());
        }
    }

    let mut results: Vec<ImageTextSearchResult> = results
        .into_iter()
        .map(|(image_id, mut models)| {
            models.sort();
            ImageTextSearchResult { image_id, models }
        })
        .collect();
    results.sort_by_key(|result| result.image_id);

    Ok(Json(ImageTextSearchResponse { results }))
}

fn normalize_media_text_search(search: Option<&str>) -> String {
    let search = search.unwrap_or_default().trim();
    if search.is_empty() {
        return String::new();
    }

    format!(
        "%{}%",
        search
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    )
}

async fn get_media_batch(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<MediaBatchRequest>,
) -> AppResult<Json<MediaBatchResponse>> {
    if request.ids.is_empty() {
        return Ok(Json(MediaBatchResponse { items: Vec::new() }));
    }

    let conn = state.pool.get().map_err(AppError::Pool)?;
    let query = queries::media::build_select_by_ids(request.ids.len());
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::with_capacity(request.ids.len() + 1);
    params.push(Box::new(current_user.id));
    for media_id in &request.ids {
        params.push(Box::new(*media_id));
    }

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|param| param.as_ref()).collect();
    let items = fetch_all(&conn, &query, &param_refs, map_media_row)?;

    let mut by_id = std::collections::HashMap::new();
    for item in items {
        by_id.insert(item.id, item);
    }

    let ordered_items = request
        .ids
        .iter()
        .filter_map(|media_id| by_id.get(media_id).cloned())
        .collect();

    Ok(Json(MediaBatchResponse {
        items: ordered_items,
    }))
}

async fn update_media(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<MediaUpdateRequest>,
) -> AppResult<Json<MediaResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let exists = fetch_one(
        &conn,
        queries::media::CHECK_EXISTS,
        &[&request.media_id, &current_user.id],
        |row| row.get::<_, i64>(0),
    )?;

    if exists.is_none() {
        return Err(AppError::NotFound("Media not found".to_string()));
    }

    if request.date_taken.is_some()
        || request.gps_latitude.is_some()
        || request.gps_longitude.is_some()
    {
        execute_query(
            &conn,
            queries::media::UPSERT_EDITABLE_METADATA,
            &[
                &request.media_id,
                &request.date_taken,
                &request.gps_latitude,
                &request.gps_longitude,
            ],
        )?;
    }

    let mut media = fetch_one(
        &conn,
        queries::media::SELECT_BY_ID_AND_USER,
        &[&request.media_id, &current_user.id],
        map_media_row,
    )?
    .ok_or_else(|| AppError::NotFound("Media not found".to_string()))?;

    if request.gps_latitude.is_some() || request.gps_longitude.is_some() {
        let geohash = match (media.gps_latitude, media.gps_longitude) {
            (Some(lat), Some(lon)) => calculate_geohash(lat, lon),
            _ => None,
        };
        let location = match (media.gps_latitude, media.gps_longitude) {
            (Some(latitude), Some(longitude)) => {
                reverse_geocode(latitude, longitude).map_err(AppError::Internal)?
            }
            _ => None,
        };
        let (city, location_state, country) = location
            .map(|location| (Some(location.city), location.state, Some(location.country)))
            .unwrap_or((None, None, None));

        execute_query(
            &conn,
            queries::media::UPDATE_LOCATION,
            &[&geohash, &city, &location_state, &country, &media.id],
        )?;
        media.location_city = city;
        media.location_state = location_state;
        media.location_country = country;

        delete_from_rtree(&conn, media.id).map_err(AppError::from)?;

        if let (Some(lat), Some(lon)) = (media.gps_latitude, media.gps_longitude) {
            insert_into_rtree(&conn, media.id, lat, lon).map_err(AppError::from)?;
        }
    }

    Ok(Json(media))
}

async fn delete_media(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<MediaDeleteRequest>,
) -> AppResult<Json<DeleteMediaResponse>> {
    if request.media_ids.is_empty() {
        return Ok(Json(DeleteMediaResponse {
            message: "No media to delete".to_string(),
        }));
    }
    let media_ids = request.media_ids.into_iter().collect::<HashSet<_>>();
    if media_ids.len() > 500 {
        return Err(AppError::BadRequest(
            "mediaIds must contain at most 500 unique IDs".to_string(),
        ));
    }
    let conn = state.pool.get().map_err(AppError::Pool)?;
    let deleted_at = Utc::now().to_rfc3339();
    let transaction = conn.unchecked_transaction()?;
    let mut deleted_count = 0;
    for media_id in media_ids {
        deleted_count += transaction.execute(
            queries::media::UPDATE_DELETED_AT,
            rusqlite::params![deleted_at, media_id, current_user.id],
        )?;
    }
    transaction.commit()?;

    Ok(Json(DeleteMediaResponse {
        message: format!("{} media moved to trash", deleted_count),
    }))
}

async fn get_media_file(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Path(media_id): Path<i64>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let media = fetch_one(
        &conn,
        queries::media::SELECT_FILE_INFO,
        &[&media_id, &current_user.id],
        |row| {
            Ok(FileInfo {
                file_path: row.get(0)?,
                mime_type: row.get(1)?,
                original_filename: row.get(2)?,
            })
        },
    )?
    .ok_or_else(|| AppError::NotFound("Media not found".to_string()))?;

    let full_path = paths().originals.join(&media.file_path);
    if !full_path.exists() {
        return Err(AppError::NotFound("File not found".to_string()));
    }

    serve_file_with_range(
        full_path,
        &media
            .mime_type
            .unwrap_or_else(|| "application/octet-stream".to_string()),
        &headers,
        Some(&media.original_filename),
    )
    .await
}

fn fetch_default_media(
    conn: &crate::database::DbConn,
    user_id: i64,
    limit: i32,
    search: &str,
) -> AppResult<Vec<MediaResponse>> {
    fetch_all(
        conn,
        queries::media::SELECT_PAGINATED_FOR_USER,
        &[
            &user_id,
            &search,
            &search,
            &Utc::now().to_rfc3339(),
            &Utc::now().to_rfc3339(),
            &i64::MAX,
            &(limit + 1),
        ],
        map_media_row,
    )
    .or_else(|_| {
        let future_date = "9999-12-31T23:59:59";
        fetch_all(
            conn,
            queries::media::SELECT_PAGINATED_FOR_USER,
            &[
                &user_id,
                &search,
                &search,
                &future_date,
                &future_date,
                &i64::MAX,
                &(limit + 1),
            ],
            map_media_row,
        )
    })
}

pub(crate) fn map_media_row(row: &rusqlite::Row) -> rusqlite::Result<MediaResponse> {
    let media_row = MediaRowData::from_row(row)?;
    Ok(row_to_media_response(media_row))
}

fn timeline_group_key(date_taken: Option<&str>, group_by: &str) -> String {
    let date_taken = match date_taken {
        Some(dt) => dt,
        None => return "Unknown".to_string(),
    };

    let dt = if let Ok(dt) = NaiveDateTime::parse_from_str(date_taken, "%Y-%m-%dT%H:%M:%S") {
        dt
    } else if let Ok(dt) =
        NaiveDateTime::parse_from_str(&date_taken.replace("Z", ""), "%Y-%m-%dT%H:%M:%S%.f")
    {
        dt
    } else if date_taken.len() >= 10 {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(&date_taken[..10], "%Y-%m-%d") {
            d.and_hms_opt(0, 0, 0).unwrap()
        } else {
            return "Unknown".to_string();
        }
    } else {
        return "Unknown".to_string();
    };

    match group_by {
        "year" => dt.year().to_string(),
        "month" => format!("{}-{:02}", dt.year(), dt.month()),
        "week" => {
            let week = dt.iso_week();
            format!("{}-W{:02}", week.year(), week.week())
        }
        _ => date_taken.chars().take(10).collect(),
    }
}

struct TimelineQuery<'a> {
    user_id: i64,
    cursor: Option<&'a str>,
    search: &'a str,
    media_type: Option<&'a str>,
    classification: Option<&'a str>,
    start_date: &'a str,
    end_date: &'a str,
    direction: TimelineDirection,
    anchor_date: Option<&'a str>,
    group_by: &'a str,
}

struct TimelinePage {
    rows: Vec<(MediaResponse, Option<String>)>,
    has_more: bool,
}

#[derive(Clone, Copy)]
struct TimelineFilter<'a> {
    user_id: i64,
    search: &'a str,
    media_type: &'a str,
    classification: &'a str,
    start_date: &'a str,
    end_date: &'a str,
}

fn fetch_timeline_page(
    conn: &crate::database::DbConn,
    query: TimelineQuery<'_>,
) -> AppResult<TimelinePage> {
    let TimelineQuery {
        user_id,
        cursor,
        search,
        media_type,
        classification,
        start_date,
        end_date,
        direction,
        anchor_date,
        group_by,
    } = query;
    let media_type = media_type.unwrap_or("");
    let classification = classification.unwrap_or("");
    let filter = TimelineFilter {
        user_id,
        search,
        media_type,
        classification,
        start_date,
        end_date,
    };
    let candidate = fetch_timeline_candidate(conn, filter, direction, cursor, anchor_date)?;
    let Some((_, Some(candidate_date))) = candidate.first() else {
        return Ok(TimelinePage {
            rows: Vec::new(),
            has_more: false,
        });
    };

    let (period_start, period_end) = timeline_period_bounds(candidate_date, group_by)?;
    let rows = fetch_timeline_period(
        conn,
        TimelineFilter {
            start_date: &period_start,
            end_date: &period_end,
            ..filter
        },
        direction,
    )?;
    let Some((last_media, Some(last_date))) = rows.last() else {
        return Ok(TimelinePage {
            rows,
            has_more: false,
        });
    };
    let last_cursor = format!("{}_{}", last_date, last_media.id);
    let has_more =
        !fetch_timeline_candidate(conn, filter, direction, Some(&last_cursor), None)?.is_empty();

    Ok(TimelinePage { rows, has_more })
}

fn fetch_timeline_candidate(
    conn: &crate::database::DbConn,
    filter: TimelineFilter<'_>,
    direction: TimelineDirection,
    cursor: Option<&str>,
    anchor_date: Option<&str>,
) -> AppResult<Vec<(MediaResponse, Option<String>)>> {
    let TimelineFilter {
        user_id,
        search,
        media_type,
        classification,
        start_date,
        end_date,
    } = filter;
    let query_params = [
        &user_id as &dyn rusqlite::ToSql,
        &start_date,
        &end_date,
        &search,
        &search,
        &media_type,
        &media_type,
        &classification,
        &classification,
        &classification,
    ];

    if let Some(cursor) = cursor {
        let parts: Vec<&str> = cursor.split('_').collect();
        if parts.len() != 2 {
            return Ok(Vec::new());
        }
        let cursor_date = parts[0];
        let cursor_id: i64 = parts[1].parse().unwrap_or(0);
        let query = if direction == TimelineDirection::Older {
            queries::timeline::SELECT_PAGINATED_WINDOW
        } else {
            queries::timeline::SELECT_PAGINATED_WINDOW_ASC
        };
        return fetch_all(
            conn,
            query,
            &[
                query_params[0],
                query_params[1],
                query_params[2],
                query_params[3],
                query_params[4],
                query_params[5],
                query_params[6],
                query_params[7],
                query_params[8],
                query_params[9],
                &cursor_date,
                &cursor_date,
                &cursor_id,
                &1_i64,
            ],
            map_timeline_row,
        );
    }

    let anchor = anchor_date.ok_or_else(|| {
        AppError::BadRequest("initial timeline requests require anchorDate".to_string())
    })?;
    fetch_all(
        conn,
        queries::timeline::SELECT_WINDOW,
        &[
            query_params[0],
            query_params[1],
            query_params[2],
            query_params[3],
            query_params[4],
            query_params[5],
            query_params[6],
            query_params[7],
            query_params[8],
            query_params[9],
            &anchor,
            &1_i64,
        ],
        map_timeline_row,
    )
}

fn fetch_timeline_period(
    conn: &crate::database::DbConn,
    filter: TimelineFilter<'_>,
    direction: TimelineDirection,
) -> AppResult<Vec<(MediaResponse, Option<String>)>> {
    let TimelineFilter {
        user_id,
        search,
        media_type,
        classification,
        start_date: period_start,
        end_date: period_end,
    } = filter;
    let max_rows = i64::MAX;
    let query_params = [
        &user_id as &dyn rusqlite::ToSql,
        &period_start,
        &period_end,
        &search,
        &search,
        &media_type,
        &media_type,
        &classification,
        &classification,
        &classification,
    ];

    if direction == TimelineDirection::Older {
        return fetch_all(
            conn,
            queries::timeline::SELECT_WINDOW,
            &[
                query_params[0],
                query_params[1],
                query_params[2],
                query_params[3],
                query_params[4],
                query_params[5],
                query_params[6],
                query_params[7],
                query_params[8],
                query_params[9],
                &period_end,
                &max_rows,
            ],
            map_timeline_row,
        );
    }

    fetch_all(
        conn,
        queries::timeline::SELECT_PAGINATED_WINDOW_ASC,
        &[
            query_params[0],
            query_params[1],
            query_params[2],
            query_params[3],
            query_params[4],
            query_params[5],
            query_params[6],
            query_params[7],
            query_params[8],
            query_params[9],
            &period_start,
            &period_start,
            &-1_i64,
            &max_rows,
        ],
        map_timeline_row,
    )
}

fn map_timeline_row(row: &rusqlite::Row) -> rusqlite::Result<(MediaResponse, Option<String>)> {
    let media_row = MediaRowData::from_row(row)?;
    let date_taken = media_row.date_taken.clone();
    let media = row_to_media_response(media_row);

    Ok((media, date_taken))
}

struct FileInfo {
    file_path: String,
    mime_type: Option<String>,
    original_filename: String,
}

async fn get_media_thumbnail_batch(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<ThumbnailBatchRequest>,
) -> AppResult<Json<ThumbnailBatchResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;
    if request.media_ids.is_empty() {
        return Ok(Json(ThumbnailBatchResponse {
            thumbnails: HashMap::new(),
        }));
    }

    let thumbnail_base_dir = match request.size {
        ThumbnailSize::Normal => &paths().thumbnails,
        ThumbnailSize::Tiny => &paths().thumbnails_tiny,
        ThumbnailSize::Place => &paths().thumbnails_places,
    };

    let rows: Vec<(i64, Option<String>, String, String, i64)> = fetch_all(
        &conn,
        queries::media::SELECT_THUMBNAIL_BATCH,
        &[&current_user.id],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        },
    )?;

    let requested_ids: std::collections::HashSet<i64> = request.media_ids.into_iter().collect();
    let rows = rows
        .into_iter()
        .filter(|(id, _, _, _, _)| requested_ids.contains(id))
        .collect::<Vec<_>>();

    let mut thumbnails: HashMap<i64, Option<String>> = HashMap::new();

    for (media_id, thumbnail_path, file_path, _media_type, _user_id) in rows {
        let stem = PathBuf::from(&file_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("thumb")
            .to_string();

        let thumbnail_relative = thumbnail_path.clone().unwrap_or_else(|| {
            let parent = PathBuf::from(&file_path)
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
            format!("{}/{}.jpg", parent, stem)
        });

        let full_path = thumbnail_base_dir.join(&thumbnail_relative);

        if full_path.exists() {
            if let Ok(data) = tokio::fs::read(&full_path).await {
                let encoded = STANDARD.encode(data);
                thumbnails.insert(
                    media_id,
                    Some(format!("data:image/jpeg;base64,{}", encoded)),
                );
                continue;
            }
        }

        thumbnails.insert(media_id, None);
    }

    Ok(Json(ThumbnailBatchResponse { thumbnails }))
}

async fn get_media_preview_batch(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<PreviewBatchRequest>,
) -> AppResult<Json<PreviewBatchResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;
    if request.ids.is_empty() {
        return Ok(Json(PreviewBatchResponse {
            previews: HashMap::new(),
        }));
    }

    let rows: Vec<(i64, String, String, Option<String>)> = fetch_all(
        &conn,
        queries::media::SELECT_PREVIEW_BATCH,
        &[&current_user.id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;

    let requested_ids: std::collections::HashSet<i64> = request.ids.into_iter().collect();
    let rows = rows
        .into_iter()
        .filter(|(id, _, _, _)| requested_ids.contains(id))
        .collect::<Vec<_>>();

    let mut previews: HashMap<i64, Option<String>> = HashMap::new();

    for (media_id, file_path, media_type, mime_type) in rows {
        let original_path = paths().originals.join(&file_path);
        if !original_path.exists() {
            previews.insert(media_id, None);
            continue;
        }

        if media_type == "video" {
            previews.insert(media_id, None);
            continue;
        }

        let web_compatible = ["image/jpeg", "image/png", "image/webp", "image/gif"];
        if let Some(ref mime) = mime_type {
            if web_compatible.contains(&mime.as_str()) {
                if let Ok(data) = tokio::fs::read(&original_path).await {
                    let encoded = STANDARD.encode(data);
                    previews.insert(media_id, Some(format!("data:{};base64,{}", mime, encoded)));
                    continue;
                }
            }
        }

        let preview_filename = format!(
            "{}_preview.jpg",
            original_path.file_stem().unwrap().to_string_lossy()
        );
        let preview_path = paths()
            .previews
            .join(current_user.id.to_string())
            .join(&preview_filename);

        if !preview_path.exists() {
            tokio::fs::create_dir_all(preview_path.parent().unwrap())
                .await
                .ok();
            generate_image_preview(&original_path, &preview_path, 2048, 90).await;
        }

        if preview_path.exists() {
            if let Ok(data) = tokio::fs::read(&preview_path).await {
                let encoded = STANDARD.encode(data);
                previews.insert(
                    media_id,
                    Some(format!("data:image/jpeg;base64,{}", encoded)),
                );
                continue;
            }
        }

        previews.insert(media_id, None);
    }

    Ok(Json(PreviewBatchResponse { previews }))
}

async fn serve_file_with_range(
    path: std::path::PathBuf,
    content_type: &str,
    headers: &HeaderMap,
    filename: Option<&str>,
) -> AppResult<Response> {
    let metadata = tokio::fs::metadata(&path).await?;
    let file_size = metadata.len();

    let range_header = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("bytes="));

    if let Some(range_str) = range_header {
        let (start, end) = parse_range(range_str, file_size);

        let mut file = File::open(&path).await?;
        file.seek(std::io::SeekFrom::Start(start)).await?;

        let length = end - start + 1;
        let stream = ReaderStream::new(file.take(length));
        let body = Body::from_stream(stream);

        let mut response = Response::builder()
            .status(StatusCode::PARTIAL_CONTENT)
            .header(header::CONTENT_TYPE, content_type)
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_LENGTH, length)
            .header(
                header::CONTENT_RANGE,
                format!("bytes {}-{}/{}", start, end, file_size),
            );

        if let Some(name) = filename {
            response = response.header(
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", name),
            );
        }

        response
            .body(body)
            .map_err(|e| AppError::Internal(e.to_string()))
    } else {
        let file = File::open(&path).await?;
        let stream = ReaderStream::new(file);
        let body = Body::from_stream(stream);

        let mut response = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type)
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_LENGTH, file_size);

        if let Some(name) = filename {
            response = response.header(
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", name),
            );
        }

        response
            .body(body)
            .map_err(|e| AppError::Internal(e.to_string()))
    }
}

fn parse_range(range_str: &str, file_size: u64) -> (u64, u64) {
    let parts: Vec<&str> = range_str.split('-').collect();
    if parts.len() != 2 {
        return (0, file_size - 1);
    }

    let start = if parts[0].is_empty() {
        let suffix_len: u64 = parts[1].parse().unwrap_or(0);
        file_size.saturating_sub(suffix_len)
    } else {
        parts[0].parse().unwrap_or(0)
    };

    let end = if parts[1].is_empty() {
        file_size - 1
    } else {
        parts[1].parse().unwrap_or(file_size - 1)
    };

    let start = start.min(file_size.saturating_sub(1));
    let end = end.min(file_size - 1).max(start);

    (start, end)
}
