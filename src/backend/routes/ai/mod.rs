use axum::{extract::Path, extract::State, response::Response, routing::post, Router};

use crate::auth::{AppState, RequireAdmin};
use crate::config::validate_ai_cron_expression;
use crate::error::{AppError, AppResult};
use crate::models::{AiFeatureActionResult, AiFeatureScheduleResponse, AiScheduleUpdateRequest};
use crate::processor::ai::operation::{action_response, AiFeature, AiFeatureCleanOutcome};
use crate::routes::{render_json, CpuJson};

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
) -> AppResult<Response> {
    let config = state.config.current();
    let mut results = Vec::with_capacity(AiFeature::ALL.len());
    for feature in AiFeature::ALL {
        if !config.llm.enabled {
            results.push(AiFeatureActionResult {
                feature: feature.name().to_string(),
                outcome: "disabled".to_string(),
                affected_jobs: 0,
                error: None,
            });
            continue;
        }
        match state
            .executors
            .sqlite
            .start_ai_feature_request(feature, "manual".to_string(), None)
            .await
        {
            Ok(queued) => results.push(start_result(feature, queued)),
            Err(error) => results.push(AiFeatureActionResult {
                feature: feature.name().to_string(),
                outcome: "failed".to_string(),
                affected_jobs: 0,
                error: Some(error.to_string()),
            }),
        }
    }
    if results.iter().any(|result| result.affected_jobs > 0) {
        state.llm_transport.wake_submissions();
    }
    render_json(&state, action_response("start", results)).await
}

async fn all_status(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
) -> AppResult<Response> {
    let config = state.config.current();
    let schedules = AiFeature::ALL
        .into_iter()
        .map(|feature| AiFeatureScheduleResponse {
            feature: feature.name().to_string(),
            cron_expression: feature.cron_expression(&config.llm).to_string(),
        })
        .collect();
    let response = state
        .executors
        .sqlite
        .load_ai_status_request(config.as_ref().clone(), schedules)
        .await?;
    render_json(&state, response).await
}

async fn update_schedule(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    CpuJson(request): CpuJson<AiScheduleUpdateRequest>,
) -> AppResult<Response> {
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
    render_json(
        &state,
        AiFeatureScheduleResponse {
            feature: feature.name().to_string(),
            cron_expression,
        },
    )
    .await
}

async fn cancel_all(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
) -> AppResult<Response> {
    let results = state
        .executors
        .sqlite
        .cancel_all_ai_features_request()
        .await?;
    if results
        .iter()
        .any(|result| result.outcome == "cancellationRequested")
    {
        state.llm_transport.wake_cancellations();
    }
    render_json(&state, action_response("cancel", results)).await
}

async fn clean_all(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
) -> AppResult<Response> {
    let mut results = Vec::with_capacity(AiFeature::ALL.len());
    for feature in AiFeature::ALL {
        match clean_feature_with_executors(&state, feature).await {
            Ok(result) => results.push(result),
            Err(error) => results.push(AiFeatureActionResult {
                feature: feature.name().to_string(),
                outcome: "failed".to_string(),
                affected_jobs: 0,
                error: Some(error.to_string()),
            }),
        }
    }
    render_json(&state, action_response("clean", results)).await
}

async fn start_feature(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Path(feature_name): Path<String>,
) -> AppResult<Response> {
    let feature = parse_feature(&feature_name)?;
    let config = state.config.current();
    if !config.llm.enabled {
        return Err(AppError::Validation(format!(
            "{} is unavailable because LLM is disabled",
            feature.name()
        )));
    }
    let queued = state
        .executors
        .sqlite
        .start_ai_feature_request(feature, "manual".to_string(), None)
        .await?;
    let result = start_result(feature, queued);
    if result.affected_jobs > 0 {
        state.llm_transport.wake_submissions();
    }
    render_json(&state, action_response("start", vec![result])).await
}

fn start_result(feature: AiFeature, queued: usize) -> AiFeatureActionResult {
    AiFeatureActionResult {
        feature: feature.name().to_string(),
        outcome: if queued > 0 { "queued" } else { "noWork" }.to_string(),
        affected_jobs: queued as i64,
        error: None,
    }
}

async fn cancel_feature(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Path(feature_name): Path<String>,
) -> AppResult<Response> {
    let feature = parse_feature(&feature_name)?;
    let result = state
        .executors
        .sqlite
        .cancel_ai_feature_request(feature)
        .await?;
    if result.outcome == "cancellationRequested" {
        state.llm_transport.wake_cancellations();
    }
    render_json(&state, action_response("cancel", vec![result])).await
}

async fn clean_feature(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Path(feature_name): Path<String>,
) -> AppResult<Response> {
    let feature = parse_feature(&feature_name)?;
    let response = action_response(
        "clean",
        vec![clean_feature_with_executors(&state, feature).await?],
    );
    render_json(&state, response).await
}

async fn clean_feature_with_executors(
    state: &AppState,
    feature: AiFeature,
) -> AppResult<AiFeatureActionResult> {
    let cleanup_group_id = format!(
        "ai-clean-{}-{}",
        feature.name(),
        uuid::Uuid::new_v4().simple()
    );
    match state
        .executors
        .sqlite
        .clean_ai_feature_request(feature, cleanup_group_id)
        .await?
    {
        AiFeatureCleanOutcome::Cleaned {
            result,
            cleanup_group_created,
        } => {
            if cleanup_group_created {
                state.scheduler.wake_journal_recovery();
            }
            Ok(result)
        }
        AiFeatureCleanOutcome::ActiveWork => Err(AppError::Conflict(format!(
            "{} has active work and cannot be cleaned",
            feature.name()
        ))),
        AiFeatureCleanOutcome::PendingCancellation => Err(AppError::Conflict(format!(
            "{} cancellation has not been acknowledged",
            feature.name()
        ))),
        AiFeatureCleanOutcome::PendingResultCleanup => {
            state.scheduler.wake_llm_results();
            Err(AppError::Conflict(format!(
                "{} result cleanup is still finishing",
                feature.name()
            )))
        }
        AiFeatureCleanOutcome::PathConflict => Err(AppError::Conflict(format!(
            "{} files are being changed by another operation",
            feature.name()
        ))),
    }
}

fn parse_feature(feature_name: &str) -> AppResult<AiFeature> {
    AiFeature::from_control_name(feature_name)
        .ok_or_else(|| AppError::NotFound(format!("Unknown AI feature: {feature_name}")))
}
