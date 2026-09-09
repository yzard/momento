use super::*;

fn prepare(connection: &Connection) {
    connection.execute_batch("CREATE TABLE media(id INTEGER PRIMARY KEY, created_at TEXT); CREATE TABLE media_access(media_id INTEGER, user_id INTEGER, deleted_at TEXT); CREATE TABLE media_metadata(media_id INTEGER, gps_latitude REAL, gps_longitude REAL, geohash TEXT, date_taken TEXT);
        INSERT INTO media VALUES (1, '2026-01-01'), (2, '2026-01-02'), (3, '2026-01-03');
        INSERT INTO media_access VALUES (1, 10, NULL), (2, 10, NULL), (2, 20, NULL), (3, 10, NULL);
        INSERT INTO media_metadata VALUES (1, 40.1, -74.1, 'dr5abc', NULL), (2, 40.3, -74.3, 'dr5def', NULL), (3, -30, 120, 'q7abc', NULL);").unwrap();
}

fn query(user_id: i64, south: f64, north: f64) -> MapClustersQuery {
    MapClustersQuery {
        user_id,
        precision: 3,
        bounds: SpatialBounds {
            north,
            south,
            west: -180.,
            east: 180.,
        },
    }
}

#[test]
fn panning_keeps_whole_cell_centers_counts_and_representatives_and_reuses_index() {
    let connection = Connection::open_in_memory().unwrap();
    prepare(&connection);
    let mut indexed = IndexedConnection::new(connection);
    indexed.pragma_update(None, "query_only", true).unwrap();
    let first = indexed.load_map_clusters(query(10, 40., 40.25)).unwrap();
    assert_eq!(first.clusters.len(), 1);
    assert_eq!(first.clusters[0].count, 2);
    assert_eq!(first.clusters[0].representative_id, 2);
    assert!((first.clusters[0].lat - 40.2).abs() < 1e-9);
    let storage = indexed.indexes[0].clusters.as_ptr();
    let panned = indexed.load_map_clusters(query(10, 40.15, 40.4)).unwrap();
    assert_eq!(first.clusters[0].lat, panned.clusters[0].lat);
    assert_eq!(first.clusters[0].lng, panned.clusters[0].lng);
    assert_eq!(storage, indexed.indexes[0].clusters.as_ptr());
    assert_eq!(
        indexed
            .load_map_clusters(query(10, 50., 60.))
            .unwrap()
            .total_count,
        0
    );
}

#[test]
fn indexes_are_isolated_by_user_and_precision() {
    let connection = Connection::open_in_memory().unwrap();
    prepare(&connection);
    let mut indexed = IndexedConnection::new(connection);
    indexed.load_map_clusters(query(10, -90., 90.)).unwrap();
    let other = indexed.load_map_clusters(query(20, -90., 90.)).unwrap();
    assert_eq!(other.total_count, 1);
    assert_eq!(other.clusters[0].lat, 40.3);
    let mut detailed = query(10, -90., 90.);
    detailed.precision = 6;
    assert_eq!(
        indexed.load_map_clusters(detailed).unwrap().clusters.len(),
        3
    );
    assert_eq!(indexed.indexes.len(), 3);
}

#[test]
fn same_connection_writes_invalidate_access_locations_and_representatives() {
    let connection = Connection::open_in_memory().unwrap();
    prepare(&connection);
    let mut indexed = IndexedConnection::new(connection);
    indexed.load_map_clusters(query(10, -90., 90.)).unwrap();
    indexed
        .execute(
            "UPDATE media_access SET deleted_at = 'deleted' WHERE media_id = 2 AND user_id = 10",
            [],
        )
        .unwrap();
    let updated = indexed.load_map_clusters(query(10, 40., 41.)).unwrap();
    assert_eq!(updated.clusters[0].count, 1);
    assert_eq!(updated.clusters[0].representative_id, 1);
    indexed
        .execute(
            "UPDATE media_metadata SET gps_latitude = 45 WHERE media_id = 1",
            [],
        )
        .unwrap();
    assert_eq!(
        indexed
            .load_map_clusters(query(10, 40., 41.))
            .unwrap()
            .total_count,
        0
    );
    assert_eq!(
        indexed
            .load_map_clusters(query(10, 44., 46.))
            .unwrap()
            .total_count,
        1
    );
    indexed
        .execute(
            "UPDATE media_access SET deleted_at = NULL WHERE media_id = 2",
            [],
        )
        .unwrap();
    assert_eq!(
        indexed
            .load_map_clusters(query(10, -90., 90.))
            .unwrap()
            .total_count,
        3
    );
}

#[test]
fn commits_on_other_connections_invalidate_cached_access() {
    let directory = crate::temporary::tempdir().unwrap();
    let path = directory.path().join("map.sqlite");
    let writer = Connection::open(&path).unwrap();
    prepare(&writer);
    let mut indexed = IndexedConnection::new(Connection::open(&path).unwrap());
    assert_eq!(
        indexed
            .load_map_clusters(query(10, -90., 90.))
            .unwrap()
            .total_count,
        3
    );
    writer
        .execute("DELETE FROM media_access WHERE user_id = 10", [])
        .unwrap();
    assert_eq!(
        indexed
            .load_map_clusters(query(10, -90., 90.))
            .unwrap()
            .total_count,
        0
    );
}

#[test]
fn viewport_query_handles_antimeridian_and_does_not_return_global_index() {
    let connection = Connection::open_in_memory().unwrap();
    prepare(&connection);
    connection.execute_batch("UPDATE media_metadata SET gps_longitude = 179, geohash = 'xb1' WHERE media_id = 1; UPDATE media_metadata SET gps_longitude = -179, geohash = '801' WHERE media_id = 2;").unwrap();
    let mut indexed = IndexedConnection::new(connection);
    let mut crossing = query(10, -90., 90.);
    crossing.bounds.west = 170.;
    crossing.bounds.east = -170.;
    let response = indexed.load_map_clusters(crossing).unwrap();
    assert_eq!(response.clusters.len(), 2);
    assert_eq!(response.total_count, 2);
    assert_eq!(indexed.indexes[0].clusters.len(), 3);
}

#[test]
fn index_cache_evicts_least_recently_used_user_without_leaking_results() {
    let connection = Connection::open_in_memory().unwrap();
    prepare(&connection);
    let mut indexed = IndexedConnection::new(connection);
    for user_id in 10..20 {
        indexed
            .load_map_clusters(query(user_id, -90., 90.))
            .unwrap();
    }
    assert_eq!(indexed.indexes.len(), MAX_INDEX_ENTRIES);
    assert_eq!(
        indexed
            .load_map_clusters(query(20, -90., 90.))
            .unwrap()
            .total_count,
        1
    );
    assert_eq!(
        indexed
            .load_map_clusters(query(10, -90., 90.))
            .unwrap()
            .total_count,
        3
    );
}

#[test]
fn small_viewport_can_query_an_index_larger_than_the_response_limit() {
    let connection = Connection::open_in_memory().unwrap();
    prepare(&connection);
    connection.execute_batch("WITH RECURSIVE n(i) AS (SELECT 100 UNION ALL SELECT i+1 FROM n WHERE i<5099) INSERT INTO media SELECT i, '2026-01-01' FROM n; INSERT INTO media_access SELECT id, 10, NULL FROM media WHERE id>=100; INSERT INTO media_metadata SELECT id, 50, 100, PRINTF('%08d', id), NULL FROM media WHERE id>=100;").unwrap();
    let mut indexed = IndexedConnection::new(connection);
    let mut request = query(10, 40., 41.);
    request.precision = 8;
    let response = indexed.load_map_clusters(request).unwrap();
    assert_eq!(response.clusters.len(), 2);
    assert!(indexed.indexes[0].clusters.len() > MAX_RESPONSE_ROWS);
}
