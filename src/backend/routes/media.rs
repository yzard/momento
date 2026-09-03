use axum::{
    extract::{Extension, Path, State},
    http::{header, HeaderMap, HeaderValue},
    response::Response,
    routing::{get, post},
    Router,
};
use chrono::{Datelike, NaiveDateTime, Utc};
use indexmap::IndexMap;

use crate::auth::{
    create_media_access_ticket as sign_media_access_ticket, AppState, CurrentUser,
    MediaAccessAuthorization,
};
use crate::database::operations::{
    BinaryMediaQuery, BinaryMediaRecord, FinalizeMediaUpdate, MediaBatchQuery, MoveMediaToTrash,
    PrepareMediaUpdate, TimelineMarkersQuery, TimelinePageQuery,
};
use crate::error::{AppError, AppResult};
use crate::io::file::{NormalizedStoragePath, StorageRootId};
use crate::models::{
    DeleteMediaResponse, MediaAccessResource, MediaAccessTicketRequest, MediaAccessTicketResponse,
    MediaBatchRequest, MediaBatchResponse, MediaDeleteRequest, MediaResponse, MediaUpdateRequest,
    ThumbnailSize, TimelineDirection, TimelineListRequest, TimelineListResponse, TimelineMarker,
    TimelineMarkersRequest, TimelineMarkersResponse,
};
use crate::routes::{render_json, CpuJson};
use crate::runtime::HttpRequestAdmission;
use std::collections::HashSet;

use super::file_stream::{serve_file, ContentDisposition, FileResponseOptions};

const MAX_MEDIA_BATCH_SIZE: usize = 500;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/timeline/list", post(list_timeline))
        .route("/timeline/markers", post(get_timeline_markers))
        .route("/media/get-batch", post(get_media_batch))
        .route("/media/access-ticket", post(create_media_access_ticket))
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

async fn list_timeline(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<TimelineListRequest>,
) -> AppResult<Response> {
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
    if !matches!(request.group_by.as_str(), "year" | "month" | "week" | "day") {
        return Err(AppError::BadRequest("Invalid groupBy".to_string()));
    }
    if request.cursor.is_none() && request.anchor_date.is_none() {
        return Err(AppError::BadRequest(
            "initial timeline requests require anchorDate".to_string(),
        ));
    }
    if let Some(cursor) = request.cursor.as_deref() {
        validate_timeline_cursor(cursor)?;
    }
    let page = state
        .executors
        .sqlite
        .load_timeline_page_request(TimelinePageQuery {
            user_id: current_user.id,
            cursor: request.cursor.clone(),
            search: normalize_media_text_search(Some(&request.search)),
            media_type: media_type.map(str::to_string),
            classification: classification.map(str::to_string),
            direction: request.direction,
            anchor_date: request.anchor_date.clone(),
            limit: request.limit,
        })
        .await?;
    let rows = page.rows;

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
        page.has_more
    } else {
        first_cursor.is_some()
    };
    let has_newer = if request.direction == TimelineDirection::Newer {
        page.has_more
    } else if request.cursor.is_some() {
        true
    } else {
        page.has_newer_candidate
    };

    render_json(
        &state,
        TimelineListResponse {
            groups,
            next_cursor: if has_older { last_cursor } else { None },
            previous_cursor: if has_newer { first_cursor } else { None },
            has_older,
            has_newer,
        },
    )
    .await
}

async fn get_timeline_markers(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<TimelineMarkersRequest>,
) -> AppResult<Response> {
    let search = normalize_media_text_search(Some(&request.search));
    let media_type = validate_media_type(request.media_type.as_deref())?;
    let classification = validate_classification(request.classification.as_deref())?;
    let markers = state
        .executors
        .sqlite
        .load_timeline_markers_request(TimelineMarkersQuery {
            user_id: current_user.id,
            search,
            media_type: media_type.map(str::to_string),
            classification: classification.map(str::to_string),
        })
        .await?
        .into_iter()
        .map(|marker| TimelineMarker {
            label: marker.label,
            anchor_date: marker.anchor_date,
        })
        .collect::<Vec<_>>();

    render_json(&state, TimelineMarkersResponse { markers }).await
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
    CpuJson(request): CpuJson<MediaBatchRequest>,
) -> AppResult<Response> {
    if request.ids.is_empty() {
        return render_json(&state, MediaBatchResponse { items: Vec::new() }).await;
    }

    let ordered_items = state
        .executors
        .sqlite
        .load_media_batch_request(MediaBatchQuery {
            user_id: current_user.id,
            media_ids: request.ids,
        })
        .await?;

    render_json(
        &state,
        MediaBatchResponse {
            items: ordered_items,
        },
    )
    .await
}

