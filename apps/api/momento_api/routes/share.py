import secrets
from datetime import datetime, timedelta, timezone
from typing import Annotated

from fastapi import APIRouter, Depends, HTTPException, status

from momento_api.auth.dependencies import CurrentUser, get_current_user
from momento_api.auth.password import hash_password
from momento_api.database import execute_query, fetch_all, fetch_one, get_connection, insert_returning_id
from momento_api.models.share import ShareCreateRequest, ShareDeleteRequest, ShareLinkResponse, ShareListResponse

router = APIRouter(prefix="/share", tags=["share"])


def _row_to_share_response(row) -> ShareLinkResponse:
    return ShareLinkResponse(
        id=row["id"],
        token=row["token"],
        media_id=row["media_id"],
        album_id=row["album_id"],
        has_password=row["password_hash"] is not None,
        expires_at=row["expires_at"],
        view_count=row["view_count"],
        created_at=row["created_at"],
    )


@router.post("/create", response_model=ShareLinkResponse)
async def create_share_link(
    request: ShareCreateRequest, current_user: Annotated[CurrentUser, Depends(get_current_user)]
) -> ShareLinkResponse:
    if request.media_id is None and request.album_id is None:
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail="Must specify media_id or album_id")

    if request.media_id is not None and request.album_id is not None:
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail="Cannot specify both media_id and album_id")

    if request.media_id:
        media_row = fetch_one("SELECT id FROM media WHERE id = ? AND user_id = ?", (request.media_id, current_user.id))
        if media_row is None:
            raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Media not found")

    if request.album_id:
        album_row = fetch_one("SELECT id FROM albums WHERE id = ? AND user_id = ?", (request.album_id, current_user.id))
        if album_row is None:
            raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Album not found")

    token = secrets.token_urlsafe(16)
    password_hash = hash_password(request.password) if request.password else None
    expires_at = None
    if request.expires_in_days:
        expires_at = (datetime.now(timezone.utc) + timedelta(days=request.expires_in_days)).isoformat()

    share_id = insert_returning_id(
        """
        INSERT INTO share_links (user_id, media_id, album_id, token, password_hash, expires_at)
        VALUES (?, ?, ?, ?, ?, ?)
        """,
        (current_user.id, request.media_id, request.album_id, token, password_hash, expires_at),
    )

    row = fetch_one("SELECT * FROM share_links WHERE id = ?", (share_id,))
    if row is None:
        raise HTTPException(status_code=status.HTTP_500_INTERNAL_SERVER_ERROR, detail="Failed to create share link")

    return _row_to_share_response(row)


@router.post("/list", response_model=ShareListResponse)
async def list_share_links(current_user: Annotated[CurrentUser, Depends(get_current_user)]) -> ShareListResponse:
    rows = fetch_all("SELECT * FROM share_links WHERE user_id = ? ORDER BY created_at DESC", (current_user.id,))
    return ShareListResponse(shares=[_row_to_share_response(row) for row in rows])


@router.post("/delete")
async def delete_share_link(
    request: ShareDeleteRequest, current_user: Annotated[CurrentUser, Depends(get_current_user)]
) -> dict:
    share_row = fetch_one(
        "SELECT id FROM share_links WHERE id = ? AND user_id = ?", (request.share_id, current_user.id)
    )
    if share_row is None:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Share link not found")

    execute_query("DELETE FROM share_links WHERE id = ?", (request.share_id,))
    get_connection().commit()

    return {"message": "Share link deleted successfully"}
