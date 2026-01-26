use axum::{extract::State, routing::post, Json, Router};

use crate::auth::{AppState, CurrentUser};
use crate::database::{fetch_all, queries};
use crate::error::{AppError, AppResult};
use crate::models::{
    Cluster, MapClustersRequest, MapClustersResponse, MapMediaListResponse, MapMediaRequest,
    MediaResponse,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/map/clusters", post(get_clusters))
        .route("/map/media", post(get_media))
}

fn zoom_to_geohash_precision(zoom: u8) -> usize {
    match zoom {
        0..=3 => 1,
        4..=6 => 2,
        7..=9 => 3,
        10..=12 => 4,
        13..=15 => 5,
        16..=18 => 7,
        _ => 7,
    }
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
    content_hash: Option<String>,
    created_at: String,
}

fn map_media_row(row: &rusqlite::Row) -> rusqlite::Result<MediaResponse> {
    let media_row = MediaRowData {
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
        content_hash: row.get(27)?,
        created_at: row.get(28)?,
    };

    Ok(MediaResponse {
        id: media_row.id,
        filename: media_row.filename,
        original_filename: media_row.original_filename,
        media_type: media_row.media_type,
        mime_type: media_row.mime_type,
        width: media_row.width,
        height: media_row.height,
        file_size: media_row.file_size,
        duration_seconds: media_row.duration_seconds,
        date_taken: media_row.date_taken,
        gps_latitude: media_row.gps_latitude,
        gps_longitude: media_row.gps_longitude,
        camera_make: media_row.camera_make,
        camera_model: media_row.camera_model,
        lens_make: media_row.lens_make,
        lens_model: media_row.lens_model,
        iso: media_row.iso,
        exposure_time: media_row.exposure_time,
        f_number: media_row.f_number,
        focal_length: media_row.focal_length,
        focal_length_35mm: media_row.focal_length_35mm,
        gps_altitude: media_row.gps_altitude,
        location_city: media_row.location_city,
        location_state: media_row.location_state,
        location_country: media_row.location_country,
        video_codec: media_row.video_codec,
        keywords: media_row.keywords,
        content_hash: media_row.content_hash,
        created_at: media_row.created_at,
    })
}

async fn get_clusters(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(req): Json<MapClustersRequest>,
) -> AppResult<Json<MapClustersResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;
    let precision = zoom_to_geohash_precision(req.zoom);
    let longitude_clause = if req.bounds.west <= req.bounds.east {
        queries::map::LONGITUDE_CLAUSE_STANDARD
    } else {
        queries::map::LONGITUDE_CLAUSE_ANTIMERIDIAN
    };

    let query = queries::map::build_clusters_query(precision, longitude_clause);

    let params: Vec<&dyn rusqlite::ToSql> = vec![
        &current_user.id,
        &req.bounds.south,
        &req.bounds.north,
        &req.bounds.west,
        &req.bounds.east,
    ];

    let clusters = fetch_all(&conn, &query, &params, |row| {
        Ok(Cluster {
            id: row.get(0)?,
            count: row.get(1)?,
            lat: row.get(2)?,
            lng: row.get(3)?,
            representative_id: row.get(4)?,
        })
    })?;

    let total_count: i64 = clusters.iter().map(|c| c.count).sum();

    Ok(Json(MapClustersResponse {
        clusters,
        total_count,
    }))
}

async fn get_media(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(req): Json<MapMediaRequest>,
) -> AppResult<Json<MapMediaListResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;
    let longitude_clause = if req.bounds.west <= req.bounds.east {
        queries::map::LONGITUDE_CLAUSE_STANDARD
    } else {
        queries::map::LONGITUDE_CLAUSE_ANTIMERIDIAN
    };

    let query = queries::map::build_media_query(req.geohash_prefixes.len(), longitude_clause);
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![
        Box::new(current_user.id),
        Box::new(req.bounds.south),
        Box::new(req.bounds.north),
        Box::new(req.bounds.west),
        Box::new(req.bounds.east),
    ];

    for prefix in &req.geohash_prefixes {
        params.push(Box::new(format!("{}%", prefix)));
    }

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|param| param.as_ref()).collect();
    let items = fetch_all(&conn, &query, &param_refs, map_media_row)?;

    Ok(Json(MapMediaListResponse { items }))
}
