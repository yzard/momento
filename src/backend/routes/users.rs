use axum::{extract::State, response::Response, routing::post, Router};

use crate::auth::{AppState, CurrentUser, RequireAdmin, RESERVED_ADMIN_USERNAME};
use crate::database::operations::{
    CreateUser, CreateUserOutcome, DeleteUserOutcome, UpdateUser, UpdateUserOutcome, UserRecord,
};
use crate::error::{AppError, AppResult};
use crate::models::{
    UserCreateRequest, UserDeleteRequest, UserListResponse, UserResponse, UserUpdateRequest,
};
use crate::routes::{render_json, render_message, CpuJson};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/user/create", post(create_user))
        .route("/user/list", post(list_users))
        .route("/user/get", post(get_user))
        .route("/user/update", post(update_user))
        .route("/user/delete", post(delete_user))
}

fn user_response(record: UserRecord) -> UserResponse {
    let is_reserved = record.username == RESERVED_ADMIN_USERNAME;
    UserResponse {
        id: record.id,
        username: record.username,
        email: record.email,
        role: record.role,
        is_reserved,
        must_change_password: record.must_change_password,
        is_active: record.is_active,
        created_at: record.created_at,
    }
}

async fn create_user(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    CpuJson(request): CpuJson<UserCreateRequest>,
) -> AppResult<Response> {
    if request.password.len() < 8 {
        return Err(AppError::BadRequest(
            "Password must be at least 8 characters".to_string(),
        ));
    }
    let hashed = state
        .authentication_protection
        .hash_password(&request.password)
        .await?;

    match state
        .executors
        .sqlite
        .create_user_request(CreateUser {
            username: request.username,
            email: request.email,
            password_hash: hashed,
            role: request.role,
        })
        .await?
    {
        CreateUserOutcome::Duplicate => Err(AppError::BadRequest(
            "Username or email already exists".to_string(),
        )),
        CreateUserOutcome::Created(user) => render_json(&state, user_response(user)).await,
    }
}

async fn list_users(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
) -> AppResult<Response> {
    let users = state
        .executors
        .sqlite
        .list_users_request()
        .await?
        .into_iter()
        .map(user_response)
        .collect();

    render_json(&state, UserListResponse { users }).await
}

async fn get_user(State(state): State<AppState>, current_user: CurrentUser) -> AppResult<Response> {
    let mut user = user_response(
        state
            .executors
            .sqlite
            .load_user_record_request(current_user.id)
            .await?
            .ok_or_else(|| AppError::NotFound("User not found".to_string()))?,
    );
    user.must_change_password = current_user.must_change_password;

    render_json(&state, user).await
}

async fn update_user(
    State(state): State<AppState>,
    RequireAdmin(admin): RequireAdmin,
    CpuJson(request): CpuJson<UserUpdateRequest>,
) -> AppResult<Response> {
    let user_id = request.user_id;
    match state
        .executors
        .sqlite
        .update_user_request(UpdateUser {
            administrator_id: admin.id,
            user_id,
            role: request.role,
            is_active: request.is_active,
        })
        .await?
    {
        UpdateUserOutcome::NotFound => Err(AppError::NotFound("User not found".to_string())),
        UpdateUserOutcome::CannotDemoteSelf => {
            Err(AppError::BadRequest("Cannot demote yourself".to_string()))
        }
        UpdateUserOutcome::CannotDeactivateSelf => Err(AppError::BadRequest(
            "Cannot deactivate yourself".to_string(),
        )),
        UpdateUserOutcome::CannotDeactivateReservedAdmin => Err(AppError::Conflict(
            "The reserved admin account cannot be deactivated".to_string(),
        )),
        UpdateUserOutcome::Updated(user) => render_json(&state, user_response(user)).await,
    }
}

async fn delete_user(
    State(state): State<AppState>,
    RequireAdmin(admin): RequireAdmin,
    CpuJson(request): CpuJson<UserDeleteRequest>,
) -> AppResult<Response> {
    if request.user_id == admin.id {
        return Err(AppError::BadRequest("Cannot delete yourself".to_string()));
    }

    match state
        .executors
        .sqlite
        .delete_user_request(request.user_id)
        .await?
    {
        DeleteUserOutcome::NotFound => {
            return Err(AppError::NotFound("User not found".to_string()));
        }
        DeleteUserOutcome::CannotDeleteReservedAdmin => {
            return Err(AppError::Conflict(
                "The reserved admin account cannot be deleted".to_string(),
            ));
        }
        DeleteUserOutcome::Deleted => {}
    }

    render_message(&state, "User deleted successfully").await
}
