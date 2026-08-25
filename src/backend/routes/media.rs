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
use crate::constants::paths;
use crate::database::{execute_query, fetch_all, fetch_one, queries};
use crate::error::{AppError, AppResult};
use crate::models::{
    DeleteMediaResponse, MediaBatchRequest, MediaBatchResponse, MediaDeleteRequest, MediaResponse,
    MediaUpdateRequest, PreviewBatchRequest, PreviewBatchResponse, ThumbnailBatchRequest,
    ThumbnailBatchResponse, ThumbnailSize, TimelineDirection, TimelineListRequest,
    TimelineListResponse, TimelineMarker, TimelineMarkersRequest, TimelineMarkersResponse,
};
use crate::processor::media_processor::{calculate_geohash, delete_from_rtree, insert_into_rtree};
use crate::processor::metadata::reverse_geocoding::reverse_geocode;
use crate::processor::thumbnails::generate_image_preview;
use crate::utils::path::{resolve_existing_storage_path, resolve_storage_path};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

const TIMELINE_START_DATE: &str = "0000-01-01T00:00:00";
const TIMELINE_END_DATE: &str = "9999-12-31T23:59:59";
const MAX_MEDIA_BATCH_SIZE: usize = 500;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/timeline/list", post(list_timeline))
        .route("/timeline/markers", post(get_timeline_markers))
        .route("/media/get-batch", post(get_media_batch))
        .route("/media/update", post(update_media))
        .route("/media/delete", post(delete_media))
        .route("/media/:media_id/original", get(get_media_original))
        .route("/media/:media_id/thumbnail", get(get_media_thumbnail))
        .route(
            "/media/:media_id/thumbnail/tiny",
            get(get_media_tiny_thumbnail),
        )
        .route("/media/:media_id/preview", get(get_media_preview))
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

    if !(1..=500).contains(&request.limit) {
        return Err(AppError::BadRequest(
            "limit must be within 1..=500".to_string(),
        ));
    }

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
            limit: request.limit,
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
    limit: u32,
}

struct TimelinePage {
    rows: TimelineRows,
    has_more: bool,
}

type TimelineRows = Vec<(MediaResponse, Option<String>)>;

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
        limit,
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
    let (rows, period_has_more) = fetch_timeline_period(
        conn,
        TimelineFilter {
            start_date: &period_start,
            end_date: &period_end,
            ..filter
        },
        direction,
        cursor,
        limit,
    )?;
    let Some((last_media, Some(last_date))) = rows.last() else {
        return Ok(TimelinePage {
            rows,
            has_more: false,
        });
    };
    let last_cursor = format!("{}_{}", last_date, last_media.id);
    let has_more = period_has_more
        || !fetch_timeline_candidate(conn, filter, direction, Some(&last_cursor), None)?.is_empty();

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
    cursor: Option<&str>,
    limit: u32,
) -> AppResult<(TimelineRows, bool)> {
    let TimelineFilter {
        user_id,
        search,
        media_type,
        classification,
        start_date: period_start,
        end_date: period_end,
    } = filter;
    let max_rows = i64::from(limit) + 1;
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
        if let Some((cursor_date, cursor_id)) = parse_timeline_cursor(cursor)? {
            let mut rows = fetch_all(
                conn,
                queries::timeline::SELECT_PAGINATED_WINDOW,
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
                    &max_rows,
                ],
                map_timeline_row,
            )?;
            let has_more = rows.len() > limit as usize;
            rows.truncate(limit as usize);
            return Ok((rows, has_more));
        }
        let mut rows = fetch_all(
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
        )?;
        let has_more = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        return Ok((rows, has_more));
    }

    let (cursor_date, cursor_id) = match parse_timeline_cursor(cursor)? {
        Some(cursor) => cursor,
        None => (period_start.to_string(), -1),
    };
    let mut rows = fetch_all(
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
            &cursor_date,
            &cursor_date,
            &cursor_id,
            &max_rows,
        ],
        map_timeline_row,
    )?;
    let has_more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    Ok((rows, has_more))
}

fn parse_timeline_cursor(cursor: Option<&str>) -> AppResult<Option<(String, i64)>> {
    let Some(cursor) = cursor else {
        return Ok(None);
    };
    let Some((date, id)) = cursor.rsplit_once('_') else {
        return Err(AppError::BadRequest("Invalid timeline cursor".to_string()));
    };
    let id = id
        .parse::<i64>()
        .map_err(|_| AppError::BadRequest("Invalid timeline cursor".to_string()))?;
    Ok(Some((date.to_string(), id)))
}

fn map_timeline_row(row: &rusqlite::Row) -> rusqlite::Result<(MediaResponse, Option<String>)> {
    let media_row = MediaRowData::from_row(row)?;
    let date_taken = media_row.date_taken.clone();
    let media = row_to_media_response(media_row);

    Ok((media, date_taken))
}

