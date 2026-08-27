use axum::{routing::get, Json, Router};
use serde::Serialize;

use crate::auth::AppState;
use crate::constants::SUPPORTED_EXTENSIONS;
use crate::VERSION;

pub fn router() -> Router<AppState> {
    Router::new().route("/client/capabilities", get(get_capabilities))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CapabilitiesResponse {
    app_version: String,
    api_version: u8,
    supported_media_extensions: Vec<String>,
    features: FeatureFlags,
    backup: BackupCapabilities,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FeatureFlags {
    llm: bool,
    image_tagging: bool,
    deduplicate: bool,
    face_detection: bool,
    image_aesthetics: bool,
    screenshot_detection: bool,
    document_detection: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupCapabilities {
    enabled: bool,
    max_upload_bytes: u64,
    max_chunk_bytes: u64,
    max_active_uploads_per_user: usize,
    session_expiry_hours: u64,
}

async fn get_capabilities(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Json<CapabilitiesResponse> {
    let config = state.config.current();
    let mut supported_media_extensions = SUPPORTED_EXTENSIONS
        .iter()
        .map(|extension| (*extension).to_string())
        .collect::<Vec<_>>();
    supported_media_extensions.sort();

    Json(CapabilitiesResponse {
        app_version: VERSION.to_string(),
        api_version: 1,
        supported_media_extensions,
        features: FeatureFlags {
            llm: config.llm.enabled,
            image_tagging: config.llm.enabled,
            deduplicate: config.llm.enabled,
            face_detection: config.llm.enabled,
            image_aesthetics: config.llm.enabled,
            screenshot_detection: config.llm.enabled,
            document_detection: config.llm.enabled,
        },
        backup: BackupCapabilities {
            enabled: true,
            max_upload_bytes: config.backup.max_upload_bytes,
            max_chunk_bytes: config.backup.max_chunk_bytes,
            max_active_uploads_per_user: config.backup.max_active_uploads_per_user,
            session_expiry_hours: config.backup.session_expiry_hours,
        },
    })
}
