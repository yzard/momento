from typing import Optional

from pydantic import BaseModel

from momento_api.models.media import MediaResponse


class AlbumResponse(BaseModel):
    id: int
    name: str
    description: Optional[str]
    cover_media_id: Optional[int]
    media_count: int
    created_at: str


class AlbumDetailResponse(BaseModel):
    id: int
    name: str
    description: Optional[str]
    cover_media_id: Optional[int]
    media: list[MediaResponse]
    created_at: str


class AlbumGetRequest(BaseModel):
    album_id: int


class AlbumCreateRequest(BaseModel):
    name: str
    description: Optional[str] = None


class AlbumUpdateRequest(BaseModel):
    album_id: int
    name: Optional[str] = None
    description: Optional[str] = None
    cover_media_id: Optional[int] = None


class AlbumDeleteRequest(BaseModel):
    album_id: int


class AlbumAddMediaRequest(BaseModel):
    album_id: int
    media_ids: list[int]


class AlbumRemoveMediaRequest(BaseModel):
    album_id: int
    media_ids: list[int]


class AlbumReorderRequest(BaseModel):
    album_id: int
    media_ids: list[int]


class AlbumListResponse(BaseModel):
    albums: list[AlbumResponse]