struct BinaryMediaInfo {
    file_path: String,
    mime_type: Option<String>,
    original_filename: String,
    media_type: String,
    thumbnail_path: Option<String>,
}

async fn get_media_original(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Path(media_id): Path<i64>,
    headers: HeaderMap,
) -> AppResult<Response> {
    serve_original(&state, current_user.id, media_id, &headers).await
}

async fn get_media_thumbnail(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Path(media_id): Path<i64>,
    headers: HeaderMap,
) -> AppResult<Response> {
    serve_thumbnail(
        &state,
        current_user.id,
        media_id,
        ThumbnailSize::Normal,
        &headers,
        queries::media::SELECT_BINARY_MEDIA_INFO,
    )
    .await
}

async fn get_media_tiny_thumbnail(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Path(media_id): Path<i64>,
    headers: HeaderMap,
) -> AppResult<Response> {
    serve_thumbnail(
        &state,
        current_user.id,
        media_id,
        ThumbnailSize::Tiny,
        &headers,
        queries::media::SELECT_BINARY_MEDIA_INFO,
    )
    .await
}

async fn get_media_preview(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Path(media_id): Path<i64>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let media = load_binary_media_info(
        &state,
        current_user.id,
        media_id,
        queries::media::SELECT_BINARY_MEDIA_INFO,
    )?;
    let (path, content_type) = resolve_preview_path(&media, current_user.id).await?;
    serve_file(path, &content_type, &headers, None, false).await
}

async fn serve_original(
    state: &AppState,
    user_id: i64,
    media_id: i64,
    headers: &HeaderMap,
) -> AppResult<Response> {
    let media = load_binary_media_info(
        state,
        user_id,
        media_id,
        queries::media::SELECT_BINARY_MEDIA_INFO,
    )?;
    let path = resolve_existing_storage_path(&paths().originals, &media.file_path).await?;
    let content_type = media
        .mime_type
        .unwrap_or_else(|| "application/octet-stream".to_string());
    serve_file(
        path,
        &content_type,
        headers,
        Some(&media.original_filename),
        true,
    )
    .await
}

async fn serve_thumbnail(
    state: &AppState,
    user_id: i64,
    media_id: i64,
    size: ThumbnailSize,
    headers: &HeaderMap,
    query: &str,
) -> AppResult<Response> {
    let media = load_binary_media_info(state, user_id, media_id, query)?;
    let relative_path = thumbnail_relative_path(media.thumbnail_path.as_deref(), &media.file_path);
    let relative_path = relative_path
        .to_str()
        .ok_or_else(|| AppError::NotFound("Thumbnail path is invalid".to_string()))?;
    let path = resolve_existing_storage_path(thumbnail_base_directory(size), relative_path).await?;
    serve_file(path, "image/jpeg", headers, None, false).await
}

fn load_binary_media_info(
    state: &AppState,
    user_id: i64,
    media_id: i64,
    query: &str,
) -> AppResult<BinaryMediaInfo> {
    let conn = state.pool.get().map_err(AppError::Pool)?;
    fetch_one(&conn, query, &[&media_id, &user_id], |row| {
        Ok(BinaryMediaInfo {
            file_path: row.get(0)?,
            mime_type: row.get(1)?,
            original_filename: row.get(2)?,
            media_type: row.get(3)?,
            thumbnail_path: row.get(4)?,
        })
    })?
    .ok_or_else(|| AppError::NotFound("Media not found".to_string()))
}

pub(crate) async fn serve_deleted_tiny_thumbnail(
    state: &AppState,
    user_id: i64,
    media_id: i64,
    headers: &HeaderMap,
) -> AppResult<Response> {
    serve_thumbnail(
        state,
        user_id,
        media_id,
        ThumbnailSize::Tiny,
        headers,
        queries::media::SELECT_DELETED_BINARY_MEDIA_INFO,
    )
    .await
}

fn resolve_thumbnail_path(
    thumbnail_path: Option<&str>,
    file_path: &str,
    size: ThumbnailSize,
) -> AppResult<PathBuf> {
    let relative_path = thumbnail_relative_path(thumbnail_path, file_path);
    let relative_path = relative_path
        .to_str()
        .ok_or_else(|| AppError::NotFound("Thumbnail path is invalid".to_string()))?;
    resolve_storage_path(thumbnail_base_directory(size), relative_path)
}

fn thumbnail_base_directory(size: ThumbnailSize) -> &'static std::path::Path {
    match size {
        ThumbnailSize::Normal => &paths().thumbnails,
        ThumbnailSize::Tiny => &paths().thumbnails_tiny,
    }
}

