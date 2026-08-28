use axum::{extract::State, routing::post, Json, Router};

use crate::auth::{AppState, CurrentUser};
use crate::database::{fetch_all, queries};
use crate::error::{AppError, AppResult};
use crate::models::{
    map_media_response_with_content_hash, Cluster, MapClustersRequest, MapClustersResponse,
    MapMediaListResponse, MapMediaRequest,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/map/clusters", post(get_clusters))
        .route("/map/media", post(get_media))
}

fn zoom_to_geohash_precision(zoom: u8) -> usize {
    match zoom {
        0..=3 => 2,
        4..=6 => 3,
        7..=9 => 4,
        10..=12 => 5,
        13..=15 => 6,
        16..=18 => 8,
        _ => 8,
    }
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
    let items = fetch_all(
        &conn,
        &query,
        &param_refs,
        map_media_response_with_content_hash,
    )?;

    Ok(Json(MapMediaListResponse { items }))
}
