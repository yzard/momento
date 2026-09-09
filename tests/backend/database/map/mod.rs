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
        precision_bits: 15,
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
    detailed.precision_bits = 30;
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
    for user_id in 10..(11 + MAX_INDEX_ENTRIES as i64) {
        indexed
            .load_map_clusters(query(user_id, -90., 90.))
            .unwrap();
    }
    assert_eq!(indexed.indexes.len(), MAX_INDEX_ENTRIES);
    assert!(!indexed.indexes.iter().any(|index| index.user_id == 10));
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
    request.precision_bits = 40;
    let response = indexed.load_map_clusters(request).unwrap();
    assert_eq!(response.clusters.len(), 2);
    assert!(indexed.indexes[0].clusters.len() > MAX_RESPONSE_ROWS);
}

#[test]
fn wider_zoom_cells_reduce_markers_without_losing_members_or_panning_stability() {
    let connection = Connection::open_in_memory().unwrap();
    prepare(&connection);
    let mut indexed = IndexedConnection::new(connection);
    let mut request = query(10, 40., 41.);
    request.precision_bits = 20;
    let finer = indexed.load_map_clusters(request).unwrap();
    assert_eq!(finer.clusters.len(), 2);

    let mut request = query(10, 40., 41.);
    request.precision_bits = cluster_precision_bits_for_zoom(7);
    let wider = indexed.load_map_clusters(request).unwrap();
    assert_eq!(wider.clusters.len(), 1);
    assert_eq!(wider.total_count, finer.total_count);
    assert_eq!(wider.clusters[0].representative_id, 2);

    let mut request = query(10, 40.15, 40.25);
    request.precision_bits = cluster_precision_bits_for_zoom(7);
    let panned = indexed.load_map_clusters(request).unwrap();
    assert_eq!(panned.clusters[0].lat, wider.clusters[0].lat);
    assert_eq!(panned.clusters[0].lng, wider.clusters[0].lng);
    assert_eq!(panned.total_count, wider.total_count);
}

#[test]
fn intermediate_levels_split_cells_into_at_most_four_children() {
    let connection = Connection::open_in_memory().unwrap();
    prepare(&connection);
    connection
        .execute_batch("DELETE FROM media; DELETE FROM media_access; DELETE FROM media_metadata;")
        .unwrap();
    for (index, character) in GEOHASH_ALPHABET.chars().enumerate() {
        let id = index as i64 + 1;
        connection
            .execute("INSERT INTO media VALUES (?, '2026-01-01')", [id])
            .unwrap();
        connection
            .execute("INSERT INTO media_access VALUES (?, 10, NULL)", [id])
            .unwrap();
        connection
            .execute(
                "INSERT INTO media_metadata VALUES (?, 40.2, -74.2, ?, NULL)",
                rusqlite::params![id, format!("dr{character}00000")],
            )
            .unwrap();
    }
    let mut indexed = IndexedConnection::new(connection);
    for (zoom, expected) in [(5, 1), (6, 4), (7, 16), (8, 32)] {
        let mut request = query(10, 40., 41.);
        request.precision_bits = cluster_precision_bits_for_zoom(zoom);
        let result = indexed.load_map_clusters(request).unwrap();
        assert_eq!(result.clusters.len(), expected);
        assert_eq!(result.total_count, 32);
        for cluster in result.clusters {
            let pattern = cluster_media_pattern(&cluster.id).unwrap();
            let count: i64 = indexed
                .query_row(
                    "SELECT COUNT(*) FROM media_metadata WHERE geohash GLOB ?",
                    [pattern],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, cluster.count);
        }
    }
}

#[test]
fn cluster_selection_rejects_wildcards_and_noncanonical_partial_cells() {
    for id in [
        "", "%", "dr*", "dr[01]", "dr0:0", "dr0:5", "dr1:1", "dr0:2:3", "é",
    ] {
        assert!(cluster_media_pattern(id).is_none(), "{id}");
    }
    assert_eq!(
        cluster_media_pattern("dr0:2").as_deref(),
        Some("dr[01234567]*")
    );
    assert_eq!(cluster_media_pattern("dr5").as_deref(), Some("dr5*"));
}
