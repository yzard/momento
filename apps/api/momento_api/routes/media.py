# pyright: reportMissingTypeArgument=false
from typing import Annotated

from fastapi import APIRouter, Depends, HTTPException, status
from fastapi.responses import FileResponse
from pydantic import BaseModel


from momento_api.auth.dependencies import CurrentUser, get_current_user
from momento_api.constants import ORIGINALS_DIR, PREVIEWS_DIR, THUMBNAILS_DIR
from momento_api.database import execute_query, fetch_all, fetch_one, get_connection
from momento_api.models.media import (
    MediaDeleteRequest,
    MediaGetRequest,
    MediaListRequest,
    MediaListResponse,
    MediaResponse,
    MediaUpdateRequest,
)
from momento_api.processor.media_processor import delete_media_files
from momento_api.processor.thumbnails import generate_image_preview

router = APIRouter(prefix="/media", tags=["media"])
thumbnail_router = APIRouter(prefix="/thumbnail", tags=["thumbnail"])
preview_router = APIRouter(prefix="/preview", tags=["preview"])


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


@router.post("/list", response_model=MediaListResponse)
async def list_media(
    request: MediaListRequest,
    current_user: Annotated[CurrentUser, Depends(get_current_user)],
) -> MediaListResponse:
    limit = min(request.limit, 100)

    if request.cursor:
        cursor_parts = request.cursor.split("_")
        if len(cursor_parts) == 2:
            cursor_date, cursor_id = cursor_parts
            rows = fetch_all(
                """
                SELECT * FROM media
                WHERE user_id = ? AND deleted_at IS NULL
                  AND (date_taken < ? OR (date_taken = ? AND id < ?))
                ORDER BY date_taken DESC, id DESC
                LIMIT ?
                """,
                (current_user.id, cursor_date, cursor_date, int(cursor_id), limit + 1),
            )
        else:
            rows = fetch_all(
                "SELECT * FROM media WHERE user_id = ? AND deleted_at IS NULL ORDER BY date_taken DESC, id DESC LIMIT ?",
                (current_user.id, limit + 1),
            )
    else:
        rows = fetch_all(
            "SELECT * FROM media WHERE user_id = ? AND deleted_at IS NULL ORDER BY date_taken DESC, id DESC LIMIT ?",
            (current_user.id, limit + 1),
        )

    has_more = len(rows) > limit
    items = [_row_to_media_response(row) for row in rows[:limit]]

    next_cursor = None
    if has_more and items:
        last = items[-1]
        next_cursor = f"{last.date_taken}_{last.id}"

    return MediaListResponse(items=items, next_cursor=next_cursor, has_more=has_more)


@router.post("/get", response_model=MediaResponse)
async def get_media(
    request: MediaGetRequest,
    current_user: Annotated[CurrentUser, Depends(get_current_user)],
) -> MediaResponse:
    row = fetch_one(
        "SELECT * FROM media WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
        (request.media_id, current_user.id),
    )
    if row is None:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND, detail="Media not found"
        )
    return _row_to_media_response(row)


@router.post("/update", response_model=MediaResponse)
async def update_media(
    request: MediaUpdateRequest,
    current_user: Annotated[CurrentUser, Depends(get_current_user)],
) -> MediaResponse:
    row = fetch_one(
        "SELECT * FROM media WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
        (request.media_id, current_user.id),
    )
    if row is None:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND, detail="Media not found"
        )

    updates: list[str] = []
    params: list[object] = []

    if request.date_taken is not None:
        updates.append("date_taken = ?")
        params.append(request.date_taken)

    if request.gps_latitude is not None:
        updates.append("gps_latitude = ?")
        params.append(request.gps_latitude)

    if request.gps_longitude is not None:
        updates.append("gps_longitude = ?")
        params.append(request.gps_longitude)

    if not updates:
        return _row_to_media_response(row)

    params.append(request.media_id)
    execute_query(f"UPDATE media SET {', '.join(updates)} WHERE id = ?", tuple(params))
    get_connection().commit()

    updated_row = fetch_one("SELECT * FROM media WHERE id = ?", (request.media_id,))
    if updated_row is None:
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR, detail="Update failed"
        )

    return _row_to_media_response(updated_row)


class DeleteMediaResponse(BaseModel):
    message: str


@router.post("/delete")
async def delete_media(
    request: MediaDeleteRequest,
    current_user: Annotated[CurrentUser, Depends(get_current_user)],
) -> DeleteMediaResponse:
    row = fetch_one(
        "SELECT id FROM media WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
        (request.media_id, current_user.id),
    )
    if row is None:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND, detail="Media not found"
        )

    execute_query(
        "UPDATE media SET deleted_at = datetime('now') WHERE id = ?",
        (request.media_id,),
    )
    get_connection().commit()

    return DeleteMediaResponse(message="Media moved to trash")


@router.get("/file/{media_id}")
async def get_media_file(
    media_id: int, current_user: Annotated[CurrentUser, Depends(get_current_user)]
) -> FileResponse:
    row = fetch_one(
        "SELECT file_path, mime_type, original_filename FROM media WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
        (media_id, current_user.id),
    )
    if row is None:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND, detail="Media not found"
        )

    file_path = ORIGINALS_DIR / row["file_path"]
    if not file_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND, detail="File not found"
        )

    return FileResponse(
        path=file_path,
        media_type=row["mime_type"] or "application/octet-stream",
        filename=row["original_filename"],
    )


@thumbnail_router.get("/{media_id}")
async def get_media_thumbnail(
    media_id: int, current_user: Annotated[CurrentUser, Depends(get_current_user)]
) -> FileResponse:
    row = fetch_one(
        "SELECT thumbnail_path FROM media WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
        (media_id, current_user.id),
    )
    if row is None:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND, detail="Media not found"
        )

    if row["thumbnail_path"] is None:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND, detail="Thumbnail not available"
        )

    thumb_path = THUMBNAILS_DIR / row["thumbnail_path"]
    if not thumb_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND, detail="Thumbnail file not found"
        )

    return FileResponse(path=thumb_path, media_type="image/jpeg")


@preview_router.get("/{media_id}")
async def get_media_preview(
    media_id: int, current_user: Annotated[CurrentUser, Depends(get_current_user)]
) -> FileResponse:
    row = fetch_one(
        "SELECT file_path, media_type, mime_type FROM media WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
        (media_id, current_user.id),
    )
    if row is None:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND, detail="Media not found"
        )

    original_path = ORIGINALS_DIR / row["file_path"]
    if not original_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND, detail="File not found"
        )

    if row["media_type"] == "video":
        return FileResponse(
            path=original_path, media_type=row["mime_type"] or "video/mp4"
        )

    web_compatible = {"image/jpeg", "image/png", "image/webp", "image/gif"}
    if row["mime_type"] in web_compatible:
        return FileResponse(path=original_path, media_type=row["mime_type"])

    preview_filename = f"{original_path.stem}_preview.jpg"
    preview_path = PREVIEWS_DIR / str(current_user.id) / preview_filename

    if not preview_path.exists():
        generate_image_preview(original_path, preview_path, 2048, 90)

    if not preview_path.exists():
        thumb_row = fetch_one(
            "SELECT thumbnail_path FROM media WHERE id = ?", (media_id,)
        )
        if thumb_row and thumb_row["thumbnail_path"]:
            return FileResponse(
                path=THUMBNAILS_DIR / thumb_row["thumbnail_path"],
                media_type="image/jpeg",
            )
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND, detail="Preview not available"
        )

    return FileResponse(path=preview_path, media_type="image/jpeg")
