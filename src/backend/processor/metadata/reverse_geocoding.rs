use std::cmp::Ordering;
use std::io::Read;
use std::sync::Arc;

use flate2::read::GzDecoder;

const GEONAMES_DATA: &[u8] = include_bytes!("../../assets/geonames/geonames-cities500.tsv.gz");

const MAX_DECOMPRESSED_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RECORDS: usize = 250_000;
const MAX_FIELD_BYTES: usize = 512;
const MAX_STRING_ARENA_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReverseGeocodedLocation {
    pub city: String,
    pub state: Option<String>,
    pub country: String,
}

#[derive(Debug, Clone)]
pub struct ReverseGeocoderSnapshot(Arc<PackedReverseGeocoder>);

#[derive(Debug)]
struct PackedReverseGeocoder {
    nodes: Vec<PackedPlace>,
    strings: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
struct PackedPlace {
    point: [f64; 3],
    city: StringSpan,
    state: StringSpan,
    country: StringSpan,
}

#[derive(Debug, Clone, Copy)]
struct StringSpan {
    start: u32,
    length: u16,
}

impl ReverseGeocoderSnapshot {
    pub fn from_embedded() -> Result<Self, String> {
        PackedReverseGeocoder::from_compressed_tsv(GEONAMES_DATA)
            .map(Arc::new)
            .map(Self)
    }

    pub fn search(&self, latitude: f64, longitude: f64) -> Option<ReverseGeocodedLocation> {
        self.0.search(latitude, longitude)
    }

    pub fn record_count(&self) -> usize {
        self.0.nodes.len()
    }
}

impl PackedReverseGeocoder {
    fn from_compressed_tsv(data: &[u8]) -> Result<Self, String> {
        let decoder = GzDecoder::new(data);
        let bounded_decoder = decoder.take(MAX_DECOMPRESSED_BYTES + 1);
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(b'\t')
            .has_headers(true)
            .from_reader(bounded_decoder);
        let mut strings = Vec::new();
        strings
            .try_reserve_exact(MAX_STRING_ARENA_BYTES.min(4 * 1024 * 1024))
            .map_err(|error| format!("failed to reserve GeoNames string arena: {error}"))?;
        let mut nodes = Vec::new();
        nodes
            .try_reserve_exact(MAX_RECORDS)
            .map_err(|error| format!("failed to reserve GeoNames place index: {error}"))?;

        for row in reader.byte_records() {
            let row = row.map_err(|error| format!("failed to parse GeoNames data: {error}"))?;
            if row.len() != 5 {
                return Err(format!(
                    "GeoNames row has {} fields; expected exactly 5",
                    row.len()
                ));
            }
            if nodes.len() == MAX_RECORDS {
                return Err(format!(
                    "GeoNames data exceeds the {MAX_RECORDS}-record bound"
                ));
            }
            let latitude = parse_coordinate(row.get(0), "latitude")?;
            let longitude = parse_coordinate(row.get(1), "longitude")?;
            let city = append_string(&mut strings, row.get(2), "city")?;
            let state = append_string(&mut strings, row.get(3), "state")?;
            let country = append_string(&mut strings, row.get(4), "country")?;
            nodes.push(PackedPlace {
                point: coordinates_on_unit_sphere(latitude, longitude),
                city,
                state,
                country,
            });
        }
        let remaining = reader.into_inner().limit();
        if remaining == 0 {
            return Err(format!(
                "GeoNames decompressed data exceeds {MAX_DECOMPRESSED_BYTES} bytes"
            ));
        }
        if nodes.is_empty() {
            return Err("GeoNames data contains no places".to_string());
        }
        build_packed_kd_tree(&mut nodes, 0);
        strings.shrink_to_fit();
        nodes.shrink_to_fit();
        Ok(Self { nodes, strings })
    }

    fn search(&self, latitude: f64, longitude: f64) -> Option<ReverseGeocodedLocation> {
        if !valid_coordinates(latitude, longitude) {
            return None;
        }
        let target = coordinates_on_unit_sphere(latitude, longitude);
        let mut best = None;
        nearest_in_packed_tree(&self.nodes, &target, 0, &mut best);
        let place = &self.nodes[best?.0];
        let city = self.string(place.city).to_string();
        let state = self.string(place.state);
        let country = self.string(place.country).to_string();
        Some(ReverseGeocodedLocation {
            city,
            state: (!state.is_empty()).then(|| state.to_string()),
            country,
        })
    }

    fn string(&self, span: StringSpan) -> &str {
        let start = span.start as usize;
        let end = start + span.length as usize;
        std::str::from_utf8(&self.strings[start..end])
            .expect("validated GeoNames string span must remain UTF-8")
    }
}

fn parse_coordinate(bytes: Option<&[u8]>, name: &str) -> Result<f64, String> {
    let value = std::str::from_utf8(bytes.ok_or_else(|| format!("missing GeoNames {name}"))?)
        .map_err(|error| format!("GeoNames {name} is not UTF-8: {error}"))?
        .parse::<f64>()
        .map_err(|error| format!("invalid GeoNames {name}: {error}"))?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(format!("GeoNames {name} is not finite"))
    }
}