async fn update_media(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<MediaUpdateRequest>,
) -> AppResult<Response> {
    let editable_state = state
        .executors
        .sqlite
        .prepare_media_update_request(PrepareMediaUpdate {
            user_id: current_user.id,
            media_id: request.media_id,
        })
        .await?
        .ok_or_else(|| AppError::NotFound("Media not found".to_string()))?;
    let update_editable_metadata = request.date_taken.is_some()
        || request.gps_latitude.is_some()
        || request.gps_longitude.is_some();
    let update_location = request.gps_latitude.is_some() || request.gps_longitude.is_some();
    let effective_gps_latitude = request.gps_latitude.or(editable_state.gps_latitude);
    let effective_gps_longitude = request.gps_longitude.or(editable_state.gps_longitude);
    let derived_location = if update_location {
        Some(
            state
                .executors
                .cpu
                .derive_media_location_request(effective_gps_latitude, effective_gps_longitude)
                .await?,
        )
    } else {
        None
    };
    let media = state
        .executors
        .sqlite
        .finalize_media_update_request(FinalizeMediaUpdate {
            user_id: current_user.id,
            media_id: request.media_id,
            date_taken: request.date_taken,
            gps_latitude: request.gps_latitude,
            gps_longitude: request.gps_longitude,
            effective_gps_latitude,
            effective_gps_longitude,
            update_editable_metadata,
            update_location,
            geohash: derived_location
                .as_ref()
                .and_then(|location| location.geohash.clone()),
            city: derived_location
                .as_ref()
                .and_then(|location| location.city.clone()),
            state: derived_location
                .as_ref()
                .and_then(|location| location.state.clone()),
            country: derived_location.and_then(|location| location.country),
        })
        .await?
        .ok_or_else(|| AppError::NotFound("Media not found".to_string()))?;
    render_json(&state, media).await
}

