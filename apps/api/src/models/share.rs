use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareLinkResponse {
    pub id: i64,
    pub token: String,
    pub media_id: Option<i64>,
    pub album_id: Option<i64>,
    pub has_password: bool,
    pub expires_at: Option<String>,
    pub view_count: i64,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareCreateRequest {
    pub media_id: Option<i64>,
    pub album_id: Option<i64>,
    pub password: Option<String>,
    pub expires_in_days: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareDeleteRequest {
    pub share_id: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareListResponse {
    pub shares: Vec<ShareLinkResponse>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareVerifyRequest {
    pub password: String,
}
