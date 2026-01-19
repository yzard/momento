use axum::{extract::State, routing::post, Json, Router};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::fs;
use tracing::warn;

use crate::auth::{AppState, CurrentUser};
use crate::constants::THUMBNAILS_DIR;
use crate::database::fetch_all;
use crate::error::{AppError, AppResult};
use crate::models::{GeoMediaResponse, MapMediaResponse};

pub fn router() -> Router<AppState> {
    Router::new().route("/map/media", post(get_map_media))
}

fn read_thumbnail_as_base64(thumbnail_path: &str) -> Option<String> {
    if thumbnail_path.is_empty() {
        return None;
    }

    let full_path = THUMBNAILS_DIR.join(thumbnail_path);
    if !full_path.exists() {
        return None;
    }

    match fs::read(&full_path) {
        Ok(data) => {
            let base64_data = STANDARD.encode(&data);
            Some(format!("data:image/jpeg;base64,{}", base64_data))
        }
        Err(e) => {
            warn!("Failed to read thumbnail {}: {}", full_path.display(), e);
            None
        }
    }
}

async fn get_map_media(
    State(state): State<AppState>,
    current_user: CurrentUser,
) -> AppResult<Json<MapMediaResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let items = fetch_all(
        &conn,
        r#"
        SELECT id, thumbnail_path, gps_latitude, gps_longitude, date_taken, media_type, mime_type, original_filename
        FROM media
        WHERE user_id = ? AND gps_latitude IS NOT NULL AND gps_longitude IS NOT NULL
        ORDER BY date_taken DESC
        "#,
        &[&current_user.id],
        |row| {
            let thumbnail_path: Option<String> = row.get(1)?;
            let thumbnail_data = thumbnail_path
                .as_deref()
                .and_then(read_thumbnail_as_base64);

            Ok(GeoMediaResponse {
                id: row.get(0)?,
                thumbnail_path,
                thumbnail_data,
                latitude: row.get(2)?,
                longitude: row.get(3)?,
                date_taken: row.get(4)?,
                media_type: row.get(5)?,
                mime_type: row.get(6)?,
                original_filename: row.get(7)?,
            })
        },
    )?;

    Ok(Json(MapMediaResponse { items }))
}