async fn delete_media(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<MediaDeleteRequest>,
) -> AppResult<Response> {
    if request.media_ids.is_empty() {
        return render_json(
            &state,
            DeleteMediaResponse {
                message: "No media to delete".to_string(),
            },
        )
        .await;
    }
    let media_ids = request.media_ids.into_iter().collect::<HashSet<_>>();
    if media_ids.len() > 500 {
        return Err(AppError::BadRequest(
            "mediaIds must contain at most 500 unique IDs".to_string(),
        ));
    }
    let deleted_at = Utc::now().to_rfc3339();
    let deleted_count = state
        .executors
        .sqlite
        .move_media_to_trash_request(MoveMediaToTrash {
            user_id: current_user.id,
            media_ids: media_ids.into_iter().collect(),
            deleted_at,
        })
        .await?;

    render_json(
        &state,
        DeleteMediaResponse {
            message: format!("{} media moved to trash", deleted_count),
        },
    )
    .await
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

fn validate_timeline_cursor(cursor: &str) -> AppResult<()> {
    let Some((date, id)) = cursor.rsplit_once('_') else {
        return Err(AppError::BadRequest("Invalid timeline cursor".to_string()));
    };
    if date.is_empty() {
        return Err(AppError::BadRequest("Invalid timeline cursor".to_string()));
    }
    id.parse::<i64>()
        .map_err(|_| AppError::BadRequest("Invalid timeline cursor".to_string()))?;
    Ok(())
}

async fn get_media_original(
    State(state): State<AppState>,
    Extension(admission): Extension<HttpRequestAdmission>,
    authorization: MediaAccessAuthorization,
    Path(media_id): Path<i64>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let user_id = authorization.authorize(media_id, MediaAccessResource::Original)?;
    serve_original(&state, &admission, user_id, media_id, &headers).await
}

async fn create_media_access_ticket(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(request): CpuJson<MediaAccessTicketRequest>,
) -> AppResult<Response> {
    let config = state.config.current();
    load_binary_media_info(&state, current_user.id, request.media_id, false).await?;
    let (ticket, expires_at) =
        sign_media_access_ticket(current_user.id, request.media_id, request.resource, &config)?;
    let url = format!(
        "/api/v1/media/{}/{}?ticket={}",
        request.media_id,
        request.resource.path_segment(),
        ticket
    );
    let mut response = render_json(
        &state,
        MediaAccessTicketResponse {
            url,
            expires_at: expires_at.to_rfc3339(),
        },
    )
    .await?;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

async fn get_media_thumbnail(
    State(state): State<AppState>,
    Extension(admission): Extension<HttpRequestAdmission>,
    current_user: CurrentUser,
    Path(media_id): Path<i64>,
    headers: HeaderMap,
) -> AppResult<Response> {
    serve_thumbnail(
        &state,
        &admission,
        current_user.id,
        media_id,
        ThumbnailSize::Normal,
        &headers,
        false,
    )
    .await
}

async fn get_media_tiny_thumbnail(
    State(state): State<AppState>,
    Extension(admission): Extension<HttpRequestAdmission>,
    current_user: CurrentUser,
    Path(media_id): Path<i64>,
    headers: HeaderMap,
) -> AppResult<Response> {
    serve_thumbnail(
        &state,
        &admission,
        current_user.id,
        media_id,
        ThumbnailSize::Tiny,
        &headers,
        false,
    )
    .await
}

async fn get_media_preview(
    State(state): State<AppState>,
    Extension(admission): Extension<HttpRequestAdmission>,
    current_user: CurrentUser,
    Path(media_id): Path<i64>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let media = load_binary_media_info(&state, current_user.id, media_id, false).await?;
    let (storage_root, path, content_type) = resolve_preview_path(&media, media_id)?;
    serve_file(
        &state.executors.file_io,
        storage_root,
        path,
        FileResponseOptions {
            admission: &admission,
            content_type: &content_type,
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

async fn serve_original(
    state: &AppState,
    admission: &HttpRequestAdmission,
    user_id: i64,
    media_id: i64,
    headers: &HeaderMap,
) -> AppResult<Response> {
    let media = load_binary_media_info(state, user_id, media_id, false).await?;
    let path = NormalizedStoragePath::parse(&media.file_path)
        .map_err(|_| AppError::NotFound("Media file path is invalid".to_string()))?;
    let content_type = media
        .mime_type
        .unwrap_or_else(|| "application/octet-stream".to_string());
    serve_file(
        &state.executors.file_io,
        StorageRootId::Originals,
        path,
        FileResponseOptions {
            admission,
            content_type: &content_type,
            headers,
            filename: Some(&media.original_filename),
            allow_ranges: true,
            content_disposition: ContentDisposition::Inline,
            cache_control: "private",
            head_only: false,
        },
    )
    .await
}

async fn serve_thumbnail(
    state: &AppState,
    admission: &HttpRequestAdmission,
    user_id: i64,
    media_id: i64,
    size: ThumbnailSize,
    headers: &HeaderMap,
    deleted: bool,
) -> AppResult<Response> {
    let media = load_binary_media_info(state, user_id, media_id, deleted).await?;
    let path = thumbnail_relative_path(media.thumbnail_path.as_deref())
        .ok_or_else(|| AppError::NotFound("Thumbnail path is invalid".to_string()))?;
    serve_file(
        &state.executors.file_io,
        thumbnail_storage_root(size),
        path,
        FileResponseOptions {
            admission,
            content_type: "image/jpeg",
            headers,
            filename: None,
            allow_ranges: false,
            content_disposition: ContentDisposition::Inline,
            cache_control: "private",
            head_only: false,
        },
    )
    .await
}

async fn load_binary_media_info(
    state: &AppState,
    user_id: i64,
    media_id: i64,
    deleted: bool,
) -> AppResult<BinaryMediaRecord> {
    state
        .executors
        .sqlite
        .load_binary_media_request(BinaryMediaQuery {
            user_id,
            media_id,
            deleted,
        })
        .await?
        .ok_or_else(|| AppError::NotFound("Media not found".to_string()))
}

pub(crate) async fn serve_deleted_tiny_thumbnail(
    state: &AppState,
    admission: &HttpRequestAdmission,
    user_id: i64,
    media_id: i64,
    headers: &HeaderMap,
) -> AppResult<Response> {
    serve_thumbnail(
        state,
        admission,
        user_id,
        media_id,
        ThumbnailSize::Tiny,
        headers,
        true,
    )
    .await
}

fn thumbnail_storage_root(size: ThumbnailSize) -> StorageRootId {
    match size {
        ThumbnailSize::Normal => StorageRootId::Thumbnails,
        ThumbnailSize::Tiny => StorageRootId::TinyThumbnails,
    }
}

fn resolve_preview_path(
    media: &BinaryMediaRecord,
    _media_id: i64,
) -> AppResult<(StorageRootId, NormalizedStoragePath, String)> {
    if media.media_type == "video" {
        return Err(AppError::NotFound("Preview not found".to_string()));
    }

    let web_compatible = ["image/jpeg", "image/png", "image/webp", "image/gif"];
    if let Some(mime_type) = &media.mime_type {
        if web_compatible.contains(&mime_type.as_str()) {
            let relative_path = NormalizedStoragePath::parse(&media.file_path)
                .map_err(|_| AppError::NotFound("Media file path is invalid".to_string()))?;
            return Ok((StorageRootId::Originals, relative_path, mime_type.clone()));
        }
    }

    let relative_path = media
        .preview_path
        .as_deref()
        .ok_or_else(|| AppError::NotFound("Preview not found".to_string()))
        .and_then(|path| {
            NormalizedStoragePath::parse(path)
                .map_err(|_| AppError::NotFound("Preview path is invalid".to_string()))
        })?;
    Ok((
        StorageRootId::Previews,
        relative_path,
        "image/jpeg".to_string(),
    ))
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

pub(super) fn thumbnail_relative_path(
    thumbnail_path: Option<&str>,
) -> Option<NormalizedStoragePath> {
    thumbnail_path.and_then(|path| NormalizedStoragePath::parse(path).ok())
}
