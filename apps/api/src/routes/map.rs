use axum::{extract::State, routing::post, Json, Router};

use crate::auth::{AppState, CurrentUser};
use crate::database::fetch_all;
use crate::error::{AppError, AppResult};
use crate::models::{Cluster, MapClustersRequest, MapClustersResponse};

pub fn router() -> Router<AppState> {
    Router::new().route("/map/clusters", post(get_clusters))
}

fn zoom_to_geohash_precision(zoom: u8) -> usize {
    match zoom {
        0..=3 => 1,
        4..=6 => 2,
        7..=9 => 3,
        10..=12 => 4,
        13..=15 => 5,
        16..=18 => 6,
        _ => 7,
    }
}

async fn get_clusters(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(req): Json<MapClustersRequest>,
) -> AppResult<Json<MapClustersResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;
    let precision = zoom_to_geohash_precision(req.zoom);

    let query = format!(
        r#"
        WITH clustered AS (
            SELECT SUBSTR(m.geohash, 1, {precision}) AS cell
                 , COUNT(*) AS count
                 , AVG(m.gps_latitude) AS center_lat
                 , AVG(m.gps_longitude) AS center_lon
                 , MAX(COALESCE(m.date_taken, m.created_at) || '_' || m.id) AS latest
              FROM media AS m
              JOIN media_access AS ma ON m.id = ma.media_id
             WHERE ma.user_id = ?
               AND ma.deleted_at IS NULL
               AND m.gps_latitude BETWEEN ? AND ?
               AND m.gps_longitude BETWEEN ? AND ?
               AND m.geohash IS NOT NULL
             GROUP BY cell
        )
        SELECT c.cell
             , c.count
             , c.center_lat
             , c.center_lon
             , CAST(SUBSTR(c.latest, INSTR(c.latest, '_') + 1) AS INTEGER) AS representative_id
          FROM clustered AS c
        "#,
        precision = precision
    );

    let clusters = fetch_all(
        &conn,
        &query,
        &[
            &current_user.id,
            &req.bounds.south,
            &req.bounds.north,
            &req.bounds.west,
            &req.bounds.east,
        ],
        |row| {
            Ok(Cluster {
                id: row.get(0)?,
                count: row.get(1)?,
                lat: row.get(2)?,
                lng: row.get(3)?,
                representative_id: row.get(4)?,
            })
        },
    )?;

    let total_count: i64 = clusters.iter().map(|c| c.count).sum();

    Ok(Json(MapClustersResponse {
        clusters,
        total_count,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{
        create_test_db, create_test_media_with_gps, create_test_media_with_gps_and_date,
        create_test_user, grant_media_access,
    };

    fn make_request(bounds: (f64, f64, f64, f64), zoom: u8) -> MapClustersRequest {
        MapClustersRequest {
            bounds: crate::models::BoundingBox {
                north: bounds.0,
                south: bounds.1,
                east: bounds.2,
                west: bounds.3,
            },
            zoom,
        }
    }

    fn get_clusters_sync(
        pool: &crate::database::DbPool,
        user_id: i64,
        req: &MapClustersRequest,
    ) -> AppResult<MapClustersResponse> {
        let conn = pool.get().map_err(AppError::Pool)?;
        let precision = zoom_to_geohash_precision(req.zoom);

        let query = format!(
            r#"
            WITH clustered AS (
                SELECT SUBSTR(m.geohash, 1, {precision}) AS cell
                     , COUNT(*) AS count
                     , AVG(m.gps_latitude) AS center_lat
                     , AVG(m.gps_longitude) AS center_lon
                     , MAX(COALESCE(m.date_taken, m.created_at) || '_' || m.id) AS latest
                  FROM media AS m
                  JOIN media_access AS ma ON m.id = ma.media_id
                 WHERE ma.user_id = ?
                   AND ma.deleted_at IS NULL
                   AND m.gps_latitude BETWEEN ? AND ?
                   AND m.gps_longitude BETWEEN ? AND ?
                   AND m.geohash IS NOT NULL
                 GROUP BY cell
            )
            SELECT c.cell
                 , c.count
                 , c.center_lat
                 , c.center_lon
                 , CAST(SUBSTR(c.latest, INSTR(c.latest, '_') + 1) AS INTEGER) AS representative_id
              FROM clustered AS c
            "#,
            precision = precision
        );

        let clusters = fetch_all(
            &conn,
            &query,
            &[
                &user_id,
                &req.bounds.south,
                &req.bounds.north,
                &req.bounds.west,
                &req.bounds.east,
            ],
            |row| {
                Ok(Cluster {
                    id: row.get(0)?,
                    count: row.get(1)?,
                    lat: row.get(2)?,
                    lng: row.get(3)?,
                    representative_id: row.get(4)?,
                })
            },
        )?;

        let total_count: i64 = clusters.iter().map(|c| c.count).sum();

        Ok(MapClustersResponse {
            clusters,
            total_count,
        })
    }

    #[test]
    fn test_map_clusters_empty_database() {
        let pool = create_test_db();
        let user_id = create_test_user(&pool, "testuser", "test@example.com");

        let req = make_request((50.0, 40.0, -70.0, -80.0), 10);
        let response = get_clusters_sync(&pool, user_id, &req).unwrap();

        assert!(response.clusters.is_empty());
        assert_eq!(response.total_count, 0);
    }

    #[test]
    fn test_map_clusters_single_media() {
        let pool = create_test_db();
        let user_id = create_test_user(&pool, "testuser", "test@example.com");

        let media_id = create_test_media_with_gps(&pool, "photo.jpg", 40.7128, -74.0060);
        grant_media_access(&pool, media_id, user_id);

        let req = make_request((50.0, 30.0, -60.0, -80.0), 10);
        let response = get_clusters_sync(&pool, user_id, &req).unwrap();

        assert_eq!(response.clusters.len(), 1);
        assert_eq!(response.clusters[0].count, 1);
        assert_eq!(response.clusters[0].representative_id, media_id);
        assert_eq!(response.total_count, 1);
    }

    #[test]
    fn test_map_clusters_media_outside_bounds_excluded() {
        let pool = create_test_db();
        let user_id = create_test_user(&pool, "testuser", "test@example.com");

        let media_id = create_test_media_with_gps(&pool, "photo.jpg", 40.7128, -74.0060);
        grant_media_access(&pool, media_id, user_id);

        let req = make_request((60.0, 50.0, -70.0, -80.0), 10);
        let response = get_clusters_sync(&pool, user_id, &req).unwrap();

        assert!(response.clusters.is_empty());
        assert_eq!(response.total_count, 0);
    }

    #[test]
    fn test_map_clusters_access_control() {
        let pool = create_test_db();
        let user_a = create_test_user(&pool, "user_a", "a@example.com");
        let user_b = create_test_user(&pool, "user_b", "b@example.com");

        let media_a = create_test_media_with_gps(&pool, "photo_a.jpg", 40.7128, -74.0060);
        grant_media_access(&pool, media_a, user_a);

        let media_b = create_test_media_with_gps(&pool, "photo_b.jpg", 40.7500, -73.9500);
        grant_media_access(&pool, media_b, user_b);

        let req = make_request((50.0, 30.0, -60.0, -80.0), 10);

        let response_a = get_clusters_sync(&pool, user_a, &req).unwrap();
        assert_eq!(response_a.total_count, 1);
        assert_eq!(response_a.clusters[0].representative_id, media_a);

        let response_b = get_clusters_sync(&pool, user_b, &req).unwrap();
        assert_eq!(response_b.total_count, 1);
        assert_eq!(response_b.clusters[0].representative_id, media_b);
    }

    #[test]
    fn test_map_clusters_zoom_affects_granularity() {
        let pool = create_test_db();
        let user_id = create_test_user(&pool, "testuser", "test@example.com");

        let media1 = create_test_media_with_gps(&pool, "photo1.jpg", 40.7128, -74.0060);
        let media2 = create_test_media_with_gps(&pool, "photo2.jpg", 40.7130, -74.0062);
        grant_media_access(&pool, media1, user_id);
        grant_media_access(&pool, media2, user_id);

        let req_low_zoom = make_request((50.0, 30.0, -60.0, -80.0), 5);
        let response_low = get_clusters_sync(&pool, user_id, &req_low_zoom).unwrap();

        let req_high_zoom = make_request((50.0, 30.0, -60.0, -80.0), 18);
        let response_high = get_clusters_sync(&pool, user_id, &req_high_zoom).unwrap();

        assert!(response_low.clusters.len() <= response_high.clusters.len());
        assert_eq!(response_low.total_count, 2);
        assert_eq!(response_high.total_count, 2);
    }

    #[test]
    fn test_map_clusters_representative_is_most_recent() {
        let pool = create_test_db();
        let user_id = create_test_user(&pool, "testuser", "test@example.com");

        let older_media =
            create_test_media_with_gps_and_date(&pool, "old.jpg", 40.7128, -74.0060, "2023-01-01T10:00:00");
        let newer_media =
            create_test_media_with_gps_and_date(&pool, "new.jpg", 40.7129, -74.0061, "2024-06-15T10:00:00");

        grant_media_access(&pool, older_media, user_id);
        grant_media_access(&pool, newer_media, user_id);

        let req = make_request((50.0, 30.0, -60.0, -80.0), 5);
        let response = get_clusters_sync(&pool, user_id, &req).unwrap();

        assert_eq!(response.clusters.len(), 1);
        assert_eq!(response.clusters[0].count, 2);
        assert_eq!(response.clusters[0].representative_id, newer_media);
    }

    #[test]
    fn test_zoom_to_geohash_precision() {
        assert_eq!(zoom_to_geohash_precision(0), 1);
        assert_eq!(zoom_to_geohash_precision(3), 1);
        assert_eq!(zoom_to_geohash_precision(4), 2);
        assert_eq!(zoom_to_geohash_precision(6), 2);
        assert_eq!(zoom_to_geohash_precision(7), 3);
        assert_eq!(zoom_to_geohash_precision(9), 3);
        assert_eq!(zoom_to_geohash_precision(10), 4);
        assert_eq!(zoom_to_geohash_precision(12), 4);
        assert_eq!(zoom_to_geohash_precision(13), 5);
        assert_eq!(zoom_to_geohash_precision(15), 5);
        assert_eq!(zoom_to_geohash_precision(16), 6);
        assert_eq!(zoom_to_geohash_precision(18), 6);
        assert_eq!(zoom_to_geohash_precision(19), 7);
        assert_eq!(zoom_to_geohash_precision(25), 7);
    }
}
