from typing import Annotated

from fastapi import APIRouter, Depends, HTTPException, status

from momento_api.auth.dependencies import CurrentUser, get_current_user, require_admin
from momento_api.database import execute_query, fetch_all, fetch_one, get_connection, insert_returning_id
from momento_api.models.tag import (
    TagAddToMediaRequest,
    TagCreateRequest,
    TagDeleteRequest,
    TagListResponse,
    TagRemoveFromMediaRequest,
    TagResponse,
)

router = APIRouter(prefix="/tag", tags=["tag"])


def _row_to_tag_response(row) -> TagResponse:
    return TagResponse(id=row["id"], name=row["name"], created_at=row["created_at"])


@router.post("/list", response_model=TagListResponse)
async def list_tags(_: Annotated[CurrentUser, Depends(get_current_user)]) -> TagListResponse:
    rows = fetch_all("SELECT * FROM tags ORDER BY name", ())
    return TagListResponse(tags=[_row_to_tag_response(row) for row in rows])


@router.post("/create", response_model=TagResponse)
async def create_tag(request: TagCreateRequest, _: Annotated[CurrentUser, Depends(get_current_user)]) -> TagResponse:
    existing = fetch_one("SELECT id FROM tags WHERE name = ?", (request.name,))
    if existing:
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail="Tag already exists")

    tag_id = insert_returning_id("INSERT INTO tags (name) VALUES (?)", (request.name,))

    row = fetch_one("SELECT * FROM tags WHERE id = ?", (tag_id,))
    if row is None:
        raise HTTPException(status_code=status.HTTP_500_INTERNAL_SERVER_ERROR, detail="Failed to create tag")

    return _row_to_tag_response(row)


@router.post("/delete")
async def delete_tag(request: TagDeleteRequest, _: Annotated[CurrentUser, Depends(require_admin)]) -> dict:
    tag_row = fetch_one("SELECT id FROM tags WHERE id = ?", (request.tag_id,))
    if tag_row is None:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Tag not found")

    execute_query("DELETE FROM tags WHERE id = ?", (request.tag_id,))
    get_connection().commit()

    return {"message": "Tag deleted successfully"}


@router.post("/add-to-media")
async def add_tag_to_media(
    request: TagAddToMediaRequest, current_user: Annotated[CurrentUser, Depends(get_current_user)]
) -> dict:
    tag_row = fetch_one("SELECT id FROM tags WHERE id = ?", (request.tag_id,))
    if tag_row is None:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Tag not found")

    conn = get_connection()
    params: list[tuple[int, int]] = []
    for media_id in request.media_ids:
        media_row = fetch_one("SELECT id FROM media WHERE id = ? AND user_id = ?", (media_id, current_user.id))
        if media_row is None:
            continue
        params.append((media_id, request.tag_id))

    if not params:
        return {"message": "Tag added to media"}

    conn.executemany("INSERT OR IGNORE INTO media_tags (media_id, tag_id) VALUES (?, ?)", params)
    conn.commit()

    return {"message": "Tag added to media"}


@router.post("/remove-from-media")
async def remove_tag_from_media(
    request: TagRemoveFromMediaRequest, current_user: Annotated[CurrentUser, Depends(get_current_user)]
) -> dict:
    conn = get_connection()
    for media_id in request.media_ids:
        media_row = fetch_one("SELECT id FROM media WHERE id = ? AND user_id = ?", (media_id, current_user.id))
        if media_row:
            conn.execute("DELETE FROM media_tags WHERE media_id = ? AND tag_id = ?", (media_id, request.tag_id))
    conn.commit()

    return {"message": "Tag removed from media"}
