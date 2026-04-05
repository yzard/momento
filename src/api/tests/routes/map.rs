use crate::test_utils::{
    create_test_db, create_test_media_with_gps, create_test_media_with_gps_and_date,
    create_test_user, grant_media_access,
};
use momento_api::database::{fetch_all, queries, DbPool};
use momento_api::error::{AppError, AppResult};
use momento_api::models::{BoundingBox, Cluster, MapClustersRequest, MapClustersResponse};
use std::time::{Duration, Instant};

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

fn make_request(bounds: (f64, f64, f64, f64), zoom: u8) -> MapClustersRequest {
    MapClustersRequest {
        bounds: BoundingBox {
            north: bounds.0,
            south: bounds.1,
            east: bounds.2,
            west: bounds.3,
        },
        zoom,
    }
}

fn get_clusters_sync(
    pool: &DbPool,
    user_id: i64,
    req: &MapClustersRequest,
) -> AppResult<MapClustersResponse> {
    let conn = pool.get().map_err(AppError::Pool)?;
    let precision = zoom_to_geohash_precision(req.zoom);
    let longitude_clause = if req.bounds.west <= req.bounds.east {
        queries::map::LONGITUDE_CLAUSE_STANDARD
    } else {
        queries::map::LONGITUDE_CLAUSE_ANTIMERIDIAN
    };

    let query = queries::map::build_clusters_query(precision, longitude_clause);

    let params: Vec<&dyn rusqlite::ToSql> = vec![
        &user_id,
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

    let older_media = create_test_media_with_gps_and_date(
        &pool,
        "old.jpg",
        40.7128,
        -74.0060,
        "2023-01-01T10:00:00",
    );
    let newer_media = create_test_media_with_gps_and_date(
        &pool,
        "new.jpg",
        40.7129,
        -74.0061,
        "2024-06-15T10:00:00",
    );

    grant_media_access(&pool, older_media, user_id);
    grant_media_access(&pool, newer_media, user_id);

    let req = make_request((50.0, 30.0, -60.0, -80.0), 5);
    let response = get_clusters_sync(&pool, user_id, &req).unwrap();

    assert_eq!(response.clusters.len(), 1);
    assert_eq!(response.clusters[0].count, 2);
    assert_eq!(response.clusters[0].representative_id, newer_media);
}

#[test]
fn test_map_clusters_antimeridian_bounds() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "testuser", "test@example.com");

    let media_west = create_test_media_with_gps(&pool, "west.jpg", 10.0, 179.5);
    let media_east = create_test_media_with_gps(&pool, "east.jpg", -5.0, -179.2);
    grant_media_access(&pool, media_west, user_id);
    grant_media_access(&pool, media_east, user_id);

    let req = make_request((20.0, -20.0, -170.0, 170.0), 6);
    let response = get_clusters_sync(&pool, user_id, &req).unwrap();

    assert_eq!(response.total_count, 2);
    assert!(response.clusters.len() >= 1);
}

#[test]
fn test_map_clusters_all_media_in_single_cluster() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "testuser", "test@example.com");

    for i in 0..5 {
        let media_id = create_test_media_with_gps(
            &pool,
            &format!("photo{}.jpg", i),
            40.7128 + i as f64 * 0.0001,
            -74.0060,
        );
        grant_media_access(&pool, media_id, user_id);
    }

    let req = make_request((50.0, 30.0, -60.0, -80.0), 4);
    let response = get_clusters_sync(&pool, user_id, &req).unwrap();

    assert_eq!(response.total_count, 5);
    assert_eq!(response.clusters.len(), 1);
    assert_eq!(response.clusters[0].count, 5);
}

