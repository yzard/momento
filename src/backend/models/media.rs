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

pub fn map_media_response(row: &rusqlite::Row) -> rusqlite::Result<MediaResponse> {
    map_media_response_columns(row, None, 27)
}

pub fn map_media_response_with_content_hash(
    row: &rusqlite::Row,
) -> rusqlite::Result<MediaResponse> {
    map_media_response_columns(row, Some(27), 28)
}

fn map_media_response_columns(
    row: &rusqlite::Row,
    content_hash_column: Option<usize>,
    created_at_column: usize,
) -> rusqlite::Result<MediaResponse> {
    Ok(MediaResponse {
        id: row.get(0)?,
        filename: row.get(1)?,
        original_filename: row.get(2)?,
        media_type: row.get(3)?,
        mime_type: row.get(4)?,
        width: row.get(5)?,
        height: row.get(6)?,
        file_size: row.get(7)?,
        duration_seconds: row.get(8)?,
        date_taken: row.get(9)?,
        gps_latitude: row.get(10)?,
        gps_longitude: row.get(11)?,
        camera_make: row.get(12)?,
        camera_model: row.get(13)?,
        lens_make: row.get(14)?,
        lens_model: row.get(15)?,
        iso: row.get(16)?,
        exposure_time: row.get(17)?,
        f_number: row.get(18)?,
        focal_length: row.get(19)?,
        focal_length_35mm: row.get(20)?,
        gps_altitude: row.get(21)?,
        location_city: row.get(22)?,
        location_state: row.get(23)?,
        location_country: row.get(24)?,
        video_codec: row.get(25)?,
        keywords: row.get(26)?,
        content_hash: content_hash_column
            .map(|column| row.get(column))
            .transpose()?,
        created_at: row.get(created_at_column)?,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaBatchRequest {
    pub ids: Vec<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineListRequest {
    pub cursor: Option<String>,
    pub limit: u32,
    pub group_by: String,
    pub search: String,
    pub media_type: Option<String>,
    pub classification: Option<String>,
    pub direction: TimelineDirection,
    pub anchor_date: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TimelineDirection {
    Older,
    Newer,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineMarkersRequest {
    pub media_type: Option<String>,
    pub classification: Option<String>,
    pub search: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineMarker {
    pub label: String,
    pub anchor_date: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineMarkersResponse {
    pub markers: Vec<TimelineMarker>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineListResponse {
    pub groups: Vec<TimelineGroup>,
    pub next_cursor: Option<String>,
    pub previous_cursor: Option<String>,
    pub has_older: bool,
    pub has_newer: bool,
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
    pub media_ids: Vec<i64>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MediaAccessResource {
    Original,
}

impl MediaAccessResource {
    pub fn path_segment(self) -> &'static str {
        match self {
            Self::Original => "original",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaAccessTicketRequest {
    pub media_id: i64,
    pub resource: MediaAccessResource,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaAccessTicketResponse {
    pub url: String,
    pub expires_at: String,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineGroup {
    pub date: String,
    pub media: Vec<MediaResponse>,
}
