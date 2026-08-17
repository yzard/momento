use momento_api::config::MetadataConfig;
use momento_api::processor::metadata::reverse_geocoding::reverse_geocode;
use serde_json::json;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn configuration(server: &MockServer) -> MetadataConfig {
    MetadataConfig {
        reverse_geocoding_enabled: true,
        reverse_geocoding_base_url: format!("{}/reverse", server.uri()),
        reverse_geocoding_user_agent: "Momento reverse geocoding test".to_string(),
        reverse_geocoding_timeout_seconds: 2,
        reverse_geocoding_rate_limit_seconds: 0.0,
        ..MetadataConfig::default()
    }
}

#[tokio::test]
async fn reverse_geocode_sends_coordinates_and_parses_location() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/reverse"))
        .and(query_param("format", "json"))
        .and(query_param("lat", "40.759"))
        .and(query_param("lon", "-73.9859"))
        .and(query_param("zoom", "10"))
        .and(query_param("addressdetails", "1"))
        .and(header("user-agent", "Momento reverse geocoding test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "address": {
                "city": "New York",
                "state": "New York",
                "country": "United States"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let location = reverse_geocode(&configuration(&server), 40.759, -73.9859).await;

    assert_eq!(
        location,
        (
            Some("New York".to_string()),
            Some("New York".to_string()),
            Some("United States".to_string())
        )
    );
}

#[tokio::test]
async fn reverse_geocode_uses_locality_and_region_fallbacks() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/reverse"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "address": {
                "village": "Keswick",
                "province": "Cumbria",
                "country": "United Kingdom"
            }
        })))
        .mount(&server)
        .await;

    let location = reverse_geocode(&configuration(&server), 54.6013, -3.1347).await;

    assert_eq!(location.0.as_deref(), Some("Keswick"));
    assert_eq!(location.1.as_deref(), Some("Cumbria"));
    assert_eq!(location.2.as_deref(), Some("United Kingdom"));
}

#[tokio::test]
async fn disabled_reverse_geocoding_does_not_send_a_request() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let mut config = configuration(&server);
    config.reverse_geocoding_enabled = false;

    let location = reverse_geocode(&config, 40.759, -73.9859).await;

    assert_eq!(location, (None, None, None));
}
