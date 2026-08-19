use serde::{Deserialize, Serialize};

use super::MediaResponse;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlacesListRequest {
    pub cursor: Option<String>,
    pub limit: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlaceGetRequest {
    pub place_id: String,
    pub cursor: Option<String>,
    pub limit: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlaceThumbnailRequest {
    pub place_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceSummary {
    pub place_id: String,
    pub city: String,
    pub state: Option<String>,
    pub country: String,
    pub media_count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceThumbnailResponse {
    pub thumbnail: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacesListResponse {
    pub places: Vec<PlaceSummary>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceGetResponse {
    pub place: PlaceSummary,
    pub media: Vec<MediaResponse>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}
