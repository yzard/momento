use crate::test_utils::{
    create_test_db, create_test_media_with_gps, create_test_media_with_gps_and_date,
    create_test_user, grant_media_access, test_executor_handles,
};
use momento_api::database::DbPool;
use momento_api::executor::SqliteExecutorHandle;
use momento_api::models::{
    BoundingBox, MapClustersRequest, MapClustersResponse, MapMediaListResponse,
};
use momento_api::{
    database::operations::MapClustersQuery, database::operations::MapMediaQuery,
    database::operations::SpatialBounds,
};
use std::time::{Duration, Instant};

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

async fn get_clusters(
    pool: &DbPool,
    user_id: i64,
    req: &MapClustersRequest,
) -> MapClustersResponse {
    let precision = zoom_to_geohash_precision(req.zoom);
    test_executor_handles(pool.clone())
        .sqlite
        .load_map_clusters_request(MapClustersQuery {
            user_id,
            bounds: SpatialBounds {
                north: req.bounds.north,
                south: req.bounds.south,
                east: req.bounds.east,
                west: req.bounds.west,
            },
            precision,
        })
        .await
        .expect("map clusters")
}

async fn get_clusters_with_executor(
    sqlite: &SqliteExecutorHandle,
    user_id: i64,
    req: &MapClustersRequest,
) -> MapClustersResponse {
    sqlite
        .load_map_clusters_request(MapClustersQuery {
            user_id,
            bounds: SpatialBounds {
                north: req.bounds.north,
                south: req.bounds.south,
                east: req.bounds.east,
                west: req.bounds.west,
            },
            precision: zoom_to_geohash_precision(req.zoom),
        })
        .await
        .expect("map clusters")
}

async fn get_media(
    sqlite: &SqliteExecutorHandle,
    user_id: i64,
    bounds: SpatialBounds,
) -> MapMediaListResponse {
    sqlite
        .load_map_media_request(MapMediaQuery {
            user_id,
            bounds,
            geohash_prefixes: Vec::new(),
        })
        .await
        .expect("map media")
}

#[tokio::test]
async fn test_map_clusters_empty_database() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "testuser", "test@example.com");

    let req = make_request((50.0, 40.0, -70.0, -80.0), 10);
    let response = get_clusters(&pool, user_id, &req).await;

    assert!(response.clusters.is_empty());
    assert_eq!(response.total_count, 0);
}

#[tokio::test]
async fn test_map_clusters_single_media() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "testuser", "test@example.com");

    let media_id = create_test_media_with_gps(&pool, "photo.jpg", 40.7128, -74.0060);
    grant_media_access(&pool, media_id, user_id);

    let req = make_request((50.0, 30.0, -60.0, -80.0), 10);
    let response = get_clusters(&pool, user_id, &req).await;

    assert_eq!(response.clusters.len(), 1);
    assert_eq!(response.clusters[0].count, 1);
    assert_eq!(response.clusters[0].representative_id, media_id);
    assert_eq!(response.total_count, 1);
}

#[tokio::test]
async fn test_map_clusters_excludes_zero_coordinates() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "testuser", "test@example.com");

    let media_id = create_test_media_with_gps(&pool, "photo.jpg", 0.0, 0.0);
    grant_media_access(&pool, media_id, user_id);

    let req = make_request((1.0, -1.0, 1.0, -1.0), 10);
    let response = get_clusters(&pool, user_id, &req).await;

    assert!(response.clusters.is_empty());
    assert_eq!(response.total_count, 0);
}

#[tokio::test]
async fn test_map_clusters_excludes_each_zero_coordinate() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "testuser", "test@example.com");

    let zero_latitude_media = create_test_media_with_gps(&pool, "zero-latitude.jpg", 0.0, 10.0);
    let zero_longitude_media = create_test_media_with_gps(&pool, "zero-longitude.jpg", 10.0, 0.0);
    grant_media_access(&pool, zero_latitude_media, user_id);
    grant_media_access(&pool, zero_longitude_media, user_id);

    let req = make_request((20.0, -1.0, 20.0, -1.0), 10);
    let response = get_clusters(&pool, user_id, &req).await;
    assert!(response.clusters.is_empty());
    assert_eq!(response.total_count, 0);
}

