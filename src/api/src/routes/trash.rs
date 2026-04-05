use axum::{extract::State, routing::post, Json, Router};
use chrono::{Duration, Utc};

use crate::auth::{AppState, CurrentUser};
use crate::constants::TRASH_RETENTION_DAYS;
use crate::database::{execute_query, fetch_all, fetch_one, queries};
use crate::error::{AppError, AppResult};
use crate::models::{
    TrashDeleteRequest, TrashListResponse, TrashMediaResponse, TrashResponse, TrashRestoreRequest,
};
use crate::processor::media_processor::{delete_from_rtree, delete_media_files};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/trash/list", post(list_trash))
        .route("/trash/restore", post(restore_from_trash))
        .route("/trash/delete", post(permanently_delete))
        .route("/trash/empty", post(empty_trash))
}

fn map_trash_row(row: &rusqlite::Row) -> rusqlite::Result<TrashMediaResponse> {
    Ok(TrashMediaResponse {
        id: row.get(0)?,
        filename: row.get(1)?,
        original_filename: row.get(2)?,
        media_type: row.get(3)?,
        mime_type: row.get(4)?,
        width: row.get(5)?,
        height: row.get(6)?,
        file_size: row.get(7)?,
        duration_seconds: row.get(8)?,
        date_taken: row.get(9)?,
        deleted_at: row.get(10)?,
        created_at: row.get(11)?,
    })
}

async fn list_trash(
    State(state): State<AppState>,
    current_user: CurrentUser,
) -> AppResult<Json<TrashListResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let items = fetch_all(
        &conn,
        queries::trash::SELECT_DELETED,
        &[&current_user.id],
        map_trash_row,
    )?;

    let total_count = items.len() as i64;

    Ok(Json(TrashListResponse { items, total_count }))
}

async fn restore_from_trash(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<TrashRestoreRequest>,
) -> AppResult<Json<TrashResponse>> {
    if request.media_ids.is_empty() {
        return Ok(Json(TrashResponse {
            message: "No media to restore".to_string(),
            affected_count: 0,
        }));
    }

    let conn = state.pool.get().map_err(AppError::Pool)?;

    let placeholders: String = request
        .media_ids
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");

    let mut params: Vec<Box<dyn rusqlite::ToSql>> = request
        .media_ids
        .iter()
        .map(|id| Box::new(*id) as Box<dyn rusqlite::ToSql>)
        .collect();
    params.push(Box::new(current_user.id));

    let sql = queries::trash::RESTORE_MEDIA.replace("{}", &placeholders);
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    execute_query(&conn, &sql, &param_refs)?;

    Ok(Json(TrashResponse {
        message: "Media restored successfully".to_string(),
        affected_count: request.media_ids.len() as i64,
    }))
}

async fn permanently_delete(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<TrashDeleteRequest>,
) -> AppResult<Json<TrashResponse>> {
    if request.media_ids.is_empty() {
        return Ok(Json(TrashResponse {
            message: "No media to delete".to_string(),
            affected_count: 0,
        }));
    }

    let conn = state.pool.get().map_err(AppError::Pool)?;

    let placeholders: String = request
        .media_ids
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");

    let mut params: Vec<Box<dyn rusqlite::ToSql>> = request
        .media_ids
        .iter()
        .map(|id| Box::new(*id) as Box<dyn rusqlite::ToSql>)
        .collect();
    params.push(Box::new(current_user.id));

    let sql = queries::trash::SELECT_FOR_DELETE.replace("{}", &placeholders);
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let rows: Vec<MediaFileInfo> = fetch_all(&conn, &sql, &param_refs, |row| {
        Ok(MediaFileInfo {
            id: row.get(0)?,
            file_path: row.get(1)?,
            thumbnail_path: row.get(2)?,
        })
    })?;

    let mut deleted_count = 0;
    for row in rows {
        execute_query(
            &conn,
            queries::trash::DELETE_ACCESS,
            &[&row.id, &current_user.id],
        )?;

        let access_count: i64 =
            fetch_one(&conn, queries::trash::CHECK_ACCESS_COUNT, &[&row.id], |r| {
                r.get(0)
            })?
            .unwrap_or(0);

        if access_count == 0 {
            let _ = delete_from_rtree(&conn, row.id);
            delete_media_files(&row.file_path, row.thumbnail_path.as_deref());
            execute_query(&conn, queries::trash::DELETE_PERMANENTLY, &[&row.id])?;
        }

        deleted_count += 1;
    }

    Ok(Json(TrashResponse {
        message: "Media permanently deleted".to_string(),
        affected_count: deleted_count,
    }))
}

struct MediaFileInfo {
    id: i64,
    file_path: String,
    thumbnail_path: Option<String>,
}

async fn empty_trash(
    State(state): State<AppState>,
    current_user: CurrentUser,
) -> AppResult<Json<TrashResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;

    let rows: Vec<MediaFileInfo> = fetch_all(
        &conn,
        queries::trash::SELECT_ALL_DELETED,
        &[&current_user.id],
        |row| {
            Ok(MediaFileInfo {
                id: row.get(0)?,
                file_path: row.get(1)?,
                thumbnail_path: row.get(2)?,
            })
        },
    )?;

    let mut deleted_count = 0;
    for row in rows {
        execute_query(
            &conn,
            queries::trash::DELETE_ACCESS,
            &[&row.id, &current_user.id],
        )?;

        let access_count: i64 =
            fetch_one(&conn, queries::trash::CHECK_ACCESS_COUNT, &[&row.id], |r| {
                r.get(0)
            })?
            .unwrap_or(0);

        if access_count == 0 {
            let _ = delete_from_rtree(&conn, row.id);
            delete_media_files(&row.file_path, row.thumbnail_path.as_deref());
            execute_query(&conn, queries::trash::DELETE_PERMANENTLY, &[&row.id])?;
        }

        deleted_count += 1;
    }

    Ok(Json(TrashResponse {
        message: "Trash emptied".to_string(),
        affected_count: deleted_count,
    }))
}

pub fn cleanup_expired_trash(conn: &crate::database::DbConn) -> AppResult<i64> {
    let cutoff_date = (Utc::now() - Duration::days(TRASH_RETENTION_DAYS)).to_rfc3339();

    let rows: Vec<MediaFileInfoWithUser> = fetch_all(
        conn,
        queries::trash::SELECT_OLD_DELETED,
        &[&cutoff_date],
        |row| {
            Ok(MediaFileInfoWithUser {
                id: row.get(0)?,
                file_path: row.get(1)?,
                thumbnail_path: row.get(2)?,
                user_id: row.get(3)?,
            })
        },
    )?;

    let mut deleted_count = 0;
    for row in rows {
        execute_query(
            conn,
            queries::trash::DELETE_ACCESS,
            &[&row.id, &row.user_id],
        )?;

        let access_count: i64 =
            fetch_one(conn, queries::trash::CHECK_ACCESS_COUNT, &[&row.id], |r| {
                r.get(0)
            })?
            .unwrap_or(0);

        if access_count == 0 {
            let _ = delete_from_rtree(conn, row.id);
            delete_media_files(&row.file_path, row.thumbnail_path.as_deref());
            execute_query(conn, queries::trash::DELETE_PERMANENTLY, &[&row.id])?;
        }

        deleted_count += 1;
    }

    Ok(deleted_count)
}

struct MediaFileInfoWithUser {
    id: i64,
    file_path: String,
    thumbnail_path: Option<String>,
    user_id: i64,
}
