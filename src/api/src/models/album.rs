use crate::models::MediaResponse;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumResponse {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub cover_media_id: Option<i64>,
    pub media_count: i64,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumDetailResponse {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub cover_media_id: Option<i64>,
    pub media: Vec<MediaResponse>,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumGetRequest {
    pub album_id: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumCreateRequest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumUpdateRequest {
    pub album_id: i64,
    pub name: Option<String>,
    pub description: Option<String>,
    pub cover_media_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumDeleteRequest {
    pub album_id: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumAddMediaRequest {
    pub album_id: i64,
    pub media_ids: Vec<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumRemoveMediaRequest {
    pub album_id: i64,
    pub media_ids: Vec<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumReorderRequest {
    pub album_id: i64,
    pub media_ids: Vec<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumListResponse {
    pub albums: Vec<AlbumResponse>,
}
