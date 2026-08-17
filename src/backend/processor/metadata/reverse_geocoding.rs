use crate::config::MetadataConfig;

pub async fn reverse_geocode(
    config: &MetadataConfig,
    latitude: f64,
    longitude: f64,
) -> (Option<String>, Option<String>, Option<String>) {
    if !config.reverse_geocoding_enabled {
        return (None, None, None);
    }

    let url = format!(
        "{}?format=json&lat={}&lon={}&zoom=10&addressdetails=1",
        config.reverse_geocoding_base_url, latitude, longitude
    );
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(
            config.reverse_geocoding_timeout_seconds,
        ))
        .user_agent(&config.reverse_geocoding_user_agent)
        .build()
    {
        Ok(client) => client,
        Err(_) => return (None, None, None),
    };
    let response = match client.get(url).send().await {
        Ok(response) => response,
        Err(_) => return (None, None, None),
    };
    let response: serde_json::Value = match response.json().await {
        Ok(response) => response,
        Err(_) => return (None, None, None),
    };
    let Some(address) = response.get("address") else {
        return (None, None, None);
    };

    let city = address
        .get("city")
        .or_else(|| address.get("town"))
        .or_else(|| address.get("village"))
        .or_else(|| address.get("hamlet"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let state = address
        .get("state")
        .or_else(|| address.get("region"))
        .or_else(|| address.get("province"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let country = address
        .get("country")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);

    (city, state, country)
}
