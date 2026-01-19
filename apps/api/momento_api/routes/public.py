from datetime import datetime, timezone
from typing import Optional

from fastapi import APIRouter, HTTPException, status
from fastapi.responses import FileResponse

from momento_api.auth.password import verify_password
from momento_api.constants import ORIGINALS_DIR, THUMBNAILS_DIR
from momento_api.database import execute_query, fetch_all, fetch_one, get_connection
from momento_api.models.media import MediaResponse
from momento_api.models.share import ShareVerifyRequest

router = APIRouter(prefix="/public", tags=["public"])


def _validate_share_token(token: str, password: Optional[str]) -> dict:
    share_row = fetch_one("SELECT * FROM share_links WHERE token = ?", (token,))
    if share_row is None:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Share link not found")

    if share_row["expires_at"]:
        expires = datetime.fromisoformat(share_row["expires_at"])
        if datetime.now(timezone.utc) > expires:
            raise HTTPException(status_code=status.HTTP_410_GONE, detail="Share link has expired")

    if share_row["password_hash"]:
        if not password:
            raise HTTPException(status_code=status.HTTP_401_UNAUTHORIZED, detail="Password required")
        if not verify_password(password, share_row["password_hash"]):
            raise HTTPException(status_code=status.HTTP_401_UNAUTHORIZED, detail="Invalid password")

    execute_query("UPDATE share_links SET view_count = view_count + 1 WHERE id = ?", (share_row["id"],))
    get_connection().commit()

    return dict(share_row)


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
        created_at=row["created_at"],
    )


@router.get("/share/{token}")
async def get_shared_content(token: str, password: Optional[str]) -> dict:
    if password is None:
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail="Password is required")
    share = _validate_share_token(token, password)

    if share["media_id"]:
        media_row = fetch_one("SELECT * FROM media WHERE id = ?", (share["media_id"],))
        if media_row is None:
            raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Media not found")
        return {"type": "media", "media": _row_to_media_response(media_row)}

    if share["album_id"]:
        album_row = fetch_one("SELECT * FROM albums WHERE id = ?", (share["album_id"],))
        if album_row is None:
            raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Album not found")

        media_rows = fetch_all(
            """
            SELECT m.* FROM media m
            JOIN album_media am ON m.id = am.media_id
            WHERE am.album_id = ?
            ORDER BY am.position
            """,
            (share["album_id"],),
        )

        return {
            "type": "album",
            "album": {"id": album_row["id"], "name": album_row["name"], "description": album_row["description"]},
            "media": [_row_to_media_response(row) for row in media_rows],
        }

    raise HTTPException(status_code=status.HTTP_500_INTERNAL_SERVER_ERROR, detail="Invalid share link")


@router.post("/share/{token}/verify")
async def verify_share_password(token: str, request: ShareVerifyRequest) -> dict:
    share_row = fetch_one("SELECT password_hash FROM share_links WHERE token = ?", (token,))
    if share_row is None:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Share link not found")

    if not share_row["password_hash"]:
        return {"valid": True, "message": "No password required"}

    if verify_password(request.password, share_row["password_hash"]):
        return {"valid": True}

    raise HTTPException(status_code=status.HTTP_401_UNAUTHORIZED, detail="Invalid password")


@router.get("/share/{token}/media/{media_id}")
async def get_shared_media_file(token: str, media_id: int, password: Optional[str]) -> FileResponse:
    if password is None:
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail="Password is required")
    share = _validate_share_token(token, password)

    if share["media_id"] and share["media_id"] != media_id:
        raise HTTPException(status_code=status.HTTP_403_FORBIDDEN, detail="Media not in share")

    if share["album_id"]:
        album_media = fetch_one(
            "SELECT 1 FROM album_media WHERE album_id = ? AND media_id = ?", (share["album_id"], media_id)
        )
        if album_media is None:
            raise HTTPException(status_code=status.HTTP_403_FORBIDDEN, detail="Media not in shared album")

    media_row = fetch_one("SELECT file_path, mime_type, original_filename FROM media WHERE id = ?", (media_id,))
    if media_row is None:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Media not found")

    file_path = ORIGINALS_DIR / media_row["file_path"]
    if not file_path.exists():
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="File not found")

    return FileResponse(
        path=file_path,
        media_type=media_row["mime_type"] or "application/octet-stream",
        filename=media_row["original_filename"],
    )


@router.get("/share/{token}/thumbnail/{media_id}")
async def get_shared_thumbnail(token: str, media_id: int, password: Optional[str]) -> FileResponse:
    if password is None:
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail="Password is required")
    share = _validate_share_token(token, password)

    if share["media_id"] and share["media_id"] != media_id:
        raise HTTPException(status_code=status.HTTP_403_FORBIDDEN, detail="Media not in share")

    if share["album_id"]:
        album_media = fetch_one(
            "SELECT 1 FROM album_media WHERE album_id = ? AND media_id = ?", (share["album_id"], media_id)
        )
        if album_media is None:
            raise HTTPException(status_code=status.HTTP_403_FORBIDDEN, detail="Media not in shared album")

    media_row = fetch_one("SELECT thumbnail_path FROM media WHERE id = ?", (media_id,))
    if media_row is None:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Media not found")

    if media_row["thumbnail_path"] is None:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Thumbnail not available")

    thumb_path = THUMBNAILS_DIR / media_row["thumbnail_path"]
    if not thumb_path.exists():
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Thumbnail file not found")

    return FileResponse(path=thumb_path, media_type="image/jpeg")
