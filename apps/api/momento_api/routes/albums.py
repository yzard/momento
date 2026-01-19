from typing import Annotated

from fastapi import APIRouter, Depends, HTTPException, status

from momento_api.auth.dependencies import CurrentUser, get_current_user
from momento_api.database import execute_query, fetch_all, fetch_one, get_connection, insert_returning_id
from momento_api.models.album import (
    AlbumAddMediaRequest,
    AlbumCreateRequest,
    AlbumDeleteRequest,
    AlbumDetailResponse,
    AlbumGetRequest,
    AlbumListResponse,
    AlbumRemoveMediaRequest,
    AlbumReorderRequest,
    AlbumResponse,
    AlbumUpdateRequest,
)
from momento_api.models.media import MediaResponse

router = APIRouter(prefix="/album", tags=["album"])


def _row_to_album_response(row) -> AlbumResponse:
    return AlbumResponse(
        id=row["id"],
        name=row["name"],
        description=row["description"],
        cover_media_id=row["cover_media_id"],
        media_count=row["media_count"] if "media_count" in row.keys() else 0,
        created_at=row["created_at"],
    )


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


@router.post("/create", response_model=AlbumResponse)
async def create_album(
    request: AlbumCreateRequest, current_user: Annotated[CurrentUser, Depends(get_current_user)]
) -> AlbumResponse:
    album_id = insert_returning_id(
        "INSERT INTO albums (user_id, name, description) VALUES (?, ?, ?)",
        (current_user.id, request.name, request.description),
    )

    row = fetch_one("SELECT a.*, 0 as media_count FROM albums a WHERE a.id = ?", (album_id,))
    if row is None:
        raise HTTPException(status_code=status.HTTP_500_INTERNAL_SERVER_ERROR, detail="Failed to create album")

    return _row_to_album_response(row)


@router.post("/list", response_model=AlbumListResponse)
async def list_albums(current_user: Annotated[CurrentUser, Depends(get_current_user)]) -> AlbumListResponse:
    rows = fetch_all(
        """
        SELECT a.*, COUNT(am.media_id) as media_count
        FROM albums a
        LEFT JOIN album_media am ON a.id = am.album_id
        WHERE a.user_id = ?
        GROUP BY a.id
        ORDER BY a.created_at DESC
        """,
        (current_user.id,),
    )
    return AlbumListResponse(albums=[_row_to_album_response(row) for row in rows])


@router.post("/get", response_model=AlbumDetailResponse)
async def get_album(
    request: AlbumGetRequest, current_user: Annotated[CurrentUser, Depends(get_current_user)]
) -> AlbumDetailResponse:
    album_row = fetch_one("SELECT * FROM albums WHERE id = ? AND user_id = ?", (request.album_id, current_user.id))
    if album_row is None:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Album not found")

    media_rows = fetch_all(
        """
        SELECT m.* FROM media m
        JOIN album_media am ON m.id = am.media_id
        WHERE am.album_id = ?
        ORDER BY am.position
        """,
        (request.album_id,),
    )

    return AlbumDetailResponse(
        id=album_row["id"],
        name=album_row["name"],
        description=album_row["description"],
        cover_media_id=album_row["cover_media_id"],
        media=[_row_to_media_response(row) for row in media_rows],
        created_at=album_row["created_at"],
    )


@router.post("/update", response_model=AlbumResponse)
async def update_album(
    request: AlbumUpdateRequest, current_user: Annotated[CurrentUser, Depends(get_current_user)]
) -> AlbumResponse:
    album_row = fetch_one("SELECT * FROM albums WHERE id = ? AND user_id = ?", (request.album_id, current_user.id))
    if album_row is None:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Album not found")

    updates: list[str] = []
    params: list[object] = []

    if request.name is not None:
        updates.append("name = ?")
        params.append(request.name)

    if request.description is not None:
        updates.append("description = ?")
        params.append(request.description)

    if request.cover_media_id is not None:
        updates.append("cover_media_id = ?")
        params.append(request.cover_media_id)

    if updates:
        params.append(request.album_id)
        execute_query(f"UPDATE albums SET {', '.join(updates)} WHERE id = ?", tuple(params))
        get_connection().commit()

    row = fetch_one(
        """
        SELECT a.*, COUNT(am.media_id) as media_count
        FROM albums a
        LEFT JOIN album_media am ON a.id = am.album_id
        WHERE a.id = ?
        GROUP BY a.id
        """,
        (request.album_id,),
    )
    return _row_to_album_response(row)


@router.post("/delete")
async def delete_album(
    request: AlbumDeleteRequest, current_user: Annotated[CurrentUser, Depends(get_current_user)]
) -> dict:
    album_row = fetch_one("SELECT id FROM albums WHERE id = ? AND user_id = ?", (request.album_id, current_user.id))
    if album_row is None:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Album not found")

    execute_query("DELETE FROM albums WHERE id = ?", (request.album_id,))
    get_connection().commit()

    return {"message": "Album deleted successfully"}


@router.post("/add-media")
async def add_media_to_album(
    request: AlbumAddMediaRequest, current_user: Annotated[CurrentUser, Depends(get_current_user)]
) -> dict:
    album_row = fetch_one("SELECT id FROM albums WHERE id = ? AND user_id = ?", (request.album_id, current_user.id))
    if album_row is None:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Album not found")

    max_pos_row = fetch_one(
        "SELECT COALESCE(MAX(position), -1) as max_pos FROM album_media WHERE album_id = ?", (request.album_id,)
    )
    next_pos = (max_pos_row["max_pos"] if max_pos_row else -1) + 1

    conn = get_connection()
    insert_params: list[tuple[int, int, int]] = []
    for index, media_id in enumerate(request.media_ids):
        media_row = fetch_one("SELECT id FROM media WHERE id = ? AND user_id = ?", (media_id, current_user.id))
        if media_row is None:
            continue
        insert_params.append((request.album_id, media_id, next_pos + index))

    if not insert_params:
        return {"message": "Media added to album"}

    conn.executemany("INSERT OR IGNORE INTO album_media (album_id, media_id, position) VALUES (?, ?, ?)", insert_params)
    conn.commit()

    return {"message": "Media added to album"}


@router.post("/remove-media")
async def remove_media_from_album(
    request: AlbumRemoveMediaRequest, current_user: Annotated[CurrentUser, Depends(get_current_user)]
) -> dict:
    album_row = fetch_one("SELECT id FROM albums WHERE id = ? AND user_id = ?", (request.album_id, current_user.id))
    if album_row is None:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Album not found")

    conn = get_connection()
    for media_id in request.media_ids:
        conn.execute("DELETE FROM album_media WHERE album_id = ? AND media_id = ?", (request.album_id, media_id))
    conn.commit()

    return {"message": "Media removed from album"}


@router.post("/reorder")
async def reorder_album_media(
    request: AlbumReorderRequest, current_user: Annotated[CurrentUser, Depends(get_current_user)]
) -> dict:
    album_row = fetch_one("SELECT id FROM albums WHERE id = ? AND user_id = ?", (request.album_id, current_user.id))
    if album_row is None:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Album not found")

    conn = get_connection()
    for i, media_id in enumerate(request.media_ids):
        conn.execute(
            "UPDATE album_media SET position = ? WHERE album_id = ? AND media_id = ?", (i, request.album_id, media_id)
        )
    conn.commit()

    return {"message": "Album reordered successfully"}
