import base64
from logging import getLogger
from pathlib import Path
from typing import Annotated, Optional

from fastapi import APIRouter, Depends

from momento_api.auth.dependencies import CurrentUser, get_current_user
from momento_api.constants import THUMBNAILS_DIR
from momento_api.database import fetch_all
from momento_api.models.media import GeoMediaResponse, MapMediaResponse

logger = getLogger(__name__)
router = APIRouter(prefix="/map", tags=["map"])


def _read_thumbnail_as_base64(thumbnail_path_str: str) -> Optional[str]:
    """
    Reads a thumbnail file and returns it as a base64 encoded data URI.
    Returns None if the file cannot be read.
    """
    if not thumbnail_path_str:
        return None

    thumb_path = THUMBNAILS_DIR / thumbnail_path_str
    if not thumb_path.exists():
        return None

    try:
        with open(thumb_path, "rb") as f:
            base64_data = base64.b64encode(f.read()).decode("utf-8")
            return f"data:image/jpeg;base64,{base64_data}"
    except OSError as e:
        logger.warning(f"Failed to read thumbnail {thumb_path}: {e}")
        return None


@router.post("/media", response_model=MapMediaResponse)
async def get_map_media(current_user: Annotated[CurrentUser, Depends(get_current_user)]) -> MapMediaResponse:
    rows = fetch_all(
        """
        SELECT id, thumbnail_path, gps_latitude, gps_longitude, date_taken, media_type, mime_type, original_filename
        FROM media
        WHERE user_id = ? AND gps_latitude IS NOT NULL AND gps_longitude IS NOT NULL
        ORDER BY date_taken DESC
        """,
        (current_user.id,),
    )

    items = []
    for row in rows:
        thumb_data = _read_thumbnail_as_base64(row["thumbnail_path"])

        items.append(
            GeoMediaResponse(
                id=row["id"],
                thumbnail_path=row["thumbnail_path"],
                thumbnail_data=thumb_data,
                latitude=row["gps_latitude"],
                longitude=row["gps_longitude"],
                date_taken=row["date_taken"],
                media_type=row["media_type"],
                mime_type=row["mime_type"],
                original_filename=row["original_filename"],
            )
        )

    return MapMediaResponse(items=items)
