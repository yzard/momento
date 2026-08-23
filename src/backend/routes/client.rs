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
            llm: state.config.llm.enabled,
            image_tagging: state.config.llm.image_tagging_enabled,
            deduplicate: state.config.llm.deduplicate_enabled,
            face_detection: state.config.llm.face_detection_enabled,
            image_aesthetics: state.config.llm.image_aesthetics_enabled,
            screenshot_detection: state.config.llm.screenshot_detection_enabled,
            document_detection: state.config.llm.document_detection_enabled,
        },
        backup: BackupCapabilities {
            enabled: true,
            max_upload_bytes: state.config.backup.max_upload_bytes,
            max_chunk_bytes: state.config.backup.max_chunk_bytes,
            max_active_uploads_per_user: state.config.backup.max_active_uploads_per_user,
            session_expiry_hours: state.config.backup.session_expiry_hours,
        },
    })
}
