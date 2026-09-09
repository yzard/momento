use std::collections::VecDeque;
use std::ops::{Deref, DerefMut};

use rusqlite::Connection;

use crate::database::operations::{MapClustersQuery, SpatialBounds};
use crate::database::queries::map::build_clusters_query;
use crate::models::{Cluster, MapClustersResponse};

// Per pooled connection: sixteen user/level indexes, with a shared 32 MiB ceiling.
// SQL orders latitude bands and then longitude; building the spatial index only maps rows.
const MAX_INDEX_BYTES: usize = 32 * 1024 * 1024;
const MAX_INDEX_ENTRIES: usize = 16;
const MAX_RESPONSE_ROWS: usize = 4096;

/// Add two spatial bits per zoom step: at most four children per stable cell.
pub fn cluster_precision_bits_for_zoom(zoom: u8) -> usize {
    (10 + usize::from(zoom.saturating_sub(5)) * 2).min(40)
}

pub(crate) const GEOHASH_ALPHABET: &str = "0123456789bcdefghjkmnpqrstuvwxyz";

/// Full prefixes and partial-bit cells share the existing opaque cluster ID transport.
/// Partial cells use the lowest child prefix followed by :1 through :4.
pub(crate) fn cluster_media_pattern(id: &str) -> Option<String> {
    let (prefix, partial) = match id.split_once(':') {
        Some((prefix, bits)) => (
            prefix,
            Some(match bits {
                "1" => 1,
                "2" => 2,
                "3" => 3,
                "4" => 4,
                _ => return None,
            }),
        ),
        None => (id, None),
    };
    if prefix.is_empty()
        || prefix.len() > 12
        || !prefix
            .bytes()
            .all(|byte| GEOHASH_ALPHABET.as_bytes().contains(&byte))
    {
        return None;
    }
    let Some(bits) = partial else {
        return Some(format!("{prefix}*"));
    };
    let start = GEOHASH_ALPHABET.find(prefix.chars().last()?)?;
    let width = 1 << (5 - bits);
    if start % width != 0 {
        return None;
    }
    Some(format!(
        "{}[{}]*",
        &prefix[..prefix.len() - 1],
        &GEOHASH_ALPHABET[start..start + width]
    ))
}

struct ClusterIndex {
    user_id: i64,
    precision_bits: usize,
    clusters: Vec<Cluster>,
    bytes: usize,
}

/// Cache lifetime is tied to one SQLite connection: data_version is connection-local.
/// Indexes never cross users, connections, or a changed database snapshot.
pub struct IndexedConnection {
    connection: Connection,
    revision: Option<(i64, i64)>,
    indexes: VecDeque<ClusterIndex>,
}

impl IndexedConnection {
    pub(crate) fn new(connection: Connection) -> Self {
        Self {
            connection,
            revision: None,
            indexes: VecDeque::new(),
        }
    }

    pub(crate) fn load_map_clusters(
        &mut self,
        request: MapClustersQuery,
    ) -> rusqlite::Result<MapClustersResponse> {
        let transaction = self.connection.unchecked_transaction()?;
        // Acquire one read snapshot for both invalidation and a possible index rebuild.
        let version =
            transaction.pragma_query_value(None, "data_version", |row| row.get::<_, i64>(0))?;
        let revision = (
            version,
            transaction.query_row(crate::database::queries::map::TOTAL_CHANGES, [], |row| {
                row.get::<_, i64>(0)
            })?,
        );
        if self.revision != Some(revision) {
            self.indexes.clear();
            self.revision = Some(revision);
        }
        let cached = self.indexes.iter().position(|index| {
            index.user_id == request.user_id && index.precision_bits == request.precision_bits
        });
        let index = if let Some(position) = cached {
            self.indexes.remove(position).expect("existing map index")
        } else {
            let query = build_clusters_query(request.precision_bits);
            let mut statement = transaction.prepare(&query)?;
            let mut rows = statement.query([request.user_id])?;
            let mut clusters = Vec::new();
            let mut bytes = 0;
            while let Some(row) = rows.next()? {
                let cluster = Cluster {
                    id: row.get(0)?,
                    count: row.get(1)?,
                    lat: row.get(2)?,
                    lng: row.get(3)?,
                    representative_id: row.get(4)?,
                };
                // Include vector capacity growth as well as owned string storage.
                bytes += 2 * (std::mem::size_of::<Cluster>() + cluster.id.capacity());
                if bytes > MAX_INDEX_BYTES {
                    return Err(index_error("per-user map index exceeds 32 MiB"));
                }
                while self.indexes.iter().map(|index| index.bytes).sum::<usize>() + bytes
                    > MAX_INDEX_BYTES
                {
                    self.indexes.pop_front();
                }
                clusters.push(cluster);
            }
            ClusterIndex {
                user_id: request.user_id,
                precision_bits: request.precision_bits,
                clusters,
                bytes,
            }
        };
        while self.indexes.len() >= MAX_INDEX_ENTRIES
            || self.indexes.iter().map(|index| index.bytes).sum::<usize>() + index.bytes
                > MAX_INDEX_BYTES
        {
            self.indexes.pop_front();
        }
        self.indexes.push_back(index);
        let response = self
            .indexes
            .back()
            .expect("map index")
            .query(request.bounds);
        transaction.commit()?;
        response
    }
}

impl ClusterIndex {
    fn query(&self, bounds: SpatialBounds) -> rusqlite::Result<MapClustersResponse> {
        let mut clusters = Vec::new();
        let mut total_count = 0;
        let longitude_ranges = if bounds.west <= bounds.east {
            vec![(bounds.west, bounds.east)]
        } else {
            vec![(bounds.west, 180.0), (-180.0, bounds.east)]
        };
        for band in latitude_band(bounds.south)..=latitude_band(bounds.north) {
            let start = self
                .clusters
                .partition_point(|cluster| latitude_band(cluster.lat) < band);
            let end = self
                .clusters
                .partition_point(|cluster| latitude_band(cluster.lat) <= band);
            let strip = &self.clusters[start..end];
            for &(west, east) in &longitude_ranges {
                let start = strip.partition_point(|cluster| cluster.lng < west);
                let end = strip.partition_point(|cluster| cluster.lng <= east);
                for cluster in &strip[start..end] {
                    if cluster.lat < bounds.south || cluster.lat > bounds.north {
                        continue;
                    }
                    if clusters.len() == MAX_RESPONSE_ROWS {
                        return Err(index_error("map viewport exceeds 4096 clusters"));
                    }
                    total_count += cluster.count;
                    clusters.push(cluster.clone());
                }
            }
        }
        Ok(MapClustersResponse {
            clusters,
            total_count,
        })
    }
}

fn latitude_band(latitude: f64) -> i32 {
    (latitude + 90.0) as i32
}

fn index_error(message: &str) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(message.to_string())))
}

impl Deref for IndexedConnection {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        &self.connection
    }
}

impl DerefMut for IndexedConnection {
    fn deref_mut(&mut self) -> &mut Connection {
        &mut self.connection
    }
}

#[cfg(test)]
#[path = "../../../../tests/backend/database/map/mod.rs"]
mod tests;