#[tokio::test]
async fn test_map_clusters_media_outside_bounds_excluded() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "testuser", "test@example.com");

    let media_id = create_test_media_with_gps(&pool, "photo.jpg", 40.7128, -74.0060);
    grant_media_access(&pool, media_id, user_id);

    let req = make_request((60.0, 50.0, -70.0, -80.0), 10);
    let response = get_clusters(&pool, user_id, &req).await;

    assert!(response.clusters.is_empty());
    assert_eq!(response.total_count, 0);
}

#[tokio::test]
async fn test_map_clusters_access_control() {
    let pool = create_test_db();
    let user_a = create_test_user(&pool, "user_a", "a@example.com");
    let user_b = create_test_user(&pool, "user_b", "b@example.com");

    let media_a = create_test_media_with_gps(&pool, "photo_a.jpg", 40.7128, -74.0060);
    grant_media_access(&pool, media_a, user_a);

    let media_b = create_test_media_with_gps(&pool, "photo_b.jpg", 40.7500, -73.9500);
    grant_media_access(&pool, media_b, user_b);

    let req = make_request((50.0, 30.0, -60.0, -80.0), 10);

    let response_a = get_clusters(&pool, user_a, &req).await;
    assert_eq!(response_a.total_count, 1);
    assert_eq!(response_a.clusters[0].representative_id, media_a);

    let response_b = get_clusters(&pool, user_b, &req).await;
    assert_eq!(response_b.total_count, 1);
    assert_eq!(response_b.clusters[0].representative_id, media_b);
}

#[tokio::test]
async fn test_map_clusters_zoom_affects_granularity() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "testuser", "test@example.com");

    let media1 = create_test_media_with_gps(&pool, "photo1.jpg", 40.7128, -74.0060);
    let media2 = create_test_media_with_gps(&pool, "photo2.jpg", 40.7130, -74.0062);
    grant_media_access(&pool, media1, user_id);
    grant_media_access(&pool, media2, user_id);

    let req_low_zoom = make_request((50.0, 30.0, -60.0, -80.0), 5);
    let response_low = get_clusters(&pool, user_id, &req_low_zoom).await;

    let req_high_zoom = make_request((50.0, 30.0, -60.0, -80.0), 18);
    let response_high = get_clusters(&pool, user_id, &req_high_zoom).await;

    assert!(response_low.clusters.len() <= response_high.clusters.len());
    assert_eq!(response_low.total_count, 2);
    assert_eq!(response_high.total_count, 2);
}

#[tokio::test]
async fn test_map_clusters_representative_is_most_recent() {
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
    let response = get_clusters(&pool, user_id, &req).await;

    assert_eq!(response.clusters.len(), 1);
    assert_eq!(response.clusters[0].count, 2);
    assert_eq!(response.clusters[0].representative_id, newer_media);
}

#[tokio::test]
async fn test_map_clusters_antimeridian_bounds() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "testuser", "test@example.com");

    let media_west = create_test_media_with_gps(&pool, "west.jpg", 10.0, 179.5);
    let media_east = create_test_media_with_gps(&pool, "east.jpg", -5.0, -179.2);
    grant_media_access(&pool, media_west, user_id);
    grant_media_access(&pool, media_east, user_id);

    let req = make_request((20.0, -20.0, -170.0, 170.0), 6);
    let response = get_clusters(&pool, user_id, &req).await;

    assert_eq!(response.total_count, 2);
    assert!(!response.clusters.is_empty());
}

#[tokio::test]
async fn test_map_clusters_accepts_leaflet_repeated_world_bounds() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "testuser", "test@example.com");

    for (filename, latitude, longitude) in [
        ("west.jpg", 10.0, -170.0),
        ("center.jpg", 20.0, 5.0),
        ("east.jpg", -10.0, 170.0),
    ] {
        let media_id = create_test_media_with_gps(&pool, filename, latitude, longitude);
        grant_media_access(&pool, media_id, user_id);
    }

    let request = make_request(
        (
            86.83673396186525,
            -86.83673396186525,
            428.5546875,
            -428.5546875,
        ),
        2,
    );
    let response = get_clusters(&pool, user_id, &request).await;

    assert_eq!(response.total_count, 3);
}

