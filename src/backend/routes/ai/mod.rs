use axum::{extract::Path, extract::State, routing::post, Json, Router};

use crate::auth::{AppState, RequireAdmin};
use crate::config::validate_ai_cron_expression;
use crate::error::{AppError, AppResult};
use crate::models::{
    AiActionResponse, AiFeatureScheduleResponse, AiScheduleUpdateRequest, AiStatusResponse,
};
use crate::processor::ai::operation::{
    action_response, cancel_all_actions, cancel_feature_action, clean_all_actions,
    clean_feature_action, start_all_actions, start_feature_action, status, AiFeature,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ai/start", post(start_all))
        .route("/ai/status", post(all_status))
        .route("/ai/cancel", post(cancel_all))
        .route("/ai/clean", post(clean_all))
        .route("/ai/schedule/update", post(update_schedule))
        .route("/ai/:feature/start", post(start_feature))
        .route("/ai/:feature/cancel", post(cancel_feature))
        .route("/ai/:feature/clean", post(clean_feature))
}

async fn start_all(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
) -> AppResult<Json<AiActionResponse>> {
    let config = state.config.current();
    let results = start_all_actions(&config, &state.pool);
    if results.iter().any(|result| result.affected_jobs > 0) {
        state.llm_transport.wake_submissions();
    }
    Ok(Json(action_response("start", results)))
}

async fn all_status(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
) -> AppResult<Json<AiStatusResponse>> {
    let config = state.config.current();
    let schedules = AiFeature::ALL
        .into_iter()
        .map(|feature| AiFeatureScheduleResponse {
            feature: feature.name().to_string(),
            cron_expression: feature.cron_expression(&config.llm).to_string(),
        })
        .collect();
    Ok(Json(status(&config, &state.pool, schedules)?))
}

async fn update_schedule(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(request): Json<AiScheduleUpdateRequest>,
) -> AppResult<Json<AiFeatureScheduleResponse>> {
    let feature = parse_feature(&request.feature)?;
    validate_ai_cron_expression(feature.name(), &request.cron_expression)
        .map_err(|error| AppError::BadRequest(error.to_string()))?;
    let cron_expression = state
        .config
        .update_llm_cron_expression(
            feature.cron_config_field(),
            feature.name(),
            request.cron_expression,
        )
        .await?;
    Ok(Json(AiFeatureScheduleResponse {
        feature: feature.name().to_string(),
        cron_expression,
    }))
}

async fn cancel_all(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
) -> AppResult<Json<AiActionResponse>> {
    let results = cancel_all_actions(&state.pool);
    if results
        .iter()
        .any(|result| result.outcome == "cancellationRequested")
    {
        state.llm_transport.wake_cancellations();
    }
    Ok(Json(action_response("cancel", results)))
}

async fn clean_all(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
) -> AppResult<Json<AiActionResponse>> {
    Ok(Json(action_response(
        "clean",
        clean_all_actions(&state.pool),
    )))
}

async fn start_feature(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Path(feature_name): Path<String>,
) -> AppResult<Json<AiActionResponse>> {
    let feature = parse_feature(&feature_name)?;
    let config = state.config.current();
    let result = start_feature_action(&config, &state.pool, feature)?;
    if result.affected_jobs > 0 {
        state.llm_transport.wake_submissions();
    }
    Ok(Json(action_response("start", vec![result])))
}

async fn cancel_feature(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Path(feature_name): Path<String>,
) -> AppResult<Json<AiActionResponse>> {
    let feature = parse_feature(&feature_name)?;
    let result = cancel_feature_action(&state.pool, feature)?;
    if result.outcome == "cancellationRequested" {
        state.llm_transport.wake_cancellations();
    }
    Ok(Json(action_response("cancel", vec![result])))
}

async fn clean_feature(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Path(feature_name): Path<String>,
) -> AppResult<Json<AiActionResponse>> {
    let feature = parse_feature(&feature_name)?;
    Ok(Json(action_response(
        "clean",
        vec![clean_feature_action(&state.pool, feature)?],
    )))
}

fn parse_feature(feature_name: &str) -> AppResult<AiFeature> {
    AiFeature::from_control_name(feature_name)
        .ok_or_else(|| AppError::NotFound(format!("Unknown AI feature: {feature_name}")))
}
