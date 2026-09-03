use axum::{
    body::{to_bytes, Body},
    extract::{FromRef, FromRequest, Request},
    http::{header, HeaderValue},
    response::Response,
};

use crate::auth::AppState;
use crate::error::{AppError, AppResult};
use crate::executor::{ControlRequestDto, ControlResponse, MessageResponse};
use crate::logging::TypedControlLogPayloadSender;

const MAX_CONTROL_BODY_BYTES: usize = 1024 * 1024;

pub(crate) struct CpuJson<RequestDto>(pub RequestDto);

pub(crate) async fn render_json<ResponseDto>(
    state: &AppState,
    response: ResponseDto,
) -> AppResult<Response>
where
    ResponseDto: Into<ControlResponse>,
{
    let bytes = state
        .executors
        .cpu
        .serialize_control_response(response.into())
        .await?;
    let mut response = Response::new(Body::from(bytes));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    Ok(response)
}

pub(crate) async fn render_message(state: &AppState, message: &str) -> AppResult<Response> {
    render_json(
        state,
        MessageResponse {
            message: message.to_string(),
        },
    )
    .await
}

#[axum::async_trait]
impl<State, RequestDto> FromRequest<State> for CpuJson<RequestDto>
where
    State: Send + Sync,
    AppState: FromRef<State>,
    RequestDto: ControlRequestDto,
{
    type Rejection = AppError;

    async fn from_request(request: Request<Body>, state: &State) -> Result<Self, Self::Rejection> {
        let content_type = request
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .map(str::trim);
        if !content_type.is_some_and(is_json_content_type) {
            return Err(AppError::BadRequest(
                "request Content-Type must be application/json".to_string(),
            ));
        }
        let app_state = AppState::from_ref(state);
        let configured_limit = app_state.config.current().server.api_request_body_max_bytes;
        let maximum_bytes = configured_limit.min(MAX_CONTROL_BODY_BYTES);
        let log_sender = request
            .extensions()
            .get::<TypedControlLogPayloadSender>()
            .cloned();
        let bytes = to_bytes(request.into_body(), maximum_bytes)
            .await
            .map_err(|_| {
                AppError::PayloadTooLarge(format!(
                    "control request body exceeds {maximum_bytes} bytes"
                ))
            })?;
        let parsed = app_state
            .executors
            .cpu
            .parse_control_request(RequestDto::KIND, bytes.to_vec())
            .await?;
        if let Some(sender) = log_sender {
            sender.send(parsed.log_payload);
        }
        let request = parsed.request.map_err(|error| {
            let detail = format!("invalid JSON request: {}", error.detail);
            if error.data_error {
                AppError::UnprocessableEntity(detail)
            } else {
                AppError::BadRequest(detail)
            }
        })?;
        RequestDto::from_control_request(request)
            .map(Self)
            .ok_or_else(|| {
                AppError::Internal("CPU parser returned the wrong request DTO".to_string())
            })
    }
}

fn is_json_content_type(content_type: &str) -> bool {
    let content_type = content_type.to_ascii_lowercase();
    content_type == "application/json"
        || content_type
            .strip_prefix("application/")
            .is_some_and(|subtype| subtype.ends_with("+json"))
}
