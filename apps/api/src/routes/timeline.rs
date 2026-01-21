use axum::{extract::State, routing::post, Json, Router};
use chrono::{Datelike, NaiveDateTime};
use indexmap::IndexMap;

use crate::auth::{AppState, CurrentUser};
use crate::database::{fetch_all, queries};
use crate::error::{AppError, AppResult};
use crate::models::{MediaResponse, TimelineGroup, TimelineListRequest, TimelineListResponse};

pub fn router() -> Router<AppState> {
    Router::new().route("/timeline/list", post(list_timeline))
}

fn get_group_key(date_taken: Option<&str>, group_by: &str) -> String {
    let date_taken = match date_taken {
        Some(dt) => dt,
        None => return "Unknown".to_string(),
    };

    // Parse date - try ISO format first
    let dt = if let Ok(dt) = NaiveDateTime::parse_from_str(date_taken, "%Y-%m-%dT%H:%M:%S") {
        dt
    } else if let Ok(dt) = NaiveDateTime::parse_from_str(&date_taken.replace("Z", ""), "%Y-%m-%dT%H:%M:%S%.f") {
        dt
    } else if date_taken.len() >= 10 {
        // Just date
        if let Ok(d) = chrono::NaiveDate::parse_from_str(&date_taken[..10], "%Y-%m-%d") {
            d.and_hms_opt(0, 0, 0).unwrap()
        } else {
            return "Unknown".to_string();
        }
    } else {
        return "Unknown".to_string();
    };

    match group_by {
        "year" => dt.year().to_string(),
        "month" => format!("{}-{:02}", dt.year(), dt.month()),
        "week" => {
            let week = dt.iso_week();
            format!("{}-W{:02}", week.year(), week.week())
        }
        _ => date_taken.chars().take(10).collect(),
    }
}

async fn list_timeline(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(request): Json<TimelineListRequest>,
) -> AppResult<Json<TimelineListResponse>> {
    let conn = state.pool.get().map_err(AppError::Pool)?;
    let limit = request.limit.min(500);

    let rows = if let Some(ref cursor) = request.cursor {
        let parts: Vec<&str> = cursor.split('_').collect();
        if parts.len() == 2 {
            let cursor_date = parts[0];
            let cursor_id: i64 = parts[1].parse().unwrap_or(0);
            fetch_all(
                &conn,
                queries::timeline::SELECT_PAGINATED,
                &[&current_user.id, &cursor_date, &cursor_date, &cursor_id, &(limit + 1)],
                map_timeline_row,
            )?
        } else {
            fetch_default_timeline(&conn, current_user.id, limit)?
        }
    } else {
        fetch_default_timeline(&conn, current_user.id, limit)?
    };

    let has_more = rows.len() > limit as usize;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    // Group by date using IndexMap to preserve insertion order
    // (rows are already ordered by date_taken DESC from database)
    let mut grouped: IndexMap<String, Vec<MediaResponse>> = IndexMap::new();
    for (media, date_taken) in &rows {
        let key = get_group_key(date_taken.as_deref(), &request.group_by);
        grouped.entry(key).or_default().push(media.clone());
    }

    let groups: Vec<TimelineGroup> = grouped
        .into_iter()
        .map(|(date, media)| TimelineGroup { date, media })
        .collect();

    let next_cursor = if has_more && !rows.is_empty() {
        let (last, last_date) = rows.last().unwrap();
        last_date.as_ref().map(|dt| format!("{}_{}", dt, last.id))
    } else {
        None
    };

    Ok(Json(TimelineListResponse {
        groups,
        next_cursor,
        has_more,
    }))
}

fn fetch_default_timeline(
    conn: &crate::database::DbConn,
    user_id: i64,
    limit: i32,
) -> AppResult<Vec<(MediaResponse, Option<String>)>> {
    fetch_all(
        conn,
        queries::timeline::SELECT_DEFAULT,
        &[&user_id, &(limit + 1)],
        map_timeline_row,
    )
}

fn map_timeline_row(row: &rusqlite::Row) -> rusqlite::Result<(MediaResponse, Option<String>)> {
    let date_taken: Option<String> = row.get(9)?;
    let media = MediaResponse {
        id: row.get(0)?,
        filename: row.get(1)?,
        original_filename: row.get(2)?,
        media_type: row.get(3)?,
        mime_type: row.get(4)?,
        width: row.get(5)?,
        height: row.get(6)?,
        file_size: row.get(7)?,
        duration_seconds: row.get(8)?,
        date_taken: date_taken.clone(),
        gps_latitude: row.get(10)?,
        gps_longitude: row.get(11)?,
        camera_make: row.get(12)?,
        camera_model: row.get(13)?,
        lens_make: row.get(14)?,
        lens_model: row.get(15)?,
        iso: row.get(16)?,
        exposure_time: row.get(17)?,
        f_number: row.get(18)?,
        focal_length: row.get(19)?,
        focal_length_35mm: row.get(20)?,
        gps_altitude: row.get(21)?,
        location_city: row.get(22)?,
        location_state: row.get(23)?,
        location_country: row.get(24)?,
        video_codec: row.get(25)?,
        keywords: row.get(26)?,
        created_at: row.get(27)?,
    };
    Ok((media, date_taken))
}