#[tokio::test]
async fn test_map_media_wraps_repeated_antimeridian_bounds() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "testuser", "test@example.com");
    let included = create_test_media_with_gps(&pool, "included.jpg", 10.0, -175.0);
    let excluded = create_test_media_with_gps(&pool, "excluded.jpg", 10.0, 0.0);
    grant_media_access(&pool, included, user_id);
    grant_media_access(&pool, excluded, user_id);
    let executors = test_executor_handles(pool);

    let response = get_media(
        &executors.sqlite,
        user_id,
        SpatialBounds {
            north: 95.0,
            south: -95.0,
            east: 190.0,
            west: 170.0,
        },
    )
    .await;

    assert_eq!(response.items.len(), 1);
    assert_eq!(response.items[0].id, included);
}

#[tokio::test]
async fn test_map_bounds_still_reject_non_finite_coordinates() {
    let pool = create_test_db();
    let executors = test_executor_handles(pool);
    let error = executors
        .sqlite
        .load_map_clusters_request(MapClustersQuery {
            user_id: 1,
            bounds: SpatialBounds {
                north: f64::NAN,
                south: -10.0,
                east: 10.0,
                west: -10.0,
            },
            precision: 2,
        })
        .await
        .expect_err("non-finite map bounds must fail");

    assert!(error.to_string().contains("map bounds are invalid"));
}

#[tokio::test]
async fn test_map_clusters_all_media_in_single_cluster() {
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
    let response = get_clusters(&pool, user_id, &req).await;

    assert_eq!(response.total_count, 5);
    assert_eq!(response.clusters.len(), 1);
    assert_eq!(response.clusters[0].count, 5);
}

#[tokio::test]
async fn test_map_clusters_empty_bounds_returns_empty() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "testuser", "test@example.com");

    let media_id = create_test_media_with_gps(&pool, "photo.jpg", 40.7128, -74.0060);
    grant_media_access(&pool, media_id, user_id);

    let req = make_request((10.0, 10.0, 10.0, 10.0), 10);
    let response = get_clusters(&pool, user_id, &req).await;

    assert!(response.clusters.is_empty());
    assert_eq!(response.total_count, 0);
}

#[tokio::test]
async fn test_map_clusters_performance_with_large_dataset() {
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
    let executors = test_executor_handles(pool.clone());
    let start = Instant::now();
    let response = get_clusters_with_executor(&executors.sqlite, user_id, &req).await;
    let elapsed = start.elapsed();

    assert_eq!(response.total_count, 1000);
    assert!(elapsed < Duration::from_millis(100));
}

#[tokio::test]
async fn test_map_clusters_performance_with_10k_media_under_2s() {
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
    let executors = test_executor_handles(pool.clone());
    let start = Instant::now();
    let response = get_clusters_with_executor(&executors.sqlite, user_id, &req).await;
    let elapsed = start.elapsed();

    assert_eq!(response.total_count, 10000);
    assert!(elapsed < Duration::from_secs(2));
}

#[tokio::test]
async fn test_map_clusters_updates_within_300ms() {
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
    let executors = test_executor_handles(pool.clone());
    let start = Instant::now();
    let response = get_clusters_with_executor(&executors.sqlite, user_id, &req).await;
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
    assert_eq!(zoom_to_geohash_precision(0), 2);
    assert_eq!(zoom_to_geohash_precision(3), 2);
    assert_eq!(zoom_to_geohash_precision(4), 3);
    assert_eq!(zoom_to_geohash_precision(6), 3);
    assert_eq!(zoom_to_geohash_precision(7), 4);
    assert_eq!(zoom_to_geohash_precision(9), 4);
    assert_eq!(zoom_to_geohash_precision(10), 5);
    assert_eq!(zoom_to_geohash_precision(12), 5);
    assert_eq!(zoom_to_geohash_precision(13), 6);
    assert_eq!(zoom_to_geohash_precision(15), 6);
    assert_eq!(zoom_to_geohash_precision(16), 8);
    assert_eq!(zoom_to_geohash_precision(18), 8);
    assert_eq!(zoom_to_geohash_precision(19), 8);
    assert_eq!(zoom_to_geohash_precision(25), 8);
}
