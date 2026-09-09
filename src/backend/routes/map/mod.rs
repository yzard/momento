use axum::{extract::State, response::Response, routing::post, Router};

use crate::auth::{AppState, CurrentUser};
use crate::database::operations::{MapClustersQuery, MapMediaQuery, SpatialBounds};
use crate::error::AppResult;
use crate::models::{MapClustersRequest, MapMediaRequest};
use crate::routes::{render_json, CpuJson};

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
    CpuJson(req): CpuJson<MapClustersRequest>,
) -> AppResult<Response> {
    let precision = zoom_to_geohash_precision(req.zoom);
    let response = state
        .executors
        .sqlite
        .load_map_clusters_request(MapClustersQuery {
            user_id: current_user.id,
            bounds: SpatialBounds {
                north: req.bounds.north,
                south: req.bounds.south,
                east: req.bounds.east,
                west: req.bounds.west,
            },
            precision,
        })
        .await?;
    render_json(&state, response).await
}

async fn get_media(
    State(state): State<AppState>,
    current_user: CurrentUser,
    CpuJson(req): CpuJson<MapMediaRequest>,
) -> AppResult<Response> {
    let response = state
        .executors
        .sqlite
        .load_map_media_request(MapMediaQuery {
            user_id: current_user.id,
            bounds: SpatialBounds {
                north: req.bounds.north,
                south: req.bounds.south,
                east: req.bounds.east,
                west: req.bounds.west,
            },
            geohash_prefixes: req.geohash_prefixes,
        })
        .await?;
    render_json(&state, response).await
}
