use momento_api::processor::metadata::reverse_geocoding::ReverseGeocoderSnapshot;
use sha2::{Digest, Sha256};

const COMPRESSED_GEONAMES_DATA: &[u8] =
    include_bytes!("../../../../src/backend/assets/geonames/geonames-cities500.tsv.gz");

#[test]
fn local_reverse_geocoder_finds_city_state_and_country() {
    let geocoder = ReverseGeocoderSnapshot::from_embedded().expect("local reverse geocoder");

    let location = geocoder.search(40.759, -73.9859).expect("location");

    assert_eq!(location.city, "Times Square");
    assert_eq!(location.state.as_deref(), Some("New York"));
    assert_eq!(location.country, "United States");
}

#[test]
fn embedded_geonames_snapshot_matches_manifest() {
    let checksum = format!("{:x}", Sha256::digest(COMPRESSED_GEONAMES_DATA));

    assert_eq!(
        checksum,
        "9d43c79540f5dd7b706132972a1d92845189148d5914ef2ce14a179870ffcb69"
    );
    let geocoder = ReverseGeocoderSnapshot::from_embedded().expect("local reverse geocoder");
    assert_eq!(geocoder.record_count(), 235_408);
}

#[test]
fn local_reverse_geocoder_handles_global_coordinates() {
    let geocoder = ReverseGeocoderSnapshot::from_embedded().expect("local reverse geocoder");
    let vermont = geocoder
        .search(44.5325, -72.7865)
        .expect("Vermont location");
    let tokyo = geocoder.search(35.6762, 139.6503).expect("Tokyo location");

    assert_eq!(vermont.city, "Stowe");
    assert_eq!(vermont.state.as_deref(), Some("Vermont"));
    assert_eq!(vermont.country, "United States");
    assert_eq!(tokyo.country, "Japan");
}

#[test]
fn local_reverse_geocoder_rejects_invalid_coordinates() {
    let geocoder = ReverseGeocoderSnapshot::from_embedded().expect("local reverse geocoder");
    assert_eq!(geocoder.search(91.0, 0.0), None);
    assert_eq!(geocoder.search(0.0, -181.0), None);
    assert_eq!(geocoder.search(0.0, 1.0), None);
    assert_eq!(geocoder.search(1.0, 0.0), None);
    assert_eq!(geocoder.search(f64::NAN, 0.0), None);
}
