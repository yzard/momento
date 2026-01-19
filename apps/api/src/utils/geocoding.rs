use crate::config::ReverseGeocodingConfig;

pub fn reverse_geocode(
    config: &ReverseGeocodingConfig,
    latitude: f64,
    longitude: f64,
) -> (Option<String>, Option<String>) {
    if !config.enabled {
        return (None, None);
    }

    let url = format!(
        "{}?format=json&lat={}&lon={}&zoom=10&addressdetails=1",
        config.base_url, latitude, longitude
    );

    let client = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(config.timeout_seconds))
        .user_agent(&config.user_agent)
        .build()
    {
        Ok(c) => c,
        Err(_) => return (None, None),
    };

    let response = match client.get(&url).send() {
        Ok(r) => r,
        Err(_) => return (None, None),
    };

    let json: serde_json::Value = match response.json() {
        Ok(j) => j,
        Err(_) => return (None, None),
    };

    let address = match json.get("address") {
        Some(a) => a,
        None => return (None, None),
    };

    let state = address
        .get("state")
        .or_else(|| address.get("region"))
        .or_else(|| address.get("province"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let country = address
        .get("country")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    (state, country)
}
