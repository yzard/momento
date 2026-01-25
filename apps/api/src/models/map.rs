use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundingBox {
    pub north: f64,
    pub south: f64,
    pub east: f64,
    pub west: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapClustersRequest {
    pub bounds: BoundingBox,
    pub zoom: u8,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cluster {
    pub id: String,
    pub lat: f64,
    pub lng: f64,
    pub count: i64,
    pub representative_id: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapClustersResponse {
    pub clusters: Vec<Cluster>,
    pub total_count: i64,
}