#[test]
fn test_map_clusters_empty_bounds_returns_empty() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "testuser", "test@example.com");

    let media_id = create_test_media_with_gps(&pool, "photo.jpg", 40.7128, -74.0060);
    grant_media_access(&pool, media_id, user_id);

    let req = make_request((10.0, 10.0, 10.0, 10.0), 10);
    let response = get_clusters_sync(&pool, user_id, &req).unwrap();

    assert!(response.clusters.is_empty());
    assert_eq!(response.total_count, 0);
}

#[test]
fn test_map_clusters_performance_with_large_dataset() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "testuser", "test@example.com");

    for i in 0..1000 {
        let latitude = 37.0 + (i as f64 * 0.0001);
        let longitude = -122.0 + (i as f64 * 0.0001);
        let media_id =
            create_test_media_with_gps(&pool, &format!("photo{}.jpg", i), latitude, longitude);
        grant_media_access(&pool, media_id, user_id);
    }

    let req = make_request((38.0, 36.0, -121.0, -123.0), 8);
    let start = Instant::now();
    let response = get_clusters_sync(&pool, user_id, &req).unwrap();
    let elapsed = start.elapsed();

    assert_eq!(response.total_count, 1000);
    assert!(elapsed < Duration::from_millis(100));
}

#[test]
fn test_map_clusters_performance_with_10k_media_under_2s() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "testuser", "test@example.com");

    for i in 0..10000 {
        let latitude = 37.0 + (i as f64 * 0.00001);
        let longitude = -122.0 + (i as f64 * 0.00001);
        let media_id =
            create_test_media_with_gps(&pool, &format!("photo{}.jpg", i), latitude, longitude);
        grant_media_access(&pool, media_id, user_id);
    }

    let req = make_request((38.0, 36.0, -121.0, -123.0), 8);
    let start = Instant::now();
    let response = get_clusters_sync(&pool, user_id, &req).unwrap();
    let elapsed = start.elapsed();

    assert_eq!(response.total_count, 10000);
    assert!(elapsed < Duration::from_secs(2));
}

#[test]
fn test_map_clusters_updates_within_300ms() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "testuser", "test@example.com");

    for i in 0..2000 {
        let latitude = 37.0 + (i as f64 * 0.0001);
        let longitude = -122.0 + (i as f64 * 0.0001);
        let media_id =
            create_test_media_with_gps(&pool, &format!("photo{}.jpg", i), latitude, longitude);
        grant_media_access(&pool, media_id, user_id);
    }

    let req = make_request((38.0, 36.0, -121.0, -123.0), 10);
    let start = Instant::now();
    let response = get_clusters_sync(&pool, user_id, &req).unwrap();
    let elapsed = start.elapsed();

    assert_eq!(response.total_count, 2000);
    assert!(elapsed < Duration::from_millis(300));
}

#[test]
fn test_rtree_query_performance() {
    let pool = create_test_db();
    let conn = pool.get().expect("Failed to get connection");

    for i in 0..1000 {
        let latitude = 40.0 + (i as f64 * 0.0001);
        let longitude = -74.0 + (i as f64 * 0.0001);
        let statement = format!(
            "INSERT INTO media_rtree (media_id, min_lat, max_lat, min_lon, max_lon) VALUES ({}, {}, {}, {}, {})",
            i, latitude, latitude, longitude, longitude
        );
        conn.execute_batch(&statement)
            .expect("Failed to insert rtree entry");
    }

    let start = Instant::now();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM media_rtree WHERE min_lat >= 39.0 AND max_lat <= 41.0 AND min_lon >= -75.0 AND max_lon <= -73.0",
            [],
            |row| row.get(0),
        )
        .expect("Failed to query rtree");
    let elapsed = start.elapsed();

    assert_eq!(count, 1000);
    assert!(elapsed < Duration::from_millis(100));
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
    assert_eq!(zoom_to_geohash_precision(16), 7);
    assert_eq!(zoom_to_geohash_precision(18), 7);
    assert_eq!(zoom_to_geohash_precision(19), 7);
    assert_eq!(zoom_to_geohash_precision(25), 7);
}
