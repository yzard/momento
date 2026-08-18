use std::io::Read;
use std::sync::OnceLock;

use flate2::read::GzDecoder;
use rstar::{primitives::GeomWithData, RTree};
use serde::Deserialize;

const GEONAMES_DATA: &[u8] = include_bytes!("../../assets/geonames/geonames-cities500.tsv.gz");

static LOCAL_REVERSE_GEOCODER: OnceLock<Result<LocalReverseGeocoder, String>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReverseGeocodedLocation {
    pub city: String,
    pub state: Option<String>,
    pub country: String,
}

#[derive(Debug, Deserialize)]
struct GeoNamesRecord {
    latitude: f64,
    longitude: f64,
    city: String,
    state: String,
    country: String,
}

struct LocalReverseGeocoder {
    records: Vec<GeoNamesRecord>,
    index: RTree<GeomWithData<[f64; 3], usize>>,
}

impl LocalReverseGeocoder {
    fn from_compressed_tsv(data: &[u8]) -> Result<Self, String> {
        let mut decompressed = Vec::new();
        GzDecoder::new(data)
            .read_to_end(&mut decompressed)
            .map_err(|error| format!("failed to decompress GeoNames data: {error}"))?;
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(b'\t')
            .from_reader(decompressed.as_slice());
        let records = reader
            .deserialize::<GeoNamesRecord>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to parse GeoNames data: {error}"))?;
        if records.is_empty() {
            return Err("GeoNames data contains no places".to_string());
        }
        let points = records
            .iter()
            .enumerate()
            .map(|(index, record)| {
                GeomWithData::new(
                    coordinates_on_unit_sphere(record.latitude, record.longitude),
                    index,
                )
            })
            .collect();
        Ok(Self {
            records,
            index: RTree::bulk_load(points),
        })
    }

    fn search(&self, latitude: f64, longitude: f64) -> Option<ReverseGeocodedLocation> {
        if !valid_coordinates(latitude, longitude) {
            return None;
        }
        let nearest = self
            .index
            .nearest_neighbor(&coordinates_on_unit_sphere(latitude, longitude))?;
        let record = &self.records[nearest.data];
        Some(ReverseGeocodedLocation {
            city: record.city.clone(),
            state: (!record.state.is_empty()).then(|| record.state.clone()),
            country: record.country.clone(),
        })
    }
}

pub fn initialize() -> Result<(), String> {
    local_reverse_geocoder().map(|_| ())
}

pub fn reverse_geocode(
    latitude: f64,
    longitude: f64,
) -> Result<Option<ReverseGeocodedLocation>, String> {
    Ok(local_reverse_geocoder()?.search(latitude, longitude))
}

pub fn record_count() -> Result<usize, String> {
    Ok(local_reverse_geocoder()?.records.len())
}

fn local_reverse_geocoder() -> Result<&'static LocalReverseGeocoder, String> {
    LOCAL_REVERSE_GEOCODER
        .get_or_init(|| LocalReverseGeocoder::from_compressed_tsv(GEONAMES_DATA))
        .as_ref()
        .map_err(Clone::clone)
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
