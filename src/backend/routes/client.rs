use axum::{response::Response, routing::get, Router};

use crate::auth::AppState;
use crate::constants::SUPPORTED_EXTENSIONS;
use crate::error::AppResult;
use crate::executor::{BackupCapabilitiesResponse, CapabilitiesResponse, FeatureFlagsResponse};
use crate::routes::render_json;
use crate::VERSION;

pub fn router() -> Router<AppState> {
    Router::new().route("/client/capabilities", get(get_capabilities))
}

async fn get_capabilities(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> AppResult<Response> {
    let config = state.config.current();
    let mut supported_media_extensions = SUPPORTED_EXTENSIONS
        .iter()
        .map(|extension| (*extension).to_string())
        .collect::<Vec<_>>();
    supported_media_extensions.sort();

    render_json(
        &state,
        CapabilitiesResponse {
            app_version: VERSION.to_string(),
            api_version: 1,
            supported_media_extensions,
            features: FeatureFlagsResponse {
                llm: config.llm.enabled,
                image_tagging: config.llm.enabled,
                deduplicate: config.llm.enabled,
                face_detection: config.llm.enabled,
                image_aesthetics: config.llm.enabled,
                screenshot_detection: config.llm.enabled,
                document_detection: config.llm.enabled,
            },
            backup: BackupCapabilitiesResponse {
                enabled: true,
                protocol_version: 2,
                max_upload_bytes: config.backup.max_upload_bytes,
                max_chunk_bytes: config.backup.max_chunk_bytes,
                session_expiry_hours: config.backup.session_expiry_hours,
            },
        },
    )
    .await
}
