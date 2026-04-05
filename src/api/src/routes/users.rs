use axum::{
    extract::{Query, State},
    routing::post,
    Json, Router,
};
use serde::Deserialize;

use crate::auth::{hash_password, AppState, CurrentUser, RequireAdmin};
use crate::database::{execute_query, fetch_all, fetch_one, insert_returning_id, queries};
use crate::error::{AppError, AppResult};
use crate::models::{
    UserCreateRequest, UserDeleteRequest, UserListResponse, UserResponse, UserUpdateRequest,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/user/create", post(create_user))
        .route("/user/list", post(list_users))
        .route("/user/get", post(get_user))
        .route("/user/update", post(update_user))
        .route("/user/delete", post(delete_user))
}

fn row_to_user_response(
    id: i64,
    username: String,
    email: String,
    role: String,
    must_change_password: i32,
    is_active: i32,
    created_at: String,
) -> UserResponse {
    UserResponse {
        id,
        username,
        email,
        role,
        must_change_password: must_change_password != 0,
        is_active: is_active != 0,
        created_at,
    }
}

async fn create_user(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(request): Json<UserCreateRequest>,
) -> AppResult<Json<UserResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    // Check existing
    let existing = fetch_one(
        &conn,
        queries::users::SELECT_ID_BY_CREDENTIALS,
        &[&request.username, &request.email],
        |row| row.get::<_, i64>(0),
    )?;

    if existing.is_some() {
        return Err(AppError::BadRequest(
            "Username or email already exists".to_string(),
        ));
    }

    if request.password.len() < 8 {
        return Err(AppError::BadRequest(
            "Password must be at least 8 characters".to_string(),
        ));
    }

    let hashed = hash_password(&request.password)
        .map_err(|e| AppError::Internal(format!("Failed to hash password: {}", e)))?;

    let user_id = insert_returning_id(
        &conn,
        queries::users::INSERT,
        &[&request.username, &request.email, &hashed, &request.role],
    )?;

    let user = fetch_one(&conn, queries::users::SELECT_BY_ID, &[&user_id], |row| {
        Ok(row_to_user_response(
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
        ))
    })?
    .ok_or_else(|| AppError::Internal("Failed to create user".to_string()))?;

    Ok(Json(user))
}

async fn list_users(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
) -> AppResult<Json<UserListResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let users = fetch_all(&conn, queries::users::SELECT_ALL, &[], |row| {
        Ok(row_to_user_response(
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
        ))
    })?;

    Ok(Json(UserListResponse { users }))
}

async fn get_user(
    State(state): State<AppState>,
    current_user: CurrentUser,
) -> AppResult<Json<UserResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let user = fetch_one(
        &conn,
        queries::users::SELECT_BY_ID,
        &[&current_user.id],
        |row| {
            Ok(row_to_user_response(
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ))
        },
    )?
    .ok_or_else(|| AppError::NotFound("User not found".to_string()))?;

    Ok(Json(user))
}

#[derive(Deserialize)]
struct UserIdQuery {
    user_id: i64,
}

async fn update_user(
    State(state): State<AppState>,
    RequireAdmin(admin): RequireAdmin,
    Query(query): Query<UserIdQuery>,
    Json(request): Json<UserUpdateRequest>,
) -> AppResult<Json<UserResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;
    let user_id = query.user_id;

    // Check user exists
    let exists = fetch_one(&conn, queries::users::CHECK_EXISTS, &[&user_id], |row| {
        row.get::<_, i64>(0)
    })?;

    if exists.is_none() {
        return Err(AppError::NotFound("User not found".to_string()));
    }

    if user_id == admin.id && request.role.as_deref() == Some("user") {
        return Err(AppError::BadRequest("Cannot demote yourself".to_string()));
    }

    let mut updates = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(ref role) = request.role {
        updates.push("role = ?");
        params.push(Box::new(role.clone()));
    }

    if let Some(is_active) = request.is_active {
        if user_id == admin.id && !is_active {
            return Err(AppError::BadRequest(
                "Cannot deactivate yourself".to_string(),
            ));
        }
        updates.push("is_active = ?");
        params.push(Box::new(if is_active { 1i32 } else { 0i32 }));
    }

    if !updates.is_empty() {
        params.push(Box::new(user_id));
        let sql = format!("UPDATE users SET {} WHERE id = ?", updates.join(", "));
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        execute_query(&conn, &sql, &param_refs)?;
    }

    let user = fetch_one(&conn, queries::users::SELECT_BY_ID, &[&user_id], |row| {
        Ok(row_to_user_response(
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
        ))
    })?
    .ok_or_else(|| AppError::Internal("Failed to update user".to_string()))?;

    Ok(Json(user))
}

async fn delete_user(
    State(state): State<AppState>,
    RequireAdmin(admin): RequireAdmin,
    Json(request): Json<UserDeleteRequest>,
) -> AppResult<Json<serde_json::Value>> {
    if request.user_id == admin.id {
        return Err(AppError::BadRequest("Cannot delete yourself".to_string()));
    }

    let conn = state.pool.get().map_err(AppError::Pool)?;

    let exists = fetch_one(
        &conn,
        queries::users::CHECK_EXISTS,
        &[&request.user_id],
        |row| row.get::<_, i64>(0),
    )?;

    if exists.is_none() {
        return Err(AppError::NotFound("User not found".to_string()));
    }

    execute_query(&conn, queries::users::DELETE, &[&request.user_id])?;

    Ok(Json(
        serde_json::json!({"message": "User deleted successfully"}),
    ))
}
