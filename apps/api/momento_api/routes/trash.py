from datetime import datetime, timedelta
from typing import Annotated

from fastapi import APIRouter, Depends

from momento_api.auth.dependencies import CurrentUser, get_current_user
from momento_api.constants import TRASH_RETENTION_DAYS
from momento_api.database import execute_query, fetch_all, get_connection
from momento_api.models.trash import (
    TrashDeleteRequest,
    TrashListResponse,
    TrashMediaResponse,
    TrashResponse,
    TrashRestoreRequest,
)
from momento_api.processor.media_processor import delete_media_files

router = APIRouter(prefix="/trash", tags=["trash"])


def _row_to_trash_response(row) -> TrashMediaResponse:
    return TrashMediaResponse(
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
        deleted_at=row["deleted_at"],
        created_at=row["created_at"],
    )


@router.post("/list", response_model=TrashListResponse)
async def list_trash(
    current_user: Annotated[CurrentUser, Depends(get_current_user)],
) -> TrashListResponse:
    rows = fetch_all(
        """
        SELECT id, filename, original_filename, media_type, mime_type,
               width, height, file_size, duration_seconds, date_taken,
               deleted_at, created_at
        FROM media
        WHERE user_id = ? AND deleted_at IS NOT NULL
        ORDER BY deleted_at DESC
        """,
        (current_user.id,),
    )

    items = [_row_to_trash_response(row) for row in rows]
    return TrashListResponse(items=items, total_count=len(items))


@router.post("/restore", response_model=TrashResponse)
async def restore_from_trash(
    request: TrashRestoreRequest,
    current_user: Annotated[CurrentUser, Depends(get_current_user)],
) -> TrashResponse:
    if not request.media_ids:
        return TrashResponse(message="No media to restore", affected_count=0)

    placeholders = ",".join("?" * len(request.media_ids))
    execute_query(
        f"""
        UPDATE media
        SET deleted_at = NULL
        WHERE id IN ({placeholders}) AND user_id = ? AND deleted_at IS NOT NULL
        """,
        tuple(request.media_ids) + (current_user.id,),
    )
    get_connection().commit()

    return TrashResponse(
        message="Media restored successfully",
        affected_count=len(request.media_ids),
    )


@router.post("/delete", response_model=TrashResponse)
async def permanently_delete(
    request: TrashDeleteRequest,
    current_user: Annotated[CurrentUser, Depends(get_current_user)],
) -> TrashResponse:
    if not request.media_ids:
        return TrashResponse(message="No media to delete", affected_count=0)

    placeholders = ",".join("?" * len(request.media_ids))
    rows = fetch_all(
        f"""
        SELECT id, file_path, thumbnail_path
        FROM media
        WHERE id IN ({placeholders}) AND user_id = ? AND deleted_at IS NOT NULL
        """,
        tuple(request.media_ids) + (current_user.id,),
    )

    deleted_count = 0
    for row in rows:
        delete_media_files(row["file_path"], row["thumbnail_path"])
        execute_query("DELETE FROM media WHERE id = ?", (row["id"],))
        deleted_count += 1

    get_connection().commit()

    return TrashResponse(
        message="Media permanently deleted",
        affected_count=deleted_count,
    )


@router.post("/empty", response_model=TrashResponse)
async def empty_trash(
    current_user: Annotated[CurrentUser, Depends(get_current_user)],
) -> TrashResponse:
    rows = fetch_all(
        """
        SELECT id, file_path, thumbnail_path
        FROM media
        WHERE user_id = ? AND deleted_at IS NOT NULL
        """,
        (current_user.id,),
    )

    deleted_count = 0
    for row in rows:
        delete_media_files(row["file_path"], row["thumbnail_path"])
        execute_query("DELETE FROM media WHERE id = ?", (row["id"],))
        deleted_count += 1

    get_connection().commit()

    return TrashResponse(
        message="Trash emptied",
        affected_count=deleted_count,
    )


def cleanup_expired_trash() -> int:
    """Delete items that have been in trash for more than TRASH_RETENTION_DAYS."""
    cutoff_date = (datetime.now() - timedelta(days=TRASH_RETENTION_DAYS)).isoformat()

    rows = fetch_all(
        """
        SELECT id, file_path, thumbnail_path
        FROM media
        WHERE deleted_at IS NOT NULL AND deleted_at < ?
        """,
        (cutoff_date,),
    )

    deleted_count = 0
    for row in rows:
        delete_media_files(row["file_path"], row["thumbnail_path"])
        execute_query("DELETE FROM media WHERE id = ?", (row["id"],))
        deleted_count += 1

    get_connection().commit()
    return deleted_count