async fn resolve_preview_path(
    media: &BinaryMediaInfo,
    user_id: i64,
) -> AppResult<(PathBuf, String)> {
    let original_path = resolve_existing_storage_path(&paths().originals, &media.file_path).await?;
    if media.media_type == "video" {
        return Err(AppError::NotFound("Preview not found".to_string()));
    }

    let web_compatible = ["image/jpeg", "image/png", "image/webp", "image/gif"];
    if let Some(mime_type) = &media.mime_type {
        if web_compatible.contains(&mime_type.as_str()) {
            return Ok((original_path, mime_type.clone()));
        }
    }

    let preview_filename = format!(
        "{}_preview.jpg",
        original_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| AppError::NotFound("Preview not found".to_string()))?
    );
    let preview_path = paths()
        .previews
        .join(user_id.to_string())
        .join(preview_filename);
    if !preview_path.is_file() {
        let parent = preview_path
            .parent()
            .ok_or_else(|| AppError::Internal("Preview path has no parent".to_string()))?;
        tokio::fs::create_dir_all(parent).await?;
        generate_image_preview(&original_path, &preview_path, 2048, 90).await;
    }
    if !preview_path.is_file() {
        return Err(AppError::NotFound("Preview not found".to_string()));
    }
    Ok((preview_path, "image/jpeg".to_string()))
}

async fn get_media_thumbnail_batch(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<ThumbnailBatchRequest>,
) -> AppResult<Json<ThumbnailBatchResponse>> {
    let media_ids = unique_batch_ids(request.media_ids)?;
    if media_ids.is_empty() {
        return Ok(Json(ThumbnailBatchResponse {
            thumbnails: HashMap::new(),
        }));
    }

    let rows: Vec<ThumbnailBatchRow> = {
        let conn = state.pool.get().map_err(AppError::Pool)?;
        let query = queries::media::build_thumbnail_batch_query(media_ids.len());
        let parameters = user_media_id_parameters(&current_user.id, &media_ids);
        fetch_all(&conn, &query, &parameters, |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?
    };

    Ok(Json(encode_thumbnail_batch(rows, request.size).await))
}

pub(super) fn unique_batch_ids(media_ids: Vec<i64>) -> AppResult<Vec<i64>> {
    let mut seen = HashSet::new();
    let media_ids = media_ids
        .into_iter()
        .filter(|media_id| seen.insert(*media_id))
        .collect::<Vec<_>>();
    if media_ids.len() > MAX_MEDIA_BATCH_SIZE {
        return Err(AppError::BadRequest(format!(
            "mediaIds must contain at most {MAX_MEDIA_BATCH_SIZE} unique IDs"
        )));
    }
    Ok(media_ids)
}

pub(super) fn user_media_id_parameters<'a>(
    user_id: &'a i64,
    media_ids: &'a [i64],
) -> Vec<&'a dyn rusqlite::ToSql> {
    let mut parameters = Vec::with_capacity(media_ids.len() + 1);
    parameters.push(user_id as &dyn rusqlite::ToSql);
    parameters.extend(
        media_ids
            .iter()
            .map(|media_id| media_id as &dyn rusqlite::ToSql),
    );
    parameters
}

pub(super) type ThumbnailBatchRow = (i64, Option<String>, String, String, i64);