fn append_string(
    arena: &mut Vec<u8>,
    value: Option<&[u8]>,
    name: &str,
) -> Result<StringSpan, String> {
    let value = value.ok_or_else(|| format!("missing GeoNames {name}"))?;
    std::str::from_utf8(value).map_err(|error| format!("GeoNames {name} is not UTF-8: {error}"))?;
    if value.len() > MAX_FIELD_BYTES || value.len() > u16::MAX as usize {
        return Err(format!(
            "GeoNames {name} exceeds the {MAX_FIELD_BYTES}-byte field bound"
        ));
    }
    let new_length = arena
        .len()
        .checked_add(value.len())
        .ok_or_else(|| "GeoNames string arena length overflowed".to_string())?;
    if new_length > MAX_STRING_ARENA_BYTES || new_length > u32::MAX as usize {
        return Err(format!(
            "GeoNames strings exceed the {MAX_STRING_ARENA_BYTES}-byte arena bound"
        ));
    }
    arena
        .try_reserve(value.len())
        .map_err(|error| format!("failed to extend GeoNames string arena: {error}"))?;
    let start = arena.len();
    arena.extend_from_slice(value);
    Ok(StringSpan {
        start: start as u32,
        length: value.len() as u16,
    })
}

fn build_packed_kd_tree(nodes: &mut [PackedPlace], depth: usize) {
    if nodes.len() <= 1 {
        return;
    }
    let axis = depth % 3;
    let middle = nodes.len() / 2;
    nodes.select_nth_unstable_by(middle, |left, right| {
        left.point[axis].total_cmp(&right.point[axis])
    });
    let (left, right_with_middle) = nodes.split_at_mut(middle);
    let (_, right) = right_with_middle
        .split_first_mut()
        .expect("non-empty middle slice");
    build_packed_kd_tree(left, depth + 1);
    build_packed_kd_tree(right, depth + 1);
}

fn nearest_in_packed_tree(
    nodes: &[PackedPlace],
    target: &[f64; 3],
    depth: usize,
    best: &mut Option<(usize, f64)>,
) {
    if nodes.is_empty() {
        return;
    }
    let middle = nodes.len() / 2;
    let node = &nodes[middle];
    let distance = squared_distance(&node.point, target);
    if best
        .as_ref()
        .is_none_or(|(_, best_distance)| distance < *best_distance)
    {
        *best = Some((middle, distance));
    }
    let axis = depth % 3;
    let (near, far, far_offset) = match target[axis].total_cmp(&node.point[axis]) {
        Ordering::Less => (&nodes[..middle], &nodes[middle + 1..], middle + 1),
        Ordering::Equal | Ordering::Greater => (&nodes[middle + 1..], &nodes[..middle], 0),
    };
    let mut near_best = None;
    nearest_in_packed_tree(near, target, depth + 1, &mut near_best);
    merge_nested_best(nodes, near, near_best, best);

    let plane_distance = target[axis] - node.point[axis];
    if best
        .as_ref()
        .is_none_or(|(_, best_distance)| plane_distance * plane_distance < *best_distance)
    {
        let mut far_best = None;
        nearest_in_packed_tree(far, target, depth + 1, &mut far_best);
        if let Some((index, distance)) = far_best {
            let base = if far_offset == 0 { 0 } else { far_offset };
            let absolute = base + index;
            if best
                .as_ref()
                .is_none_or(|(_, best_distance)| distance < *best_distance)
            {
                *best = Some((absolute, distance));
            }
        }
    }
}

fn merge_nested_best(
    nodes: &[PackedPlace],
    nested: &[PackedPlace],
    nested_best: Option<(usize, f64)>,
    best: &mut Option<(usize, f64)>,
) {
    let Some((index, distance)) = nested_best else {
        return;
    };
    let node_start = nodes.as_ptr() as usize;
    let nested_start = nested.as_ptr() as usize;
    let offset = nested_start.saturating_sub(node_start) / size_of::<PackedPlace>();
    let absolute = offset + index;
    if best
        .as_ref()
        .is_none_or(|(_, best_distance)| distance < *best_distance)
    {
        *best = Some((absolute, distance));
    }
}

fn squared_distance(left: &[f64; 3], right: &[f64; 3]) -> f64 {
    left.iter()
        .zip(right)
        .map(|(left, right)| {
            let difference = left - right;
            difference * difference
        })
        .sum()
}

fn coordinates_on_unit_sphere(latitude: f64, longitude: f64) -> [f64; 3] {
    let latitude = latitude.to_radians();
    let longitude = longitude.to_radians();
    [
        latitude.cos() * longitude.cos(),
        latitude.cos() * longitude.sin(),
        latitude.sin(),
    ]
}

fn valid_coordinates(latitude: f64, longitude: f64) -> bool {
    latitude.is_finite()
        && longitude.is_finite()
        && (-90.0..=90.0).contains(&latitude)
        && (-180.0..=180.0).contains(&longitude)
        && latitude != 0.0
        && longitude != 0.0
}
