from collections import defaultdict
from datetime import datetime
from typing import Annotated, Literal, Optional

from fastapi import APIRouter, Depends

from momento_api.auth.dependencies import CurrentUser, get_current_user
from momento_api.database import fetch_all
from momento_api.models.media import MediaResponse, TimelineGroup, TimelineListRequest, TimelineListResponse

router = APIRouter(prefix="/timeline", tags=["timeline"])


def _get_group_key(date_taken: Optional[str], group_by: Literal["year", "month", "week", "day"]) -> str:
    if not date_taken:
        return "Unknown"

    try:
        dt = datetime.fromisoformat(date_taken.replace("Z", "+00:00"))
    except (ValueError, AttributeError):
        return "Unknown"

    if group_by == "year":
        return str(dt.year)
    elif group_by == "month":
        return f"{dt.year}-{dt.month:02d}"
    elif group_by == "week":
        year, week, _ = dt.isocalendar()
        return f"{year}-W{week:02d}"
    else:
        return date_taken[:10]


def _row_to_media_response(row) -> MediaResponse:
    return MediaResponse(
        id=row["id"],
        filename=row["filename"],
        original_filename=row["original_filename"],
        media_type=row["media_type"],
        mime_type=row["mime_type"],
        width=row["width"],
        height=row["height"],
        file_size=row["file_size"],
        duration_seconds=row["duration_seconds"],
        date_taken=row["date_taken"],
        gps_latitude=row["gps_latitude"],
        gps_longitude=row["gps_longitude"],
        camera_make=row["camera_make"],
        camera_model=row["camera_model"],
        iso=row["iso"],
        exposure_time=row["exposure_time"],
        f_number=row["f_number"],
        focal_length=row["focal_length"],
        gps_altitude=row["gps_altitude"],
        location_state=row["location_state"],
        location_country=row["location_country"],
        keywords=row["keywords"],
        created_at=row["created_at"],
    )


def _fetch_timeline_rows(user_id: int, cursor: Optional[str], limit: int) -> list[dict]:
    base_query = "SELECT * FROM media WHERE user_id = ? AND deleted_at IS NULL"
    order_clause = " ORDER BY date_taken DESC, id DESC LIMIT ?"

    if cursor:
        cursor_parts = cursor.split("_")
        if len(cursor_parts) == 2:
            cursor_date, cursor_id = cursor_parts
            query = f"{base_query} AND (date_taken < ? OR (date_taken = ? AND id < ?)){order_clause}"
            return fetch_all(query, (user_id, cursor_date, cursor_date, int(cursor_id), limit + 1))

    # Default case (no cursor or invalid cursor structure basic fallback)
    query = f"{base_query}{order_clause}"
    return fetch_all(query, (user_id, limit + 1))


@router.post("/list", response_model=TimelineListResponse)
async def list_timeline(
    request: TimelineListRequest, current_user: Annotated[CurrentUser, Depends(get_current_user)]
) -> TimelineListResponse:
    # Cap limit to prevent large fetches
    limit = min(request.limit, 500)

    rows = _fetch_timeline_rows(current_user.id, request.cursor, limit)

    has_more = len(rows) > limit
    rows = rows[:limit]

    grouped: dict[str, list[MediaResponse]] = defaultdict(list)
    for row in rows:
        date_str = _get_group_key(row["date_taken"], request.group_by)
        grouped[date_str].append(_row_to_media_response(row))

    groups = [TimelineGroup(date=date, media=media) for date, media in grouped.items()]

    next_cursor = None
    if has_more and rows:
        last = rows[-1]
        next_cursor = f"{last['date_taken']}_{last['id']}"

    return TimelineListResponse(groups=groups, next_cursor=next_cursor, has_more=has_more)