pub(super) async fn encode_thumbnail_batch(
    rows: Vec<ThumbnailBatchRow>,
    size: ThumbnailSize,
) -> ThumbnailBatchResponse {
    let mut thumbnails = HashMap::new();
    for (media_id, thumbnail_path, file_path, _media_type, _user_id) in rows {
        let Ok(full_path) = resolve_thumbnail_path(thumbnail_path.as_deref(), &file_path, size)
        else {
            thumbnails.insert(media_id, None);
            continue;
        };

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

    ThumbnailBatchResponse { thumbnails }
}

pub(super) fn thumbnail_relative_path(thumbnail_path: Option<&str>, file_path: &str) -> PathBuf {
    if let Some(thumbnail_path) = thumbnail_path {
        return PathBuf::from(thumbnail_path);
    }
    let source_path = PathBuf::from(file_path);
    let parent = source_path
        .parent()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("unknown");
    let stem = source_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("thumb");
    PathBuf::from(parent).join(format!("{stem}.jpg"))
}

async fn get_media_preview_batch(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<PreviewBatchRequest>,
) -> AppResult<Json<PreviewBatchResponse>> {
    let media_ids = unique_batch_ids(request.ids)?;
    if media_ids.is_empty() {
        return Ok(Json(PreviewBatchResponse {
            previews: HashMap::new(),
        }));
    }

    let rows: Vec<(i64, String, String, Option<String>)> = {
        let conn = state.pool.get().map_err(AppError::Pool)?;
        let query = queries::media::build_preview_batch_query(media_ids.len());
        let parameters = user_media_id_parameters(&current_user.id, &media_ids);
        fetch_all(&conn, &query, &parameters, |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
    };

    let mut previews: HashMap<i64, Option<String>> = HashMap::new();

    for (media_id, file_path, media_type, mime_type) in rows {
        let media = BinaryMediaInfo {
            file_path,
            mime_type,
            original_filename: String::new(),
            media_type,
            thumbnail_path: None,
        };
        let Ok((preview_path, mime_type)) = resolve_preview_path(&media, current_user.id).await
        else {
            previews.insert(media_id, None);
            continue;
        };
        let data = tokio::fs::read(preview_path).await.ok();
        previews.insert(
            media_id,
            data.map(|data| format!("data:{mime_type};base64,{}", STANDARD.encode(data))),
        );
    }

    Ok(Json(PreviewBatchResponse { previews }))
}

async fn serve_file(
    path: std::path::PathBuf,
    content_type: &str,
    headers: &HeaderMap,
    filename: Option<&str>,
    allow_ranges: bool,
) -> AppResult<Response> {
    let metadata = tokio::fs::metadata(&path).await?;
    let file_size = metadata.len();
    let etag = file_etag(&metadata);
    if matches_if_none_match(headers, &etag) {
        return Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header(header::ETAG, etag)
            .header(header::CACHE_CONTROL, "private")
            .body(Body::empty())
            .map_err(|error| AppError::Internal(error.to_string()));
    }

    let range_header = allow_ranges
        .then(|| if_range_allows_range(headers, &etag))
        .and_then(|allowed| {
            allowed.then(|| {
                headers
                    .get(header::RANGE)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string)
            })
        })
        .flatten();

    if let Some(range_str) = range_header {
        let Some((start, end)) = parse_range(&range_str, file_size) else {
            return range_not_satisfiable(file_size);
        };

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
            .header(header::ETAG, &etag)
            .header(header::CACHE_CONTROL, "private")
            .header(
                header::CONTENT_RANGE,
                format!("bytes {}-{}/{}", start, end, file_size),
            );

        if let Some(name) = filename {
            response = response.header(
                header::CONTENT_DISPOSITION,
                format!(
                    "inline; filename=\"{}\"",
                    content_disposition_filename(name)
                ),
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
            .header(header::CONTENT_LENGTH, file_size)
            .header(header::ETAG, &etag)
            .header(header::CACHE_CONTROL, "private");

        if allow_ranges {
            response = response.header(header::ACCEPT_RANGES, "bytes");
        }

        if let Some(name) = filename {
            response = response.header(
                header::CONTENT_DISPOSITION,
                format!(
                    "inline; filename=\"{}\"",
                    content_disposition_filename(name)
                ),
            );
        }

        response
            .body(body)
            .map_err(|e| AppError::Internal(e.to_string()))
    }
}

fn file_etag(metadata: &std::fs::Metadata) -> String {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("W/\"{}-{modified}\"", metadata.len())
}

fn matches_if_none_match(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|candidate| candidate.trim() == "*" || candidate.trim() == etag)
        })
}

fn if_range_allows_range(headers: &HeaderMap, etag: &str) -> bool {
    match headers
        .get(header::IF_RANGE)
        .and_then(|value| value.to_str().ok())
    {
        None => true,
        Some(value) => value == etag,
    }
}

fn range_not_satisfiable(file_size: u64) -> AppResult<Response> {
    Response::builder()
        .status(StatusCode::RANGE_NOT_SATISFIABLE)
        .header(header::CONTENT_RANGE, format!("bytes */{file_size}"))
        .header(header::CACHE_CONTROL, "private")
        .body(Body::empty())
        .map_err(|error| AppError::Internal(error.to_string()))
}

fn parse_range(range_header: &str, file_size: u64) -> Option<(u64, u64)> {
    if file_size == 0 {
        return None;
    }
    let range = range_header.strip_prefix("bytes=")?;
    if range.contains(',') {
        return None;
    }
    let (start, end) = range.split_once('-')?;
    if start.is_empty() {
        let suffix_length = end.parse::<u64>().ok()?;
        if suffix_length == 0 {
            return None;
        }
        return Some((file_size.saturating_sub(suffix_length), file_size - 1));
    }
    let start = start.parse::<u64>().ok()?;
    if start >= file_size {
        return None;
    }
    if end.is_empty() {
        return Some((start, file_size - 1));
    }
    let end = end.parse::<u64>().ok()?;
    if end < start {
        return None;
    }
    Some((start, end.min(file_size - 1)))
}

fn content_disposition_filename(filename: &str) -> String {
    filename
        .chars()
        .map(|character| {
            if character == '"' || character == '\\' || character.is_control() {
                '_'
            } else {
                character
            }
        })
        .collect()
}
