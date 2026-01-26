use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaResponse {
    pub id: i64,
    pub filename: String,
    pub original_filename: String,
    pub media_type: String,
    pub mime_type: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub file_size: Option<i64>,
    pub duration_seconds: Option<f64>,
    pub date_taken: Option<String>,
    pub gps_latitude: Option<f64>,
    pub gps_longitude: Option<f64>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens_make: Option<String>,
    pub lens_model: Option<String>,
    pub iso: Option<i32>,
    pub exposure_time: Option<String>,
    pub f_number: Option<f64>,
    pub focal_length: Option<f64>,
    pub focal_length_35mm: Option<f64>,
    pub gps_altitude: Option<f64>,
    pub location_city: Option<String>,
    pub location_state: Option<String>,
    pub location_country: Option<String>,
    pub video_codec: Option<String>,
    pub keywords: Option<String>,
    pub content_hash: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaListRequest {
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<i32>,
    pub group_by: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaBatchRequest {
    pub ids: Vec<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaListResponse {
    pub items: Vec<MediaResponse>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<TimelineGroup>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaBatchResponse {
    pub items: Vec<MediaResponse>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaUpdateRequest {
    pub media_id: i64,
    pub date_taken: Option<String>,
    pub gps_latitude: Option<f64>,
    pub gps_longitude: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaDeleteRequest {
    pub media_id: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteMediaResponse {
    pub message: String,
}

#[derive(Debug, Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ThumbnailSize {
    #[default]
    Normal,
    Tiny,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailBatchRequest {
    pub media_ids: Vec<i64>,
    #[serde(default)]
    pub size: ThumbnailSize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailBatchResponse {
    pub thumbnails: std::collections::HashMap<i64, Option<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewBatchRequest {
    pub ids: Vec<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewBatchResponse {
    pub previews: std::collections::HashMap<i64, Option<String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineGroup {
    pub date: String,
    pub media: Vec<MediaResponse>,
}
